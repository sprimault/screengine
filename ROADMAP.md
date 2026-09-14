# Feuille de route

Les numéros font foi : ce sont eux que portent les `todo!("étape N : …")` du
code.

```
rg -o 'todo!\("étape' crates --glob '!*tests*' | wc -l
```

C'est la mesure d'avancement la plus honnête du projet. Elle descend toute
seule, et elle ne ment pas.

**Les numéros ordonnent les dépendances, pas le calendrier.** Ils ne se
renumérotent jamais — ce sont eux que portent les marqueurs du code. Une étape
qui apparaît s'ajoute à la fin, quelle que soit sa place logique.

**Chaque étape franchie est publiée.** L'étape N porte la version 0.N.0. Une
bibliothèque dont l'ABI n'est pas figée reste en `0.x` : le mineur marque une
étape, pas une rupture.

**Les notes d'une version qui ne rend encore rien doivent dire ce qu'elle ne fait
pas.** Quelqu'un télécharge une archive dont la bibliothèque panique sur
`todo!("étape 5")`, et sans cette phrase il croit à un défaut.

La conception complète est dans `docs/abi.md` et `docs/construction.md`. Ce
fichier n'en est que l'ordre d'exécution.

---

## 0 — La frontière

Un triangle en dur, affiché depuis quatre hôtes : un programme C sous Windows, un
script PHP en FFI, un navigateur en wasm, un téléphone Android via JNI.

Aucun moteur. Cinq fonctions : `scg_abi_version`, `scg_create`, `scg_destroy`,
`scg_frame_end`, `scg_last_error`, plus `scg_buffer_alloc` et `scg_buffer_free`
que le web impose.

**C'est l'étape qui décide de tout le reste.** L'ABI n'est pas une façade qu'on
pose en fin de parcours : elle interdit les callbacks, contraint la propriété des
tampons, impose des handles opaques et une gestion d'erreur par code de retour.
Découverte au bout de six mois, elle se paie en réécriture du moteur.

La suite de conformance naît ici, réduite à une empreinte : le même triangle
rendu quatre fois doit hacher pareil. C'est ce qui transforme « ça marche
partout » en fait vérifié.

**Franchie quand** les quatre hôtes affichent le même triangle, les quatre
empreintes sont identiques, et `make nostd` passe en intégration continue.

## 1 — Le pipeline

Maths — vecteurs, matrices, quaternions, tables trigonométriques maison —, pile
de matrices, projection, z-buffer, rasteriseur à fonctions de bord.

La **règle top-left** s'écrit ici. Pas à l'étape 2, pas « quand on verra des
trous » : à ce moment-là le défaut est invisible à l'arrêt, visible en mouvement,
et tout ce qui suit est construit dessus.

Rien à voir d'intéressant : un cube qui tourne, deux cubes qui s'interpénètrent
proprement. C'est en test que ça se mesure.

**Franchie quand** aucune couture n'apparaît sur la scène à arêtes partagées, en
rotation lente, à toutes les résolutions internes prévues.

## 2 — Textures

Affine d'abord, parce qu'elle est juste et qu'elle donne déjà l'image qu'on
cherche. Puis correction de perspective par interpolation de `1/z`, `u/z`, `v/z`
avec division tous les 8 ou 16 pixels. Puis clipping du plan proche uniquement,
en espace homogène avant division.

Clipper contre six plans est inutile et coûteux : les côtés se traitent par
découpe du rectangle en espace écran.

**Franchie quand** la caméra peut se coller à un mur texturé sans artefact.

## 3 — L'esthétique

Palette 256 couleurs, colormap d'atténuation par la distance, résolution interne
paramétrable sans recréation du contexte, conversion indexé → RGBA en fin
d'image.

Les défauts d'époque se décident ici : pas de filtrage, pas de mip-mapping,
éclairage par sommet, sommets arrondis à l'entier avant projection. Les ajouter
après coup ne marche pas — ils tiennent au pipeline, pas à un post-traitement.

**Franchie quand** une capture est montrable sans qu'on ait à expliquer ce qu'on
regarde. C'est la première version qui donne envie de continuer, et ce n'est pas
une considération accessoire sur un projet de cette longueur.

## 4 — Les données

Format de maillage et format de carte, versionnés dès la première ligne, chargés
depuis un bloc mémoire. L'hôte lit les fichiers, jamais le moteur.

Le format cassera une douzaine de fois. Un numéro de version en tête permet
d'écrire des migrations au lieu de jeter les cartes de test — et on n'y pense
jamais avant d'en avoir perdu une série.

Identifiants stables pour secteurs, murs et entités. Jamais d'index de tableau
comme référence persistante.

## 5 — Le monde

Secteurs, portails, traversée avec réduction de fenêtre de clipping, pile de
(secteur, fenêtre) avec limite de profondeur.

Aucune étape de compilation. Les liens de portails se déduisent au chargement.

**C'est le vrai test d'architecture du projet.** Si la traversée est propre, tout
ce qui suit se déroule. Si elle est bancale, il y aura des trous dans l'image
pour toujours, et ils ne se rattrapent pas par un correctif local.

**Franchie quand** un niveau multi-secteurs se parcourt sans trou, avec des
secteurs superposés et des pentes.

## 6 — Animation et sprites

Interpolation entre trames, sprites orientés caméra, ordre géré par le z-buffer
plutôt que par un tri.

## 7 — Collision

Balayage de boîte englobante contre les secteurs. Module séparé, propres entrées
d'ABI, **utilisable sans rendu** : un serveur de jeu doit pouvoir s'en servir
sans jamais allouer de tampon d'image.

C'est la partie que les documents de format ne couvrent qu'à moitié, et celle où
il faudra réellement réfléchir plutôt que transposer.

## 8 — Ce qu'il faut pour éditer

Le moteur n'a pas d'éditeur : il expose ce qu'un éditeur réclame — tracé de
lignes et de points dans le tampon, interrogation de la scène, modification d'un
secteur sans rechargement complet.

**Séparation stricte entre état du monde et état de jeu.** Le premier est
rechargeable à chaud, le second est jeté au rechargement. Mélangés, l'édition à
chaud devient impossible et l'étape entière perd son objet.

L'éditeur lui-même vit dans un hôte, jamais ici.

## 9 — SIMD

NEON, SSE/AVX, `simd128`, sélectionnés à l'exécution.

Le rasteriseur scalaire reste compilé et testé. Toute variante se valide contre
son empreinte : une divergence est une variante fausse, jamais une différence
acceptable.

**Volontairement tardive.** Optimiser avant que le pipeline soit figé revient à
écrire trois fois le même code.

---

## Hors périmètre v1

- Éclairage dynamique par pixel, ombres portées, lightmaps.
- Modèles animés par squelette : les trames interpolées suffisent à l'esthétique
  visée.
- Son, entrées, réseau. Ce n'est pas un moteur de jeu.
- Un éditeur livré. L'étape 8 fournit de quoi en écrire un, pas un produit.
- iOS et macOS avant que le reste soit stable : ils exigent un runner macOS et
  un cycle de retour lent depuis un poste Windows.
- Rendu GPU. Ce serait un autre projet, pas une étape de celui-ci.
