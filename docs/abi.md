# Contrat d'ABI

Ce document fait foi. Le code s'y conforme, et un désaccord entre les deux est un
défaut du code.

Un auteur de liaison qui ne lit pas le français trouve l'essentiel dans
`include/screengine.h`, dont la documentation est en anglais : ce qui ne peut pas
être ignoré à l'appel y figure, fonction par fonction.

**État : l'étape 3 est publiée — les sept points d'entrée de l'étape 0, le rendu
par tuiles, les textures avec leur niveau de filtrage, et la lumière :
lightmaps, lumières dynamiques, brouillard, résolution interne et courbe de
sortie.** Chaque décision garde ci-dessous l'option écartée et pourquoi. Un seul
point reste marqué **À trancher** : la dépréciation, qui attend le gel de l'ABI
en 1.0.

**L'étape 4 est exposée entière** : les six fonctions du maillage, les six de la
carte et les sept accesseurs de ses lumières et de ses entités. Le
contrat d'un format se fige avant son premier décodeur, comme les formats de
virgule fixe se sont figés avant le premier remplissage : ce qui s'écrit après
s'écrit contre ce qui a déjà été codé.

## Principes

Arrêtés. Ils découlent des invariants du projet et ne se rediscutent pas ici.

- **Handles opaques uniquement.** L'hôte ne voit jamais la disposition mémoire
  d'un objet du moteur, seulement un pointeur vers un type incomplet.
- **Les structures qui traversent sont du POD `#[repr(C)]`, en entrée.** Aucune
  structure n'est rendue à l'hôte par valeur ou par pointeur vers la mémoire du
  moteur, à la seule exception du message de `scg_last_error`. **Remplir une
  structure que l'hôte possède n'est ni l'un ni l'autre** : `scg_world_light` le
  fait, comme `scg_mesh_texture_name` remplit un tampon de l'appelant.
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
  fixe le sien — arrondi au plus proche, DAZ et FTZ désactivés, exceptions
  masquées — et rend celui de l'hôte au retour, masques compris. Une bibliothèque
  audio ou un moteur de jeu qui a activé DAZ dans le même processus ne change
  donc pas l'image, et le moteur ne change rien pour eux. Un hôte qui a démasqué
  une exception pour traquer un défaut chez lui ne tombe pas dans le moteur, qui
  produit des résultats inexacts à chaque image.
- **Toute allocation a lieu dans un appel nommé** : création, chargement d'une
  ressource, calcul de lightmaps. Aucune entre le début et la fin d'une image, ni
  au changement de résolution, qui reste sous le maximum fixé à la création.
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
- **Une structure publiée ne change plus**, pas plus qu'une signature. Une
  structure étendue est une nouvelle structure, et elle arrive avec une nouvelle
  fonction. Écarté : le champ de taille en premier champ, à la manière de
  plusieurs API système. Son type naturel est `size_t`, qui fait
  quatre octets sur wasm32 et armv7 et huit ailleurs : le champ censé rendre la
  disposition sûre serait le seul de toute l'ABI à la faire varier selon la
  cible. Les API qui ont pris ce chemin ont d'ailleurs fini par en exiger
  l'égalité stricte avec `sizeof`, ce qui n'est plus un mécanisme d'extension
  mais un contrôle de plus à remplir.
- **L'extensibilité passe par des champs `_reserved` explicites**, nuls
  obligatoires : une valeur non nulle rend `SCG_ERR_INVALID_ARGUMENT`. Les
  décalages restent figés, et un hôte écrit avant qu'un champ serve obtient le
  comportement par défaut en passant des zéros.
- **L'hôte met la structure entière à zéro avant de la remplir.** C'est ce qui
  rend la clause précédente vraie de son côté.
- **Aucune structure ne traverse par valeur**, toujours par pointeur. L'ABI
  `extern "C"` de `wasm32-unknown-unknown` a divergé de celle de clang pendant
  des années sur le passage d'agrégats par valeur.
- **Aucun `size_t` ni pointeur dans une structure qui traverse** : leur largeur
  change entre wasm32 et armv7 d'un côté, x86_64 et arm64 de l'autre, et avec
  elle tous les décalages qui suivent. Les `float` et les entiers 32 bits
  s'alignent sur quatre octets partout ; un entier 64 bits s'aligne sur huit et
  introduit du bourrage dès qu'il suit un champ plus étroit, qui se déclare
  alors en `_reserved`.

## Codes de retour

Toute fonction qui peut échouer rend un `int32_t`, `0` en cas
de succès, négatif en cas d'erreur ; une valeur produite passe par un paramètre
de sortie. Les exceptions sont nommées, et ce sont les seules : la version
d'ABI, le message d'erreur, et la paire d'allocation de tampon, qui rend un
pointeur nul en cas d'échec comme le ferait `malloc`.

Écarté : rendre la valeur produite avec une valeur sentinelle et l'erreur à
côté, à la manière d'`errno`. Un indicateur en bande finit toujours par devenir
une valeur légitime, et rien ne garantit qu'un emplacement d'erreur soit
préservé en cas de succès — une liaison ne sait alors pas quand le lire.

**Un code positif est un succès accompagné d'un statut.** Aucun n'est rendu en
v1. C'est écrit maintenant parce que ça ne coûte rien maintenant : une liaison
qui teste « différent de `0` » au lieu de « négatif » se trompera le jour où un
appel devra dire « incomplet ».

Arrêté :

- **Un code publié ne change jamais de sens.** Un code retiré n'est pas réattribué.
- **Une liaison dégrade un code inconnu vers sa catégorie** plutôt que de le
  traiter en erreur générique — voir la règle arithmétique ci-dessous. Une
  bibliothèque plus récente peut rendre un code qu'elle ne connaît pas encore, et
  c'est ce qui permet d'en ajouter sans incrémenter la version d'ABI.
- **Le code est l'information ; le message est pour un humain.** Aucune liaison
  ne décide d'un comportement en analysant le texte de `scg_last_error`.

Les plages sont réservées par étape dès maintenant. Écarté : numéroter à la
suite au fil de l'eau, qui rend illisible la catégorie d'un code — précisément
ce dont une liaison a besoin pour traiter celui qu'elle ne connaît pas.

| Plage | Domaine | Codes proposés |
|---|---|---|
| `0` | succès | `SCG_OK` |
| `-1` à `-99` | généraux | `-1` `SCG_ERR_NULL`, `-2` `SCG_ERR_INVALID_ARGUMENT`, `-3` `SCG_ERR_OUT_OF_MEMORY`, `-4` `SCG_ERR_INVALID_STATE`, `-5` `SCG_ERR_PANIC`, `-6` `SCG_ERR_FAULTED` |
| `-100` à `-199` | données (étape 4) | `-100` `SCG_ERR_UNKNOWN_RESOURCE`, `-101` `SCG_ERR_INVALID_FORMAT`, `-102` `SCG_ERR_UNSUPPORTED_FORMAT_VERSION` |
| `-200` à `-299` | monde (étape 5) | — |
| `-300` à `-399` | collision (étape 7) | — |
| `-400` à `-499` | édition (étape 8) | — |

`SCG_ERR_INVALID_STATE` couvre un appel hors séquence, par exemple une tuile
rendue avant le début de l'image. Une fin d'image sans début n'en est pas un :
elle fait le début elle-même, voir « Rendu par tuiles ».

Les trois codes de la plage données se distinguent par **où est la faute**.
`SCG_ERR_INVALID_ARGUMENT` est une faute dans l'**appel** — une longueur qui ne
concorde pas, un emplacement hors borne, un tableau de textures de la mauvaise
taille. Les deux suivants sont des fautes dans le **contenu**, que l'hôte n'a pas
écrit : ce qu'il rapporte à son utilisateur comme « ce fichier est mauvais », et
non à son développeur comme « tu m'as mal appelé ». Rendre `-2` sur un fichier
malformé le classerait « général », et le chemin « mauvais asset » d'un hôte ne
se déclencherait jamais.

- **`SCG_ERR_INVALID_FORMAT`** : tout ce qui est faux à l'intérieur du bloc —
  signature, genre, table de sections incohérente, compte qui ne recoupe pas la
  longueur de sa section, indice hors borne, coordonnée non finie, identifiant
  dupliqué, section de genre inconnu.
- **`SCG_ERR_UNSUPPORTED_FORMAT_VERSION`** : signature et genre justes, mais une
  version de format que cette bibliothèque ne lit pas. C'est le seul des trois
  qui dise à l'hôte quoi faire — prendre une version plus récente, ou réexporter
  la donnée. **Le contrôle de version passe immédiatement après la signature et
  le genre, avant tout contrôle de structure** : sinon un fichier plus récent
  remonte « format invalide » et l'intégrateur cherche une corruption qui
  n'existe pas.
