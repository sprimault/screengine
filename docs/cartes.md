# Écrire un décor

Comment on fabrique une carte et un maillage que ce moteur charge. Ce document
s'adresse à qui écrit un décor, ou l'éditeur qui en écrira : il dit la
**méthode** et ce que le chargement refuse.

Les **dispositions binaires** sont dans [`rust.md`](rust.md), section « Formats
de fichier », qui fait foi. Elles sont rappelées ici en tableaux de champs, sans
leurs raisons — deux textes qui disent la même chose divergent, et c'est celui
qu'on ne relit pas qui finit par mentir.

Le moteur ne lit aucun fichier : c'est l'hôte qui passe le bloc d'octets à
`scg_world_load` ou `scg_mesh_load`. Voir [`abi.md`](abi.md).

## Ce qui est source, et ce qui ne l'est pas

Un fichier ne porte que ce dont rien ne peut le déduire. Tout le reste est
**dérivé au chargement**, et un décor qui le porterait quand même n'aurait pas où
l'écrire.

Sont source : la table de matériaux, les cellules avec leurs sommets, leurs
surfaces et leurs portails, les lumières statiques, les entités.

Sont dérivés : les liens de portails, les plans, la triangulation, les
coordonnées de texture et de lightmap, les boîtes englobantes, les étendues en
luxels, les tables d'identifiants, le classement des arêtes partagées.

Trois conséquences qu'un éditeur doit connaître avant de choisir ses structures :

- **les liens de portails ne s'écrivent pas.** Ils sont déduits en appariant les
  portails qui partagent tous leurs sommets, ce qui dispense l'éditeur de
  maintenir une adjacence à chaque opération ;
- **les coordonnées de texture et de lightmap ne s'écrivent pas** non plus. Le
  fichier porte un **repère** par surface, et les coordonnées s'en déduisent par
  projection ;
- **les lightmaps ne sont pas dans la carte.** Elles se calculent par
  `scg_lighting_build`, cellule par cellule, et se rangent dans un bloc de cache
  à côté — un troisième genre du même conteneur, décrit dans `rust.md`.

## Les identifiants

Les cellules, portails, surfaces, entités, lumières et matériaux portent chacun
un identifiant, **attribué par l'éditeur** et jamais par le moteur, avec **un
espace par famille** : une cellule 1 et une surface 1 coexistent sans conflit.

- **`0` est réservé à « aucun »**, dans toutes les familles. Un identifiant nul
  dans un fichier le fait refuser.
- **Un identifiant est unique dans sa famille.** Un doublon fait refuser le
  fichier.
- **Ils n'ont pas à être triés dans le fichier.** Le chargement en trie une copie
  pour vérifier l'unicité ; l'exiger du fichier obligerait l'éditeur à réécrire
  la carte entière pour un ajout.
- **Ce sont eux que l'ABI prend et rend**, jamais un rang : la cellule de départ
  d'une traversée, celle dont on calcule les lightmaps, la surface qu'un balayage
  touche. Un rang serait faux dès la première suppression au milieu.

## Une cellule

Une cellule est un volume **fermé** : tout rayon qui en sort traverse une de ses
surfaces ou un de ses portails. C'est la seule propriété dont tout le reste
dépend — la traversée, la localisation d'un point, le balayage d'une boîte et le
calcul des lightmaps reposent tous dessus.

**Elle n'a pas à être convexe.** Avec un tampon de profondeur, la convexité ne
sert qu'à la finesse de l'élimination, pas à la justesse de l'image : une cellule
concave rend la traversée conservatrice, jamais fausse. Une salle en L est donc
une cellule, et non deux.

### L'enroulement décide de ce qu'on voit

**La face avant d'une surface est celle dont les sommets tournent en sens
antihoraire quand on la regarde.** Une surface écrite à l'envers est éliminée au
découpage : elle disparaît, sans erreur et sans trace.

Ce n'est pas une subtilité théorique. Deux défauts se sont produits en écrivant
les décors du dépôt, et ils sont instructifs parce qu'ils ne ressemblent pas à ce
qu'ils sont :

- **une empreinte au sol parcourue dans le mauvais sens** retourne *toutes* les
  surfaces de la cellule d'un coup. Vue de l'intérieur, la cellule devient une
  coquille vue de dehors : l'image montre du fond là où le décor devrait être, et
  cela se lit comme un défaut du moteur ;
