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

## [0.1.0] — 2026-09-21 — Le pipeline

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
