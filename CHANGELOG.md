# Journal des versions

Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/).

SemVer avec la clause du zéro : **en `0.x`, rien n'est imposé**. Le mineur
marque une étape de la feuille de route, pas une rupture d'API — tout le reste
s'accumule en correctif, correctifs, fonctionnalités et ruptures confondus.

Trois numéros à ne pas confondre :

| Numéro | Où | Ce qu'il suit |
|---|---|---|
| version du dépôt | tag git | la bibliothèque |
| `SCG_ABI_VERSION` | `include/screengine.h` et `scg_abi_version()` | la frontière C |
| `version_format` | chaque carte et chaque maillage | le format de fichier |

Les deux derniers ne suivent pas SemVer. Ce sont des entiers : ajouter une
fonction à l'ABI ou un champ optionnel à un format ne les incrémente pas, tout
le reste les incrémente, et un incrément de `version_format` oblige à écrire la
migration des fichiers existants. Une version peut sortir sans qu'ils bougent ;
ils ne bougent jamais sans version.

Un titre de section s'écrit `## [version] — date — titre`. La publication en
tire le nom et les notes de la version : ce qui est relu ici est ce qui sera lu
sur la page des versions, et il n'y a rien à recopier ensuite. Le titre est
facultatif ; sans lui, la version se nomme par son tag.

**Une section absente arrête la publication.** Une version sans notes ne dit ni
ce qui change, ni ce qu'un auteur de liaison doit reprendre.

**Les notes d'une version qui ne rend encore rien doivent dire ce qu'elle ne
fait pas.** Quelqu'un compile une bibliothèque qui panique sur `todo!`, et sans
cette phrase il croit à un défaut.

La section en cours s'écrit `## [Non publié]`, sans date ni titre, et chaque lot
y ajoute ce qu'il change : écrite au fil de l'eau, elle est relue en même temps
que le code qu'elle décrit, alors qu'une section rédigée le jour du tag résume
de mémoire un mois de travail. **Elle prend son numéro et sa date au moment du
tag**, faute de quoi la publication cherche une section qui n'existe pas et
s'arrête.

Chaque section est **bilingue, français d'abord, séparé par `***`** — les notes
de version sont ce que lit un auteur de liaison étranger avant de savoir s'il
doit reprendre son travail. Ce préambule reste en français : il n'est jamais
publié, et explique les conventions du dépôt à qui y contribue.

## [Non publié]

### Ajouté

- L'API Rust accepte des sommets portant un second jeu de coordonnées, celui
  des lightmaps : `VertexUv2` et `Context::submit_each_lit`. Les coordonnées
  traversent la projection et le découpage, et leurs équations de plan se
  rangent dans un tableau annexe du contexte.
- Les lightmaps rendent. Une lightmap se charge comme une texture, se lit
  toujours en bilinéaire et par sa propre chaîne de mipmaps, et se combine au
  texel par `t·(l + 1) >> 8` — la forme qui rend le texel intact sous pleine
  lumière. Un triangle non texturé s'éclaire aussi, sa couleur tenant lieu de
  texel. Le sur-éclairement se règle par `Context::set_overbright`, de zéro à
  deux, et vaut zéro par défaut.

### Modifié

- `docs/abi.md` annonçait qu'une lightmap n'aurait pas de mipmaps, au motif
  qu'elle n'est jamais réduite. C'est vrai en intérieur et faux dès qu'une
  surface s'éloigne au-delà de quelques dizaines de mètres, ce qu'un décor à
  ciel ouvert fait par construction. La clause est amendée, et la chaîne est
  engendrée comme pour une texture.

***

### Added

- The Rust API accepts vertices carrying a second set of coordinates, the one
  used by lightmaps: `VertexUv2` and `Context::submit_each_lit`. Those
  coordinates travel through projection and clipping, and their plane equations
  are stored in a side table of the context.
- Lightmaps now render. A lightmap loads like a texture, is always read
  bilinearly and through its own mipmap chain, and combines with the texel as
  `t·(l + 1) >> 8` — the form that leaves the texel untouched under full light.
  An untextured triangle is lit as well, its colour standing in for the texel.
  Overbright is set through `Context::set_overbright`, from zero to two, and
  defaults to zero.

### Changed

- `docs/abi.md` stated that a lightmap would have no mipmaps, on the grounds
  that it is never minified. That holds indoors and stops holding as soon as a
  surface moves a few dozen metres away, which an open-sky set does by
  construction. The clause is amended, and the chain is generated as for a
  texture.

## [0.2.1] — 2026-09-23

**Ce que cette version rend autrement.** Sur une scène inchangée, un sol
échantillonné en bilinéaire change de onze pixels sur 230 400, d'une unité sur
un canal : la pente des coordonnées de texture était sous-estimée d'un seizième
sur le dernier segment de chaque ligne. Le filtrage par défaut rend exactement
la même image, l'écart tombant sous le texel qu'il tramait déjà. Aucune
signature ne change et `SCG_ABI_VERSION` reste à **1**.

**Ce qu'un hôte doit savoir.** Le canal alpha du tampon de sortie est désormais
écrit à 255 partout, comme le contrat d'ABI l'annonce, là où la valeur soumise
avec la couleur d'un triangle le traversait. Un hôte qui soumettait une couleur
non opaque voyait ce canal ressortir, et un navigateur l'y composait ; il
obtient maintenant une image opaque.

### Corrigé
- API Rust : une fin d'image refusée — tampon trop court, `stride` faux —
  refermait quand même l'image. `Frame::end` consomme la `Frame`, et son `Drop`
  remettait le contexte en enregistrement alors que le noyau venait de laisser
  l'image ouverte pour permettre une reprise ; l'appelant perdait sa scène, là
  où un hôte C rappelle simplement `scg_frame_end`. Le `Drop` distingue
  désormais une `Frame` abandonnée, qu'il referme, d'une fin tentée et refusée,
  qui se reprend par `Context::end`.
