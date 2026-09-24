# Feuille de route

Les numéros font foi : ce sont eux que portent les `todo!("étape N : …")` du
code.

```
rg -o 'todo!\("étape' src crates --glob '!*tests*' | wc -l
```

`src` autant que `crates` : le noyau est le paquet racine, et une mesure qui ne
regarde que `crates/` ignore précisément l'endroit où le travail se fait.

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

Un triangle en dur, affiché depuis quatre hôtes : un programme C lié à la
bibliothèque statique, un programme C++ lié à la bibliothèque dynamique, un
navigateur en wasm, un téléphone Android via JNI.

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

**Franchie, publiée en 0.0.0.**

## 1 — Le pipeline

Maths — vecteurs, matrices, quaternions, tables trigonométriques maison —,
projection, clipping en espace homogène avant division : le plan proche, et les
quatre plans de la bande de garde pour que les coordonnées écran tiennent dans
leur format ; z-buffer, rasteriseur à fonctions de bord, découpage en tuiles.

Pas de pile de matrices : la matrice modèle est un paramètre de la soumission,
et le moteur y compose la vue de sa caméra. Une pile servirait une hiérarchie de
transformations que ce projet n'a pas — la traversée de l'étape 5 empile des
cellules, pas des matrices —, et le noyau seul l'offrirait, ce que la règle des
deux chemins interdit. La matrice reste celle du modèle et jamais la modèle-vue :
composer l'inverse de la caméra dans chaque liaison y ferait entrer autant de
bibliothèques mathématiques que de langages.

Clipper contre les six plans du tronc est inutile et coûteux : les côtés de
l'image se traitent par découpe du rectangle en espace écran, les fonctions de
bord s'évaluant en coordonnées globales. Le clipping arrive avec la projection et
non avec les textures : dès qu'une caméra existe, un sommet derrière elle ou hors
de la bande de garde casserait le format 28.4, et rien d'autre ne l'en protège.

La **virgule fixe après projection** et la **règle top-left** ne s'écrivent pas
ici : elles sont figées dans `docs/rust.md` et servent dès le premier
remplissage, celui du triangle de l'étape 0. Découvertes après, le défaut est
invisible à l'arrêt, visible en mouvement, et tout ce qui suit est construit
dessus. Ce que cette étape y ajoute, c'est la preuve qu'elles tiennent partout.

**Les tuiles s'installent ici**, même rendues l'une après l'autre par un seul
thread : c'est la forme du rasteriseur qu'elles décident, pas le parallélisme. La
fonction de rendu d'une tuile existe dès cette étape dans l'ABI.

Rien à voir d'intéressant : un couloir parcouru, avec une caisse posée au sol
qui le traverse. C'est en test que ça se mesure.

**Un couloir plutôt qu'un objet posé devant la caméra**, et ce n'est pas un
choix de décor. La caméra est à l'intérieur d'une géométrie fermée, ce qu'elle
sera toujours dans un monde de cellules : les deux orientations se mêlent dans
la même image, les murs passent derrière le plan proche à chaque pas et
débordent de l'écran, si bien que le clipping et la bande de garde travaillent
en permanence. Un objet regardé de loin ne déclenche ni l'un ni l'autre, et
laisserait le chemin le plus délicat de l'étape sans aucune scène visible pour
le rejouer. Le sol qui fuit vers l'horizon vient avec, et c'est ce que l'étape
suivante demande. La caisse porte ce que la géométrie du couloir ne montre pas :
deux surfaces qui s'interpénètrent, que seul le tampon de profondeur départage.

**Franchie quand** aucune couture n'apparaît sur la scène à arêtes partagées, en
rotation lente, à toutes les résolutions internes prévues, et que chaque scène
de conformance rend la même empreinte en tuiles de 32, de 64 et en image
entière.

**Franchie, publiée en 0.1.0.**

## 2 — Textures

Correction de perspective par interpolation de `1/w`, `u/w`, `v/w` en virgule
fixe, avec division tous les 16 pixels — des segments alignés sur la grille de
l'image, jamais sur le bord de la tuile. Mipmaps, générés au chargement de la
texture. Le clipping du plan proche, écrit à l'étape 1, découpe désormais aussi
les coordonnées de texture.

**Le filtrage par défaut est le tramage ordonné des coordonnées de texture**,
technique de la fin des années 90 : un décalage sous-texel tiré d'une table
indexée par la position du pixel, pour un coût quasi nul. Le bilinéaire est un
niveau de qualité au-dessus, choisi par l'hôte.

**Franchie quand** la caméra peut se coller à un mur texturé sans artefact, et
qu'un sol qui fuit vers l'horizon ne scintille pas en mouvement.

**Franchie, publiée en 0.2.0.**

## 3 — La lumière et l'image

Lightmaps, quelques lumières dynamiques ajoutées aux lightmaps, brouillard par
la distance, couleurs directes, résolution interne paramétrable sans recréation
du contexte, post-traitement local au pixel.