- **`SCG_ERR_UNKNOWN_RESOURCE`** : une recherche par identifiant stable qui ne
  trouve rien. **Aucun appel ne le rend à l'étape 4** — un identifiant absent y
  est attrapé au chargement, et un index hors borne est une faute d'appel. Il est
  défini maintenant parce qu'un code publié ne change jamais de sens, et qu'un
  code défini sans être rendu ne coûte rien.

Écarté : un code distinct pour un fichier tronqué. Du côté du moteur, une
troncature ne se distingue pas d'un en-tête qui ment sur sa longueur ; aucun hôte
ne peut agir différemment sur les deux, et un quatrième code obligerait à figer
l'ordre des contrôles du décodeur dans le contrat.

**La plage est une règle arithmétique, pas une convention de rédaction** : la
catégorie d'un code est `(-code) / 100`. Une liaison qui rencontre un code
qu'elle ne connaît pas le ramène à sa catégorie — un `-203` inconnu reste une
erreur du monde, et se traite comme telle.

C'est une liaison PHP que cette règle sert le plus : `FFI::cdef` n'exécute aucun
préprocesseur, les `#define SCG_ERR_*` du header lui sont invisibles, et elle
recopie les constantes à la main. Elle en aura donc toujours en retard.

## Erreurs

### `scg_last_error`

Arrêté :

- Le pointeur rendu est **valide jusqu'au prochain appel sur le même contexte**.
  Une liaison qui veut garder le message le copie immédiatement. Le rendu d'une
  tuile n'écrit pas dans ce message, mais dans l'emplacement par thread : voir
  « Rendu par tuiles ».
- La chaîne est en UTF-8, terminée par un octet nul, et appartient au moteur.
  L'hôte ne la libère jamais.
- Sans erreur depuis le dernier appel, le message est la chaîne vide, jamais un
  pointeur nul.

Le message est en **anglais**, comme les docstrings exportées, et la section
« Langue » de `CONTRIBUTING.fr.md` le dit. Écarté : le français des autres
messages du projet. Celui-ci est lu par les mêmes personnes que le header, des
auteurs de liaisons et des intégrateurs qui ne parlent pas forcément français.

**Le message n'est jamais localisé** — ni par `LC_MESSAGES`, ni par un paramètre
de langue qu'on ajouterait plus tard. Le défaut connu n'est pas la langue qu'on
choisit, c'est qu'elle varie selon l'environnement : des journaux qu'on ne peut
plus rapprocher d'un poste à l'autre, et un message qu'un intégrateur ne
retrouve pas dans les sources.

### Erreurs sans contexte

L'erreur d'une fonction qui n'a pas de contexte auquel la rattacher se lit dans
un **emplacement par thread**, par `scg_last_error(NULL)`. La couche FFI dispose
de `std` et le tient ; le noyau n'en sait rien. C'est le cas de `scg_create`
quand elle échoue, et ce sera celui des ressources indépendantes du contexte
(A8, plus bas), qu'impose l'étape 7 : un serveur de jeu charge
une carte pour la collision sans jamais créer de contexte de rendu. `scg_create`
rend donc un code et écrit le contexte dans un paramètre de sortie.

Écarté : rendre un pointeur nul sans message, qui perd la cause d'un échec de
création — or c'est l'appel où un intégrateur se trompe de configuration.
Écarté aussi, faire porter son erreur à chaque objet : ça ne dit rien de plus
d'une construction qui a échoué, puisqu'il n'y a pas d'objet.

**L'emplacement est par thread, pas par appel**, et c'est ce qu'une liaison doit
lire ici :

- le message se lit **sur le thread de l'appel qui a échoué, immédiatement après
  lui**. Une fonction `suspend` Kotlin qui appelle le moteur puis lit le message
  après un point de suspension retombe sur un autre thread du pool et reçoit la
  chaîne vide, sans que rien ne le signale ;
- **chaque point d'entrée vide l'emplacement en entrant**, pour qu'un thread
  recyclé ne rende jamais le message d'une tâche précédente.

### Après une panique

Un objet dont un appel a paniqué est **défaillant**. Une panique interrompt le
moteur au milieu d'une mise à jour et rien ne garantit qu'il soit encore cohérent :
tout appel suivant rend `SCG_ERR_FAULTED`, sauf `scg_last_error` et la
destruction, qui restent permises pour que l'hôte lise la cause et libère.

Écarté : réinitialiser l'objet plutôt que le déclarer défaillant. Une panique
signale un défaut du moteur, pas une entrée invalide ; repartir le masquerait, et
aucune réinitialisation n'est fiable depuis un état inconnu.

`SCG_ERR_POISONED`, le nom de ce code dans la 0.0.0, reste défini dans le header
avec la même valeur. Il est déprécié, et ne sera pas retiré en `0.x`.

**L'état défaillant n'est pas observé sur wasm.** La bibliothèque standard de
`wasm32-unknown-unknown` est précompilée en `panic = "abort"` et la chaîne
stable n'y déroule pas la pile : une panique est un trap, que `catch_unwind` ne
rattrape pas, même dans un profil en `panic = "unwind"`. `SCG_ERR_PANIC` et
`SCG_ERR_FAULTED` sont donc des garanties de bureau et d'Android, et une
liaison JavaScript ne compte sur aucun des deux.

Après un trap, l'instance reste appelable, mais l'état du moteur est inconnu :
sa pile n'a pas été rétablie, son allocateur a pu être interrompu. **Elle ne se
réutilise pas**, l'hôte en crée une autre. Le message n'est pas perdu pour
autant : un crochet écrit le texte de la panique, avant le trap, dans
l'emplacement sans contexte. Sur wasm, cet emplacement a une adresse fixe pour
la vie de l'instance — faute de threads, le stockage local y est un statique —,
que l'hôte prend par `scg_last_error(NULL)` avant tout appel risqué et relit
dans la mémoire linéaire après le trap, sans rappeler le module.

## Durées de vie et propriété

- **Contexte.** Créé par `scg_create`, libéré par `scg_destroy`. `scg_destroy(NULL)`
  ne fait rien, comme `free(NULL)`. Un handle détruit puis réutilisé n'est pas
  détecté : c'est une précondition, pas un cas d'erreur.
- **Budget du contexte.** La création reçoit une structure de configuration —
  la première structure publiée, et donc le premier cas de la règle ci-dessus.

  ```c
  typedef struct ScgContextConfig {
      uint32_t max_width;
      uint32_t max_height;
      uint32_t width;
      uint32_t height;
      uint32_t tile_size;
      uint32_t max_triangles;
      uint32_t reserved1;
      uint32_t reserved2;
  } ScgContextConfig;
  ```

  Tout est en `uint32_t` : alignement de quatre octets sur les quatre cibles,
  aucun bourrage interne ni de queue, décalages de 0 à 28 identiques partout.
  Les dimensions ne sont pas en `uint16_t` — mélanger les largeurs rouvrirait la
  question du bourrage pour économiser huit octets, et `stride` est déjà un
  `uint32_t`.

  `tile_size` vaut 32 ou 64. Il figure dès l'étape 0 alors qu'aucune tuile
  n'existe encore : c'est la contrepartie du gel des structures, tout ce que la
  feuille de route réclame entre maintenant ou impose une seconde structure et
  une seconde fonction.

  **Le premier champ réservé est devenu `max_triangles` à l'étape 1**, la
  capacité de triangles soumis par image, et `0` y vaut 16 384. C'est l'usage
  prévu des champs réservés, et il a servi comme annoncé : un hôte de l'étape 0
  qui passe des zéros obtient la capacité par défaut, les décalages n'ont pas
  bougé, et `SCG_ABI_VERSION` non plus. Une soumission au-delà de la capacité rend `SCG_ERR_INVALID_ARGUMENT`
  au moment où elle est faite, jamais pendant le rendu. Écartée : une constante
  du noyau, trop petite pour un niveau détaillé ou trop coûteuse sur téléphone
  selon la valeur qu'on lui donne.

  **`max_width` et `max_height` sont plafonnés à 2048**, au-delà la création rend
  `SCG_ERR_INVALID_ARGUMENT`. Ce n'est pas un confort : les pires cas des formats
  en virgule fixe de [`rust.md`](rust.md) — coordonnées 28.4, bande de garde,
  fonctions de bord en `i64` — sont calculés sur cette borne.

  Tous les tampons propres à l'image — triangles préparés, répartition par
  tuile — sont dimensionnés dès la création, pour la capacité de triangles et
  pour le nombre de tuiles de la résolution maximale. Le clipping n'en a aucun :
  l'intersection d'un triangle avec les cinq plans tient en huit sommets, dont
  le pire cas est prouvé, et ses deux tampons vivent sur la pile de l'appel.
  Couleur et profondeur n'existent pas à l'échelle de l'image : chaque tuile les
  tient sur la pile de l'appel qui la rend. Changer de résolution sous le
  maximum n'alloue rien ; au-delà, c'est une erreur. La mémoire qui dépend de la scène — textures, mipmaps, lightmaps —
  appartient aux ressources, pas au contexte.