- `scg_frame_end` pouvait rendre `SCG_OK` alors qu'une tuile avait paniqué et
  que son rectangle n'était pas dessiné. Le contrat annonce `SCG_ERR_FAULTED`
  dans ce cas, et c'est désormais vrai quel que soit l'entrelacement : le
  décompte des tuiles en vol et la marque de défaillance sont une seule valeur
  atomique, là où deux valeurs laissaient entre elles un instant pendant lequel
  la fin d'image concluait à tort.
- Les quatre points d'entrée qui ne rendent pas de code — les deux
  destructions, la libération et l'allocation de tampon — passent désormais par
  l'enveloppe commune : une panique n'y traverse plus la frontière C,
  l'environnement flottant y est fixé comme ailleurs, et l'emplacement d'erreur
  par thread y est vidé à l'entrée. Ce dernier point était violable **sans
  aucune panique** : sur un thread de pool, un chargement de texture qui
  échouait laissait son message, et l'appel suivant le rendait comme s'il était
  le sien.
- Le header donne le repère complet de la caméra — le monde en main droite, Z
  en haut, le zénith vers le haut de l'écran et le −Y vers la droite — là où il
  n'en disait que la direction du regard, ce qui laissait le roulis
  indéterminé. Il dit aussi que la couleur d'un triangle est ignorée sur le
  chemin texturé, et le message de `SCG_ERR_INVALID_STATE` ne parle plus de
  tuiles : il couvre aussi une soumission faite pendant le rendu.
- Une texture restait dans la table de l'image quand son lot était accepté sans
  qu'aucun de ses triangles ne survive à la projection. L'entrée n'était plus
  désignée par rien, et le plafond de la table, qui se déduit du nombre de
  triangles préparés, cessait d'être une borne : des lots invisibles suffisaient
  à le remplir.
- Le remplissage prenait l'appui de sa pente au pixel qui suit le segment même
  lorsque ce pixel n'était plus couvert : la borne testée était la boîte
  englobante du triangle, qui déborde le span sur toute ligne d'un triangle non
  rectangle. La profondeur y est prolongée hors de la surface, où elle n'a plus
  de sens.

***

**What this version renders differently.** On an unchanged scene, a
bilinearly sampled floor changes by eleven pixels out of 230,400, by one unit
on one channel: the texture coordinate slope was falling short by one sixteenth
on each row's last segment. Default filtering renders exactly the same image,
the gap staying below the texel it was already dithering. No signature changes
and `SCG_ABI_VERSION` stays at **1**.

**What a host should know.** The output buffer's alpha channel is now written
at 255 everywhere, as the ABI contract states, where the value submitted with a
triangle's colour used to reach it. A host submitting a non-opaque colour saw
that channel come out, and a browser composited it; it now gets an opaque
image.

### Fixed
- Rust API: a refused frame end — buffer too short, wrong `stride` — closed the
  frame anyway. `Frame::end` consumes the `Frame`, and its `Drop` put the
  context back into recording although the engine had just left the frame open
  so it could be retried; the caller lost its scene, where a C host simply
  calls `scg_frame_end` again. `Drop` now tells an abandoned `Frame`, which it
  closes, from an attempted and refused end, which `Context::end` retries.
- `scg_frame_end` could return `SCG_OK` although a tile had panicked and its
  rectangle was never drawn. The contract states `SCG_ERR_FAULTED` in that
  case, and it now holds whatever the interleaving: the in-flight tile count
  and the fault mark are a single atomic value, where two values left an
  instant between them during which the frame end concluded wrongly.
- The four entry points that return no code — both destructors, the buffer
  release and the buffer allocation — now go through the common wrapper: a
  panic no longer crosses the C boundary there, the floating-point environment
  is set as elsewhere, and the per-thread error slot is cleared on entry. That
  last point was breakable **with no panic at all**: on a pool thread, a failing
  texture load left its message behind, and the next call returned it as its
  own.
- The header now gives the camera's full frame — right-handed world, Z up,
  zenith towards the top of the screen and world −Y towards the right — where it
  only stated the viewing direction, leaving the roll undetermined. It also
  states that a triangle's colour is ignored on the textured path, and the
  `SCG_ERR_INVALID_STATE` message no longer talks about tiles: it also covers a
  submission made during rendering.
- A texture stayed in the frame's table when its batch was accepted without a
  single triangle surviving projection. Nothing referenced the entry any more,
  and the table's ceiling, derived from the number of prepared triangles,
  stopped being a bound: invisible batches were enough to fill it.
- The fill took its slope anchor at the pixel following the segment even when
  that pixel was no longer covered: the bound being tested was the triangle's
  bounding box, which overshoots the span on every row of a non-right triangle.
  Depth is extended past the surface there, where it no longer means anything.

## [0.2.0] — 2026-09-22 — Textures

**Ce qu'un hôte de la 0.1.0 doit reprendre.** Une coordonnée de sommet non
finie refuse désormais le lot entier, là où elle faisait disparaître son
triangle en silence ; c'est ce que le contrat d'ABI annonçait déjà. Un hôte qui
soumettait un `NaN` sans le savoir voyait un trou dans son décor, il reçoit
maintenant `SCG_ERR_INVALID_ARGUMENT` et son lot n'est pas posé. Aucune
signature ne change et `SCG_ABI_VERSION` reste à **1**.

### Ajouté
- API Rust : `Texture::load`, qui copie un bloc de pixels RGBA8 et engendre
  toute sa chaîne de mipmaps, jusqu'à 1×1, par moyenne des texels et jamais par
  échantillonnage. Les deux côtés sont des puissances de deux indépendantes,
  entre 1 et `MAX_TEXTURE_SIZE`. Le bloc est copié : l'hôte peut le libérer au
  retour, et une texture chargée ne change plus. Tous les niveaux tiennent dans
  une seule allocation, faite à ce seul appel.
