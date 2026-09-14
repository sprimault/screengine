# Contrat d'ABI

Ce document fait foi. Le code s'y conforme, et un désaccord entre les deux est un
défaut du code.

Un auteur de liaison qui ne lit pas le français trouve l'essentiel dans
`include/screengine.h`, dont la documentation est en anglais : ce qui ne peut pas
être ignoré à l'appel y figure, fonction par fonction.

**État : brouillon, avant le lot 1 de l'étape 0.** Les points marqués
**À trancher** portent un identifiant (`A1`, `A2`…) et une recommandation. Ils
se figent au lot 1, sauf mention contraire. Un point tranché perd son marqueur,
et garde en une phrase l'option écartée et pourquoi.

## Principes

Arrêtés. Ils découlent des invariants du projet et ne se rediscutent pas ici.

- **Handles opaques uniquement.** L'hôte ne voit jamais la disposition mémoire
  d'un objet du moteur, seulement un pointeur vers un type incomplet.
- **Les structures qui traversent sont du POD `#[repr(C)]`, en entrée.** Aucune
  structure n'est rendue à l'hôte par valeur ou par pointeur vers la mémoire du
  moteur, à la seule exception du message de `scg_last_error`.
- **Aucun callback.** Le moteur n'appelle jamais l'hôte. Il n'y a donc ni
  réentrance, ni question de pile traversée dans l'autre sens.
- **Aucune allocation qui traverse.** Le tampon de sortie appartient à l'hôte. La
  paire `scg_buffer_alloc` / `scg_buffer_free` ne fait pas exception : la mémoire
  y est allouée par le moteur *pour le compte* de l'hôte, qui en est propriétaire
  jusqu'à la libération, et le moteur n'en garde aucune référence.
- **Toute erreur est un code de retour.** Chaque point d'entrée est enveloppé de
  `catch_unwind` : une panique qui atteindrait l'appelant serait un comportement
  indéfini, pas un plantage propre.
- **On ajoute des fonctions, on ne modifie jamais une signature publiée.** Un hôte
  compilé contre un `screengine.h` d'il y a six mois doit continuer à se lier.
- **Le moteur n'ouvre rien.** Cartes, maillages et textures arrivent en blocs
  d'octets. Le moteur n'a ni chemin de fichier, ni horloge, ni thread.
- **L'environnement flottant de l'hôte est préservé.** Chaque point d'entrée
  fixe le sien — arrondi au plus proche, DAZ et FTZ désactivés — et rend celui de
  l'hôte au retour. Une bibliothèque audio ou un moteur de jeu qui a activé DAZ
  dans le même processus ne change donc pas l'image, et le moteur ne change rien
  pour eux.
- **Toute allocation a lieu dans un appel nommé** : création, chargement d'une
  ressource, calcul de lightmaps, changement de résolution au-delà du maximum.
  Aucune entre le début et la fin d'une image.
- **Rien du jeu ne traverse.** Aucune fonction ne prend un joueur, une arme ou un
  score. Une fonction de rendu qui en réclame un manque d'un paramètre générique.

## Conventions

### Nommage

- Fonctions : `scg_` puis `snake_case`. Les fonctions du contexte de rendu n'ont
  pas de nom d'objet (`scg_create`, `scg_frame_end`) ; les autres suivent
  `scg_<objet>_<verbe>` (`scg_mesh_load`, `scg_world_destroy`).
- Types : `Scg` puis `CamelCase` (`ScgContext`).
- Constantes : `SCG_` puis `SCREAMING_SNAKE_CASE`.

### Types

- **Entiers à largeur fixe** (`uint32_t`, `int32_t`…) pour tout ce qui a un sens
  numérique : dimensions, identifiants, codes, compteurs. `size_t` est réservé
  aux longueurs en octets et aux tailles d'allocation, parce qu'il suit la
  largeur des pointeurs — 32 bits sur wasm et armv7, 64 ailleurs.