- **Tampon de sortie.** Appartient à l'hôte. Le moteur y écrit pendant
  `scg_frame_tile` et `scg_frame_end`, et n'en garde aucune référence au retour
  de chaque appel.
- **Tampons alloués par `scg_buffer_alloc`.** Appartiennent à l'hôte entre
  l'allocation et `scg_buffer_free`. Ils se libèrent par `scg_buffer_free` et par
  rien d'autre : l'allocateur du moteur n'est pas celui de l'hôte, sur bureau non
  plus. La libération est `scg_buffer_free(ptr, len)`, où `len` est exactement
  celle passée à `scg_buffer_alloc`. Elle doit reconstruire à
  l'identique la description de l'allocation — taille et alignement —, d'où la
  longueur en paramètre et l'alignement en **constante de l'ABI**, jamais en
  paramètre. `scg_buffer_alloc(0)` rend un pointeur nul.

  Écarté : un en-tête caché devant chaque bloc, et une table latérale tenue par
  la couche FFI. Le premier ferait garder au moteur de l'état sur un bloc dont
  le principe dit qu'il n'en garde aucune référence ; la seconde imposerait un
  verrou global à une fonction déclarée appelable depuis n'importe quel thread.

  **L'alignement garanti est de 16 octets**, et ce n'est ni pour les vues typées
  de JavaScript — `Uint32Array` n'exige qu'un multiple de quatre — ni pour les
  chemins SIMD de l'étape 9, qui liront le tampon de l'hôte et non un bloc du
  moteur. C'est pour la seule chose qui l'exige vraiment, les chargements alignés
  de SSE. Il est gratuit sur bureau, où l'allocateur système donne déjà 16.
- **Octets de données. Le moteur copie**, pour les textures comme pour les
  maillages et les cartes. L'hôte peut libérer son bloc dès le retour de l'appel,
  et aucune durée de vie ne traverse la frontière. Trois raisons, dans l'ordre :
  un bloc emprunté que l'hôte pourrait modifier entre deux images casserait le
  déterminisme ; un tampon de décodeur n'a aucun alignement garanti ; et l'hôte
  doit pouvoir libérer au retour, faute de quoi toute liaison devrait maintenir
  une durée de vie qu'aucun langage ne lui impose de la même façon.

  Écarté : emprunter le bloc et en documenter la durée de vie. Le coût de la
  copie est payé une fois au chargement, jamais par image ; le coût de l'emprunt
  serait payé par chaque auteur de liaison, et une seule fois de travers suffit.
- **Ressources** (textures, maillages, mondes). Elles sont indépendantes de tout
  contexte, ce qu'impose la collision sans rendu de l'étape 7. Leur mémoire est
  allouée à leur chargement, mipmaps compris. **Une lightmap fournie par l'hôte
  est une ressource comme les autres**, et suit donc cette règle ; celle que le
  noyau calculera à l'étape 5 alloue à l'appel qui la calcule. Détruire une
  ressource encore référencée par un appel de dessin est une précondition, pas
  un cas d'erreur.

  **Détruire une ressource pendant une image est sans effet sur cette image,
  mais pas pour la même raison selon la ressource, et c'est la différence qu'un
  auteur de liaison écrit de mémoire et se trompe.** Une texture survit parce que
  le contexte en garde une référence forte jusqu'à la fin de l'image. Un maillage
  ou une carte survit parce que **plus rien ne les lit après le retour de la
  soumission** : chaque soumission transforme, découpe et prépare immédiatement.
  Les deux se comportent pareil aujourd'hui ; supposer que c'est la même règle
  se paiera le jour où l'une des deux changera.

## Concurrence

Arrêté :

- Le moteur ne crée aucun thread et n'en suppose aucun.
- **Un objet est utilisé par un seul thread à la fois.** Deux contextes distincts
  sont indépendants et peuvent servir chacun sur son thread.
- Les fonctions sans objet — `scg_abi_version`, `scg_buffer_alloc`,
  `scg_buffer_free` — sont appelables depuis n'importe quel thread.
- **Une exception, et une seule : le rendu des tuiles.** Entre le début et la fin
  d'une image, l'hôte peut rendre des tuiles d'index distincts depuis des threads
  distincts. Aucun autre appel sur le contexte n'est permis pendant ce temps.
  Chaque tuile écrit un rectangle disjoint du tampon de l'hôte : le partage du
  tampon entre threads est sûr par construction.
- **L'image ne dépend ni de la taille des tuiles, ni du nombre de threads, ni de
  l'ordre dans lequel les tuiles sont rendues.**

**Une texture immuable se partage en lecture entre contextes et entre threads**,
tranché à l'étape 2. Il en ira de même des autres ressources, mais ce n'est
acquis que pour celles dont la copie à l'entrée est décidée — voir A8.

### Rendu par tuiles

Écrit à l'étape 1. Les deux premières fonctions s'ajoutent à l'ABI publiée, et
la troisième garde sa signature et son sens : `SCG_ABI_VERSION` ne change pas.

```c
int32_t scg_frame_begin(ScgContext *ctx, uint32_t *tile_count);
int32_t scg_frame_tile(ScgContext *ctx, uint32_t index, uint8_t *pixels, uint32_t stride);
int32_t scg_frame_end(ScgContext *ctx, uint8_t *pixels, uint32_t stride);
```

- **La scène se soumet avant `scg_frame_begin`.** Chaque soumission transforme,
  clippe et prépare son triangle immédiatement ; le début scelle la soumission et
  répartit les triangles par tuile, sur le thread de l'appel et en exclusif, puis
  écrit le nombre de tuiles. Écarté : soumettre entre le début et les tuiles, et
  répartir à la première tuile. La répartition est la seule phase qui écrit dans
  un état partagé ; faite paresseusement, elle imposerait un verrou au chemin des
  tuiles, et une panique y tomberait sur un thread quelconque de l'hôte.

  **Le découpage a lieu à la soumission, et pas au début d'image**, parce que
  c'est la seule place qui rende vraie la clause de capacité ci-dessous : un
  triangle clippé en produit jusqu'à six, et le refus doit tomber sur l'appel qui
  déborde, pas sur une image entière déjà soumise. Corollaire : `max_triangles`
  compte des triangles **préparés**, non soumis.
- **Le contexte a deux états.** En enregistrement, il accepte la soumission, la
  caméra et le changement de résolution ; `scg_frame_begin` le fait passer au
  rendu ; `scg_frame_end` le ramène à l'enregistrement, liste de dessin vidée,
  caméra conservée. Rend `SCG_ERR_INVALID_STATE` : une tuile hors du rendu, un
  second début, une soumission ou un changement de résolution pendant le rendu,
  une fin pendant qu'une tuile tourne encore. Un index au-delà du nombre de
  tuiles rend `SCG_ERR_INVALID_ARGUMENT`.
- **Les tuiles se numérotent ligne par ligne**, de gauche à droite puis de haut
  en bas, et c'est contractuel. Celles de la dernière colonne et de la dernière
  ligne sont partielles quand la résolution n'est pas un multiple de
  `tile_size`. Une tuile vide se rend aussi : fond, alpha et post-traitement
  s'écrivent partout. Aucune fonction ne rend le rectangle d'une tuile ; elle
  pourra s'ajouter sans changer la version d'ABI.
- **Un tampon refusé par `scg_frame_end` ne laisse pas l'image ouverte.** Quand
  elle fait le début elle-même, elle vérifie la sortie **avant** : un `stride`
  plus court que la largeur, ou un tampon trop court côté Rust, rend
  `SCG_ERR_INVALID_ARGUMENT` et le contexte reste en enregistrement. Sans cette
  clause, un hôte qui se trompe de tampon se retrouvait en rendu et voyait tout
  appel exclusif suivant rendre `SCG_ERR_INVALID_STATE`, sans rien pour en
  comprendre la cause. Après un début que l'hôte a demandé, en revanche, une
  fin refusée laisse l'image ouverte et il la retente : elle existe, elle.