- API Rust : `VertexUv`, un sommet qui porte ses coordonnées de texture en
  texels, non normalisées.
- ABI : `scg_texture_load` et `scg_texture_destroy`, avec `ScgTextureDesc` et
  `SCG_TEXTURE_FORMAT_RGBA8` — qui vaut **1 et non 0**, pour qu'une description
  laissée à zéro soit refusée plutôt qu'interprétée. Les deux prennent **aucun
  contexte** : une texture n'appartient à personne, et leur message se lit par
  `scg_last_error(NULL)` sur le thread appelant. Le bloc de pixels est copié,
  l'hôte peut le libérer au retour.
- ABI : `scg_submit_textured` et `ScgVertexUv`, un sommet de vingt octets qui
  porte ses coordonnées de texture en texels. La texture vaut pour le lot
  entier. Détruire une texture qu'une image référence encore est sans effet sur
  elle. Aucune signature publiée ne change et `SCG_ABI_VERSION` reste à **1**.
- Le filtrage par défaut : niveau de mipmap choisi par segment de seize pixels,
  sur la plus forte des quatre dérivées — **la verticale comprise**, sans quoi
  un sol sous-sélectionne et scintille —, puis tramage ordonné des coordonnées
  par une matrice de Bayer 4×4 indexée sur la position dans l'image. Le
  décalage vaut moins d'un demi-texel et somme à zéro sur un bloc : il masque
  l'escalier de la troncature sans déplacer la texture.
- Le remplissage échantillonne une texture, par segments de seize pixels
  alignés sur la grille de l'image : la réciproque de la profondeur se calcule
  aux extrémités du segment et les coordonnées s'interpolent affinement entre
  elles. Les segments se bornent au span du triangle et jamais à la tuile, donc
  la même surface se texture pareil quel que soit le découpage. Le niveau 0
  seulement — les mipmaps existent mais rien ne les choisit encore.
- Une septième scène de conformance, `texture` : un sol en damier qui fuit vers
  l'horizon, motif écrit dans la suite et non chargé. Les six empreintes
  précédentes sont inchangées.
- Une huitième, `texture-bilineaire` : le même sol au texel près, échantillonné
  en bilinéaire. Une scène et non une passe de plus — les passes d'une scène
  doivent rendre la même empreinte, et un filtrage qui change l'image a besoin
  de sa propre référence. Les sept précédentes sont inchangées.
- `make conform-images` écrit chaque vue en image : une empreinte dit qu'une
  image a changé, jamais qu'elle est juste.
- ABI : `scg_set_filter`, avec `SCG_FILTER_DITHER` — qui vaut **0**, à l'inverse
  du format de texture, parce qu'un contexte qu'on ne configure pas doit rendre
  le défaut — et `SCG_FILTER_BILINEAR`. Une valeur inconnue est refusée et non
  rabattue sur le défaut : c'est ce qui rend l'ajout d'un filtrage compatible.
  Refusé pendant le rendu. Aucune signature publiée ne change,
  `SCG_ABI_VERSION` reste à **1**, et aucun code d'erreur n'est ajouté.
- API Rust : `Context::submit_textured`, qui habille un lot d'une texture. Elle
  vaut pour le lot entier, et le moteur en garde une référence forte jusqu'à la
  fin de l'image : l'hôte peut la libérer de son côté sans que l'image en cours
  change. Rien ne rend encore la texture.
- API Rust : `Context::submit_uv`, qui soumet un lot dont les sommets portent
  leurs coordonnées de texture. Elles traversent le découpage sans
  prémultiplication — la projection étant linéaire avant la division, le point
  d'intersection les porte sans un calcul de plus —, puis se multiplient par la
  profondeur à la division. Bornées à `MAX_TEXEL_COORD` texels : au-delà, ou
  non finies, le lot est refusé en entier.
- API Rust : `screengine_play::load_png`, qui décode un PNG et en fait une
  texture du moteur — palette étendue, gris répété sur trois canaux, `tRNS`
  devenu alpha, échantillons de seize bits ramenés à huit, alpha opaque quand
  le format n'en porte pas. Le moteur n'ouvre toujours aucun fichier et ne
  connaît aucun format d'image : décoder appartient à l'hôte, par ce chemin
  comme derrière l'ABI C.
- Trois textures pour l'étage d'accueil — brique, pierre, bois — sous
  `crates/screengine-play/assets/`, en 512×512. Elles sont produites pour le
  projet et suivent ses licences.
- API Rust : `Filter`, `Context::set_filter` et `Context::filter`. Le
  bilinéaire mélange les quatre voisins d'un seul niveau de mipmap, poids sur
  huit bits, en retranchant un demi-texel pour que le centre d'un texel rende
  ce texel. Il **exclut** le tramage, qui n'a plus d'escalier à masquer. Le
  défaut ne change pas, et le filtre se refuse pendant le rendu : deux tuiles
  de la même image ne se lisent pas autrement.

### Modifié
- L'exemple `couloir` est texturé : brique sur les murs et le plafond, pavage
  au sol, planches sur les caisses, chaque face de caisse habillée selon ses
  deux axes propres. C'est là que se jugent le scintillement du sol et le
  tramage vu de près, qu'aucune empreinte ne montre. `F` y bascule le
  filtrage, en partant du tramage : c'est en marchant que les deux se
  départagent, une image fixe ne montrant qu'un grain contre un flou.
- API Rust : `Tick::set_title`, qui change le titre de la fenêtre en cours de
  session. Tant que le moteur ne dessine pas de texte, la barre de titre est le
  seul endroit où un jeu peut écrire un mode ou un compteur.