- **un plafond écrit dans le même sens que son sol** en retourne une seule. Le
  sol d'une cellule manquait, l'image ne peignait qu'un cinquième de l'écran, et
  le contrôle d'égalité entre la traversée et le chemin brut divergeait sur trois
  vues.

D'où la règle pratique, celle qu'appliquent les décors du dépôt : une empreinte
au sol d'**aire signée positive**, le sol parcouru dans l'ordre de l'empreinte et
le plafond **à l'envers**. Un générateur a tout intérêt à vérifier le signe de
l'aire plutôt qu'à faire confiance à l'ordre dans lequel les sommets ont été
saisis.

**Le sens de la normale intérieure, lui, n'est pas pris face par face** : le
calcul des lightmaps le déduit du signe du volume de la cellule entière,
portails compris. Une cellule mal fermée fausse donc ce signe pour toutes ses
surfaces d'un coup, et une normale à l'envers rend le terme de Lambert négatif —
la surface reste noire.

### Disposition

| Champ | Type |
|---|---|
| longueur de l'enregistrement | `u32` |
| `id`, `flags`, `vertex_count`, `surface_count`, `portal_count` | `u32` |
| les sommets, puis les surfaces, puis les portails | dans cet ordre |

**L'enregistrement est longueur-préfixé**, et cette longueur borne tout ce qu'il
contient : les trois comptes se recoupent avec elle, jamais avec la longueur de
la section. C'est ce qui permet de remplacer une cellule sans toucher aux autres.

`flags` **n'a aucun bit défini** et doit être nul.

Un sommet de cellule est `x, y, z` en `f32`, douze octets. Le monde est en main
droite, **Z en haut**. Un format en Y vers le haut se convertit dans l'outil qui
exporte, jamais dans le moteur.

## Les portails

Un portail est le polygone par lequel deux cellules se rejoignent. Il n'est pas
dessiné : il n'a ni matériau, ni drapeaux, ni repère.

| Champ | Type |
|---|---|
| `id`, `index_count` | `u32` |
| les indices de ses sommets dans la cellule | `u32` |

**Il doit être plan et convexe**, et la convexité est exigée là où elle ne l'est
pas pour une cellule parce que le portail décide de ce qu'on voit : la traversée
réduit sa fenêtre de découpe à la projection du portail, et une projection
concave n'a pas d'intersection exprimable comme réduction de fenêtre. L'erreur se
paierait en trou définitif dans l'image.

### L'appariement, et ce qu'il exige de l'éditeur

Deux portails sont appariés quand ils portent **les mêmes positions, au bit
près**. Il n'y a aucune tolérance, et ce n'est pas une rigueur gratuite : un
epsilon rendrait la relation non transitive — `a≈b`, `b≈c`, `a≉c` —, donc le
résultat dépendrait de l'ordre de parcours, ce que le déterminisme interdit.

**C'est donc à l'éditeur d'écrire les mêmes octets des deux côtés**, et c'est la
clause la plus facile à rater. Quatre règles s'y attachent :

- **les deux portails d'une paire ont des enroulements inverses**, chacun vu de
  sa propre cellule ;
- **un portail non apparié est un mur**, pas une erreur. Une carte en cours
  d'édition en a toujours, et le balayage le traite comme solide — sans quoi un
  mobile tomberait hors du monde là où l'éditeur n'a pas fini ;
- **deux portails de la même cellule ne s'apparient pas.** Le lien ramènerait sur
  la cellule courante, et la traversée tournerait sur place ;
- **trois portails sur la même clé font refuser le fichier.** Il n'y a pas de
  réponse à « lequel des deux ».

Conséquence géométrique qui se découvre tard : **deux cellules qui se rejoignent
sur une partie seulement d'un mur doivent porter les sommets de la coupure des
deux côtés.** Un mur de huit unités qui ouvre sur un couloir d'une unité a besoin
de deux sommets colinéaires supplémentaires, aux bornes de l'ouverture, pour que
l'arête du portail soit exactement celle du couloir. Sans eux, les deux portails
n'ont aucun sommet commun, le chargement en fait deux murs, et plus rien ne se
franchit — sans erreur.

## Les surfaces

| Champ | Type |
|---|---|
| `id`, `flags`, `material`, `index_count` | `u32` |
| les indices de ses sommets dans la cellule | `u32` |
| le repère de texture, puis celui de lightmap | 36 octets chacun |