- **`scg_frame_end` garde sa signature et son sens de l'étape 0.** Elle rend les
  tuiles que l'hôte n'a pas rendues, puis clôt l'image ; appelée sans début, elle
  fait le début elle-même. Un hôte qui n'appelle qu'elle obtient donc toujours
  une image complète, et un hôte écrit contre la 0.0.0 reste valide. Écarté : une
  clôture sans tampon, qui laisserait passer une tuile oubliée en rendant une
  image trouée sans rien dire.
- **Le tampon et le `stride` sont les mêmes** pour toutes les tuiles et pour la
  fin d'une même image. C'est une précondition, comme sa longueur.
- **Une tuile se rend une fois par image.** Un index déjà pris, y compris par un
  appel simultané sur un autre thread, rend `SCG_ERR_INVALID_STATE` : la prise
  est atomique, et c'est elle qui permet à la fin de savoir ce qui reste à
  rendre.
- **Le handle n'est pas `const`.** Chaque tuile écrit dans l'état du contexte, et
  un `const` annoncerait en C une lecture seule qui n'existe pas ; le contrat de
  concurrence est dans la documentation de la fonction.
- **Le message d'erreur d'une tuile va dans l'emplacement par thread**, et se lit
  par `scg_last_error(NULL)` sur le thread de l'appel. Celui du contexte n'est
  écrit que par les appels exclusifs : sa durée de vie reste celle écrite plus
  haut, même pendant le rendu. Écarté : le message du contexte sous verrou,
  dont « valide jusqu'au prochain appel » n'a plus de sens avec plusieurs appels
  simultanés.
- **Une panique dans une tuile rend le contexte défaillant.** Les tuiles déjà
  lancées se terminent ; les suivantes et `scg_frame_end` rendent
  `SCG_ERR_FAULTED`, et le texte de la panique se lit par `scg_last_error(ctx)`
  après la fin. C'est le code de `scg_frame_end` qui dit si l'image est bonne.

  **La garantie ne dépend pas de qui gagne la course.** Le décompte des tuiles
  en vol et la marque de défaillance sont une seule valeur atomique, posée d'une
  seule opération quand une tuile ne revient pas de son rendu : il n'existe donc
  aucun instant où la fin d'image verrait zéro tuile en vol sans voir la tuile
  perdue. En deux valeurs, cet instant existe — quelques instructions entre le
  décompte relâché et la défaillance posée —, et c'est exactement le cas que
  cette clause sert à couvrir.
- **Chaque tuile tient sa couleur et sa profondeur sur la pile de l'appel.** Le
  thread qui appelle `scg_frame_tile` ou `scg_frame_end` doit disposer d'au
  moins 128 Ko de pile. Écartés : des tampons par tuile dans le contexte, jusqu'à
  32 Mo à la résolution maximale, et des tampons par thread, qui imposeraient un
  nombre de threads dans la configuration et un index de thread à chaque appel.
- **La profondeur ne survit pas à la tuile.** Rien ne la relit après l'image ;
  l'interrogation de la scène de l'étape 8 passe par la géométrie.
- **Un hôte sans threads appelle les tuiles dans une boucle**, ou `scg_frame_end`
  seule. C'est le cas du web.

## Tampon de sortie

- **Dimensions : la résolution interne**, jamais celle de l'écran. La mise à
  l'échelle entière appartient à l'hôte.
- **`stride` est exprimé en pixels**, et vaut au moins la largeur. Le tampon
  fait au moins `stride × hauteur` pixels. Le moteur ne reçoit pas sa longueur et
  ne peut pas la vérifier : c'est une précondition documentée.
- **La résolution interne se change sans recréer le contexte** (étape 3), et
  sans allocation sous le maximum fixé à la création. **C'est à l'hôte de faire
  suivre son tampon** : le moteur n'en reçoit pas la longueur et ne peut pas
  s'apercevoir qu'il est resté trop court. Voir « Étape 3 ».
- **Format des pixels** : quatre octets par pixel, dans l'ordre **R, G, B, A en
  mémoire**, alpha toujours écrit à 255. C'est l'ordre natif d'`ImageData` dans
  un navigateur, et celui que `Bitmap.Config.ARGB_8888` a réellement en mémoire
  sur Android — son nom vient de l'entier `0xAARRGGBB` de la classe `Color`, pas
  de la disposition, et un hôte qui passerait par `setPixels(int[])` rendrait une
  image aux rouges et aux bleus échangés, sans erreur ni avertissement. Sous
  Windows, GDI le lit tel quel avec `BI_BITFIELDS` et les masques
  correspondants : **aucune cible ne permute**.

  Écarté : l'ordre BGRA de GDI, qui imposerait une permutation par pixel sur wasm
  et sur Android — les deux cibles où la bande passante est le vrai plafond —
  pour épargner la seule qui n'en a pas besoin.

  **L'alpha est écrit, jamais laissé indéfini.** Un octet non initialisé donne un
  rendu troué dans le navigateur, seule cible où ce canal est réellement
  composité. À 255, prémultiplié et non prémultiplié sont identiques : les hôtes
  annoncent l'opacité — `setHasAlpha(false)` sur Android, contexte en
  `{alpha: false}` sur le web — pour que le compositeur saute le mélange.
- **Le niveau de qualité du filtrage est un paramètre de contexte** (étape 2) :
  tramage ordonné des coordonnées par défaut, bilinéaire au-dessus. Le motif de
  tramage est une table fixe du noyau, indexée par la position du pixel dans
  l'image : il n'a pas de graine, et rien de ce qu'il produit ne dépend de
  l'hôte.

  **Il règle l'échantillonnage des textures, et lui seul.** Une lightmap se lit
  toujours en bilinéaire, quel que soit ce réglage : elle est grossie d'un
  facteur seize à soixante-quatre par construction, et sans filtrage chaque
  luxel deviendrait un pavé franc de seize pixels de côté. Le tramage n'y
  aiderait pas — il ne toucherait que la frontière entre deux luxels, et y
  remplacerait un escalier par du bruit régulier.

  **Elle porte en revanche une chaîne de mipmaps, et son niveau se choisit
  comme celui d'une texture.** Cette clause disait l'inverse jusqu'à l'étape 3,
  au motif qu'une lightmap n'est jamais réduite ; c'est vrai en intérieur, où
  un mur de quatre mètres portant seize luxels reste agrandi jusqu'à
  quatre-vingts mètres environ. Ce ne l'est plus dès qu'une surface s'éloigne
  au-delà, ce qu'un décor à ciel ouvert fait par construction — et un niveau
  zéro forcé y scintillerait exactement comme une texture sans chaîne. Le coût
  est un tiers de mémoire sur une ressource qui en occupe peu, et un choix de
  niveau par segment de seize pixels.
- **Le post-traitement** — une affine par canal, gain puis décalage, puis le
  gamma — s'applique pendant l'écriture de chaque tuile dans le tampon de l'hôte.
  Il n'y a pas de passe plein écran, et donc rien qui lise les pixels voisins.

  **Tout se compose en une table de 256 entrées par canal**, remplie au
  réglage : le pixel ne paie que trois lectures. L'affine vient avant le gamma,
  et gain et gamma commutent à reparamétrisation près — `(a·x)^γ = a^γ·x^γ` —, si
  bien qu'aucune image n'est perdue d'un ordre à l'autre ; ce qui change est ce
  que le nombre veut dire, et l'ordre publié est celui qu'on lit « je corrige
  la source, puis j'encode pour l'écran ». **Il ne se change donc jamais** : un
  hôte qui a réglé ses paramètres obtiendrait une autre image, sans qu'aucune
  version ne l'en prévienne.

  Le gamma est celui de l'écran, dans le sens où **2,2 éclaircit**. Le décalage,
  lui, déplace le noir et le blanc — c'est ce qui le distingue du gain, qui ne
  peut ni lever l'un ni abaisser l'autre. La courbe n'écrit pas l'alpha :
  celui-ci reste forcé à 255 au seul endroit que les deux chemins de sortie
  traversent.

  Écarté : le tonemapping. Le tampon est en huit bits par canal, sans dynamique
  étendue, donc il n'y a aucune plage à compresser — ce qu'on appellerait ainsi
  ne serait qu'une courbe de contraste, qui n'apporte rien que le gain, le
  décalage et le gamma ne donnent déjà et qui obligerait à graver un pivot.

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
- **`scg_abi_version` rend un `uint32_t`, qu'une liaison peut recevoir signé.**
  En JNI il arrive en `jint`, sur wasm il revient en `i32` côté JavaScript : la
  comparaison se fait sur la valeur non signée, et la version reste loin de 2³¹.
- **À trancher — A10** : la dépréciation. Une fonction remplacée reste exportée et
  documentée comme dépréciée. Reste à dire si elle est un jour retirée.
  Recommandation : jamais en `0.x` ; la question se rouvre au gel de l'ABI en 1.0.