- Le remplissage teste la profondeur avant d'écrire, en deux appels distincts
  au lieu d'un : c'est entre les deux que viendra l'échantillonnage de la
  texture, qui ne se paiera donc que pour les pixels visibles. L'image est
  inchangée, et le compte des propositions aussi.
- Un triangle préparé porte les équations de plan de ses coordonnées de
  texture à côté de celle de sa profondeur, et les trois partagent leur point
  de référence : les écarts ne se calculent qu'une fois par pixel, et un
  triangle préparé tient dans deux lignes de cache. Les empreintes sont
  inchangées — rien ne lit encore ces plans.
- Le remplissage d'un triangle ne teste plus chaque pixel de sa boîte
  englobante : il résout les trois fonctions de bord pour obtenir, ligne par
  ligne, les abscisses extrêmes qu'il couvre, puis ne parcourt que celles-là.
  Le résultat est rigoureusement le même — c'est la résolution entière qui le
  garantit, pas une approximation —, et les empreintes de conformance sont
  inchangées. C'est ce qui donnera leurs extrémités aux segments de
  perspective : hors du triangle, la profondeur peut s'annuler, et il n'y
  aurait aucun quotient à prendre.

### Corrigé
- Une coordonnée de sommet démesurée pouvait produire un triangle de bruit au
  lieu de disparaître. La borne qui l'écarte portait sur la coordonnée de vue,
  avant la mise à l'échelle par le champ de vision, alors que ce qu'elle doit
  couvrir sont les produits du découpage, quadratiques en la coordonnée de
  clip : au-delà, une intersection donnait un `NaN` sans erreur ni panique.
  Elle porte désormais sur la coordonnée de clip. Les empreintes de
  conformance sont inchangées — il fallait des coordonnées absurdes pour
  l'atteindre.
- Une coordonnée de sommet non finie refuse le lot entier, comme le contrat
  d'ABI l'annonce, au lieu de faire disparaître son triangle sans rien dire.
  Un sommet **fini mais démesuré** continue de disparaître sans erreur : il
  dépend de la caméra, et refuser le lot rendrait une scène valide irrendable
  selon l'endroit où l'on se place.

***

**What a 0.1.0 host must revisit.** A non-finite vertex coordinate now rejects
the whole batch, where it used to make its triangle vanish silently; this is
what the ABI contract already promised. A host unknowingly submitting a `NaN`
saw a hole in its scenery, it now gets `SCG_ERR_INVALID_ARGUMENT` and its batch
is not recorded. No signature changes and `SCG_ABI_VERSION` stays at **1**.

### Added
- Rust API: `Texture::load`, which copies a block of RGBA8 pixels and builds its
  whole mipmap chain down to 1×1, by averaging texels and never by sampling.
  Both sides are independent powers of two, between 1 and `MAX_TEXTURE_SIZE`.
  The block is copied: the host may free it once the call returns, and a loaded
  texture never changes. Every level lives in a single allocation, made in that
  one call.
- Rust API: `VertexUv`, a vertex carrying its texture coordinates in texels,
  unnormalised.
- ABI: `scg_texture_load` and `scg_texture_destroy`, with `ScgTextureDesc` and
  `SCG_TEXTURE_FORMAT_RGBA8` — which is **1, not 0**, so that a description
  left zeroed is refused rather than interpreted. Both take **no context**: a
  texture belongs to nobody, and their message is read through
  `scg_last_error(NULL)` on the calling thread. The pixel block is copied, the
  host may free it on return.
- ABI: `scg_submit_textured` and `ScgVertexUv`, a twenty-byte vertex carrying
  its texture coordinates in texels. The texture applies to the whole batch.
  Destroying a texture a frame still references does not affect it. No
  published signature changes and `SCG_ABI_VERSION` stays at **1**.
- Default filtering: mipmap level picked per sixteen-pixel segment, on the
  largest of the four derivatives — **the vertical one included**, without
  which a floor underselects and shimmers —, then ordered dithering of the
  coordinates through a 4×4 Bayer matrix indexed on the position in the image.
  The offset is under half a texel and sums to zero over a block: it masks the
  staircase of truncation without moving the texture.
- Filling samples a texture, in sixteen-pixel segments aligned on the image
  grid: the reciprocal of depth is computed at the segment ends and coordinates
  are interpolated affinely between them. Segments are bounded by the
  triangle's span and never by the tile, so the same surface is textured alike
  whatever the split. Level 0 only — mipmaps exist but nothing selects them yet.
- A seventh conformance scene, `texture`: a checkerboard floor receding towards
  the horizon, its pattern written in the suite rather than loaded. The six
  previous digests are unchanged.
- An eighth, `texture-bilineaire`: the same floor down to the texel, sampled
  bilinearly. A scene and not another pass — the passes of one scene must all
  produce the same digest, and a filter that changes the image needs its own
  reference. The seven previous ones are unchanged.
- `make conform-images` writes every view out as an image: a digest says an
  image changed, never that it is correct.
- ABI: `scg_set_filter`, with `SCG_FILTER_DITHER` — which is **0**, unlike the
  texture format, because a context that is never configured must render the
  default — and `SCG_FILTER_BILINEAR`. An unknown value is refused rather than
  falling back on the default: that is what makes adding a filter compatible.
  Refused during rendering. No published signature changes,
  `SCG_ABI_VERSION` stays at **1**, and no error code is added.
- Rust API: `Context::submit_textured`, dressing a batch with a texture. It
  applies to the whole batch, and the engine keeps a strong reference to it
  until the frame ends: the host may release its own without the current frame
  changing. Nothing renders the texture yet.