Une surface est un polygone **plan et simple** ; la convexité n'est pas exigée,
la triangulation se faisant par découpe d'oreilles au chargement — plafonnée à
**64 sommets**. Une face en L, que la moindre extrusion produit, est donc une
surface.

### Les trois drapeaux

Trois bits sont définis, et **tous les autres doivent être nuls** :

| Bit | Sens | Lecteur |
|---|---|---|
| `0b001` | deux faces | aucun aujourd'hui |
| `0b010` | ne reçoit pas de lightmap | le calcul d'éclairage |
| `0b100` | non solide | le balayage, et lui seul |

**« Non solide » décrit la géométrie, pas le joueur** : ce n'est pas une notion
de jeu. Et le drapeau n'exclut que du balayage — la surface compte toujours dans
la parité qui localise un point, et elle occulte toujours la cuisson. Une grille
projette donc son ombre tout en laissant passer.

### Le repère, et pourquoi ce n'est pas des coordonnées

Chaque surface porte **deux repères complets et indépendants**, un de texture et
un de lightmap : une origine et deux axes, dont la **longueur porte l'échelle**.
Les coordonnées par sommet s'en déduisent au chargement, par projection.

C'est le repère qui est la source et les coordonnées la dérivée, dans ce sens-là
précisément : déplacer un sommet d'un mur dont on ne connaîtrait que les
coordonnées obligerait l'éditeur à reconstruire le repère par moindres carrés à
chaque opération, et deux éditeurs le reconstruiraient différemment.

Les deux repères sont indépendants, et c'est leur raison d'être : une surface
répète sa texture plusieurs fois et étire sa lightmap une seule fois sur toute
son étendue.

**L'origine n'a pas à appartenir au plan de la surface.** Un éditeur pose
volontiers un repère par matériau et le partage entre les surfaces qu'il
habille ; le calcul des lightmaps rabat lui-même le point sur le plan.

### Les trois contraintes du repère de lightmap

Le chargement les vérifie, et une carte qu'elles refusent était déjà fausse.
Chacune répond à un besoin distinct, et aucune ne remplace une autre :

1. **la longueur au carré de chaque axe est finie et non nulle**, et rien de
   plus : un axe dégénéré n'a pas de direction, donc pas de réciproque ;
2. **l'origine tombe sur un nœud de sa propre grille**, mesurée depuis le zéro du
   monde, sans quoi deux grilles de même pas restent décalées en phase ;
3. **les deux axes sont orthogonaux**, faute de quoi la reconstruction demande
   l'inverse d'une 2×2 quelconque, donc une division par luxel ;
4. **ils sont contenus dans le plan de la surface**, faute de quoi la grille de
   luxels ne recouvre pas ce qu'elle éclaire.

**Aucune pente n'est interdite, et il y a eu un temps où elles l'étaient presque
toutes.** La longueur au carré d'un axe a dû être une puissance de deux, ce qui
retombait sur la géométrie : l'axe horizontal du repère d'un mur est naturellement
**l'arête de ce mur multipliée par le pas de lightmap**, si bien que la somme des
carrés des composantes de l'arête devait en être une. Une arête de 4 ou 8 unités
passait, un déplacement de `(4, −4)` aussi — mais `(3, 1)` donne 10, et c'était le
**fichier entier** qui était refusé, pas seulement sa surface. Autrement dit :
l'axial ou le 45°, rien d'autre. Une rampe de parking à 1:2, une pente à 30°, une
rampe de coin étaient hors d'atteinte, et renoncer à leur lightmap n'y changeait
rien.

La clause est tombée parce que sa raison ne tenait pas : la cuisson **divise de
toute façon**, une fois par surface, et le contrôle n'en rendait que le quotient
exact. Une division IEEE est exactement arrondie, donc identique sur toutes les
cibles. Ce qui reste proscrit est une division **par luxel**, et c'est la
contrainte d'orthogonalité qui l'écarte.

Ce qu'un générateur perd en échange, et c'est peu : deux surfaces coplanaires
adjacentes n'alignent plus leurs grilles automatiquement. Leur donner le même pas
suffit à les aligner, et l'oubli ne coûte qu'une marche d'éclairage à la jointure
— ni la justesse, ni le déterminisme.

**Et c'est alors la deuxième contrainte qui décide, pas la pente.** L'origine doit
tomber sur un nœud de sa grille, et ce contrôle est **exact, sans tolérance** : le
produit scalaire de l'origine par chaque axe doit être un entier en arithmétique
double, sur des composantes écrites en simple précision. Deux conséquences qui ne
se devinent pas :