## Fonctions

### Étape 0

Les sept points d'entrée de la feuille de route, dans leur forme arrêtée.

```c
uint32_t    scg_abi_version(void);
int32_t     scg_create(const ScgContextConfig *config, ScgContext **out);
void        scg_destroy(ScgContext *ctx);
int32_t     scg_frame_end(ScgContext *ctx, uint8_t *pixels, uint32_t stride);
const char *scg_last_error(const ScgContext *ctx);
uint8_t    *scg_buffer_alloc(size_t len);
void        scg_buffer_free(uint8_t *ptr, size_t len);
```

`scg_last_error` accepte `NULL` : elle rend alors le message de l'emplacement par
thread, celui des fonctions qui n'ont pas de contexte auquel se rattacher.

À l'étape 0, `scg_frame_end` rendait un triangle en dur dans l'image entière : il
n'y avait pas encore de scène à soumettre. Ce triangle est pourtant rempli par
les fonctions de bord en virgule fixe et la règle top-left définitives — c'est le
premier remplissage. Les tuiles et la projection sont arrivées depuis, sans
qu'aucune de ces sept signatures change.

### Étape 1

```c
int32_t scg_frame_begin(ScgContext *ctx, uint32_t *tile_count);
int32_t scg_frame_tile(ScgContext *ctx, uint32_t index, uint8_t *pixels, uint32_t stride);
int32_t scg_set_camera(ScgContext *ctx, const ScgCamera *camera);
int32_t scg_submit(ScgContext *ctx, const ScgMat4 *model,
                   const ScgVertex *vertices, uint32_t vertex_count,
                   const ScgTriangle *triangles, uint32_t triangle_count);
```

Le contrat des deux premières est dans « Rendu par tuiles », celui des deux
autres dans « Soumettre une scène ». Aucune signature publiée n'a changé, et
`SCG_ABI_VERSION` reste à 1.

**Il n'y a plus de scène en dur.** Un contexte auquel rien n'a été soumis rend
une image de fond, sans erreur : une scène vide est une scène, pas un appel
fautif. C'est le seul changement de rendu de l'étape, et il ne touche que les
hôtes qui ne soumettaient rien — jusqu'ici, tous.

### Soumettre une scène

**La matrice de soumission est celle du modèle**, qui porte l'objet dans le
monde ; le moteur y compose la vue de sa caméra. Écartée : la modèle-vue, qui
obligerait chaque liaison à inverser elle-même la pose de la caméra, donc à
normaliser un quaternion par sa propre bibliothèque mathématique — et deux
liaisons ne rendraient plus la même image. C'est la clause « ne rien calculer »
appliquée à la seule chose qui, sans elle, aurait dû l'être partout.

```c
typedef struct ScgVertex { float x, y, z; } ScgVertex;

typedef struct ScgTriangle {
    uint32_t i0, i1, i2;
    uint8_t  r, g, b, a;
} ScgTriangle;

typedef struct ScgCamera {
    float position[3];
    float orientation[4];
    float fov_y;
    float near_plane;
} ScgCamera;

typedef struct ScgMat4 { float m[16]; } ScgMat4;
```

- **Aucun champ réservé dans ces quatre structures.** L'étape 2 apporte une
  structure de sommet nouvelle et une fonction nouvelle, ce que la règle
  d'extension prévoit ; un champ qui dormirait en attendant serait un pari sur
  sa forme. Les décalages sont 0/4/8 pour `ScgVertex`, 0/4/8/12/13/14/15 pour
  `ScgTriangle`, 0/12/28/32 pour `ScgCamera` — identiques sur les quatre
  cibles, sans un octet de bourrage.
- **La couleur est portée par le triangle**, en quatre octets nommés, dans
  l'ordre mémoire des pixels. Écarté : un `uint32_t`, qui rouvrirait à chaque
  liaison la question de l'ordre des canaux — celle-là même que le format des
  pixels tranche plus haut.
- **Les indices ne sont pas facultatifs.** Un hôte sans géométrie indexée écrit
  `0, 1, 2` puis `3, 4, 5` ; le moteur n'a pas deux chemins à tenir, et le lot
  reste un seul franchissement de la frontière.
- **Un lot est accepté ou refusé en entier.** Un indice au-delà de
  `vertex_count`, une coordonnée non finie, un dépassement de capacité :
  l'image reste exactement ce qu'elle était. Un lot à demi accepté laisserait
  un mur dont il manque la moitié, sans que l'hôte sache où la coupure est
  tombée. Un triangle qui ne se projette pas — derrière le plan proche, hors de
  la bande de garde — disparaît sans erreur : c'est une donnée.
- **`ScgMat4` est une 4×4 rangée par colonnes**, `m[colonne × 4 + ligne]`, dont
  la dernière ligne — `m[3]`, `m[7]`, `m[11]`, `m[15]` — vaut exactement
  `0, 0, 0, 1`, sans quoi `SCG_ERR_INVALID_ARGUMENT`. Une 4×4 parce que c'est
  ce que tout hôte a sous la main ; la dernière ligne vérifiée parce que le
  moteur inverse la pose de la caméra sans division, ce qui n'est juste que
  pour une transformation rigide — une matrice à perspective y passerait pour
  telle.
- **Le quaternion se range `x, y, z, w`**, la partie réelle en dernier :
  l'identité est `{0, 0, 0, 1}`. Il n'est pas exigé unitaire, le moteur le
  normalise.
- **Avec l'orientation neutre, la caméra regarde le +X du monde**, le zénith
  vers le haut de l'écran et le −Y vers la droite. Le monde est en main droite,
  Z en haut.
- **`near_plane` et non `near`** : `windows.h` définit encore `near` et `far`
  comme macros vides, héritées de la mémoire segmentée 16 bits, et un champ de
  ce nom disparaîtrait dans toute unité de compilation qui l'inclut d'abord.
- **La caméra vaut pour l'image entière.** `scg_set_camera` et `scg_submit`
  sont refusés entre le début et la fin d'une image, comme toute écriture dans
  l'état du contexte.

  **Et `scg_set_camera` est refusé dès qu'un triangle de l'image en cours est
  retenu**, par `SCG_ERR_INVALID_STATE` : chaque soumission projette
  immédiatement, si bien qu'une caméra changée au milieu laisserait dans la
  même image deux espaces écran, chacun correctement rasterisé et l'ensemble
  faux. Aucune reprojection n'est possible en aval, la géométrie source n'étant
  pas conservée. Le refus porte sur les triangles **retenus** et non sur les
  lots reçus : un lot dont pas un triangle n'a survécu à la projection ne
  laisse rien dans l'image, donc rien à mêler.

  Cette clause a toujours été la règle ; jusqu'à la 0.2.1 le moteur ne la
  tenait pas, et l'appel passait en rendant une image que rien ne décrit.
  `SCG_ABI_VERSION` ne bouge pas : un hôte conforme ne fait jamais cet appel,
  et n'observe donc aucune différence — le même raisonnement qu'au passage du
  premier champ réservé à `max_triangles`.
- **La liste de dessin se vide à la fin de l'image**, pas à son début : un hôte
  qui soumet dès le retour de `scg_frame_end` doit retrouver sa scène.

### Étape 2

```c
int32_t scg_texture_load(const ScgTextureDesc *desc, const uint8_t *pixels,
                         size_t len, ScgTexture **out);
void    scg_texture_destroy(ScgTexture *texture);
int32_t scg_submit_textured(ScgContext *ctx, const ScgMat4 *model,
                            const ScgVertexUv *vertices, uint32_t vertex_count,
                            const ScgTriangle *triangles, uint32_t triangle_count,
                            const ScgTexture *texture);
int32_t scg_set_filter(ScgContext *ctx, uint32_t filter);
```

```c
typedef struct ScgVertexUv { float x, y, z, u, v; } ScgVertexUv;

typedef struct ScgTextureDesc {
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t reserved0;
    uint32_t reserved1;
    uint32_t reserved2;
} ScgTextureDesc;
```

Quatre fonctions ajoutées, deux structures nouvelles, trois constantes :
`SCG_ABI_VERSION` reste à **1**.

- **Un seul format, `SCG_TEXTURE_FORMAT_RGBA8`, qui vaut 1 et non 0.** Une
  description laissée à zéro est ainsi refusée plutôt qu'interprétée. Quatre
  octets par texel dans l'ordre mémoire des pixels de sortie, lignes jointives,
  longueur exactement `width × height × 4`. Écartés : le 565, dont la moyenne
  des mipmaps perd la précision et qui ferait deux boucles ; le RGB sur trois
  octets, dont l'accès n'est pas aligné ; la palette avec colormap, que
  « couleurs directes » exclut.