- **Aucun type énuméré.** Une valeur à choix multiple est un `uint32_t`
  accompagné de constantes `SCG_`. Un `enum` Rust qui reçoit un discriminant
  inconnu de la part d'un hôte C est un comportement indéfini, et un hôte C peut
  toujours passer 7.
- **Aucun `bool`.** Pour la même raison : un drapeau est un `uint8_t`, `0` ou
  `1`, et toute autre valeur est rejetée par `SCG_ERR_INVALID_ARGUMENT`.
- **Flottants en `float` (`f32`) uniquement.** Le moteur ne calcule pas en `f64`,
  et une conversion à la frontière serait un arrondi de plus à rendre
  déterministe.
- **Chaînes en UTF-8.** En entrée : pointeur et longueur, sans terminateur exigé.
  En sortie : terminées par un octet nul.
- **Petit-boutisme.** Toutes les cibles le sont. Un format de fichier lu par le
  moteur l'impose explicitement plutôt que de s'en remettre à la cible.

### Structures

- **Aucun remplissage implicite.** Les champs sont ordonnés pour qu'aucun octet
  de bourrage ne soit inséré, ou le bourrage est déclaré en champ `_reserved`.
  Sur wasm, une liaison JavaScript écrit les structures à la main dans la mémoire
  linéaire, octet par octet : un décalage calculé par le compilateur et non écrit
  dans le header est un décalage qu'elle se trompera à reproduire.
- **Une structure publiée ne change plus**, pas plus qu'une signature.
  **À trancher — A6** : figer purement et simplement (une structure étendue est
  une nouvelle structure et une nouvelle fonction), ou faire porter sa taille à
  chaque structure en premier champ.

## Codes de retour

**À trancher — A1** : la forme générale. Recommandation : toute fonction qui
peut échouer rend un `int32_t`, `0` en cas de succès, négatif en cas d'erreur ;
une valeur produite passe par un paramètre de sortie. Les codes positifs sont
réservés et ne sont jamais rendus en v1. Les exceptions sont nommées : la version
d'ABI, le message d'erreur, et la paire d'allocation de tampon, qui rend un
pointeur nul en cas d'échec comme le ferait `malloc`.

Arrêté, quelle que soit la forme retenue :

- **Un code publié ne change jamais de sens.** Un code retiré n'est pas réattribué.
- **Une liaison traite un code négatif inconnu comme une erreur générique.** Une
  bibliothèque plus récente peut rendre un code qu'elle ne connaît pas encore, et
  c'est ce qui permet d'en ajouter sans incrémenter la version d'ABI.
- **Le code est l'information ; le message est pour un humain.** Aucune liaison
  ne décide d'un comportement en analysant le texte de `scg_last_error`.

**À trancher — A2** : la numérotation. Elle se fige au lot 1 et réserve dès
maintenant la place des étapes suivantes, plutôt que de numéroter à la suite au
fil de l'eau. Proposition :

| Plage | Domaine | Codes proposés |
|---|---|---|
| `0` | succès | `SCG_OK` |
| `-1` à `-99` | généraux | `-1` `SCG_ERR_NULL`, `-2` `SCG_ERR_INVALID_ARGUMENT`, `-3` `SCG_ERR_OUT_OF_MEMORY`, `-4` `SCG_ERR_INVALID_STATE`, `-5` `SCG_ERR_PANIC`, `-6` `SCG_ERR_POISONED` |
| `-100` à `-199` | données (étape 4) | `-100` `SCG_ERR_UNKNOWN_RESOURCE`, `-101` `SCG_ERR_INVALID_FORMAT`, `-102` `SCG_ERR_UNSUPPORTED_FORMAT_VERSION` |
| `-200` à `-299` | monde (étape 5) | — |
| `-300` à `-399` | collision (étape 7) | — |
| `-400` à `-499` | édition (étape 8) | — |

`SCG_ERR_INVALID_STATE` couvre un appel hors séquence, par exemple une fin
d'image sans début.

## Erreurs

### `scg_last_error`

Arrêté :

- Le pointeur rendu est **valide jusqu'au prochain appel sur le même contexte**.
  Une liaison qui veut garder le message le copie immédiatement.