**À cette étape, une lightmap est une ressource fournie par l'hôte**, chargée
comme une texture et échantillonnée par un second jeu de coordonnées au sommet.
Le noyau n'en calcule aucune : il n'a rien à partir de quoi le faire, le format
de carte arrivant à l'étape 4 et les cellules à l'étape 5. Les numéros ordonnent
les dépendances, et cette étape-ci ne peut pas les devancer.

**Le calcul est à l'étape 5**, avec la traversée qui lui donne son unité. Ce qui
s'y décidera ne change rien ici : une lightmap reste un bloc de texels, qu'il
vienne de l'hôte ou du noyau.

**Le post-traitement se fait pendant la recopie de tuile** — un gain par canal,
puis le gamma —, jamais en passe plein écran. Ce qui lit les pixels voisins en
est exclu : une tuile ne voit pas au-delà de son bord.

**Le tonemapping n'en fait pas partie, et le mot est abandonné ici.** Le tampon
est en huit bits par canal, sans dynamique étendue : il n'y a aucune plage à
compresser, et ce qu'on appellerait ainsi ne serait qu'une courbe de contraste
de plus. Celle-ci n'apporte rien que le gain et le gamma ne donnent déjà, ne
commute avec aucun des deux, et obligerait à graver un pivot dans l'ABI. Elle
s'ajoutera le jour où une capture montrera qu'elle manque.

**L'atténuation d'une lumière dynamique ne dépend que de la distance**, jamais
de l'orientation de la surface. Les faces d'une caisse à égale distance d'une
torche reçoivent donc la même lumière, et la caisse ressemble à un bloc
uniformément éclairci plutôt qu'à un objet dont une face capte la lumière.

Y ajouter du relief demanderait une **normale par sommet**, donc une structure
de sommet de plus dans l'ABI — et une structure publiée ne se retire jamais. La
question s'est tranchée sur la capture qui franchit l'étape : les objets se
lisent par leur silhouette et par leur texture, et **elle est reportée à
l'étape 6**, dont les maillages animés et les sprites sont les vrais
demandeurs. Le report ne coûte rien — une structure et une fonction de
soumission s'ajoutent sans toucher aux publiées.

**Franchie quand** une capture est montrable sans qu'on ait à expliquer ce qu'on
regarde. C'est la première version qui donne envie de continuer, et ce n'est pas
une considération accessoire sur un projet de cette longueur.

**Franchie, publiée en 0.3.0.**

## 4 — Les données

Format de maillage et format de carte, versionnés dès la première ligne, chargés
depuis un bloc mémoire. L'hôte lit les fichiers, jamais le moteur.

Le format cassera une douzaine de fois. Un numéro de version en tête permet
d'écrire des migrations au lieu de jeter les cartes de test — et on n'y pense
jamais avant d'en avoir perdu une série.

Identifiants stables pour cellules, portails, surfaces et entités. Jamais d'index
de tableau comme référence persistante.

**Un hôte de démonstration par langage, avec fenêtre, entrées et boucle.** Les
hôtes existants vérifient une empreinte : ils rendent trois scènes et comparent
un nombre, sans jamais ouvrir de fenêtre. C'est ce qu'il faut pour la
conformance, et c'est inutilisable comme point de départ — quelqu'un qui veut
parcourir un décor depuis un autre langage n'a rien à copier, alors que l'étage
d'accueil Rust lui offre un couloir qu'on traverse.

C'est ici et pas plus tôt parce que c'est ici qu'un hôte a quelque chose à
charger : avant le format de carte, un exemple ne peut qu'écrire sa géométrie en
dur, et il en existe déjà un. Ces hôtes restent des exemples, jamais des
composants du moteur : la bibliothèque continue de n'avoir ni fenêtre, ni
entrées, ni fichiers, et c'est ce qui la rend intégrable dans une application
qui a déjà les siennes.

## 5 — Le monde

Cellules 3D fermées, portails polygonaux plans et convexes, traversée avec
réduction de fenêtre de clipping, pile de (cellule, fenêtre) avec limite de
profondeur. La cellule de la caméra se suit par ses traversées de portails.

Aucune étape de compilation. Les liens de portails se déduisent au chargement.
Une cellule n'a pas à être convexe : avec le z-buffer, une cellule non convexe
rend la traversée conservatrice, jamais fausse.

**Le calcul des lightmaps arrive ici**, et non à l'étape 3 : c'est la cellule
qui lui donne son unité, et elle n'existe pas avant. **Elles restent un cache
dérivable** — le noyau les calcule à partir de la carte, cellule par cellule,
sur appel explicite de l'hôte, qui peut garder le résultat et le rendre au
chargement suivant. Une cellule modifiée recalcule les siennes, pas celles du
niveau. Aucune ne se calcule pendant une image. L'étape 3 les échantillonne
déjà, fournies par l'hôte : rien du remplissage n'est à reprendre.