- **Les deux côtés sont des puissances de deux**, indépendamment l'un de
  l'autre, de 1 à 2048. C'est ce qui permet au repli des coordonnées d'être un
  masque, sans division ni comparaison par texel.
- **Le moteur copie le bloc.** L'hôte peut le libérer au retour de l'appel.
  Trois raisons, dans l'ordre : un bloc emprunté modifiable entre deux images
  casserait le déterminisme ; un tampon de décodeur n'a aucun alignement
  garanti ; et aucune durée de vie ne traverse alors la frontière. C'est la
  réponse à **A8** pour les textures ; les mondes restent ouverts, leur
  sémantique de mutation n'étant pas décidée.
- **Les mipmaps sont engendrés au chargement**, jusqu'à 1×1, par moyenne des
  texels. Rien n'est exposé : l'hôte ne fournit pas ses niveaux, et un niveau
  engendré plus tard violerait « aucune allocation par image ».
- **Une texture n'appartient à aucun contexte** : `scg_texture_load` et
  `scg_texture_destroy` n'en prennent pas, et leur message va dans
  l'emplacement par thread. Immuable une fois chargée, elle se partage en
  lecture entre contextes et entre threads — ce qui referme aussi la question
  du partage de ressources laissée ouverte plus haut.
- **Détruire une texture qu'une image référence encore est sans effet sur
  elle** : le moteur en garde une référence jusqu'à la fin de cette image.
- **La texture vaut pour le lot entier**, et non pour chaque triangle :
  `ScgTriangle` est publiée et ne peut plus gagner de champ, et une surface
  continue se soumet de toute façon en un seul franchissement.
- **`u` et `v` sont en texels, jamais normalisées**, et bornées à 16384 en
  valeur absolue. C'est le seul choix qui rende le bornage vérifiable sur le
  tableau de sommets seul : normalisées, la borne dépendrait de la texture avec
  laquelle le lot est finalement dessiné. Au-delà, ou non finie, une coordonnée
  refuse le lot entier.
- **Le niveau de filtrage est un réglage du contexte**, pas un paramètre de
  soumission : `SCG_FILTER_DITHER`, qui vaut **0**, et `SCG_FILTER_BILINEAR`,
  qui vaut 1. Zéro pour le défaut, à l'inverse du format de texture et pour la
  raison inverse — un contexte qu'on ne configure pas doit rendre ce que le
  moteur rend par défaut, alors qu'une description laissée à zéro doit être
  refusée.
- **Une valeur de filtrage inconnue est refusée**, jamais rabattue sur le
  défaut. C'est ce qui rend l'ajout d'un filtrage compatible : une liaison
  écrite contre une version ultérieure reçoit `SCG_ERR_INVALID_ARGUMENT` au
  lieu d'une image filtrée autrement qu'elle ne le croit.
- **Les deux filtrages s'excluent**, et `scg_set_filter` est refusé entre
  `scg_frame_begin` et `scg_frame_end` : les tuiles d'une image se rendent
  depuis des threads que le moteur ne connaît pas, et un filtre changé au
  milieu laisserait une part de l'image lue autrement que le reste.
- **Aucun code d'erreur nouveau.** La plage des données reste fermée jusqu'à
  l'étape 4 : `SCG_ERR_INVALID_FORMAT` nomme un format de fichier versionné,
  qu'un bloc de pixels brut n'a pas. Ce qui grossit, ce sont les messages.

### Étape 3

```c
int32_t scg_submit_lit(ScgContext *ctx, const ScgMat4 *model,
                       const ScgVertexUv2 *vertices, uint32_t vertex_count,
                       const ScgTriangle *triangles, uint32_t triangle_count,
                       const ScgTexture *texture, const ScgTexture *lightmap);
int32_t scg_set_overbright(ScgContext *ctx, uint32_t shift);
int32_t scg_set_fog(ScgContext *ctx, uint8_t r, uint8_t g, uint8_t b,
                    float start, float end);
int32_t scg_clear_fog(ScgContext *ctx);
int32_t scg_set_lights(ScgContext *ctx, const ScgLight *lights, uint32_t count);
int32_t scg_set_resolution(ScgContext *ctx, uint32_t width, uint32_t height);
int32_t scg_set_grade(ScgContext *ctx, const ScgGrade *grade);
int32_t scg_clear_grade(ScgContext *ctx);
```

```c
typedef struct ScgGrade {
    float    gamma;
    float    gain_r;
    float    gain_g;
    float    gain_b;
    float    offset_r;
    float    offset_g;
    float    offset_b;
    uint32_t reserved0;
    uint32_t reserved1;
} ScgGrade;
```

```c
typedef struct ScgLight {
    float   x, y, z;
    float   radius;
    uint8_t r, g, b;
    uint8_t _reserved;
} ScgLight;
```

```c
typedef struct ScgVertexUv2 { float x, y, z, u, v, u2, v2; } ScgVertexUv2;
```

Huit fonctions ajoutées, trois structures nouvelles, aucune constante :
`SCG_ABI_VERSION` reste à **1**.

- **Une structure de sommet nouvelle, et non deux champs de plus sur
  `ScgVertexUv`.** Celle-ci est publiée et ne s'élargit pas : un hôte qui
  n'éclaire rien continue d'écrire cinq flottants par sommet. Vingt-huit
  octets, offsets 0 à 24 sur toutes les cibles, sans bourrage.
- **`texture` peut être nul, `lightmap` non**, et c'est le seul point de cette
  section qu'une liaison écrite de mémoire se trompera. Un mur uni éclairé est
  la surface la plus courante d'un décor : il rend `couleur × lightmap`, la
  couleur de chaque triangle tenant lieu de texel. Une `lightmap` nulle, elle,
  est `SCG_ERR_NULL` — un lot qui n'en a pas n'a rien à faire sur ce chemin, et
  `scg_submit` ou `scg_submit_textured` le rend. C'est une différence avec
  cette dernière, où un pointeur nul est une erreur.
- **Les deux jeux de coordonnées sont indépendants**, tous deux en texels de
  leur propre image et bornés de la même façon. C'est la raison d'être du
  second : une surface répète sa texture plusieurs fois et étire sa lightmap
  une seule fois sur toute son étendue.
- **Le sur-éclairement vaut zéro par défaut**, ce qui rend le texel intact sous
  pleine lumière et jamais plus clair. C'est le réglage juste et une scène
  terne — une lightmap réelle n'atteint le blanc nulle part. Un décalage de un
  ou deux double ou quadruple la valeur combinée, avec saturation. Toute autre
  valeur que 0, 1 ou 2 rend `SCG_ERR_INVALID_ARGUMENT`.
- **Les huit fonctions de l'étape sont refusées pendant une image**, par
  `SCG_ERR_INVALID_STATE`, y compris celles qui effacent un réglage. Elles
  écrivent toutes dans un état que les tuiles lisent depuis des threads que le
  moteur ne connaît pas, et un réglage changé au milieu laisserait une part de
  l'image rendue autrement que le reste. C'est la raison de `scg_set_filter`,
  et elle ne souffre pas d'exception.
- **Ce que chacune exige de ses arguments**, faute de quoi
  `SCG_ERR_INVALID_ARGUMENT` et le contexte garde ce qu'il avait :

  | Fonction | Domaine |
  |---|---|
  | `scg_set_fog` | `start` et `end` finis, `start` positif ou nul, `end` strictement supérieur à `start` |
  | `scg_set_lights` | au plus **huit** lumières ; chaque position finie ; chaque rayon fini et strictement positif ; chaque `_reserved` nul |
  | `scg_set_grade` | `gamma` fini dans `]0, 8]` ; chaque gain fini dans `[0, 4]` ; chaque décalage fini dans `[-1, 1]` ; les deux champs réservés nuls |
  | `scg_clear_fog`, `scg_clear_grade` | rien : elles n'ont pas d'argument, et effacer ce qui n'était pas réglé n'est pas une erreur — un hôte qui éteint le brouillard en fin de niveau n'a pas à se souvenir s'il l'avait allumé |

  **Un rayon nul est refusé et non ignoré** : il n'éclaire rien et ferait
  diviser par zéro. **`NaN` se teste nommément**, avant les comparaisons de
  bornes, qui sont fausses dans les deux sens et le laisseraient passer.
- **Le plafond de huit lumières est une constante du moteur**, et il ne se
  configure pas. L'atténuation se calcule par sommet, à la soumission : chaque
  lumière de plus est un parcours de plus sur chaque sommet soumis, et c'est le
  budget d'un cœur de téléphone qui fixe la borne. Une neuvième lumière refuse
  l'appel entier plutôt que d'être ignorée en silence — un hôte qui en place
  neuf doit savoir laquelle manque, et il n'y a pas de réponse à cette question.