- La chaîne est en UTF-8, terminée par un octet nul, et appartient au moteur.
  L'hôte ne la libère jamais.
- Sans erreur depuis le dernier appel, le message est la chaîne vide, jamais un
  pointeur nul.

**À trancher — A3** : la langue du message. `CONTRIBUTING.fr.md` met les
messages d'erreur en français. Mais celui-ci est lu par les mêmes personnes que
le header — des auteurs de liaisons et des intégrateurs qui ne parlent pas
forcément français —, et le header est en anglais pour cette raison.
Recommandation : l'anglais, comme les docstrings exportées, et une ligne de plus
dans la section « Langue » de `CONTRIBUTING`.

### Erreurs sans contexte

**À trancher — A4** : où se lit l'erreur d'une fonction qui n'a pas de contexte à
qui la rattacher. C'est le cas de `scg_create` quand elle échoue, et ce sera le
cas des ressources indépendantes du contexte (A8), qui sont une nécessité de
l'étape 7 : un serveur de jeu charge une carte pour la collision sans jamais
créer de contexte de rendu.

- (a) `scg_create` rend un pointeur nul, sans message. Simple, mais la cause d'un
  échec de création est perdue.
- (b) Un emplacement par thread, lu par `scg_last_error(NULL)`. La couche FFI
  dispose de `std` et peut le tenir ; le noyau n'en sait rien.
- (c) Chaque objet porte sa propre erreur, et les constructeurs rendent un code
  avec un paramètre de sortie. La cause d'un échec de construction reste perdue.

Recommandation : (b), avec la forme de A1. `scg_create` rend alors un code et
écrit le contexte dans un paramètre de sortie — ce qui s'écarte de l'exemple du
README, où elle rend directement le pointeur.

### Après une panique

**À trancher — A5** : l'état d'un objet dont un appel a paniqué. Une panique
interrompt le moteur au milieu d'une mise à jour, et rien ne garantit que
l'objet soit encore cohérent. Recommandation : l'objet est **empoisonné**. Tout
appel suivant rend `SCG_ERR_POISONED`, sauf `scg_last_error` et la destruction,
qui restent permises pour que l'hôte puisse lire la cause et libérer.

## Durées de vie et propriété

- **Contexte.** Créé par `scg_create`, libéré par `scg_destroy`. `scg_destroy(NULL)`
  ne fait rien, comme `free(NULL)`. Un handle détruit puis réutilisé n'est pas
  détecté : c'est une précondition, pas un cas d'erreur.
- **Budget du contexte.** **À trancher — A12** : ce que reçoit la création.
  Recommandation : une structure de configuration, premier cas concret de A6,
  portant la résolution interne maximale, la résolution initiale et la taille de
  tuile (32 ou 64). Tous les tampons propres à l'image — couleur, profondeur,
  listes de faces, tampons de clipping — sont dimensionnés pour le maximum dès la
  création. Changer de résolution sous ce maximum n'alloue rien ; au-delà, c'est
  une erreur. La mémoire qui dépend de la scène — textures, mipmaps, lightmaps —
  appartient aux ressources, pas au contexte.
- **Tampon de sortie.** Appartient à l'hôte. Le moteur y écrit pendant
  `scg_frame_end` et n'en garde aucune référence au retour.
- **Tampons alloués par `scg_buffer_alloc`.** Appartiennent à l'hôte entre
  l'allocation et `scg_buffer_free`. Ils se libèrent par `scg_buffer_free` et par
  rien d'autre : l'allocateur du moteur n'est pas celui de l'hôte, sur bureau non
  plus. **À trancher — A7** : la signature de la libération et l'alignement
  garanti. Recommandation : `scg_buffer_free(ptr, len)`, parce que l'allocateur
  Rust exige la taille et qu'un en-tête caché devant chaque bloc serait une
  hypothèse de plus pour une liaison ; alignement garanti de 16 octets, qui
  couvre une vue `Uint32Array` sur wasm comme les chemins SIMD de l'étape 9.