- **l'origine du monde le garantit toujours**, le produit scalaire valant zéro ;
- **un coin de la surface presque jamais**, et pas même sur une pente à 45° : pour
  un axe unitaire posé sur l'hypoténuse, le produit vaut 3,99999976 au lieu de 4,
  et le fichier est refusé. Une surface oblique prend donc l'origine du monde, ou
  un point dont le générateur a vérifié le produit.

**Pour une voûte facettée** — un tunnel, une rampe courbe, une coupole approchée
par facettes —, les deux clauses se lisent ensemble : l'origine du monde pour
toutes les facettes, et le **même pas** pour toutes. Sans le pas commun, chaque
jointure porte sa marche d'éclairage, ce qui se lit comme des bandes concentriques
sur la voûte.

**Les deux premières contraintes sont exactes, les deux dernières tolèrent un
résidu relatif**, et c'est à savoir avant d'écrire un générateur : une origine
tombe sur un nœud ou n'y tombe pas, alors qu'un repère oblique posé sur une surface
oblique porte le résidu de sa propre construction. Il n'y a donc rien à arrondir
pour satisfaire les deux premières, et rien à craindre des derniers bits sur les
deux autres.

Enfin, **l'étendue d'une surface est plafonnée à 256 luxels par côté**, et le
refus tombe au chargement. Un grand mur à pas de lightmap fin produirait un
atlas qui ne tient pas. Le pas se choisit en conséquence : les décors du dépôt
prennent un luxel par unité de monde, le même partout, ce qui aligne les grilles
de deux surfaces coplanaires adjacentes.

**La densité de plaquage borne de la même façon l'étendue d'une surface, et il
faut le dire parce que le refus tombe ailleurs.** Les coordonnées de texture sont
bornées à 16384 texels en valeur absolue : à 128 texels par unité de monde, une
surface ne peut donc pas dépasser 128 unités de côté, et 64 à 256 texels par
unité. Ce n'est pas une limite du moteur mais une conséquence de la densité
choisie — un mur deux fois plus grand demande un plaquage deux fois plus grossier,
ou sa coupe en deux surfaces.

Le repli ne rattrape que le décalage, jamais l'étendue : il ramène le
**minimum** des coordonnées d'une surface dans `[0, 2048)`, si bien qu'une surface
trop étendue garde un maximum hors borne. **Celui-là fait refuser la carte au
chargement**, par `SCG_ERR_INVALID_FORMAT` : une carte dont une seule surface
dépasse ne se charge pas du tout. Le message ne nomme pas la surface fautive — un
décor qui refuse de charger sans autre explication se cherche donc ici, en
commençant par les plus grandes faces.

## Les matériaux

| Champ | Type |
|---|---|
| `id` | `u32` |
| longueur du nom, puis ses octets UTF-8 | `u16`, puis les octets |

**Le fichier porte des noms, pas des images.** L'hôte découvre les noms par
`scg_world_material_count` et `scg_world_material_name`, charge ce qu'il veut
avec ses propres fichiers, et passe un tableau de handles **dans l'ordre des
emplacements**. Une entrée nulle vaut « sans texture », auquel cas la surface
rend la couleur de ses triangles.

C'est ce qui permet de dessiner le même décor avec deux habillages, et c'est ce
qui tient la promesse que le moteur n'ouvre rien.

## Les lumières statiques

| Champ | Type |
|---|---|
| `id` | `u32` |
| `x, y, z, radius` | `f32` |
| `r, g, b`, puis un octet nul | `u8` |

Vingt-quatre octets. L'octet final n'est pas du remplissage : c'est un champ
réservé, **nul obligatoire**. Le rayon doit être fini et strictement positif.

Les lumières sont une **section propre** et non des entités, parce que le calcul
des lightmaps doit pouvoir se faire sans qu'un hôte lui passe quoi que ce soit :
deux hôtes donneraient sinon deux éclairages pour la même carte, et le moteur
perdrait la même image sur toutes les cibles.

**Une lumière n'a pas de cellule.** La sélection est géométrique — intersection
de sa sphère avec la boîte englobante d'une cellule, puis distance par luxel —,
ce qui dispense d'un test d'appartenance à un volume non convexe.