- **Le brouillard ne demande pas à l'hôte d'effacer avec sa couleur.** Il
  s'applique pendant la recopie d'une tuile, où la profondeur est déjà sous la
  main, et un pixel qu'aucun triangle n'a peint est infiniment lointain : le
  fond en prend donc la couleur de lui-même. Effacer séparément redessinerait
  la couture d'horizon au moindre écart d'arrondi entre les deux chemins, et
  cet écart d'un seul niveau ne se voit sur aucune image avant qu'un décor
  entier soit construit dessus.
- **Trois canaux de couleur, pas quatre.** Le tampon de sortie est opaque par
  contrat : un alpha sur la couleur du brouillard serait une valeur que le
  moteur ignore, et un hôte se demanderait légitimement ce qu'elle fait.
- **La couleur d'une lumière tient en trois octets plus un champ réservé**, et
  non en trois flottants. Trois `float` normalisés auraient donné vingt-huit
  octets sans un seul bourrage, mais c'aurait été le seul endroit de l'ABI où
  une couleur n'est pas en octets : un hôte qui a `0xC0B090` sous la main
  devrait diviser par 255. Le quatrième octet n'est pas un alpha — une lumière
  s'ajoute, elle ne se mélange pas — mais un champ réservé au sens de la
  clause d'extensibilité, donc **nul obligatoire**.
- **L'atténuation se calcule par sommet, à la soumission.** Des lumières
  réglées après un lot ne l'éclairent pas, ce qui permet à une torche portée
  par le joueur d'éclairer un décor sans que le décor soit resoumis — à
  condition de la régler avant lui. La conséquence à connaître : un mur de
  deux triangles rend un dégradé entre ses coins et non un halo, et une source
  qui s'y déplace demande un mur découpé en panneaux.
- **Dès qu'une lumière est réglée, elle vaut pour toute la scène.** Une surface
  hors de portée s'éteint au lieu de garder sa couleur : sinon la frontière
  entre une surface qu'une lumière atteint et une qu'elle n'atteint pas serait
  une marche franche au milieu d'une géométrie continue.
- **La résolution interne se change par `scg_set_resolution`**, sous le
  maximum reçu à la création, et rien ne s'alloue : tout ce qu'une image
  consomme a été dimensionné sur ce maximum, et c'est la raison pour laquelle
  il ne se relève pas. Au-delà, ou sur une dimension nulle,
  `SCG_ERR_INVALID_ARGUMENT`, et le contexte garde la résolution qu'il avait.
  Refusée **en outre dès qu'un triangle de l'image est retenu**, comme
  `scg_set_camera` et pour la même raison — c'est la seule des huit à l'être.

  **Le tampon de l'hôte ne suit pas.** Après une hausse, un tampon laissé à sa
  taille d'avant est trop court, et le moteur ne peut pas s'en apercevoir — il
  ne reçoit pas sa longueur. Le nombre de tuiles change aussi : un hôte
  rappelle `scg_frame_begin` au lieu de réutiliser le compte de l'image
  précédente. Écartée : une fonction qui rendrait la résolution courante ;
  l'hôte vient de la fixer, et elle pourra s'ajouter sans changer la version
  d'ABI.
- **La courbe de sortie se règle par `scg_set_grade`**, en un seul appel :
  `ScgGrade` porte le gamma, trois gains et trois décalages, trente-six octets,
  décalages de 0 à 32 identiques sur les quatre cibles. Un appel plutôt que
  trois, parce que les réglages se composent en une seule table par canal :
  séparés, chacun la reconstruirait entière. `scg_clear_grade` revient au
  neutre, et l'appeler sur un contexte sans courbe n'est pas une erreur.
- **`ScgGrade` porte deux champs réservés, et la clause qui décide de ce qu'ils
  pourront tenir.** Un champ réservé doit avoir **zéro pour valeur neutre**,
  puisque l'extensibilité promet qu'un hôte écrit avant qu'il serve obtienne le
  défaut en passant des zéros. Un réglage dont le neutre est un — un gain de
  plus, une saturation — n'y entre donc pas tel quel : seuls les réglages
  **additifs** le peuvent. C'est pour cette raison que les trois décalages
  figurent dans la structure **dès sa publication** plutôt que d'attendre un
  besoin démontré : trois champs n'auraient jamais tenu dans deux réservés, et
  il aurait fallu une seconde structure et une seconde fonction, pour toujours.
- **L'affine précède le gamma, et cet ordre ne se change jamais.** Gain et
  gamma commutent à reparamétrisation près — `(a·x)^γ = a^γ·x^γ` —, donc aucune
  image n'est hors d'atteinte d'un ordre à l'autre ; ce que l'ordre fixe est ce
  que le nombre veut dire. Le changer déplacerait l'image d'un hôte qui a réglé
  ses paramètres, sans qu'aucune version ne l'en prévienne.
- **Aucun code d'erreur nouveau**, six messages de plus.

### Étape 4

**Les douze fonctions sont exposées**, et les sept accesseurs que la carte
réclame en propre avec elles. Ce qui suit est le contrat auquel elles se
conforment, figé avant le premier décodeur pour la raison dite en tête de
document. Les dispositions binaires elles-mêmes sont dans `docs/rust.md`,
section « Formats de fichier » : elles n'appartiennent pas à l'ABI, qui ne voit
qu'un bloc d'octets.

```c
int32_t scg_mesh_load(const uint8_t *bytes, size_t len, struct ScgMesh **out);
void    scg_mesh_destroy(struct ScgMesh *mesh);
int32_t scg_mesh_texture_count(const struct ScgMesh *mesh, uint32_t *out);
int32_t scg_mesh_texture_name(const struct ScgMesh *mesh, uint32_t slot,
                              char *buf, size_t cap, size_t *out_len);
int32_t scg_mesh_triangle_count(const struct ScgMesh *mesh, uint32_t *out);
int32_t scg_submit_mesh(struct ScgContext *ctx, const struct ScgMat4 *model,
                        const struct ScgMesh *mesh,
                        const struct ScgTexture *const *textures,
                        uint32_t texture_count);
```

Les six fonctions du monde sont les mêmes, `world` pour `mesh` et `material`
pour `texture` : `scg_world_load`, `scg_world_destroy`,
`scg_world_material_count`, `scg_world_material_name`,
`scg_world_triangle_count`, `scg_submit_world`. La symétrie est voulue — deux
ressources chargées depuis un bloc n'ont aucune raison de se manipuler
autrement, et une liaison écrite pour l'une se relit pour l'autre.

**Une carte porte en plus ce qu'elle seule a** : des lumières statiques et des
entités, que sept fonctions donnent à lire.

```c
int32_t scg_world_light_count(const struct ScgWorld *world, uint32_t *out);
int32_t scg_world_light(const struct ScgWorld *world, uint32_t index,
                        struct ScgLight *out);
int32_t scg_world_entity_count(const struct ScgWorld *world, uint32_t *out);
int32_t scg_world_entity_ids(const struct ScgWorld *world, uint32_t index,
                             uint32_t *id, uint32_t *cell);
int32_t scg_world_entity_pose(const struct ScgWorld *world, uint32_t index,
                              float *out);
int32_t scg_world_entity_class(const struct ScgWorld *world, uint32_t index,
                               char *buf, size_t cap, size_t *out_len);
int32_t scg_world_entity_data(const struct ScgWorld *world, uint32_t index,
                              uint8_t *buf, size_t cap, size_t *out_len);
```

- **`scg_world_light` remplit une structure que l'hôte possède**, et c'est la
  seule de l'ABI à être écrite par le moteur. Le principe qui l'interdisait vise
  une structure *rendue par valeur ou par pointeur vers la mémoire du moteur* :
  écrire dans un tampon de l'appelant est ce que fait déjà
  `scg_mesh_texture_name`. La forme est celle que l'hôte redonne à
  `scg_set_lights`, ce qui est tout ce qu'il en fait ; la lui faire reconstruire
  champ par champ aurait coûté trois appels pour respecter la lettre d'un
  principe qui dit autre chose. L'octet réservé est écrit nul.

  Écarté : une `ScgEntity` symétrique. Une pose n'a pas de structure publiée à
  réutiliser, et en inventer une la figerait pour toujours au profit d'un seul
  accesseur — d'où les sept flottants de `scg_world_entity_pose`, trois de
  position puis quatre d'un quaternion normalisé.