- **Octets de données** (cartes, maillages, textures). **À trancher — A8**,
  échéance étape 4 : le moteur copie ce qu'il garde, ou emprunte le bloc de
  l'hôte. Recommandation : il copie. L'hôte peut libérer son bloc dès le retour
  de l'appel, et aucune durée de vie ne traverse la frontière.
- **Ressources** (textures, maillages, mondes). Même échéance, même point : elles
  sont indépendantes de tout contexte, ce qu'impose la collision sans rendu de
  l'étape 7. Leur mémoire est allouée à leur chargement, mipmaps compris ; celle
  des lightmaps, à l'appel qui les calcule. Détruire une ressource encore
  référencée par un appel de dessin est une précondition, pas un cas d'erreur.

## Concurrence

Arrêté :

- Le moteur ne crée aucun thread et n'en suppose aucun.
- **Un objet est utilisé par un seul thread à la fois.** Deux contextes distincts
  sont indépendants et peuvent servir chacun sur son thread.
- Les fonctions sans objet — `scg_abi_version`, `scg_buffer_alloc`,
  `scg_buffer_free` — sont appelables depuis n'importe quel thread.
- **Une exception, et une seule : le rendu des tuiles.** Entre le début et la fin
  d'une image, l'hôte peut rendre des tuiles distinctes depuis des threads
  distincts. Aucun autre appel sur le contexte n'est permis pendant ce temps.
  Chaque tuile écrit un rectangle disjoint du tampon de l'hôte : le partage du
  tampon entre threads est sûr par construction.
- **L'image ne dépend ni de la taille des tuiles, ni du nombre de threads, ni de
  l'ordre dans lequel les tuiles sont rendues.**

**À trancher — A13**, échéance étape 1 : la forme de l'API de tuiles.
Recommandation : `scg_frame_begin` prépare l'image et rend le nombre de tuiles ;
`scg_frame_tile(ctx, index, pixels, stride)` rend une tuile et y applique le
post-traitement ; `scg_frame_end` clôt l'image. Un hôte sans threads appelle les
tuiles dans une boucle. Le noyau ne crée aucun thread et ne rappelle personne.

Le partage d'une ressource en lecture entre deux contextes sur deux threads se
tranche avec A8.

## Tampon de sortie

- **Dimensions : la résolution interne**, jamais celle de l'écran. La mise à
  l'échelle entière appartient à l'hôte.
- **`stride` est exprimé en pixels**, et vaut au moins la largeur. Le tampon
  fait au moins `stride × hauteur` pixels. Le moteur ne reçoit pas sa longueur et
  ne peut pas la vérifier : c'est une précondition documentée.
- **La résolution interne se change sans recréer le contexte** (étape 3), et
  sans allocation sous le maximum fixé à la création (A12). Le tampon de l'hôte
  suit ce changement.
- **À trancher — A9** : le format des pixels. Recommandation : 4 octets par
  pixel dans l'ordre R, G, B, A en mémoire, alpha toujours à 255. C'est l'ordre
  natif de `ImageData` dans un navigateur et de `Bitmap.Config.ARGB_8888` sur
  Android ; l'hôte Windows permute vers BGRA pour GDI.
- **Le niveau de qualité du filtrage est un paramètre de contexte** (étape 2) :
  tramage ordonné des coordonnées par défaut, bilinéaire au-dessus. Le motif de
  tramage est une table fixe du noyau, indexée par la position du pixel dans
  l'image : il n'a pas de graine, et rien de ce qu'il produit ne dépend de
  l'hôte.
- **Le post-traitement** — gamma, tonemapping, étalonnage — s'applique pendant
  l'écriture de chaque tuile dans le tampon de l'hôte. Il n'y a pas de passe plein
  écran, et donc rien qui lise les pixels voisins.

## Versionnement

- `SCG_ABI_VERSION` est une constante du header ; `scg_abi_version()` rend celle
  de la bibliothèque chargée. C'est un entier, pas un numéro SemVer.
- **Ajouter une fonction ou un code d'erreur ne l'incrémente pas. Tout le reste
  l'incrémente** : une signature, une structure, le sens d'un paramètre ou d'un
  code, une précondition.