**Placer une lampe demande de regarder de quel côté des murs elle tombe.** Le
calcul ajoute un terme de Lambert à l'atténuation : une lampe qui a une surface
dans son dos la laisse noire. Le défaut vécu est net — devant l'angle rentrant
d'une salle en L, le creux du L est *dehors*, et une lampe posée dans ce creux
avait les deux murs de l'angle dans son dos. Le décor annonçait un angle éclairé
qu'aucune vue ne montrait.

Deux clauses à connaître en composant l'éclairage d'un niveau :

- **la lumière ne tourne pas deux coins.** L'ensemble des occulteurs d'une
  cellule est elle-même, ses voisines à un portail, et le bord de cet ensemble
  rendu opaque. Une salle à trois portails de distance reste noire ;
- **une cellule close ne reçoit rien de l'extérieur.** Une cellule sans portail
  apparié a besoin de sa propre lumière, sinon elle est noire — et une image
  noire ne distingue plus rien.

## Les entités

| Champ | Type |
|---|---|
| longueur de l'enregistrement | `u32` |
| `id`, `cell` | `u32` |
| longueur de la classe, puis ses octets UTF-8 | `u16`, puis les octets |
| `x, y, z`, puis `qx, qy, qz, qw` | `f32` |
| longueur des données, puis ses octets | `u32`, puis les octets |

**La classe est une chaîne que le moteur ne compare à rien**, et le bloc de
données est copié et jamais lu : c'est à l'hôte de les interpréter. Pas de modèle
de propriétés typé — ce serait un langage de jeu qu'il faudrait faire évoluer
avec les jeux.

Le quaternion se range `x, y, z, w`, la partie réelle en dernier ; l'identité est
`{0, 0, 0, 1}`. Il est normalisé au chargement, et un quaternion nul est refusé.

**La cellule d'une entité est un identifiant, vérifié existant au chargement.**
Une entité hors cellule n'existe donc pas.

Aucune carte du dépôt ne porte de section d'entités : la section est facultative,
et le moteur ne fait rien de son contenu aujourd'hui.

## Un maillage

Un maillage sert aux objets posés et aux accessoires mobiles, pas au décor : il
n'a ni portail, ni second jeu de coordonnées, et il ne reçoit pas de lightmap.

| Élément | Disposition | Taille |
|---|---|---|
| Sommet | `u, v` en `f32` | 8 |
| Pose de sommet | `x, y, z` puis `nx, ny, nz` en `f32` | 24 |
| Triangle | `i0, i1, i2` en `u32`, puis `r, g, b, a` en `u8` | 16 |
| Groupe de surface | `id`, `first_triangle`, `triangle_count`, `texture_slot` en `u32` | 16 |
| Nom d'emplacement | longueur en `u16`, puis les octets UTF-8 | variable |

Quatre choses à savoir avant d'écrire un exportateur :

- **ce qui anime est séparé de ce qui n'anime pas.** Les coordonnées de texture
  sont par sommet et constantes sur toutes les trames ; seules les poses
  changent. La section des trames porte leur nombre, puis les poses **rangées par
  trame**, une tranche contiguë par trame. Un maillage statique déclare une
  trame, et le décodeur n'a pas deux chemins ;
- **la normale est stockée, jamais dérivée.** Une normale dérivée des sommets
  voisins arrondirait les arêtes vives d'une caisse, et seul l'outil d'export
  connaît l'intention de lissage. Les quatre coins d'une face vive portent donc
  la même normale ;
- **une normale animée suit l'échelle de sa trame, par les cofacteurs.** Pour une
  échelle `(sx, sy, sz)`, les facteurs sont `(sy·sz, sx·sz, sx·sy)`. Écrire la
  normale du repos dans toutes les trames serait un défaut du fichier, invisible
  au chargement ;
- **les groupes pavent l'intervalle des triangles dans l'ordre**, sans trou ni
  recouvrement, et c'est vérifié. Un groupe dispersé imposerait de rassembler ses
  triangles à chaque image, donc une allocation par image.

Chaque groupe **nomme toujours un emplacement de texture** : il n'y a pas de
valeur pour « sans texture » dans le fichier, et un maillage qui porte des
triangles déclare au moins un nom. « Sans texture » se dit à la soumission, par
un handle nul.

## Assembler le fichier

Les trois genres — maillage, carte, cache de lightmaps — partagent un en-tête de
vingt octets et une table de sections.

| Champ | Valeur |
|---|---|
| signature | `S` `C` `G` `0x1A` |
| genre | `MESH`, `WRLD` ou `LMAP` |
| `version_format` | `u32` — **2** pour un maillage, **1** pour une carte |
| `total_length` | `u32` |
| `section_count` | `u32` |