- **Les identifiants d'une entité sont ceux de l'éditeur**, jamais des index :
  `index` désigne un rang dans ce que `scg_world_entity_count` a rendu, `id` et
  `cell` sont stables d'un chargement à l'autre.
- **La classe se lit en deux temps**, comme un nom d'emplacement, terminateur
  compris. **Les octets d'une entité aussi, mais sans terminateur** : ce sont des
  octets, pas une chaîne, et le moteur les a copiés sans en lire un.
- **Un `index` au-delà de son compte rend `SCG_ERR_INVALID_ARGUMENT`** et n'écrit
  rien : le fichier est bon, c'est l'appel qui sort des bornes.

Arrêté :

- **Le chargement ne prend pas de contexte**, comme celui d'une texture : une
  ressource est indépendante de tout contexte, ce qu'impose la collision sans
  rendu de l'étape 7. L'erreur va dans l'emplacement par thread, et se lit par
  `scg_last_error(NULL)`.
- **Pas de structure de description en entrée.** Un fichier se décrit lui-même ;
  une description doublerait son en-tête et ouvrirait un désaccord entre les
  deux, qu'il faudrait alors arbitrer dans un sens ou dans l'autre.
- **Pas de structure rendue à l'hôte**, conformément aux principes : ce que la
  ressource dit d'elle-même passe par des accesseurs scalaires, sur le modèle de
  `scg_frame_begin`. Un `ScgMeshInfo` en un seul appel aurait été plus commode et
  n'existe pas.
- **Les textures se lient à la soumission, jamais à la ressource.** Le fichier
  porte des noms d'emplacements ; l'hôte les découvre par les deux accesseurs,
  charge ce qu'il veut avec ses propres fichiers, et passe un tableau de handles
  dans l'ordre des emplacements. C'est ce qui permet de dessiner la même
  ressource avec deux habillages, ce qui garde la ressource indépendante du
  contexte, et ce qui tient la promesse que le moteur n'ouvre rien sans exiger de
  callback. Une entrée nulle vaut « sans texture », comme pour un lot éclairé.

  Écarté : une liaison par position, sans noms. Elle fonctionne, mais l'hôte ne
  sait alors pas *quoi* charger, et cette connaissance émigre dans un second
  fichier que le format devrait décrire de toute façon.
- **`texture_count` est vérifié égal** au nombre d'emplacements de la ressource,
  jamais « au moins ». Un tableau plus long est une faute d'appel :
  `SCG_ERR_INVALID_ARGUMENT`.
- **Le nom se lit en deux temps** : `buf` nul avec `cap` à zéro rend la longueur
  nécessaire dans `out_len`, puis un second appel remplit. Le tampon rendu est
  terminé par un octet nul, et `out_len` ne le compte pas. `out_len` est
  obligatoire dans les deux temps.

  **Un `cap` non nul mais trop court rend `SCG_ERR_INVALID_ARGUMENT` et n'écrit
  rien, `out_len` compris.** L'appel de mesure est le seul chemin vers la
  longueur. Écarté : remplir quand même `out_len`, ce qui épargnerait un appel à
  une liaison qui aurait deviné trop court — ce serait le seul paramètre de
  sortie de toute l'ABI écrit sur un chemin d'erreur, et une liaison finirait par
  en dépendre. Un `slot` au-delà du compte rend le même code : le fichier est
  bon, c'est l'indice qui ne l'est pas.
- **Le compte de triangles existe** parce que l'hôte doit dimensionner
  `max_triangles` à la création du contexte, avant d'avoir soumis quoi que ce
  soit. Sans lui, la seule façon de connaître ce nombre serait d'essayer.
- **Une soumission qui dépasse la capacité restante est refusée en entier**,
  jamais groupe par groupe. Une image partiellement soumise serait une image
  fausse sans erreur.

  Le dépassement ne se voit pas à l'entrée : un triangle soumis consomme
  plusieurs places quand le découpage le multiplie, si bien qu'un groupe échoue
  après que d'autres ont été posés. Le moteur marque donc ses tampons à l'entrée
  et les ramène à cette marque au refus. Écarté : refuser d'avance sur le pire
  cas du découpage, qui rendrait `scg_mesh_triangle_count` inutilisable pour
  dimensionner le contexte — ce pour quoi il existe.
- **Le rendu du monde ne prend pas de cellule de départ**, puisqu'il ne fait
  aucune élimination : il dessine toutes les surfaces de toutes les cellules. La
  traversée par portails est l'étape 5 et ajoutera sa propre fonction. Un
  paramètre qui ne sert pas encore est un paramètre dont le sens changera, ce qui
  imposerait alors une version d'ABI.
- **Aucune allocation pendant une soumission.** Le tableau de textures est lu sur
  place, jamais recopié dans un tampon intermédiaire : c'est le chemin le plus
  banal de l'étape, et c'est celui où une allocation par image passerait
  inaperçue.
- **Aucun changement de `SCG_ABI_VERSION`** : douze fonctions nouvelles, deux
  types opaques nouveaux, trois codes d'erreur déjà réservés. Aucune signature
  publiée, aucune structure, aucune précondition ne bouge.

### Étapes suivantes

Prévisionnel. Ce qui doit être exposé est arrêté par la feuille de route ; les
noms ne le sont pas.

| Étape | Ce qui doit être exposé |
|---|---|
| 1 | ✓ début d'image et rendu d'une tuile (voir « Rendu par tuiles »), caméra et projection, soumission de triangles avec une matrice |
| 2 | ✓ chargement d'une texture, mipmaps engendrés au chargement, niveau de qualité du filtrage par `scg_set_filter` |
| 3 | ✓ soumission d'un lot éclairé, réglage du sur-éclairement, du brouillard, des lumières dynamiques, de la résolution interne et de la courbe de sortie |
| 4 | ✓ chargement d'un maillage et d'une carte depuis un bloc d'octets, libération, leurs comptes, leurs noms, leurs soumissions, et les lumières et entités d'une carte |
| 5 | rendu du monde depuis la caméra, calcul des lightmaps d'une cellule et reprise d'un cache |
| 6 | interpolation entre trames, sprites orientés caméra |
| 7 | module de collision, utilisable sans contexte de rendu |
| 8 | tracé de lignes et de points, interrogation de la scène, modification d'une cellule par identifiant |

## Ce qu'un auteur de liaison doit savoir

- **Vérifier la version au chargement**, par égalité, avant tout autre appel.
- **Copier le message de `scg_last_error` immédiatement.** Le prochain appel sur
  le même contexte l'invalide.
- **Ramener un code inconnu à sa catégorie**, `(-code) / 100`, plutôt que de le
  traiter en erreur générique.
- **Mettre une structure de configuration entièrement à zéro avant de la
  remplir.** Ses champs `_reserved` doivent être nuls, et c'est ce qui permettra
  d'en utiliser un sans casser les liaisons déjà écrites.
- **Réserver au moins 128 Ko de pile** au thread qui rend des tuiles, et lire le
  message d'une tuile par `scg_last_error(NULL)`, sur ce thread.
- **Sur wasm, passer par `scg_buffer_alloc`.** L'hôte ne peut pas fournir un
  pointeur arbitraire : seule la mémoire linéaire du module est adressable.
- **Sur wasm, un pointeur revient signé en JavaScript.** Un `i32` exporté y est
  interprété comme signé : dès que la mémoire linéaire dépasse deux gigaoctets,
  une adresse valide arrive négative. Tester la nullité par `ptr === 0`, jamais
  par `ptr > 0`, et convertir par `ptr >>> 0` avant de construire une vue.
- **Sur wasm, recréer toute vue sur la mémoire après chaque appel.** Un appel qui
  alloue peut agrandir la mémoire linéaire, ce qui détache le `ArrayBuffer`
  existant : une `Uint8ClampedArray` construite avant l'appel ne voit plus rien
  après, sans lever d'erreur à sa création.
- **Sur wasm, une panique est un trap**, jamais un code de retour. Une instance
  qui a levé un trap ne se réutilise pas ; le texte de la panique se lit dans la
  mémoire à l'adresse que `scg_last_error(NULL)` a rendue avant l'appel — voir
  « Après une panique ».
- **Sur wasm, le module s'instancie sans aucun import.** Il n'en réclame pas, et
  exporte `memory` avec les fonctions `scg_`.
- **Ne pas supposer que deux ressources se détruisent pour la même raison.**
  Détruire une texture ou une ressource chargée pendant une image est sans effet
  sur cette image, mais les mécanismes diffèrent — voir « Durées de vie et
  propriété ». Ce qui est vrai des deux aujourd'hui ne le restera pas forcément.
- **Ne rien calculer.** Une liaison convertit des types. Ce qui devrait être
  partagé entre deux liaisons remonte dans le noyau.