- Rust API: `Context::submit_uv`, submitting a batch whose vertices carry their
  texture coordinates. They cross the clipper unpremultiplied — projection
  being linear before the divide, the intersection point carries them at no
  extra cost — then get multiplied by depth at the divide. Bounded to
  `MAX_TEXEL_COORD` texels: beyond that, or non-finite, the whole batch is
  rejected.
- Rust API: `screengine_play::load_png`, decoding a PNG into an engine texture
  — palettes expanded, greyscale spread over three channels, `tRNS` turned into
  alpha, sixteen-bit samples reduced to eight, an opaque alpha when the format
  carries none. The engine still opens no file and knows no image format:
  decoding belongs to the host, on this path as behind the C ABI.
- Three textures for the host stage — brick, stone, wood — under
  `crates/screengine-play/assets/`, at 512×512. They were produced for the
  project and carry its licences.
- Rust API: `Filter`, `Context::set_filter` and `Context::filter`. Bilinear
  blends the four neighbours within a single mipmap level, eight-bit weights,
  subtracting half a texel so that a texel centre samples that texel. It
  **excludes** dithering, which no longer has a staircase to hide. The default
  is unchanged, and the filter is refused during rendering: two tiles of the
  same frame are never read differently.

### Changed
- The `couloir` example is textured: brick on the walls and ceiling, cobbles on
  the floor, planks on the crates, each crate face mapped along its own two
  axes. This is where floor shimmer and close-up dithering are judged, neither
  of which a hash shows. `F` toggles the filter there, starting from dithering:
  the two are told apart while walking, a still image showing only grain
  against blur.
- Rust API: `Tick::set_title`, changing the window title mid-session. Until the
  engine draws text, the title bar is the only place a game can write a mode or
  a counter.
- Filling tests depth before writing, in two distinct calls instead of one:
  texture sampling will happen between them, and will therefore only be paid
  for visible pixels. The image is unchanged, and so is the count of proposed
  pixels.
- A prepared triangle carries the plane equations of its texture coordinates
  next to its depth one, and all three share their reference point: offsets are
  computed once per pixel, and a prepared triangle fits in two cache lines.
  Digests are unchanged — nothing reads those planes yet.
- Filling a triangle no longer tests every pixel of its bounding box: it solves
  the three edge functions to get, line by line, the extreme abscissae it
  covers, then walks only those. The result is rigorously the same — integer
  resolution guarantees it, not an approximation — and conformance digests are
  unchanged. This is what will give perspective segments their endpoints:
  outside the triangle, depth can vanish, and there would be no quotient to
  take.

### Fixed
- An oversized vertex coordinate could produce a triangle of noise instead of
  vanishing. The bound that rejects it applied to the view coordinate, before
  the field-of-view scaling, whereas what it must cover are the clipping
  products, quadratic in the clip coordinate: beyond that, an intersection
  yielded a `NaN` with neither error nor panic. It now applies to the clip
  coordinate. Conformance digests are unchanged — reaching it took absurd
  coordinates.
- A non-finite vertex coordinate rejects the whole batch, as the ABI contract
  states, instead of making its triangle vanish silently. A **finite but
  oversized** vertex still vanishes without an error: it depends on the camera,
  and rejecting the batch would make a valid scene unrenderable depending on
  where one stands.

## [0.1.0] — 2026-09-21 — Le pipeline

**Ce qu'un hôte de la 0.0.0 doit reprendre.** Le moteur n'a plus de scène en
dur : un contexte auquel rien n'a été soumis rend désormais une image de fond.
Un hôte qui n'appelait que `scg_frame_end` obtenait une image de démonstration ;
il obtient maintenant un écran noir, et doit décrire sa scène par
`scg_submit`. Aucune signature publiée ne change et `SCG_ABI_VERSION` reste à
**1** : il se lie et s'exécute sans être recompilé, mais il ne montre plus rien
tant qu'il ne soumet rien.

### Ajouté
- `scg_frame_begin` et `scg_frame_tile` : l'hôte commence l'image, puis rend
  ses tuiles depuis ses propres threads. `scg_frame_end` garde sa signature et
  rend les tuiles que personne n'a rendues : un hôte qui n'appelle qu'elle
  reçoit toujours l'image entière. `SCG_ABI_VERSION` ne change pas. Le thread
  qui rend une tuile doit disposer de 128 Kio de pile.
- API Rust : `Vec3`, `Quat`, `Affine3` et `Angle`, en main droite avec Z en
  haut. Trigonométrie par table et racine inverse sans libm, qui rendent les
  mêmes bits sur toutes les cibles.
- Projection perspective et découpage en espace homogène, avant la division :
  le plan proche et les quatre plans de la bande de garde, qui bornent les
  coordonnées écran à ±4096 pixels. Le repère de vue est X à droite, Y vers le
  bas, Z vers l'avant, avec un plan lointain à l'infini.
- API Rust : soumission d'une scène par `Context::submit`, qui reçoit un lot de
  sommets, des triangles indexés portant chacun sa `Color`, et la matrice
  **modèle** — objet vers monde. Le contexte porte la `Camera` — position,
  orientation, champ de vision, plan proche — et compose lui-même la vue : une
  matrice modèle-vue reçue toute faite obligerait chaque hôte à inverser la pose
  de la caméra, donc à normaliser un quaternion par sa propre bibliothèque
  mathématique. Un lot est accepté ou refusé en entier.
- Une caméra d'orientation neutre regarde le +X du monde, le zénith vers le haut
  de l'écran ; le quaternion se range `x, y, z, w`, l'identité étant
  `{0, 0, 0, 1}`.
- Quatre scènes de conformance : la bande de garde, le débordement latéral sans
  découpe, l'interpénétration avec égalité de profondeur, et un sol qui
  traverse le plan proche.
- Une cinquième, en rotation : la scène à arêtes partagées sur un tour complet,
  à trois résolutions internes. Ses images s'enchaînent en une seule empreinte,
  et un contrôle sans référence vérifie qu'aucune ligne ne montre de trou entre
  les deux triangles.