**C'est le vrai test d'architecture du projet.** Si la traversée est propre, tout
ce qui suit se déroule. Si elle est bancale, il y aura des trous dans l'image
pour toujours, et ils ne se rattrapent pas par un correctif local.

**Franchie quand** un niveau multi-cellules se parcourt sans trou, avec des
cellules superposées, des cellules non convexes et des portails obliques.

## 6 — Animation et sprites

Interpolation entre trames, sprites orientés caméra, ordre géré par le z-buffer
plutôt que par un tri.

**Les sprites apportent deux façons d'écrire un pixel que le moteur n'a pas
encore** : le texel transparent, qu'on n'écrit pas, et la surface **modulée**,
qui multiplie ce qui est déjà dans le tampon au lieu de l'écraser. La première
est ce qu'un sprite réclame par définition ; la seconde vient avec, parce
qu'elle ne coûte qu'un mode de plus au même endroit du remplissage.

**Le cas d'usage à ne pas perdre de vue est l'ombre d'un objet mobile** : une
tache sombre posée au sol sous un ennemi, modulée avec le décor. Ce n'est pas
une ombre portée au sens du hors périmètre — rien n'est calculé par test de
visibilité, et c'est le jeu qui décide où la tache va. Le moteur ne fournit
que la primitive : un polygone qui assombrit au lieu de recouvrir.

**C'est ici que se tranche la normale par sommet**, laissée ouverte à l'étape 3
faute d'un cas qui la réclame : un objet animé qui traverse les lumières d'une
salle est ce cas, là où une malle posée au sol se lit par sa silhouette.

## 7 — Collision

Balayage de boîte englobante contre les cellules. Module séparé, propres entrées
d'ABI, **utilisable sans rendu** : un serveur de jeu doit pouvoir s'en servir
sans jamais allouer de tampon d'image.

C'est la partie que les documents de format ne couvrent qu'à moitié, et celle où
il faudra réellement réfléchir plutôt que transposer.

## 8 — Ce qu'il faut pour éditer

Le moteur n'a pas d'éditeur : il expose ce qu'un éditeur réclame — tracé de
lignes et de points dans le tampon, interrogation de la scène, modification d'une
cellule sans rechargement complet, recalcul de ses seules lightmaps.

**Séparation stricte entre état du monde et état de jeu.** Le premier est
rechargeable à chaud, le second est jeté au rechargement. Mélangés, l'édition à
chaud devient impossible et l'étape entière perd son objet.

L'éditeur lui-même vit dans un hôte, jamais ici.

## 9 — SIMD

NEON, SSE/AVX, `simd128`, sélectionnés à l'exécution. Rendu des tuiles en
parallèle, depuis les threads de l'hôte.

Le rasteriseur scalaire reste compilé et testé. Toute variante se valide contre
son empreinte : une divergence est une variante fausse, jamais une différence
acceptable. Aucune intrinsèque fusionnée, relâchée ou approximative : c'est là,
et non dans l'arithmétique ordinaire, que le déterminisme se perd.

**Volontairement tardive.** Optimiser avant que le pipeline soit figé revient à
écrire trois fois le même code.

---

## Hors périmètre v1

- Éclairage dynamique par pixel, ombres portées : les lightmaps et quelques
  lumières dynamiques suffisent à la classe visée, et tiennent sur téléphone.
  **« Ombre portée » désigne ici une ombre calculée par test de visibilité à
  chaque image**, pas une tache posée au sol sous un objet mobile — celle-là
  est une surface modulée que le jeu place, et l'étape 6 en donne la
  primitive. Le moteur calcule bien des ombres, mais à l'étape 5 et une fois
  pour toutes : ce sont les lightmaps du décor.

  **Une variante étroite reste à examiner, après l'étape 6.** Ce que le budget
  exclut, c'est de recalculer les ombres partout, à chaque image, pour huit
  lumières : une passe de rendu par lumière, deux millions d'accès dispersés à
  l'écran, plusieurs mégaoctets de trafic par image — alors qu'une scène
  chargée consomme déjà près de la moitié du budget d'une image à 60 i/s. Une
  **seule** lumière portant des ombres, en basse résolution, sur les **seuls
  objets mobiles**, coûterait environ une milliseconde : c'est ce que faisaient
  les moteurs de la toute fin de cette époque. Le décor garderait ses
  lightmaps, meilleures et déjà payées. À trancher quand il y aura un objet
  mobile à ombrer, pas avant : dimensionner pour des objets qui n'existent pas
  revient à choisir une résolution au hasard.
- Post-traitement plein écran : FXAA, bloom, tout ce qui lit les pixels voisins.
- Modèles animés par squelette : les trames interpolées suffisent à la classe
  visée.
- Son, entrées, réseau. Ce n'est pas un moteur de jeu.
- Un éditeur livré. L'étape 8 fournit de quoi en écrire un, pas un produit.
- iOS et macOS avant que le reste soit stable : ils exigent un runner macOS et
  un cycle de retour lent depuis un poste Windows.
- Rendu GPU. Ce serait un autre projet, pas une étape de celui-ci.