- **Une liaison vérifie l'égalité au chargement** et refuse une bibliothèque dont
  la version diffère de celle du header contre lequel elle a été écrite. Puisque
  seule une rupture incrémente, une version différente est une rupture, dans un
  sens comme dans l'autre.
- Conséquence à connaître : l'égalité ne détecte pas une fonction ajoutée
  depuis. Une liaison écrite contre un header plus récent qui appelle une telle
  fonction échoue à la résolution du symbole — à l'édition de liens pour le C, au
  premier appel pour PHP FFI.
- **À trancher — A10** : la dépréciation. Une fonction remplacée reste exportée et
  documentée comme dépréciée. Reste à dire si elle est un jour retirée.
  Recommandation : jamais en `0.x` ; la question se rouvre au gel de l'ABI en 1.0.

## Fonctions

### Étape 0

Les sept points d'entrée de la feuille de route. Leur existence est arrêtée ;
les signatures ci-dessous suivent les recommandations de A1, A4, A7 et A12, et
se figent avec elles au lot 1.

```c
uint32_t    scg_abi_version(void);
int32_t     scg_create(const ScgContextConfig *config, ScgContext **out);
void        scg_destroy(ScgContext *ctx);
int32_t     scg_frame_end(ScgContext *ctx, uint8_t *pixels, uint32_t stride);
const char *scg_last_error(const ScgContext *ctx);
uint8_t    *scg_buffer_alloc(size_t len);
void        scg_buffer_free(uint8_t *ptr, size_t len);
```

À l'étape 0, `scg_frame_end` rend un triangle en dur dans l'image entière : il
n'y a pas encore de scène à soumettre, ni de tuiles. Ce triangle est pourtant
rempli par les fonctions de bord en virgule fixe et la règle top-left
définitives — c'est le premier remplissage.

### Étapes suivantes

Prévisionnel. Ce qui doit être exposé est arrêté par la feuille de route ; les
noms ne le sont pas.

| Étape | Ce qui doit être exposé |
|---|---|
| 1 | début d'image, rendu d'une tuile (A13), caméra et projection, soumission de triangles avec une matrice |
| 2 | chargement d'une texture depuis un bloc de pixels, mipmaps générés au chargement, niveau de qualité du filtrage |
| 3 | changement de résolution interne, calcul des lightmaps d'une cellule et reprise d'un cache, lumières dynamiques, brouillard, post-traitement |
| 4 | chargement d'un maillage et d'une carte depuis un bloc d'octets, libération |
| 5 | rendu du monde depuis la caméra |
| 6 | interpolation entre trames, sprites orientés caméra |
| 7 | module de collision, utilisable sans contexte de rendu |
| 8 | tracé de lignes et de points, interrogation de la scène, modification d'une cellule par identifiant |

## Ce qu'un auteur de liaison doit savoir

- **Vérifier la version au chargement**, par égalité, avant tout autre appel.
- **Copier le message de `scg_last_error` immédiatement.** Le prochain appel sur
  le même contexte l'invalide.
- **Traiter tout code négatif inconnu comme une erreur**, sans échouer sur la
  valeur elle-même.
- **Sur wasm, passer par `scg_buffer_alloc`.** L'hôte ne peut pas fournir un
  pointeur arbitraire : seule la mémoire linéaire du module est adressable.
- **Sur wasm, recréer toute vue sur la mémoire après chaque appel.** Un appel qui
  alloue peut agrandir la mémoire linéaire, ce qui détache le `ArrayBuffer`
  existant : une `Uint8ClampedArray` construite avant l'appel ne voit plus rien
  après, sans lever d'erreur à sa création.
- **Sur wasm, une panique peut être un trap** plutôt qu'un code de retour, selon
  ce que la chaîne de compilation permet — voir
  [`construction.md`](construction.md). Une instance qui a levé un trap ne se
  réutilise pas.
- **Ne rien calculer.** Une liaison convertit des types. Ce qui devrait être
  partagé entre deux liaisons remonte dans le noyau.