- `screengine-play` : `FreeCamera`, une caméra qu'on dirige aux flèches et à la
  souris — lacet autour du zénith, tangage borné au quart de tour, aucun
  roulis —, et `Tick::capture_cursor`, qui capture le curseur ou le relâche.
  Du comportement seulement : la caméra produit celle du noyau, et rien de ce
  qu'elle permet n'échappe à l'ABI C.
- L'exemple `couloir` : un couloir qu'on parcourt, avec des caisses dont l'une
  traverse le sol. La caméra y est à l'intérieur d'une géométrie fermée, ce qui
  fait travailler le plan proche et la bande de garde à chaque pas.
- `scg_set_camera` et `scg_submit`, avec `ScgCamera`, `ScgVertex`,
  `ScgTriangle` et `ScgMat4`. La matrice reçue est celle du **modèle** — objet
  vers monde —, le moteur composant la vue de sa caméra ; une modèle-vue y
  appliquerait la vue deux fois. Un lot est accepté ou refusé en entier. Le
  quaternion se range `x, y, z, w`, et avec l'orientation neutre la caméra
  regarde le +X du monde, le zénith vers le haut de l'écran. Aucune signature
  publiée ne change, et `SCG_ABI_VERSION` reste à 1.
- Le champ `reserved0` de `ScgContextConfig` devient `max_triangles`, la
  capacité de triangles par image, `0` valant 16384. C'est l'usage prévu d'un
  champ réservé : un hôte qui passait des zéros garde le comportement par
  défaut, les décalages ne bougent pas, `SCG_ABI_VERSION` non plus.

### Retiré
- **Changement volontaire du rendu** : le moteur n'a plus de scène en dur. Un
  contexte auquel rien n'a été soumis rend une image de fond, sans erreur. Les
  hôtes qui n'appelaient que `scg_frame_end` obtenaient jusqu'ici une image de
  démonstration ; ils doivent désormais décrire leur scène par `scg_submit`.
  Les empreintes de conformance sont inchangées : la même scène, décrite par un
  hôte, rend la même image.

### Modifié
- **Changement volontaire du rendu** : l'image n'est plus un triangle écrit en
  coordonnées d'écran, mais deux triangles qui partagent une arête, vus de
  biais et projetés par la chaîne complète. Les empreintes de conformance sont
  mises à jour en conséquence. Aucune signature ne change, et
  `SCG_ABI_VERSION` non plus.
- La face avant est antihoraire dans les données : le moteur la rend en niant
  les fonctions de bord, sans permuter de sommets. Un appelant de l'API Rust
  qui soumettait ses triangles dans l'autre sens ne verra plus rien.
- Tampon de profondeur : `near/w` en 0.32 interpolé en virgule fixe, test
  strict, et à égalité le premier triangle soumis reste. L'image du triangle en
  dur est inchangée. API Rust : `Frame::region` reçoit un tampon de profondeur à
  côté du tampon de couleur, de même longueur.
- L'image se rend par tuiles de la taille choisie à la création : les triangles
  sont répartis une fois par image, et chaque tuile tient sa couleur sur la pile
  de l'appel. Le contexte ne réserve plus de tampon à la taille de l'image.
  L'image rendue est inchangée, octet pour octet.
- Le code `-6` se nomme désormais `SCG_ERR_FAULTED`, et un objet dont un appel a
  paniqué est dit défaillant. `SCG_ERR_POISONED` reste défini avec la même
  valeur, déprécié : aucun hôte n'est à reprendre, et `SCG_ABI_VERSION` ne
  change pas.

***

**What a 0.0.0 host must revisit.** The engine no longer has a built-in scene:
a context nothing was submitted to now renders a background image. A host that
only called `scg_frame_end` used to get a demonstration image; it now gets a
black screen, and must describe its scene through `scg_submit`. No published
signature changes and `SCG_ABI_VERSION` stays at **1**: it links and runs
without being recompiled, but shows nothing until it submits something.

### Added
- `scg_frame_begin` and `scg_frame_tile`: the host begins the frame, then
  renders its tiles from its own threads. `scg_frame_end` keeps its signature
  and renders every tile nobody rendered: a host that only calls it still
  receives the whole image. `SCG_ABI_VERSION` is unchanged. A thread rendering
  a tile needs 128 KiB of stack.
- Rust API: `Vec3`, `Quat`, `Affine3` and `Angle`, right-handed with Z up.
  Table-driven trigonometry and an inverse square root without libm, giving the
  same bits on every target.
- Perspective projection and clipping in homogeneous space, before the divide:
  the near plane and the four guard-band planes, which bound screen
  coordinates to ±4096 pixels. The view frame is X right, Y down, Z forward,
  with an infinite far plane.
- Rust API: scene submission through `Context::submit`, taking a batch of
  vertices, indexed triangles each carrying its `Color`, and the **model**
  matrix — object to world. The context holds the `Camera` — position,
  orientation, field of view, near plane — and composes the view itself: a
  ready-made model-view matrix would force every host to invert the camera
  pose, hence to normalise a quaternion with its own maths library. A batch is
  accepted or rejected as a whole.
- A camera with neutral orientation looks towards world +X, with the zenith
  towards the top of the screen; quaternions are stored `x, y, z, w`, identity
  being `{0, 0, 0, 1}`.
- Four conformance scenes: the guard band, lateral overflow without clipping,
  interpenetration with equal depths, and a floor crossing the near plane.
- A fifth, rotating: the shared-edge scene over a full turn, at three internal
  resolutions. Its images chain into a single hash, and a reference-free check
  verifies that no row shows a gap between the two triangles.