Puis `section_count` entrées de douze octets : genre sur quatre octets ASCII,
décalage et longueur en `u32`.

Quatre clauses décident de la validité, et trois d'entre elles surprennent :

- **`total_length` est exigé égal à la longueur reçue**, pas inférieur. Toute
  troncature devient détectable au premier contrôle, et aucun octet de queue
  n'est toléré — deux fichiers d'octets différents ne doivent pas rendre la même
  image ;
- **les sections se rangent par genre croissant, au plus une de chaque, et
  pavent le fichier sans recouvrement.** Les genres d'une carte sont donc `CELL`,
  `ENTS`, `LGTS`, `MATS` dans cet ordre, ceux d'un maillage `FRMS`, `SURF`,
  `TEXN`, `TRIS`, `VTXS` ;
- **une section de genre inconnu fait refuser le fichier.** Il n'y a pas de
  tolérance en avant, et c'est la décision la moins intuitive du format :
  presque tout ce qui s'ajoutera à un format de rendu change ce qui est rendu, et
  une bibliothèque qui sauterait en silence une section qu'une version plus
  récente emploie rendrait **une autre image sur le même fichier, sans erreur** ;
- **tout est en octet de poids faible en tête**, imposé par le format et non
  déduit de la cible.

Aucun alignement n'est exigé du bloc. Aucune somme de contrôle non plus : elle ne
protège de rien face à un bloc hostile, qui la recalcule, et l'intégrité de
transport appartient à l'archive de l'hôte.

**Il n'y a pas de section facultative dont l'absence serait signalée** : une
carte sans entités omet simplement `ENTS`, et son `section_count` le dit.

## Ce que le chargement ne vérifie pas

**La cohérence géométrique.** Triangles dégénérés, faces retournées, surfaces
colinéaires, cellule mal fermée : rien de tout cela ne corrompt la mémoire, et
refuser un état intermédiaire d'éditeur contredirait l'édition à chaud.

C'est précisément ce qui rend ce document nécessaire : un décor faux se charge
sans erreur, et le défaut se voit à l'image — ou ne se voit qu'en mouvement.

**Un fichier malformé rend une erreur, il ne panique jamais**, même en débogage.
`SCG_ERR_INVALID_FORMAT` pour ce qui est faux à l'intérieur du bloc,
`SCG_ERR_UNSUPPORTED_FORMAT_VERSION` pour une version que la bibliothèque ne lit
pas — ce dernier étant le seul des deux qui dise à l'hôte quoi faire : réexporter.

## Un piège qui n'est pas dans les octets

**L'ordre du fichier fait partie du contenu.** Le test de profondeur est strict :
à égalité, le premier triangle soumis reste. Deux surfaces coplanaires se
départagent donc par leur ordre dans le fichier, et un éditeur qui réordonne ses
tables sans rien changer d'autre **déplace l'image**.

C'est aussi l'ordre du fichier qui décide de la cellule rendue par
`scg_world_locate` quand deux cellules superposées contiennent le même point : la
première gagne.

## Les décors du dépôt

Ils sont engendrés par du Rust dans `crates/screengine-conformance/src/`, réécrits
tous ensemble par **`make mesh`**, et un test les compare octet pour octet à ce
que leur générateur produit — un fichier périmé échoue donc franchement, au lieu
de faire diverger des empreintes sans dire pourquoi. Ce sont les exemples
travaillés de tout ce qui précède.

| Fichier | Ce qu'il montre | Générateur |
|---|---|---|
| `hosts/couloir.world` | le cas le plus simple : deux tronçons de boîte appariés par un portail, les deux bouts ouverts restant des portails non appariés, donc des murs | `world_file.rs` |
| `hosts/salles.world` | quatre cellules, une concave, un portail oblique, deux cellules superposées, trois lumières | `rooms_file.rs` |
| `hosts/collision.world` | deux cellules, un portail apparié, un non apparié, une surface non solide, un passage à la largeur exacte d'une boîte | `collision_file.rs` |
| `hosts/caisse.mesh` | un maillage à deux trames, deux groupes, deux emplacements | `mesh_file.rs` |

Ce que les trois cartes écrivent de la même façon vit dans `map_bytes.rs` :
l'écriture d'une surface, les densités de plaquage et le pas de lightmap. Une
quatrième carte part de là.