- `screengine-play`: `FreeCamera`, a camera driven with the arrow keys and the
  mouse — yaw about the zenith, pitch clamped to a quarter turn, no roll — and
  `Tick::capture_cursor`, which grabs or releases the cursor. Behaviour only:
  the camera produces the engine's own, and nothing it allows is out of reach
  of the C ABI.
- The `couloir` example: a corridor you walk through, with crates, one of them
  cutting through the floor. The camera sits inside closed geometry, which
  keeps the near plane and the guard band busy at every step.

### Changed
- **Deliberate change of the rendered image**: it is no longer a triangle
  written in screen coordinates, but two triangles sharing an edge, seen at an
  angle and projected through the whole chain. Conformance hashes are updated
  accordingly. No signature changes, and neither does `SCG_ABI_VERSION`.
- Front faces are counter-clockwise in the data: the engine renders them by
  negating the edge functions, without swapping vertices. A Rust API caller
  submitting triangles the other way round will no longer see them.
- Depth buffer: `near/w` in 0.32 interpolated in fixed point, strict test, and
  on a tie the first submitted triangle stays. The image of the hard-coded
  triangle is unchanged. Rust API: `Frame::region` takes a depth buffer next to
  the colour buffer, of the same length.
- The image is rendered in tiles of the size chosen at creation: triangles are
  binned once per frame, and each tile keeps its colour on the stack of the
  call. The context no longer reserves image-sized buffers. The rendered image
  is unchanged, byte for byte.
- Code `-6` is now named `SCG_ERR_FAULTED`, and an object whose call panicked
  is called faulted. `SCG_ERR_POISONED` stays defined with the same value,
  deprecated: no host needs changes, and `SCG_ABI_VERSION` is unchanged.

## [0.0.0] — 2026-09-17 — La frontière

**Ce que cette version ne fait pas : elle ne rend aucune scène.** Un triangle
en dur, et rien d'autre : ni scène à soumettre, ni caméra, ni texture. La fin
d'image rend une image complète, que les quatre hôtes — C, C++, wasm et
Android — hachent pareil.

**Première ABI publiée : `SCG_ABI_VERSION` vaut 1.** Une liaison compare cette
valeur à celle de la bibliothèque chargée, par égalité, avant tout autre appel.
Aucun format de fichier n'existe encore : `version_format` n'a pas de valeur.

### Ajouté
- Remplissage de triangle par fonctions de bord en virgule fixe, avec la règle
  top-left : deux triangles qui partagent une arête se partagent ses pixels sans
  trou ni recouvrement.
- Tampons de couleur et de profondeur dimensionnés pour la résolution maximale à
  la création du contexte, et jamais réalloués ensuite.
- Sept points d'entrée : version d'ABI, création et destruction du contexte,
  fin d'image, dernier message d'erreur, allocation et libération d'un tampon.
  Chacun passe par une enveloppe unique qui fixe l'environnement flottant,
  rattrape les paniques et traduit l'erreur en code de retour. Un hôte qui a
  démasqué des exceptions flottantes les retrouve masquées pendant l'appel, et
  intactes au retour.
- Contrôle des décalages de `ScgContextConfig` des deux côtés de la frontière :
  un test Rust, et des assertions statiques que le compilateur de l'hôte vérifie
  sur sa propre cible.
- Hôte C sans fenêtre, lié à la bibliothèque statique sous Windows et Linux, et
  lancé par `make test` : il vérifie les refus, l'écriture hors du tampon,
  l'alignement et l'environnement flottant vus depuis C, et compare son
  empreinte du triangle à celle du chemin Rust.
- Hôte C++ sans fenêtre, lié à la bibliothèque dynamique sous Windows et Linux,
  et lancé par `make test` : mêmes contrôles, plus le header compilé en C++ et
  la preuve que les fonctions viennent de la bibliothèque chargée.
- Hôte wasm, en JavaScript sans dépendance : un test sans fenêtre sous Node,
  lancé par `make test`, et une page servie par `make web`. Il charge le module
  sans aucun import, alloue par `scg_buffer_alloc`, vérifie les mêmes refus et
  les vues détachées par la croissance de la mémoire, et compare son empreinte à
  celle du chemin Rust. Le module wasm est publié avec les autres archives.
- Hôte Android : couche JNI en C, application sans Gradle qui écrit dans la
  mémoire d'un bitmap, et un test en deux paliers lancé par `make test`. Les
  trois ABI sans appareil — aarch64 et armv7 sous émulation —, environnement
  flottant hostile compris ; puis un émulateur x86_64, en C et à travers JNI sur
  un tampon désaligné. Toutes les empreintes doivent être celle du chemin Rust.
  La bibliothèque des trois ABI est publiée, rangée comme `jniLibs/`.
- Empreinte d'image : FNV-1a 64 bits sur les dimensions puis la zone utile,
  que chaque hôte recalcule dans son langage.
- Suite de conformance : l'empreinte du triangle est une référence versionnée,
  que `make conform` compare octet pour octet après un rendu en tuiles de 32 et
  de 64. Chaque hôte compare la sienne au chemin Rust : les quatre se rejoignent
  sur le même fichier.
- Un test prouve qu'aucune image n'alloue une fois le contexte créé, la
  première comprise.
- Espace de travail en cinq crates : noyau sans std, frontière C, bibliothèques
  publiées, étage d'accueil, suite de conformance.
- Les bibliothèques s'appellent `screengine` : `screengine.dll`,
  `libscreengine.so`, `libscreengine.a`, `screengine.wasm`. Sous Linux et
  Android, la bibliothèque dynamique porte le SONAME `libscreengine.so`.
- `screengine-play`, pour faire un jeu en Rust sans écrire d'hôte : fenêtre,
  clavier et souris, boucle à pas fixe, image remontée par facteur entier ou
  remplissant la fenêtre. `make run` ouvre l'exemple.
- Le message d'un argument refusé dit lequel : résolution, taille de tuile,
  `stride` ou longueur du tampon. Le code de retour ne change pas.
- Contrat d'ABI, conventions Rust et documentation de construction.
- Intégration continue et publication sur tag.

### Modifié
- Double licence MIT ou Apache-2.0, au choix de l'utilisateur.
- Cible de rendu : la classe des moteurs logiciels de 1996 à 1998 plutôt que
  palette et texture affine. Couleurs directes, mipmaps, lightmaps, cellules 3D,
  rendu par tuiles, virgule fixe après la projection.
- Le noyau est le paquet racine du dépôt ; frontière C, étage d'accueil et
  conformance restent dans `crates/`.
- Frontière C arrêtée : codes de retour entiers à plages réservées par étape,
  message d'erreur en anglais, objet empoisonné après une panique, structures
  figées et complétées par des champs réservés, tampon de sortie en RGBA. Le
  header et les hôtes s'écrivent contre elle.
- Le header s'inclut depuis C++ : gardes `extern "C"`, et assertions de
  disposition vérifiées par un compilateur C++ comme par un compilateur C.
- Sur wasm, une panique est un trap et non un code de retour : la chaîne stable
  n'y déroule pas la pile, et ni `SCG_ERR_PANIC` ni `SCG_ERR_POISONED` n'y
  sont jamais rendus. Le texte de la panique est écrit avant le trap dans l'emplacement
  d'erreur sans contexte, que l'hôte lit dans la mémoire ; l'instance ne se
  réutilise pas.

***

**What this version does not do: it renders no scene.** A hardcoded
triangle, and nothing else: no scene to submit, no camera, no texture. Ending a
frame produces a complete image, which the four hosts — C, C++, wasm and
Android — hash identically.

**First published ABI: `SCG_ABI_VERSION` is 1.** A binding compares this value
with the loaded library's, for equality, before any other call. No file format
exists yet: `version_format` has no value.

### Added
- Triangle fill through fixed-point edge functions, with the top-left rule: two
  triangles sharing an edge share its pixels, with neither gap nor overlap.
- Colour and depth buffers sized for the maximum resolution at context creation,
  and never reallocated afterwards.
- Seven entry points: ABI version, context creation and destruction, frame end,
  last error message, buffer allocation and release. Each goes through a single
  wrapper that pins the floating-point environment, catches panics and turns
  errors into return codes. A host that unmasked floating-point exceptions finds
  them masked during the call, and untouched on return.
- `ScgContextConfig` offsets checked on both sides of the boundary: a Rust test,
  and static assertions the host's compiler verifies on its own target.
- Headless C host, linked against the static library on Windows and Linux, and
  run by `make test`: it checks rejections, writes outside the buffer,
  alignment and the floating-point environment as seen from C, and compares its
  triangle hash with the Rust path's.
- Headless C++ host, linked against the shared library on Windows and Linux, and
  run by `make test`: the same checks, plus the header compiled as C++ and proof
  that the functions come from the loaded library.
- wasm host, in dependency-free JavaScript: a headless test under Node, run by
  `make test`, and a page served by `make web`. It loads the module with no
  imports at all, allocates through `scg_buffer_alloc`, checks the same
  rejections and the views detached when memory grows, and compares its hash
  with the Rust path's. The wasm module ships alongside the other archives.
- Android host: a JNI layer in C, a Gradle-free app that writes straight into a
  bitmap's memory, and a two-stage test run by `make test`. The three ABIs
  without a device — aarch64 and armv7 under emulation — hostile floating-point
  environment included; then an x86_64 emulator, in C and through JNI on a
  misaligned buffer. Every hash must match the Rust path's. The library for the
  three ABIs ships, laid out like `jniLibs/`.
- Image hash: 64-bit FNV-1a over the dimensions then the visible area, which
  each host recomputes in its own language.
- Conformance suite: the triangle hash is a versioned reference, which
  `make conform` compares byte for byte after rendering with 32 and 64 pixel
  tiles. Each host compares its own with the Rust path: all four meet on the
  same file.
- A test proves that no frame allocates once the context exists, the first one
  included.
- Five-crate workspace: std-free core, C boundary, published libraries, Rust
  front end, conformance suite.
- The libraries are named `screengine`: `screengine.dll`, `libscreengine.so`,
  `libscreengine.a`, `screengine.wasm`. On Linux and Android the shared library
  carries the `libscreengine.so` SONAME.
- `screengine-play`, to make a game in Rust without writing a host: window,
  keyboard and mouse, fixed-step loop, image scaled up by an integer factor or
  filling the window. `make run` opens the example.
- The message for a rejected argument names it: resolution, tile size, `stride`
  or buffer length. The return code is unchanged.
- ABI contract, Rust conventions and build documentation.
- Continuous integration and tag-triggered release.

### Changed
- Dual licensed under MIT or Apache-2.0, at the user's option.
- Rendering target: the 1996–1998 software renderer class instead of palette
  and affine texturing. True colour, mipmaps, lightmaps, 3D cells, tile-based
  rendering, fixed-point after projection.
- The core is the repository's root package; C boundary, Rust front end and
  conformance stay in `crates/`.
- C boundary settled: integer return codes in per-stage reserved ranges, English
  error message, object poisoned after a panic, frozen structures extended
  through reserved fields, RGBA output buffer. The header and the hosts are
  written against it.
- The header can be included from C++: `extern "C"` guards, and layout
  assertions checked by a C++ compiler as well as a C one.
- On wasm, a panic is a trap, not a return code: the stable toolchain does not
  unwind there, so neither `SCG_ERR_PANIC` nor `SCG_ERR_POISONED` is ever
  returned. The
  panic text is written before the trap into the context-free error slot, which
  the host reads from memory; the instance is not reused.
