# Conventions Rust

`CONTRIBUTING.fr.md` en donne le résumé exigible. Ce document en porte le détail
et les raisons : ce qu'une règle écarte, pour qu'on puisse la rouvrir sans
rejouer la discussion.

Les invariants — ABI comme contrat, noyau sans système, zéro allocation par
image, déterminisme au bit près, rien du jeu dans le moteur — commandent tout ce
qui suit. Une convention qui entrerait en conflit avec l'un d'eux est fausse.

## Chaîne

- **Rust stable**, édition 2024, `rust-version = "1.85"` déclaré dans le
  `Cargo.toml` de l'espace de travail. Pas de fonctionnalité nightly, y compris
  dans les hôtes : une cible qui n'existe qu'en nightly n'est pas une cible.
- L'édition 2024 impose `#[unsafe(no_mangle)]` et `unsafe extern`. Ce n'est pas
  une contrainte de plus : c'est la marque, dans le texte, de chaque endroit où
  le compilateur cesse de garantir quelque chose.

## Découpage des crates

```
Cargo.toml          paquet screengine et espace de travail
src/                le noyau ; un sous-dossier par module, créé avec son étape
tests/              tests d'intégration du noyau
examples/           usage de l'API Rust du noyau
benches/            mesures du noyau
crates/
  screengine-ffi/           src/, tests/
  screengine-lib/           src/, build.rs
  screengine-play/          src/, examples/, assets/
  screengine-conformance/   src/, references/, tests/
```

`tests/` et `examples/` du noyau n'existent pas encore : ils naîtront avec leur
premier fichier.

**`benches/` porte la référence de performance**, prise avant que l'étape 3
touche au remplissage — une régression ne s'attribue pas sans mesure d'avant.
Deux cas seulement : un quadrilatère texturé plein cadre, qui isole la boucle de
pixels, et une scène chargée, qui donne le coût réel avec la répartition et la
recopie. Harnais maison, `#[bench]` n'existant qu'en nightly et le noyau
n'admettant aucune dépendance.

**Un fichier par chemin qu'une étape va changer**, et non un fichier qui grossit.
`carte.rs` mesure le chargement d'une carte et sa soumission par image, prise
avant que la traversée par portails remplace l'une des deux ; il intègre
`hosts/couloir.world` par `include_bytes!`, le noyau n'ouvrant aucun fichier et
un bench qui encoderait sa propre carte ne mesurant qu'un encodeur écrit pour
lui.

**`make bench` est hors de la liste fixe, et rien n'y échoue.** Une durée dépend
de la charge de la machine : en faire un contrôle le rendrait rouge pour des
raisons étrangères au code, et son seuil finirait relevé jusqu'à ne plus rien
mesurer. Les chiffres se comparent à la main, contre ceux que le fichier porte
en commentaire.

**Une mesure qu'on garde dit la charge de la machine au moment où elle a été
prise, et alterne les versions comparées.** Sans la charge, un chiffre n'est pas
une référence : on ne sait plus s'il vaut pour la machine au repos ou pour la
machine qui compilait à côté. Sans l'alternance, la dérive thermique et ce qui
démarre entre deux mesures s'imputent au code. Un poste de travail ordinaire
rend des écarts de quelques pour cent d'une exécution à l'autre, ce qui suffit à
masquer une régression réelle **ou** à en inventer une : une machine dédiée,
mesurée au repos et en alternance, donne des tours reproductibles à moins d'un
pour cent.

Une seule chose y échoue, et elle ne mesure rien : **le contrôle de couverture**.
Une scène mal cadrée ou prise de dos se chronomètre très bien et ne dit rien —
c'est arrivé à l'écriture même de ce fichier, où les deux cas rendaient zéro
pixel peint pour des chiffres flatteurs.

**Le noyau est le paquet racine**, avec la disposition standard de Cargo ; les
quatre autres crates la reprennent chacun dans `crates/`. Un répertoire naît avec
son premier fichier : pas de `tests/` vide en attendant le premier test.

**Un seul paquet ne suffirait pas.** Les dépendances d'un paquet valent pour
toutes ses cibles : l'étage d'accueil et la conformance y feraient entrer les
leurs dans le noyau. Et la frontière C exige `std` et `unsafe`, que le noyau refuse.

| Crate | Rôle | `std` | Dépendances | `unsafe` |
|---|---|---|---|---|
| `screengine` | le noyau : maths, pipeline, rasteriseur, formats, monde | non | aucune | chemins SIMD seulement |
| `screengine-ffi` | la frontière C, en rlib | oui | `screengine` | oui |
| `screengine-lib` | les bibliothèques publiées, `cdylib` + `staticlib`, sous le nom `screengine` | oui | `screengine-ffi` | non |
| `screengine-play` | étage d'accueil : fenêtre, entrées, boucle à pas fixe, mise à l'échelle, décodage PNG | oui | `winit`, `softbuffer`, `png` | non |
| `screengine-conformance` | scènes de référence, empreintes | oui | `screengine` | l'allocateur qui compte, dans son test seulement |

**Un crate se crée pour une contrainte de compilation, jamais pour ranger.** Les
cinq existants se justifient chacun par un besoin que les autres ne partagent
pas : le noyau refuse `std` et les dépendances, la frontière exige `std`, la
conformance et l'étage d'accueil portent les leurs, et `screengine-lib` donne
aux bibliothèques publiées le nom `screengine`, que la rlib de la frontière ne
peut pas porter sans entrer en collision avec celle du noyau. Ranger, c'est l'affaire des modules, qui
ne coûtent ni `Cargo.toml`, ni arbre de dépendances, ni frontière publique à
maintenir. Le module de collision de l'étape 7 en est le cas limite : il doit
servir sans rendu, mais c'est l'ABI qui l'expose séparément, pas un crate — un
serveur de jeu qui ne dessine rien n'embarque pas le rasteriseur, que l'édition
de liens écarte.

### Disposition du noyau

Un sous-dossier par domaine, **créé avec l'étape qui écrit son premier fichier**.
La carte est ici pour qu'une étape n'ait pas à choisir où poser ses fichiers ;
elle ne crée aucun répertoire d'avance.

```
src/
  lib.rs  error.rs  buffer.rs  context.rs  scene.rs   avant tout domaine
  math/       étape 1   vecteurs, matrices, quaternions, tables, virgule fixe
  raster/     étape 1   clipping, fonctions de bord, profondeur, tuiles ; simd/ à l'étape 9
  texture/    étape 2   mipmaps, filtrage
  light/      étape 3   lightmaps, brouillard, post-traitement de tuile
  format/     étape 4   maillage et carte, versionnés
  world/      étape 5   cellules, portails, traversée, calcul des lightmaps
  collide/    étape 7   balayage de boîte contre les cellules
```

**`raster/simd/` est le seul endroit du noyau qui autorise `unsafe`**, et le
scalaire qui lui sert de référence reste dans `raster/` : les fondre supprimerait
la référence.

**Le calcul des lightmaps est dans `world/` et non dans `light/`**, dont il
partage pourtant le sujet : son unité est la cellule, il lit la carte et la
traversée de portails lui donne son ensemble d'occulteurs. `light/` porte ce qui
s'applique pendant une image — échantillonnage, brouillard, post-traitement de
tuile —, et le calcul n'y met jamais les pieds.

**Le sens des dépendances ne s'inverse jamais.** Rien dans `screengine` n'importe
`screengine-ffi`. Si le noyau en avait besoin, c'est que de la logique serait
descendue dans la couche d'adaptation, ou que la frontière aurait fui vers le
noyau — les deux cassent la séparation.

**`screengine-ffi` ne contient aucune logique.** Elle convertit des types,
enveloppe les appels, traduit les erreurs en codes et formate les messages. Un
calcul qui y apparaît est un calcul que l'hôte wasm, qui passe par elle, et la
conformance, qui n'y passe pas, feraient différemment.

**Les types qui traversent la frontière sont déclarés dans `screengine-ffi`**, et
convertis vers ceux du noyau. `cbindgen` ne lit que ce crate : un type du noyau
qui apparaîtrait dans une signature exportée ne serait pas dans le header, et le
noyau se retrouverait tenu à une disposition `#[repr(C)]` qu'il n'a pas choisie.

### Deux chemins vers le moteur

**Faire un jeu** passe par `screengine-play`, en Rust : fenêtre, entrées et
boucle fournies. **Intégrer** passe par l'ABI C, depuis n'importe quel langage,
l'hôte gardant sa fenêtre, sa boucle et ses entrées.

- **`screengine-play` consomme l'API Rust du noyau, pas la frontière C.** Écarté :
  passer par l'ABI, qui l'aurait éprouvée à chaque lancement, au prix de handles
  opaques et de codes de retour là où un exemple doit tenir en quinze lignes. La
  conséquence est qu'aucun consommateur quotidien ne franchit la frontière : c'est
  à l'hôte C de l'éprouver dans la suite de tests, sans fenêtre, en comparant son
  empreinte à celle du chemin Rust.
- **Rien n'est atteignable par un seul chemin.** Ce que `screengine-play` permet
  se fait aussi par l'ABI ; le confort est réservé au chemin Rust, jamais la
  capacité. Sans quoi l'ABI cesse d'être le contrat.
- **L'inverse se vérifie aussi, et s'est raté une fois.** Les hôtes C, C++, wasm
  et Android ont chargé des fichiers pendant toute une étape sans qu'aucun
  exemple de `screengine-play` sache le faire : le chemin Rust était devenu le
  seul à ne pas lire de carte. L'exemple `carte` ferme l'écart. Ce qu'une famille
  d'hôtes sait faire, un exemple doit le montrer de l'autre côté.
- **`screengine-play` ajoute du comportement, jamais des données.** Pas de temps
  fixe, entrées, mise à l'échelle : oui. Un type de scène, de maillage, de
  matériau ou de lumière à lui : non, même pour un exemple — il réexporte ceux du
  noyau. Deux modèles de scène qui divergent est la seule façon de rater ce crate.
- **Le noyau ignore son existence**, et aucune fonctionnalité du noyau n'existe
  pour lui seul. Les invariants du noyau — `no_std`, zéro allocation par image,
  déterminisme — ne s'y appliquent pas.
- **`winit` et `softbuffer` y sont confinés.** `winit` change son API à chaque
  version mineure : c'est le seul endroit du projet qui subira des ruptures
  régulières. Écartés : `pixels` et `wgpu`, qui font dépendre l'affichage d'un
  moteur logiciel d'un pilote graphique, et SDL, qui ajoute une bibliothèque C à
  construire.
- **Le décodage d'images y est aussi**, par `png`. Le moteur n'ouvre aucun
  fichier et ne connaît aucun format : il reçoit un bloc de texels RGBA. Un hôte
  écrit contre l'ABI C décode avec ce que sa plateforme lui donne, et ce crate
  fait le même travail pour le chemin Rust.

## Dépendances

- **Le noyau n'en a aucune**, `dev-dependencies` comprises. Pas même `libm`,
  pourtant `no_std` : le déterminisme exige que la trigonométrie passe par les
  tables du noyau, et une dépendance de test finit toujours par servir hors des
  tests. Le générateur pseudo-aléatoire des tests est écrit dans le module de test
  qui s'en sert : une dizaine de lignes, et une graine fixe.
- `screengine-ffi` n'en a pas davantage : `std` suffit à ce qu'elle fait.
- L'étage d'accueil et la conformance en portent, et toutes passent
  `make deny`. Seule une dépendance qui voyage dans une archive publiée entre
  dans `THIRD-PARTY-NOTICES` ; celles de l'étage d'accueil n'y voyagent pas.
- Choisir une version, épingler, justifier un épinglage : voir
  `CONTRIBUTING.fr.md`, « Corriger une vulnérabilité sans en créer une autre ».

## `no_std`

- `#![no_std]` en tête de `src/lib.rs`, sans condition, et
  `extern crate alloc`.
- **Pas de `#![cfg_attr(not(test), no_std)]`.** Cette forme rend `std` disponible
  à tout le crate pendant les tests, et un `use std::` écrit dans du code non test
  compile alors sous `cargo test`. Un module de test qui a besoin de `std` le
  déclare lui-même : `extern crate std;` dans le `mod tests`.
- **Le noyau ne déclare aucune fonctionnalité Cargo.** Une fonctionnalité `std`
  « pour les messages d'erreur » ou « pour déboguer » est exactement le
  contournement que l'invariant interdit.
- **La preuve est `make nostd`**, qui compile le noyau pour
  `thumbv7em-none-eabihf`. `cargo build --no-default-features` ne prouve rien :
  sur la cible hôte, `std` reste dans le sysroot et un `use std::` passe au
  travers. La cible choisie est aussi 32 bits, ce qui fait apparaître une
  hypothèse sur la largeur de `usize` au même moment qu'un `std` oublié.

## Erreurs, paniques et marqueurs de stub

- **Ce qui peut échouer rend un `Result`.** Le noyau définit son type d'erreur,
  une énumération sans chaîne de caractères : le message se formate dans
  `screengine-ffi`, qui traduit chaque variante en code d'ABI, et dans
  `screengine-play`. Une variante nomme une catégorie, celle d'un code d'ABI ;
  ce qui la précise — l'argument refusé — est porté par la variante, et ne se
  lit que dans le message.
- **Aucun `unwrap` ni `expect` sur un chemin atteignable**, noyau compris. Un
  invariant se tient par le type ; ce qui ne se tient pas par le type remonte en
  erreur.
- **Une panique signale un défaut du moteur**, jamais une entrée invalide. Une
  carte malformée rend `SCG_ERR_INVALID_FORMAT` ; elle ne panique pas, même en
  débogage.
- **`debug_assert!` pour les invariants internes coûteux**, `assert!` pour ceux
  dont la violation corromprait la mémoire ou la sortie en release.
- **Les stubs portent leur étape** : `todo!("étape 5 : traversée de portails")`.
  C'est ce que compte la mesure d'avancement du `ROADMAP`. Un `todo!()` nu, un
  `unimplemented!()` ou un commentaire `// TODO` ne comptent nulle part et sont
  refusés.
- Un stub panique, la couche FFI rattrape la panique, et l'hôte reçoit
  `SCG_ERR_PANIC`. Les notes d'une version qui en contient disent ce qu'elle ne
  fait pas encore.

## `unsafe`

- **Uniquement dans `screengine-ffi` et dans les chemins SIMD du noyau.** Le
  noyau déclare `#![deny(unsafe_code)]` ; chaque module SIMD l'autorise
  localement, et c'est la seule autorisation du crate. Une exception hors du code
  livré : l'allocateur global du test d'allocation de la conformance, parce que
  `GlobalAlloc` est un trait `unsafe` et qu'aucune autre voie ne voit une
  allocation.
- **Chaque bloc porte un commentaire `// SAFETY:` qui nomme l'invariant tenu**, et
  qui le tient. « L'appelant garantit que `ptr` pointe vers `stride × hauteur`
  pixels, précondition documentée dans le header » est un commentaire ;
  « pointeur valide » n'en est pas un.
- Une fonction `unsafe fn` documente ses préconditions dans une section
  `# Safety` de sa doc. En édition 2024, son corps n'est pas implicitement
  `unsafe` : chaque opération y reprend son propre bloc et son propre
  commentaire.
- Un `unsafe` sans commentaire est un défaut, même correct. Le lint
  `clippy::undocumented_unsafe_blocks` le fait refuser par `make lint`.
- **Le tampon de l'hôte s'écrit par accès non alignés**, chemins SIMD compris. Le
  moteur ne contrôle pas son alignement — une section DIB sous Windows, le
  pointeur rendu par le verrouillage d'un bitmap sur Android —, et `stride` étant
  libre, celui de la base ne se propage pas aux lignes : une base sur seize
  octets avec un `stride` impair en désaligne une sur deux. Donc
  `_mm_storeu_si128`, jamais `_mm_store_si128` : un `movaps` désaligné est une
  faute franche sous Windows x64, et armv7 est la cible qui la révélera.
- **Aucune longueur avancée à travers un appel qui peut paniquer.** Sortir une
  valeur d'un tampon ou allonger sa longueur avant d'avoir réinitialisé ce qu'on
  laisse derrière ouvre une fenêtre où le dépliage libère deux fois — un défaut
  qui a valu son avis de sécurité à plus d'une bibliothèque. Le
  `#![deny(unsafe_code)]` du noyau le tient partout ailleurs ; dans un module
  SIMD, une indexation, un `debug_assert!` ou un débordement sous
  `overflow-checks` suffisent à ouvrir cette fenêtre.

## Frontière C, côté Rust

Le contrat est dans [`abi.md`](abi.md). Ce qui suit est la manière de l'écrire.

- **Un seul utilitaire enveloppe tous les points d'entrée** : `catch_unwind`,
  traduction de l'erreur en code, mémorisation du message pour `scg_last_error`.
  Écrit dès le premier point d'entrée, jamais recopié. Un point d'entrée ajouté
  sans lui est le défaut le plus discret du projet : il ne se manifeste que le
  jour où quelque chose panique, chez quelqu'un d'autre.
- **Aucun `enum` ni `bool` Rust dans une signature exportée ou une structure
  `#[repr(C)]`.** Un discriminant ou un octet invalide reçu d'un hôte y est un
  comportement indéfini, avant même la première ligne de vérification. On reçoit
  un entier, on le convertit en `enum` par `TryFrom`, et l'échec rend
  `SCG_ERR_INVALID_ARGUMENT`.
- **Un tableau de structures reçu de l'hôte ne se copie pas et ne se
  réinterprète pas.** Le copier serait une allocation par image ; le
  réinterpréter imposerait au noyau la disposition mémoire de la frontière,
  qu'il n'a pas choisie et que `rustfmt` ne garantit pas. Le noyau expose donc
  la soumission sous une forme qui lit chaque triangle par une fonction
  d'accès, et la frontière lui donne celle qui lit ses propres structures. La
  forme par tranches reste celle qu'emploie un appelant Rust.
- **Les tuiles partagent le handle entre threads.** Le contexte du noyau y vit
  dans un `UnsafeCell`, et l'état de rendu du noyau, atomique, décide de
  l'accès : les tuiles ne forment jamais qu'une référence partagée, et un appel
  exclusif ne forme la sienne qu'après avoir vu le rendu fermé. Une tuile écrit
  son message dans l'emplacement par thread et sa défaillance dans des
  atomiques ; le tampon de l'hôte lui est remis ligne par ligne, jamais en une
  tranche qui chevaucherait celle d'une autre tuile.
- **Un handle est un `Box` converti** par `Box::into_raw`, rendu par
  `Box::from_raw` à la destruction et jamais ailleurs. Le type pointé est opaque
  pour `cbindgen`.
- **`AssertUnwindSafe` ne se pose qu'en un point**, dans l'utilitaire
  d'enveloppe. C'est l'état défaillant de l'objet, décrit dans `abi.md`, qui rend cette
  assertion honnête : l'état non spécifié qu'une panique laisse derrière elle
  n'est plus jamais observé.
- **L'emplacement d'erreur par thread n'a pas de destructeur.** Un tableau
  d'octets de taille fixe, pas un `String` ni un `RefCell` : un thread-local à
  `Drop` coûte une clé pthread — Android en plafonne le nombre par processus — et
  surtout son accès échoue pendant la destruction du TLS du thread. Ce serait une
  panique dans `scg_last_error`, la seule fonction sans code de retour pour la
  porter. Un message trop long est tronqué, jamais alloué.
- **Le `Drop` d'un objet exporté ne panique jamais**, donc pas d'`assert!` en
  destruction. `scg_destroy` rend `void` : une panique y surviendrait après le
  défaillance, sans rien pour la transporter jusqu'à l'hôte.
- **Le même utilitaire fixe l'environnement flottant** à l'entrée — arrondi au
  plus proche, DAZ et FTZ désactivés, exceptions masquées, dans MXCSR sur x86
  et FPCR sur ARM — et rend celui de l'hôte à la sortie, panique comprise. Le
  noyau ne touche jamais à ces registres : il suppose l'environnement par
  défaut, et c'est la frontière qui le lui garantit.
- **La documentation des éléments exportés est en anglais**, et elle est la
  documentation du header : durée de vie du message d'erreur, préconditions sur
  les pointeurs, obligation de `scg_buffer_alloc` sur wasm. Ce qui n'y est pas
  n'existe pas pour un auteur de liaison.
- `panic = "abort"` rendrait tout cela inopérant : la bibliothèque se construit
  avec le profil `release-ffi`. Voir [`construction.md`](construction.md).
- **Sur wasm, une panique est un trap**, quel que soit le profil, et rien de ce
  qui précède ne la rattrape. Un crochet de panique, posé une seule fois par
  l'utilitaire d'enveloppe sous `cfg(target_arch = "wasm32")`, en écrit le texte
  dans l'emplacement sans contexte avant l'arrêt : c'est la seule trace que
  l'hôte en garde. Une fois, parce que `set_hook` alloue.

## Arithmétique et précision

**Même scène, même tampon, sur toutes les cibles, tous les chemins SIMD, toutes
les tailles de tuile et tous les nombres de threads.** C'est ce qui fait de la
conformance un détecteur de régression multi-plateforme.

**La virgule fixe commence à la projection.** Les flottants s'arrêtent à la
transformation des sommets ; tout ce qui suit est entier. C'est ce qui rend le
déterminisme accessible plutôt que coûteux : une multiplication entière rend le
même résultat en scalaire, en NEON, en SSE et en simd128, et aucun registre de
l'hôte n'y change rien. Les formats ci-dessous se figent avant le premier
remplissage — celui du triangle de l'étape 0 — parce qu'ils donnent leur forme
aux fonctions de bord.

### Côté flottant

- **Uniquement la transformation des sommets et la projection**, en `f32`, avec
  un ordre d'opérations unique écrit une fois. Une variante SIMD reproduit cet
  ordre ; elle ne le « simplifie » pas.
- **Le plan proche se clippe en espace homogène**, avant la division. Les plans
  de la bande de garde aussi, pour que les coordonnées écran tiennent dans leur
  format ; les côtés de l'image se traitent ensuite par découpe du rectangle.
- **Le passage en sous-pixels se fait dans une seule fonction**, arrondi au plus
  proche, testée sur des valeurs négatives : la conversion `as` tronque vers zéro,
  et `-0.4` et `0.4` ne tomberaient pas du même côté.
- **Aucun appel à la libm.** Pas de `sin`, `cos`, `tan`, `sqrt`, `powf`, `exp`,
  `ln` ni de leurs cousins : leur résultat dépend de l'implémentation — musl,
  Darwin, le CRT de MSVC. Trigonométrie, racine inverse, exponentielle et
  logarithme passent par les tables et polynômes du noyau. Le `no_std` en écarte
  la plupart d'office ; la règle vaut aussi dans les crates qui ont `std`, dès
  qu'un résultat entre dans une empreinte.

  **Ce que la liste de `clippy.toml` doit couvrir en `f64` autant qu'en `f32`.**
  Le noyau n'y accède pas — ces méthodes sont `std` —, mais la frontière et la
  conformance, si, et une liste tenue d'un seul côté laisse le chemin ouvert
  précisément là où une empreinte se calcule.
- **Le `f64` est permis hors image, interdit par pixel.** Il ne vivait jusqu'ici
  que dans les `const fn` qui remplissent les tables et dans les tests ; une
  table calculée au moment d'un réglage l'emploie aussi à l'exécution. Le
  déterminisme n'en souffre pas : IEEE 754 impose les quatre opérations au bit
  près en `f64` comme en `f32`, et une cible sans unité double passe par
  `compiler-builtins`, correctement arrondi lui aussi. Ce qui l'exclut d'une
  image n'est pas sa justesse mais son coût, et la virgule fixe après
  projection, qui ne laisse de toute façon aucun flottant arriver au pixel.
- **Pas de `mul_add`.** Sur une cible sans instruction FMA, il retombe sur la
  libm ; sur une autre, il ne rend pas les mêmes bits qu'une multiplication
  suivie d'une addition.
- **Opérations admises sur les flottants** : addition, soustraction,
  multiplication, division, comparaisons, conversions, et `abs`, `copysign`, la
  négation, qui ne touchent que le bit de signe. IEEE 754 les définit au bit
  près, et Rust ne contracte ni ne réassocie jamais d'expression flottante de
  lui-même. `min`, `max`, `clamp` et `signum` s'écrivent en comparaisons
  explicites : leur traitement de NaN et de −0 n'est pas celui des chemins SIMD.
- **`clippy.toml` refuse le reste**, par `disallowed-methods` : libm, arrondis de
  bibliothèque, `mul_add`, `min` et consorts. Un invariant qui ne tient qu'à la
  relecture ne tient pas. L'étage d'accueil a le sien, vide, parce que ses
  calculs flottants n'entrent dans aucune empreinte.
- **Conversion flottant → entier par `as`**, dont Rust définit la saturation. Pas
  de `floor` ni de `round` de bibliothèque : l'arrondi s'écrit sur la conversion,
  et son sens est commenté. **La valeur est ramenée dans l'intervalle de l'entier
  avant la conversion**, par un test écrit : un dépassement sature en Rust
  scalaire et rend `0x80000000` en SSE, et la saturation ne sert jamais de
  bornage.
- **NaN se teste nommément**, par `is_nan`, avant les comparaisons : toute
  comparaison avec lui est fausse, et un refus écrit `x <= seuil` le laisserait
  passer.

### Repère et transformations

- **Monde en main droite, Z en haut.** La vue de dessus de l'éditeur est
  directement (x, y), et la gravité suit un seul axe. Un format en Y vers le haut
  se convertit dans son chargeur, jamais dans le noyau.
- **Repère de vue : X à droite, Y vers le bas, Z vers l'avant**, et `w = z_vue`.
  Ce triplet est direct, comme le monde : la matrice de vue reste une
  transformation rigide, donc son inverse est une transposition et une
  translation, sans division. Écarté : Y vers le haut, qui rendrait le repère
  indirect et ferait porter un miroir à la matrice de vue — `inverse_rigid`, dont
  la précondition est l'absence d'échelle, en rendrait alors un faux inverse sans
  rien signaler.
- **Vecteur colonne, `M·v`, stockage par colonnes.** Le noyau travaille en
  `Affine3`, 3×4 : trois colonnes puis la translation. **Pas de pile de
  matrices** : la matrice **modèle** — objet vers monde — est un paramètre de la
  soumission, et le noyau y compose la vue par `product`. Une pile n'aurait de
  consommateur ni dans le noyau ni dans l'ABI, et le confort du chemin Rust ne
  crée jamais de capacité qui lui soit propre. Une composition se lit
  `parent.product(local)`, la droite s'appliquant d'abord.
- **La soumission reçoit la matrice modèle, jamais la modèle-vue.** Le contexte
  porte la caméra, et lui seul inverse sa pose. Écartée : la modèle-vue, qui
  obligerait chaque hôte à composer cet inverse, donc à normaliser un quaternion
  par sa propre bibliothèque mathématique — et deux liaisons ne rendraient plus
  la même image, ce que l'ABI interdit d'ailleurs en propres termes : une
  liaison convertit des types et ne calcule rien.
- **La caméra est une position, une orientation, un champ de vision et un plan
  proche**, et le quaternion identité fixe les axes de vue ainsi :

  ```text
  X_vue (droite) ↦ −Y monde
  Y_vue (bas)    ↦ −Z monde
  Z_vue (avant)  ↦ +X monde
  ```

  Une caméra d'orientation neutre regarde donc le +X du monde, le zénith vers le
  haut de l'écran. Une phrase — « elle regarde le +X » — ne suffirait pas : elle
  laisse le roulis indéterminé, et c'est lui qu'une erreur de signe fait
  basculer sans changer la direction du regard. Écarté : aligner les axes de vue
  sur ceux du monde, qui ferait regarder le zénith par défaut. Le déterminant de
  cette base vaut un : la composition reste rigide, et `inverse_rigid` garde sa
  précondition. Étant une permutation signée, elle est exacte au bit près, et
  une scène écrite en coordonnées monde vue par une caméra neutre rend la même
  image que la même scène écrite en coordonnées de vue.
- **Le quaternion se range `x, y, z, w`**, la quatrième composante étant la
  partie réelle : l'identité est `{0, 0, 0, 1}`. C'est ce que l'ABI fige, parce
  qu'une liaison JavaScript écrit la structure octet par octet et que `w` en
  tête est la convention concurrente la plus répandue.
- **La projection n'est pas une matrice**, c'est `(sx, sy, cx, cy, near)`
  appliqué à part, avec un plan lointain infini : `z_c = near` et `w_c = z_vue`
  donnent directement la profondeur `near/w`. Les cinq valeurs dépendent de la
  résolution interne et se recalculent quand elle change, sans allocation. Avec
  des pixels carrés, `sx = sy = (hauteur/2)·cot(fov_y/2)`, le rapport d'aspect
  étant porté par la largeur seule ; les deux champs restent distincts pour
  qu'un pixel non carré n'exige pas une nouvelle API. Le centre est en
  `(largeur/2, hauteur/2)` et non `((largeur−1)/2, …)` : le centre du pixel `i`
  est en `i + 0,5`, ce que le rasteriseur code déjà par son demi-pas en
  sous-pixels. Le plan lointain infini n'est pas un raffinement : à distance
  finie, la profondeur cesse d'être un multiple constant de `1/w`, et `to_depth`
  recevrait autre chose que ce que sa documentation annonce.
- **Chaque somme s'écrit de gauche à droite, dans l'ordre des colonnes** :
  `((m0·x + m3·y) + m6·z) + m9`. Jamais d'arbre équilibré `(a+b)+(c+d)`, ni de
  boucle générique : c'est l'ordre qu'un chemin SIMD reproduit en accumulant
  colonne par colonne, et un test compare les bits à l'expression écrite.
- **Orientations en quaternions**, normalisés à la réception plutôt qu'exigés
  unitaires, interpolés par `nlerp` : pour dix degrés par pas, l'écart au
  sphérique reste sous le millième de degré, sans arc cosinus.
- **Face avant en sens antihoraire** dans les données. Le retournement que
  produit l'axe Y de l'écran, vers le bas, se traite dans le signe des fonctions
  de bord, sans permuter de sommets. **Nier la fonction de bord et transposer
  deux sommets sont la même règle**, expression entière pour expression entière :
  `edge` est exactement antisymétrique sur les entiers, et `is_top_left(d)` est
  le complémentaire de `is_top_left(−d)` sur les quatre cas. L'étanchéité des
  arêtes est donc conservée — à une condition, qui est réelle : **la négation est
  une constante du pipeline, jamais une décision par triangle.** Deux triangles
  adjacents traités différemment auraient des tests identiques au lieu de
  complémentaires, et leur arête commune serait revendiquée deux fois ou pas du
  tout. Corollaire : une surface à deux faces se soumet par son triangle miroir,
  jamais en levant la règle de signe.
- **Les angles sont binaires**, un `u32` où 2³² vaut un tour : le tour boucle
  par l'arithmétique modulaire, et les symétries entre quadrants sont exactes.
  Le sinus se lit dans une table d'un quart de cercle, 1024 intervalles
  interpolés, calculée par une `const fn` : le calcul flottant à la compilation
  est exact au bit près depuis Rust 1.82, là où un `build.rs` dépendrait de la
  libm de la machine qui construit.
- **La racine inverse** décompose le nombre par ses bits, estime par une table
  de 64 entrées et fait deux itérations de Newton, en forme corrective. Une
  longueur au carré sous 10⁻³⁰ rend le vecteur nul : aucun dénormal n'entre dans
  un calcul.
- **Dans les chemins SIMD, aucune intrinsèque fusionnée, relâchée ou
  approximative.** Rust ne fusionne jamais `a*b+c` de lui-même ; les intrinsèques,
  si : `vfmaq_f32` fusionne quand `vmlaq_f32` ne le fait pas. `relaxed_madd` et
  les autres opérations `relaxed-simd` sont non déterministes par définition.
  `rsqrtps` et `vrsqrteq_f32` sont des approximations dont les bits varient selon
  le fondeur. `minps` et `vminq_f32` ne traitent pas NaN pareil : une comparaison
  explicite les remplace. Une conversion flottant → entier vectorielle ne se fait
  qu'après bornage, `cvttps2dq` rendant `0x80000000` là où `as` sature.

### Côté entier

Les pires cas sont calculés pour une résolution interne de 2048 pixels de côté
au plus, dans une bande de garde de ±4096 pixels. Chaque constante porte ce calcul
en commentaire. C'est [`abi.md`](abi.md) qui rend cette borne opposable : au-delà,
la création du contexte rend une erreur plutôt que de déborder en silence.

| Grandeur | Format | Pourquoi |
|---|---|---|
| Coordonnées écran | `i32`, 28.4 (un seizième de pixel) | quatre bits suffisent à une résolution basse remontée en entier ; le pixel est échantillonné en son centre, à +8 |
| Bande de garde | borne **absolue** de ±4096 pixels sur la coordonnée écran, soit ±2¹⁶ en 28.4 | au-delà, le sommet est clippé en espace homogène. Absolue et non relative à l'image : gonfler l'image de 4096 autoriserait 6144 pixels en 2048 de large, et les bornes 2¹⁷ et 2³⁴ ci-dessous seraient fausses |
| Fonctions de bord | `i64` | un écart entre sommets atteint 2¹⁷ en 28.4, un produit 2³⁴ : `i32` déborde dès que la bande de garde sert |
| Règle top-left | biais de −1 sur les arêtes ni hautes ni gauches | deux triangles partageant une arête se partagent ses pixels, sans trou ni recouvrement |
| Profondeur | `u32`, `near/w` en 0.32, bornée par `to_depth` à 64 unités des bornes | plus grand est plus proche ; `near/w` est affine en espace écran, donc s'interpole par une équation de plan. La marge couvre l'arrondi des gradients : aucun bornage par pixel. Test strict : à égalité, le premier triangle soumis reste |
| Attributs interpolés | valeurs de sommet bornées à ±2³², gradients par sous-pixel en `i64` à 12 bits fractionnaires | équation de plan établie à la mise en place du triangle, gradients arrondis vers le bas par `div_euclid` sur une aire positive, point de référence au plus petit sommet en (y, x) ; évaluation en forme close, enveloppante, exacte sur les pixels couverts, à moins de 64 unités de la valeur exacte. **Les textures interpolent `u·z` et `v·z`**, tranché à l'étape 2 : ce sont eux qui sont affines en espace écran, et leur quotient par `z` absorbe l'erreur relative commune — un `1/w` séparé aurait demandé un plan de plus pour le même résultat |
| Coordonnées de texture après division | `i32`, 16.16 | textures en puissance de deux, repli par masque |
| Poids du bilinéaire | 8 bits, tirés des bits fractionnaires | mélange entier, arrondi `(… + 128) >> 8` |
| Coordonnées de lightmap | **le format des coordonnées de texture, à l'identique** : `lu·z` et `lv·z` interpolés par équation de plan, `i32` 16.16 après division | un second jeu `u, v` par sommet, et non une application affine des premières : celle-ci serait impossible sur un lot **sans** texture, où les coordonnées valent zéro partout — or un mur uni éclairé est le premier cas de l'étape. La division est partagée : la réciproque dépend de la profondeur seule, pas de l'attribut, et une seconde série coûterait cinq divisions `u64` de plus par segment |
| Combinaison texel × lightmap | `(t·l + t) >> 8` par canal, `l` sur 8 bits où **255 est le neutre** | **pas `(t·l + 128) >> 8`**, qui est la forme de la ligne au-dessus et ne vaut que pour des poids sommant à 256 : sur un facteur en 255 elle rend le blanc à 254 sous pleine lumière, un assombrissement de 1/256 sur toute surface éclairée. `t·(l+1) >> 8` rend le texel intact à `l = 255`, zéro à `l = 0`, sans division ni table. Écart à l'idéal au plus d'une unité, toujours éclaircissant, nul là où il se verrait |
| Sur-éclairement | décalage de contexte `k ∈ {0, 1, 2}` dans la combinaison : `min(255, t·(l+1) >> (8 − k))` | sans lui, toute surface éclairée est plus sombre que sa texture et la scène entière est terne. Dans la combinaison et non dans le post-traitement : appliqué après coup, un doublement ne rendrait que des valeurs paires, et éclaircirait aussi ce qui n'est pas éclairé. Un décalage plutôt qu'un facteur quelconque, pour que l'expression reste exacte et sans division. Saturation **écrite**, jamais laissée à une conversion |
| Facteur de brouillard | `u16` valant `f ∈ [0, 256]`, **256 = brouillard plein** | neuf bits et non huit, pour que le mélange soit exact **aux deux bouts** : une surface non embrumée doit sortir identique au rendu sans brouillard, et un décalage d'un seul niveau entre la géométrie lointaine et le fond effacé *est* la couture d'horizon. Mélange à deux voies dans `0x00FF00FF`, sans retenue entre elles — chaque voie vaut au plus `255·256 + 255`, soit 65 535, **et la marge est donc nulle** : l'arrondi porte aussi le tramage, qui monte jusqu'à 255, et non le seul demi de l'arrondi au plus proche. Rien ne peut s'ajouter à ce mélange sans élargir l'accumulateur | 
| Index de brouillard | exposant et mantisse de la profondeur, par `leading_zeros`, table de 2048 entrées | indexer linéairement une profondeur 0.32 est inutilisable : tout le monde visible vit sous 2²⁶. La table se remplit **linéairement en distance** — un brouillard linéaire en `near/w`, pourtant gratuit, atteint 56 % à un dixième de sa rampe et cesse d'être un indice de profondeur. L'index se prend comme celui du mipmap, et le reste de quantification se trame par la même table ordonnée, transposée pour ne pas se corréler avec celle des texels |
| Atténuation d'une lumière dynamique | `(1 − d²/r²)²` en `f32`, **par sommet**, portée par un plan comme les autres attributs | l'atténuation a besoin d'une distance, et il n'existe aucune distance du côté entier du pipeline : la racine inverse du noyau vit avant la projection. Le carré s'annule en `r` **avec une dérivée nulle**, donc sans l'anneau visible que `1 − d²/r²` seule dessine à son bord. Aucune racine n'est appelée |
| Courbe de sortie | table de **256 entrées de huit bits par canal**, `(x·gain)^(1/gamma)` saturé puis quantifié, remplie au réglage | le pixel ne paie que trois lectures, et rien du calcul qui les a produites — lequel emploie le `f64` et l'`exp2` du noyau, ce que seul un calcul hors image peut se permettre. L'état neutre est la **vacuité de la table**, et non un réglage d'identité : la recopie se monomorphise sur ce choix, si bien qu'une scène sans courbe ne teste rien par pixel. Le gain sature avant le gamma, pour ne pas écrêter deux fois |
| Fenêtre de portail | rectangle de **pixels entiers**, obtenu par min/max des sommets 28.4 du portail projeté puis arrondi **vers l'extérieur** | elle ne borne que la boucle, jamais les valeurs : l'image est identique avec ou sans elle, exactement comme elle l'est indépendamment des tuiles. C'est ce qui fait qu'une fenêtre n'a besoin d'aucun format nouveau — elle est de la même nature qu'un rectangle de tuile, et les cinq configurations de conformance l'éprouvent déjà |

- **Tout s'évalue en coordonnées globales.** Une fonction de bord ou un attribut
  en un pixel se calcule à partir des sommets et de la position du pixel dans
  l'image, jamais d'une valeur arrondie propre à la tuile. Rebaser les valeurs à
  l'origine d'une tuile est permis, parce qu'une translation entière est exacte.
- **La division de perspective a lieu aux multiples de 16 de l'abscisse dans
  l'image**, même quand ce point tombe hors de la tuile ; entre deux,
  l'interpolation est affine. Un segment qui commencerait au bord de la tuile
  donnerait une texture différente selon le découpage.

  **Les extrémités sont celles du span du triangle sur la ligne**, et non des
  multiples de 16 quelconques : `near/w` prolongé hors du triangle peut
  s'annuler ou devenir négatif, et il n'existe alors aucun quotient à prendre.
  Le span s'obtient par résolution entière des trois fonctions de bord, ce qui
  le rend rigoureusement égal à ce que le test pixel par pixel retiendrait —
  plus large, il ferait diviser là où la profondeur n'a pas de sens ; plus
  étroit, il trouerait le triangle. Il ne dépend que des sommets et de la
  ligne, donc pas de la tuile.
- **La fenêtre d'un portail se construit sans un seul arrondi nouveau.** Le
  portail est convexe, vérifié au chargement, donc un éventail depuis son premier
  sommet est licite sans découpe ; chaque triangle de l'éventail passe par le
  chemin flottant existant — projection, clipping par les cinq plans, passage en
  sous-pixels —, et la fenêtre est le min/max des sommets rendus. La boîte d'une
  union étant l'union des boîtes, le résultat est rigoureusement la boîte du
  portail, sans qu'aucun morceau ait à être recollé.

  Deux conséquences qu'il faut dire, parce qu'elles sont l'argument du choix : le
  découpage par les cinq plans garantit que tout sommet rendu est dans la bande de
  garde, donc que le passage en 28.4 est exact et tient dans ±2¹⁶ — aucune
  saturation ne sert de bornage ; et **l'ordre de l'éventail n'a pas
  d'importance**, min et max sur des entiers étant commutatifs. C'est le seul
  calcul dérivé du projet dont l'ordre d'opérations ne soit pas contractuel, et
  c'est une propriété, pas une chance.

  **Jamais de pincement des sommets** pour les ramener dans la bande de garde
  avant le passage en sous-pixels : tirer un sommet lointain sur ±4096 tire les
  arêtes vers l'intérieur et peut mordre dans l'image. Le découpage par les plans,
  lui, préserve exactement l'intersection.

  Le passage des sous-pixels au rectangle de pixels **inclut tout pixel que le
  polygone touche**, par `div_euclid` pour que le négatif tombe du bon côté, et
  non « tout pixel dont le centre est dedans ». La dilatation est d'au plus un
  pixel et ferme une classe entière de trous : le portail partage ses arêtes avec
  les triangles du mur qui l'entoure, la règle top-left départage les centres
  tombant dessus, et une fenêtre au centre près pourrait exclure un centre que la
  géométrie exclut aussi. Un trou est définitif, un pixel dilaté est gratuit.
- **Deux fenêtres, qui ne se confondent pas.** Celle de **propagation** décide des
  cellules visitées et de la profondeur atteinte : elle est par chemin, vit sur la
  pile de traversée, et ne s'agrège jamais. Celle de **bornage** est ce que le
  remplissage reçoit : elle est par cellule, union des fenêtres par lesquelles la
  cellule a été atteinte, ce qui garde une seule soumission par cellule.

  Agréger la première ferait dégénérer la fenêtre à la taille de la cellule dès
  que deux ouvertures écartées y mènent, et perdrait l'élimination fine que le
  z-buffer nous permet précisément de garder. Unir la seconde ne peut que
  sur-dessiner, jamais trouer, et un sur-dessin de boîte coûte quelques
  millièmes d'image : ce qui est rédhibitoire pour l'une est gratuit pour l'autre.
- **Réduire la fenêtre par le portail découpé, et non par sa boîte brute.**
  `box(P ∩ W)` vaut le même rectangle que `box(P) ∩ W` dans presque tous les cas,
  et un facteur deux et demi sur un grand portail oblique vu à travers une porte
  étroite. Ce n'est pas le remplissage que cela gagne, c'est le liseré où naissent
  les faux portails — donc les cellules ramenées pour rien. Une passe sur les
  arêtes, en `i64`, hors de toute boucle de pixels, sans tampon : on n'en calcule
  que les extrema, jamais le polygone découpé, ce qui coupe la cascade d'arrondis
  qu'un clipping entier réinjecté dans un autre clipping entraînerait.
- **Ne jamais replier la fenêtre dans la boîte englobante du triangle.** C'est la
  simplification qui se présentera d'elle-même — plus de table de fenêtres, des
  répartitions resserrées gratuitement — et elle change l'image : `x0` et `x1`
  bornent le span, dont dépendent les extrémités des segments de perspective, et
  un segment rabattu sur une fenêtre texturerait la même surface autrement selon
  la traversée. Les bornes verticales, elles, sont de purs bornages de boucle et
  se replieraient sans dommage : l'asymétrie est réelle et c'est pour cela qu'elle
  est écrite.
- **Le niveau de mipmap se choisit par segment de 16 pixels**, sur la même grille,
  à partir de la dérivée entière des coordonnées de texture ; le logarithme se
  prend par `leading_zeros`.
- **Le tramage ordonné** ajoute aux coordonnées 16.16, avant troncature, un
  décalage sous-texel tiré d'une table fixe indexée par la position du pixel dans
  l'image. C'est le filtrage par défaut ; le bilinéaire est le niveau au-dessus.
  La table et son motif sont figés à l'étape 2, et testés.
- **Les deux filtrages s'excluent**, et le bilinéaire reste dans un seul niveau
  de mipmap. Il **retranche un demi-texel** avant de prendre son voisinage : le
  centre du texel `(0, 0)` est en `(0,5, 0,5)`, et sans ce recentrage l'image
  glisserait d'un demi-texel au changement de filtre. Le repli par masque
  s'applique à chacun des quatre voisins séparément, de sorte qu'une surface
  pavée mélange son dernier texel avec le premier.
- **Un attribut nouveau n'entre pas dans le triangle préparé, mais dans un
  tableau annexe compacté.** La répartition par tuile parcourt le tableau des
  triangles **deux fois par image** et n'y lit que la boîte englobante, seize
  octets sur cent vingt-huit. Y ajouter des plans ferait streamer leur volume
  entier sur une passe qui ne les lit jamais — au défaut de capacité, plus d'un
  mégaoctet par image, sur la bande passante que le projet désigne comme la
  vraie limite du téléphone. Rangés ailleurs, les mêmes octets ne coûtent rien à
  cette passe. Le triangle préparé garde donc ses cent vingt-huit octets, deux
  lignes de cache, et porte des index vers l'annexe ; compactée aux seuls
  triangles concernés, pour qu'une scène sans lightmap n'écrive pas de zéros.
- **Dans une tuile, les triangles se dessinent dans l'ordre de soumission.** Avec
  le test de profondeur strict, c'est l'ordre qui tranche une égalité : une
  répartition qui le perdrait rendrait une image différente selon la taille des
  tuiles. La liste des grands triangles, testés par tuile plutôt que référencés
  dans chacune, se fusionne donc avec celle de la tuile par index croissant. Le
  rejet d'une tuile est conservateur : une référence de trop ne change rien, une
  tuile oubliée troue l'image dans une seule configuration.
- **Toute division est entière et arrondie dans un sens écrit** : la division
  `i64` de Rust tronque vers zéro, et une équation de plan dont le signe du
  dénominateur change doit rester continue.
- **Un débordement possible s'écrit explicitement** : `wrapping_`, `checked_` ou
  `saturating_`. Ne jamais s'en remettre au profil : le profil de développement
  garde `overflow-checks` et panique, le profil release enveloppe en silence, et
  l'empreinte de la conformance, construite en release, ne verrait rien.
- **`usize` pour indexer, rien d'autre.** Ce qui entre dans un format de fichier,
  une empreinte ou une signature d'ABI a une largeur fixe.

## Allocation

- **Toute allocation a lieu dans un appel nommé** : création du contexte,
  chargement d'une ressource, calcul de lightmaps. Jamais entre le début et la fin
  d'une image — ni pour générer un mipmap manquant, ni pour agrandir un atlas au
  premier affichage. C'est le défaut type : il transforme « zéro allocation par
  image » en « une réallocation au premier niveau chargé », sans que rien ne le
  signale.
- **Le contexte dimensionne ses tampons à la création** : triangles préparés pour
  la capacité reçue, répartition par tuile pour la résolution interne maximale.
  Changer de résolution sous ce maximum n'alloue rien.
- **Le clipping n'a pas de tampon de contexte.** L'intersection d'un triangle
  avec les cinq plans a au plus trois arêtes du triangle et cinq des plans, donc
  huit sommets et six triangles : deux tampons de huit tiennent sur la pile de
  l'appel. Le pire cas est prouvé, pas majoré — une capacité au jugé serait soit
  un gaspillage par triangle, soit un dépassement qu'aucun test ne rejoue.
- **Couleur et profondeur vivent sur la pile de l'appel de tuile**, en tableaux
  de taille fixe pour une tuile de 64 de côté. Autant de tampons que d'appels
  simultanés, sans que le noyau connaisse les threads de l'hôte ; `abi.md` en
  tire le minimum de pile exigé. La profondeur ne survit pas à la tuile.
- **Les ressources portent la mémoire qui dépend de la scène** : une texture
  alloue ses mipmaps à son chargement, une cellule ses lightmaps à leur calcul.
- **Un tampon de travail se vide par `clear()`**, jamais par une réaffectation ni
  un `Vec::new()`. Sa capacité est dimensionnée à la création, pour le pire cas de
  la scène, et un dépassement est une erreur rendue, pas une croissance.
- La preuve est un test, pas une relecture : voir « Tests ».

## Formats de fichier

Deux formats de source : le maillage et la carte. Un troisième genre du même
conteneur porte le cache de lightmaps, décrit plus bas dans « Lightmaps
calculées » : il n'est pas une source, et c'est la seule différence. Tous trois
partagent un en-tête, une table de sections et un décodeur, dans `src/format/`.
`abi.md` n'en voit rien — il ne connaît qu'un bloc d'octets et un handle opaque —,
et c'est ici que les dispositions font foi.

**Les dispositions sont figées avant le premier décodeur**, pour la même raison
que les formats de virgule fixe l'ont été avant le premier remplissage : ce qui
s'écrit après s'écrit contre ce qui existe déjà, et le format en garde la forme
pour toujours.

### En-tête et sections

Vingt octets : la signature `S C G 0x1A`, le genre en quatre octets ASCII
(`MESH`, `WRLD`), puis `version_format`, `total_length` et `section_count`, trois
`u32`.

La signature se coupe en deux : le préfixe commun refuse « ce n'est pas un
fichier de ce projet » avant même de savoir quel format était attendu, et le
genre distingue « un maillage là où une carte était attendue » d'une version non
supportée. L'octet `0x1A` est le marqueur de fin de fichier des systèmes de
l'époque : il ne coûte rien et attrape un fichier transféré en mode texte.
Écarté : porter le numéro de version dans la signature — `version_format` le
fait, et deux mécanismes de version finissent par diverger.

**`total_length` est exigé égal à la longueur reçue, pas inférieur.** Toute
troncature devient alors détectable au premier contrôle, et le champ sert de
borne unique à tous les décalages qui suivent. Tolérer des octets de queue
donnerait deux fichiers d'octets différents rendant la même image, ce qui
désaccorderait l'empreinte d'intégrité de l'hôte de celle du moteur.

Aucun champ réservé, contrairement aux structures de l'ABI : un champ réservé
existe parce qu'une structure publiée ne change plus, alors qu'un fichier porte
son extension dans son numéro de version. Aucune somme de contrôle non plus —
elle ne protège de rien face à un bloc hostile, qui la recalcule, et l'intégrité
de transport appartient à l'archive de l'hôte.

La table suit l'en-tête : `section_count` entrées de douze octets — genre sur
quatre octets ASCII, décalage et longueur en `u32`. **Les sections sont rangées
par genre croissant, au plus une de chaque, et pavent le fichier sans
recouvrement**, ce qui se vérifie avant qu'un seul champ ne soit lu. Sans cette
canonicité, le même contenu aurait plusieurs écritures légitimes : un choix dont
l'écrivain n'a pas besoin, une recherche dont le lecteur n'a pas besoin non plus.

**Une section de genre inconnu refuse le fichier.** Il n'y a pas de tolérance en
avant, et c'est la décision la moins intuitive du format. La raison est le
déterminisme : presque tout ce qui s'ajoutera à un format de rendu change ce qui
est rendu, et une bibliothèque qui saute en silence une section qu'une version
plus récente emploie rend **une autre image sur le même fichier, sans erreur**.
C'est précisément ce que la conformance existe pour attraper, et elle ne le
verrait pas, chaque version étant cohérente avec elle-même. Une section ne
deviendra ignorable que le jour où son absence ne pourra changer aucun pixel, et
elle le dira.

**Aucun alignement n'est exigé du bloc**, qui peut venir d'une lecture, d'une
projection à un décalage quelconque ou d'une entrée d'archive. Chaque champ se
lit par `from_le_bytes` sur une tranche : **jamais de réinterprétation d'une
tranche d'octets en tranche de mots**. Une lecture non alignée est une
instruction sur les quatre cibles, payée une fois au chargement et jamais par
pixel. Exiger un alignement obligerait tout hôte à passer par
`scg_buffer_alloc`, donc changerait une précondition de l'ABI, pour un gain non
mesurable.

### Ce que le décodeur tient pour hostile

Que `(ptr, len)` couvre bien `len` octets lisibles est une précondition : le
moteur ne peut pas le savoir. **Tout ce qui est à l'intérieur de ces octets est
hostile, sans exception** — c'est le périmètre que `SECURITY` décrit.

**Aucune capacité d'allocation ne vient d'un nombre déclaré, toujours d'une
longueur présente dans le bloc.** Tout compte se recoupe avec la longueur de sa
section, `count × taille == longueur` exactement, en arithmétique vérifiée.
C'est la bombe d'allocation classique : quarante octets qui annoncent quatre
milliards de sommets. `checked_mul` et non `*` — sur wasm32 et sur armv7, `usize`
fait 32 bits, et un produit déborde bien en deçà de ce qu'un fichier peut
déclarer.

Vérifié une fois au chargement, et jamais ensuite : signature, genre, version,
longueur totale ; le pavage de la table ; les comptes ; tout indice sous son
compte ; toute coordonnée finie, `is_nan` nommément avant les comparaisons de
bornes ; les identifiants non nuls, **triés, l'unicité lue en une passe
adjacente**. Le tri se vérifie, il ne se suppose pas : une recherche dichotomique
sur une table non triée ne plante pas, elle rend la mauvaise surface, et c'est un
défaut « image fausse, aucune erreur ».

Ne se vérifie pas : la cohérence géométrique — triangles dégénérés, faces
retournées. Rien de cela ne corrompt la mémoire, et refuser un état intermédiaire
d'éditeur contredirait l'édition à chaud.

**Ne se revérifie surtout pas ensuite** : une soumission ne reparcourt pas les
indices d'une ressource chargée. Un tableau reçu de l'hôte n'est pas validé et
doit l'être ; une ressource chargée porte l'invariant dans son type. C'est le
premier endroit du projet où une revalidation par image serait invisible — elle
ne ferait rougir aucun test, ne changerait aucune empreinte, et coûterait un
parcours complet par image.

**Un fichier malformé rend une erreur, il ne panique pas**, même en débogage.
C'est déjà la règle générale du projet, et c'est ici qu'elle se joue.

### Maillage

Quatre sections : `SURF`, `TEXN`, `TRIS`, `VTXS`.

| Élément | Disposition | Taille |
|---|---|---|
| Sommet | `x, y, z, u, v` en `f32` | 20 |
| Triangle | `i0, i1, i2` en `u32`, puis `r, g, b, a` en `u8` | 16 |
| Groupe de surface | `id`, `first_triangle`, `triangle_count`, `texture_slot` en `u32` | 16 |
| Nom d'emplacement | longueur en `u16`, puis les octets UTF-8, sans remplissage | variable |

Un seul format de sommet, pas de masque d'attributs : le rasteriseur a
exactement trois formes de sommet et n'en aura pas de quatrième sans une fonction
de soumission de plus. Les cinq champs sont ceux de `ScgVertexUv`, dans le même
ordre, **par convergence et non par dépendance** — le décodeur vit dans le noyau
et ne peut pas voir les types de la couche C, et lier une disposition de fichier
qui cassera à une structure publiée qui ne change plus ferait gouverner la
promesse forte par la faible.

Indices en `u32` et non `u16` : un second chemin de décodage et un plafond de
65 536 sommets qu'un décor fusionné atteint, contre six octets par triangle.
Couleur par triangle et non par groupe : elle sert aux surfaces sans texture, et
la porter par groupe économiserait un quart de la section au prix d'un modèle de
fichier différent de celui du moteur.

**Les groupes pavent l'intervalle des triangles dans l'ordre, sans trou ni
recouvrement, et c'est vérifié.** Une soumission vaut pour un groupe ; un groupe
dispersé imposerait de rassembler ses triangles à chaque image, donc un tampon,
donc une allocation par image.

**Un groupe nomme toujours son emplacement de texture**, son indice étant sous le
compte des noms comme tout autre indice. Il n'y a donc pas de valeur pour « sans
texture » dans le fichier, et un maillage qui porte des triangles déclare au
moins un nom : « sans texture » se dit à la soumission, par un handle nul, ce qui
garde à l'hôte le choix de ne rien charger pour un emplacement.

Pas de second jeu de coordonnées : une lightmap se calcule par cellule, et un
accessoire mobile n'est pas une cellule. Le jour où un décor statique se livrera
en maillage, ce sera une version de format de plus. **Ce format cassera de toute
façon à l'étape 6**, qui tranche la normale par sommet, et c'est prévu.

La boîte englobante se calcule au chargement et ne se stocke pas. C'est
l'argument des liens de portails transposé : une boîte stockée est une occasion
d'incohérence à maintenir à chaque opération d'éditeur, et une boîte *fausse* est
un défaut de rendu — un objet visible éliminé — qu'aucune validation ne peut
attraper sans la recalculer.

### Carte

Quatre sections : `CELL`, `ENTS`, `LGTS`, `MATS`. Sont **source** la table de
matériaux, les cellules avec leurs sommets, surfaces et portails, les lumières
statiques et les entités. Rien d'autre.

| Élément | Disposition | Taille |
|---|---|---|
| Entrée de matériau | `id` en `u32`, puis la longueur du nom en `u16` et ses octets UTF-8 | variable |
| Cellule | longueur de l'enregistrement en `u32`, puis `id`, `flags`, `vertex_count`, `surface_count`, `portal_count` en `u32`, puis les trois tableaux dans cet ordre | variable |
| Sommet de cellule | `x, y, z` en `f32` | 12 |
| Surface | `id`, `flags`, `material`, `index_count` en `u32`, puis ses indices en `u32`, puis le repère de texture et celui de lightmap | variable |
| Repère | origine, axe `u`, axe `v`, chacun `x, y, z` en `f32` | 36 |
| Portail | `id`, `index_count` en `u32`, puis ses indices en `u32` | variable |

**Tous les indices sont en `u32`**, y compris ceux d'une surface, qu'une cellule
borne pourtant bien en deçà de 65 536. Ce n'est pas l'argument du maillage —
là-bas, un décor fusionné atteint vraiment le plafond d'un `u16` — mais la
symétrie : deux largeurs d'indice dans le même dépôt donneraient deux chemins de
décodage à écrire et à éprouver, pour quelques kilooctets par carte.

**La cellule porte des `flags` dont aucun bit n'est défini**, nuls obligatoires
comme ceux de la surface. Un drapeau de cellule est de ceux que l'étape 5
réclamera — c'est écrit plus bas — et l'ajouter après coup ferait migrer toutes
les cartes pour un mot de quatre octets.

**Le portail n'en porte pas**, et n'a pas de matériau : il n'est pas dessiné. Le
jour où il lui en faudrait, ce serait une version de format de plus, et c'est le
bon prix — un champ réservé qu'on ne sait pas remplir est un pari sur sa forme.

**L'enregistrement de cellule est longueur-préfixé**, et sa longueur borne tout
ce qu'il contient : les trois comptes se recoupent avec elle, jamais avec la
longueur de la section. C'est ce qui permet à l'étape 8 de remplacer une cellule
sans toucher aux autres, et au décodeur de refuser un compte démesuré sans avoir
lu un seul sommet.

| Élément | Disposition | Taille |
|---|---|---|
| Lumière statique | `id` en `u32`, `x, y, z, radius` en `f32`, puis `r, g, b` en `u8` et un octet nul | 24 |
| Entité | longueur de l'enregistrement en `u32`, puis `id` et `cell` en `u32`, la longueur de sa classe en `u16` et ses octets UTF-8, `x, y, z` puis `qx, qy, qz, qw` en `f32`, la longueur de ses données en `u32` et ses octets | variable |

**La lumière porte ce que le moteur connaît d'une lumière**, et rien de plus :
position, rayon, couleur. C'est ce qui permet au calcul de lightmap de l'étape 5
de n'avoir besoin d'aucun tableau produit par l'hôte — deux hôtes donneraient
alors deux éclairages pour la même carte, et le projet perdrait « la même image
sur toutes les cibles ». L'octet nul après la couleur n'est pas un remplissage
mais l'alpha, réservé et nul, comme dans `ScgLight`.

**L'orientation d'une entité est un quaternion, normalisé au chargement.** Un
seul angle de lacet suffirait à un décor de cette classe, qui pose des objets
debout — et se paierait par une version de format le jour où une entité doit
s'incliner. La normalisation est un calcul dérivé de plus, donc son ordre
d'opérations est contractuel comme les autres ; un quaternion nul est refusé,
n'ayant pas de direction à porter.

**La classe est une chaîne, jamais interprétée.** Un entier opaque serait plus
court et obligerait l'hôte à tenir hors du fichier une table de correspondance —
ce que le format refuse déjà pour les noms de matériaux. Le moteur ne compare
cette chaîne à rien : il la transmet.

**La cellule d'une entité est un identifiant**, vérifié existant au chargement,
jamais un index. Une entité hors cellule n'existe donc pas ; le jour où il en
faudrait une, zéro est disponible — l'éditeur le réserve déjà à « aucun » — et ce
serait une version de format de plus.

**Le bloc de données est copié et jamais lu.** Pas de modèle de propriétés typé :
ce serait un langage de jeu qu'il faudrait faire évoluer avec les jeux, et
l'invariant du projet l'interdit.

Sont **dérivés au chargement** les liens de portails, les plans, la
triangulation, les coordonnées de texture et de lightmap, les boîtes
englobantes, les étendues en luxels et les tables d'identifiants. Aucun n'est
optionnel, et chacun sert un appel nommé : les plans de surface portent la
normale du calcul d'éclairage, les boîtes englobantes de cellule sélectionnent
les lumières, les étendues en luxels dimensionnent un atlas, et les tables
d'identifiants servent toute désignation par identifiant — la cellule de départ
d'une traversée, celle dont on calcule les lightmaps, celle qu'une entrée de
cache nomme. Une table `(identifiant, index)` triée, interrogée par dichotomie :
aucune allocation par image, aucun ordre d'itération de table de hachage.

**Le plan d'un portail n'en fait pas partie, et c'est une clause qui manque au
format.** Il servirait à ne pas traverser un portail vu de dos, ce qui coupe les
allers-retours entre deux cellules pour le prix d'un produit scalaire. Mais le
format ne dit pas **quel côté d'un portail est l'avant** : il fixe seulement que
les deux portails d'une paire ont des enroulements inverses. Un test de face
écrit sans cette convention est juste une fois sur deux, et se paie en trou. La
traversée s'en passe donc : ses cycles sont coupés par le chemin courant, et sa
terminaison par la profondeur. La clause s'écrira le jour où l'éditeur la
garantira.

Est dérivée **sur appel explicite de l'hôte** la lightmap elle-même, et elle
seule : un calcul d'éclairage au chargement ferait de l'ouverture d'une carte une
opération de plusieurs secondes. Aucune place n'est réservée à une visibilité précalculée
— elle n'existe pas dans ce moteur, et lui en réserver une en ferait la source de
vérité que le projet refuse.

**Chaque cellule est un enregistrement longueur-préfixé qui possède ses sommets,
ses surfaces et ses portails en propre.** Ce n'est pas un choix de commodité :
l'appariement des portails se fait en comparant leurs sommets, et avec une table
globale deux portails appariés partageraient les mêmes indices, ce qui viderait
la comparaison de son sens. La duplication de part et d'autre d'un portail est la
condition du mécanisme, pas son coût. Elle permet en outre de remplacer une
cellule sans toucher aux autres, ce dont l'édition à chaud a besoin.

**La surface est un polygone plan simple ; la convexité n'est pas exigée**, et la
triangulation se fait par découpe d'oreilles au chargement, plafonnée à 64
sommets — le chargement alloue dessus et la découpe est quadratique. Ce n'est pas
la compilation de cartes que le projet écarte : celle-ci découpe une surface
*contre d'autres surfaces*, alors qu'une triangulation est locale à un polygone,
ne crée aucun sommet et se refait pour cette surface seule quand elle change.
Exiger la convexité renverrait la subdivision d'une face en L — que la moindre
extrusion produit — à l'éditeur.

Une surface porte son identifiant, ses drapeaux, son matériau, ses indices de
sommets dans la cellule, et **deux repères complets et indépendants** : un de
texture, un de lightmap, chacun une origine et deux axes dont la longueur porte
l'échelle. **Le fichier porte le repère, jamais les coordonnées par sommet** ;
elles se calculent au chargement par projection sur les axes. Déplacer un sommet
d'un mur dont on ne connaît que les coordonnées obligerait l'éditeur à
reconstruire le repère par moindres carrés à chaque opération, et deux éditeurs
le reconstruiraient différemment : le repère est la source, les coordonnées la
dérivée.

**Le repère de lightmap se vérifie au chargement sur cinq points**, et chacun
répond à un besoin distinct — c'est pourquoi aucun ne remplace un autre :

- **la longueur au carré de chaque axe est une puissance de deux**, ce qui rend son
  inverse exact : la reconstruction d'un luxel vers un point du monde se fait alors
  par deux multiplications et trois additions, sans division et donc sans arrondi
  à rendre déterministe ;
- **son exposant est pair**, donc la longueur elle-même est une puissance de deux.
  Le pas de la grille en unités de monde étant cette longueur, le seul contrôle du
  carré laisserait passer un axe de longueur `√2` — deux surfaces coplanaires
  adjacentes aux pas `2` et `√2` ne partagent alors plus leur grille, et c'est la
  marche d'éclairage à la jointure que ce contrôle existe pour interdire ;
- **l'origine est un multiple de cette longueur**, sans quoi les grilles alignées
  en pas restent décalées en phase ;
- **les deux axes sont orthogonaux**, faute de quoi la reconstruction demande
  l'inverse d'une 2×2 quelconque, donc une division ;
- **ils sont contenus dans le plan de la surface**, faute de quoi la grille de
  luxels ne recouvre pas ce qu'elle éclaire.

Ce sont des contrôles et non une disposition : `version_format` ne bouge pas, et
une carte que ces clauses refusent était déjà fausse.

**Les trois premiers sont exacts, les deux derniers tolèrent un résidu relatif**,
et la différence n'est pas un relâchement : une puissance de deux est exacte ou
n'est pas, alors qu'un repère oblique posé sur une surface oblique porte le résidu
de sa propre construction. Une tolérance serait interdite sur l'appariement des
portails, qui est une **relation** — un epsilon la rendrait non transitive ; ici
c'est un **prédicat**, dont le verdict est le même sur toutes les cibles dès que
son calcul l'est. Il se fait en `f64`, permis hors image, ce qui dispense de se
demander si le produit de grandes coordonnées déborde.

**L'étendue en luxels d'une surface est plafonnée à 256 sur un côté, et le refus
tombe au chargement.** Un grand mur à pas de lightmap fin produit un atlas qui ne
tient pas ; le vérifier dans le même passage que le repère le fait découvrir à
l'ouverture de la carte, une fois, plutôt qu'au calcul, trois appels plus tard et
pour une seule cellule. Les extrema se prennent par comparaisons écrites : le
résultat de `f32::min` sur `min(-0,0, 0,0)` n'est pas spécifié, et deux cibles
refuseraient des cartes différentes.

L'unité de lightmap est la surface ; l'atlas par cellule est un cache assemblé
au calcul, jamais dans le fichier, où il figerait une disposition que le premier
déplacement de sommet invalide.

Les drapeaux d'une surface ont trois bits définis — deux faces, ne reçoit pas de
lightmap, non solide — et **tous les autres nuls obligatoires**, même règle que
les champs réservés de l'ABI. « Non solide » décrit la géométrie, pas le joueur :
ce n'est pas une notion de jeu qui entre dans le moteur.

**Le portail est un polygone plan convexe, et la convexité est exigée là où elle
ne l'est pas pour la cellule parce que le portail décide de ce qu'on voit.** La
traversée réduira la fenêtre de découpe à l'intersection de la fenêtre courante
et de la projection du portail ; une projection concave n'a pas d'intersection
exprimable comme réduction de fenêtre, et l'erreur se paierait en trou définitif.
Une cellule concave ne rend la traversée que conservatrice, jamais fausse.

**L'appariement se fait au bit près, sans tolérance**, sur `to_bits`. Une
tolérance donnerait une relation non transitive — `a≈b`, `b≈c`, `a≉c` —, donc un
résultat dépendant de l'ordre de parcours, ce que le déterminisme interdit. C'est
l'éditeur qui écrit les mêmes octets des deux côtés, et c'est une clause du
format. L'appariement passe par un tri lexicographique de clés canoniques — les
sommets triés par bits, les deux portails ayant des enroulements inverses — et
non par une table de hachage, dont l'ordre d'itération n'est pas contractuel.

**La clé d'un sommet porte ses trois mots de trente-deux bits, jamais un condensé
de ceux-ci.** Quatre-vingt-seize bits ne tiennent pas dans un mot de soixante-quatre,
et les réduire donne un appariement exact « à collision près » : deux portails de
sommets différents peuvent alors porter la même clé et s'apparier en silence, sur
des coordonnées de carte qui sont régulières par construction. La conséquence
n'est pas un refus mais une traversée qui ouvre sur une cellule non voisine —
défaut de la classe « image fausse, aucune erreur », et de ceux qui ne se
rattrapent pas par un correctif local.

**Deux portails de la même cellule ne s'apparient pas** : le lien ramènerait sur
la cellule courante, et la traversée tournerait sur place au lieu d'avancer. Rien
d'autre n'est à vérifier de l'appartenance d'un portail à sa cellule — ses sommets
sont désignés par indices parmi ceux de la cellule, donc déjà sous son compte.
**Un portail non apparié est un mur, pas une erreur** : une carte en cours
d'édition en a toujours. **Trois portails sur la même clé sont une erreur** : il
n'y a pas de réponse à « lequel des deux ».

**Une entité est un identifiant, une classe jamais interprétée, une pose, une
cellule et un bloc d'octets que le moteur copie et ne lit pas.** Pas de classes
nommées par le moteur, pas d'entité qu'il évalue ou déplace, pas de modèle de
propriétés typé — ce serait un langage de jeu qu'il faudrait ensuite faire
évoluer avec les jeux.

**Les lumières statiques sont une section propre, et ce n'est pas une exception.**
Si les sources d'éclairage étaient des entités opaques, le calcul de lightmap
devrait recevoir un tableau produit par l'hôte, et deux hôtes donneraient deux
éclairages pour la même carte — ce qui retirerait au moteur la même image sur
toutes les cibles. La lumière est déjà dans son vocabulaire. Points de départ,
déclencheurs et objets ramassables restent des entités opaques.

Les identifiants sont attribués par l'éditeur, jamais par le moteur, avec **un
espace par famille** — cellules, portails, surfaces, entités, lumières,
matériaux. Si le moteur les attribuait, un aller-retour par le fichier
renumériserait et l'annulation de l'éditeur ne retrouverait plus ses objets. La
correspondance est un tableau `(identifiant, index)` trié, interrogé par
dichotomie : aucune allocation par image, aucun ordre d'itération de table de
hachage. **Le fichier n'exige pas qu'ils soient triés** — l'exiger obligerait
l'éditeur à réécrire la carte entière pour un ajout.

### Trois pièges du décodage

**Les coordonnées de texture dérivées d'un repère dépassent trivialement la borne
de l'ABI.** Un plaquage fin sur une grande surface produit des valeurs bien
au-delà de ce que `abi.md` admet, et la soumission refuse alors le lot entier
sans dire quelle surface. Le chargement ramène donc les coordonnées de chaque surface dans
`[0, 2048)` en soustrayant un multiple entier de 2048 texels. **L'image est
identique au bit près** : les côtés de texture sont des puissances de deux d'au
plus 2048 et le repli se fait par masque, et les dérivées — donc le niveau de
mipmap et le motif de tramage — sont invariantes par translation.

**Le calcul des coordonnées au chargement est un calcul flottant hors du
rasteriseur**, et il suit le même ordre d'opérations figé que les
transformations. Sinon deux cibles produisent deux jeux de coordonnées pour la
même carte, sur un chemin qu'aucune scène de conformance n'emprunte aujourd'hui,
toutes écrivant leurs coordonnées à la main.

**L'ordre du fichier fait partie du contenu.** Le test de profondeur est strict :
à égalité, le premier triangle soumis reste. Deux surfaces coplanaires se
départagent donc par leur ordre dans le fichier, et un éditeur qui réordonne ses
tables sans rien changer d'autre déplace les empreintes.

Corollaire général : **toute valeur dérivée au chargement entre dans le rendu**,
donc son ordre d'opérations est contractuel. Une boîte englobante par minimum et
maximum est exacte et sans piège d'arrondi ; un centre par moyenne ne le serait
pas. Elle en garde un autre, qui n'est pas dans l'arrondi : **`f32::min` ne
spécifie pas le signe qu'il rend de `min(-0,0, 0,0)`**, si bien que deux cibles
donneraient deux boîtes pour le même maillage. Les coordonnées étant finies — le
refus des non-finis à la lecture le garantit —, une comparaison suffit et rend
les mêmes bits partout.

Enfin, les `f32` se lisent par `from_bits` sur les octets lus, et **rien ne fait
d'arithmétique dessus avant le contrôle de finitude** : la charge utile d'un NaN
signalant peut être normalisée par un passage en registre, et la divergence
serait silencieuse entre cibles. Le refus des non-finis au chargement ferme le
cas.

## Lightmaps calculées

Le noyau calcule les lightmaps d'une **cellule**, sur appel explicite de l'hôte,
jamais pendant une image et jamais au chargement. Elles restent un **cache
dérivable** : la carte est la source, et ce qui suit est ce que deux cibles
doivent produire au bit près.

### Ce que la géométrie doit garantir

**La reconstruction d'un luxel vers un point du monde est exacte, sans division.**
Le luxel `(i, j)` s'évalue en `origine + (min_u + i)·u/|u|² + (min_v + j)·v/|v|²`.
Les cinq contrôles du repère de lightmap, plus haut, sont exactement ce qui rend
cette expression exacte : `|u|²` puissance de deux rend son inverse exact, et
l'orthogonalité évite l'inverse d'une 2×2 quelconque. Sans eux il faudrait une
division par pixel de lightmap, donc un arrondi de plus à rendre contractuel.

**Le point d'échantillonnage est décalé le long de la normale du plan d'une
puissance de deux en unités de monde.** La surface est de toute façon exclue de
ses propres occulteurs ; le décalage sert à ce qu'aucun sous-normal n'entre dans
un calcul, même clause que la racine inverse du noyau. Sur armv7, le SIMD avancé
n'a que la sémantique du zéro forcé là où le VFP scalaire traite les sous-normaux :
c'est architectural, et c'est aussi la raison pour laquelle **le calcul des
lightmaps reste scalaire et sort du périmètre de l'étape 9.**

**Le rayon se teste contre le polygone de la surface, jamais contre sa
triangulation.** C'est la règle top-left transposée, et le piège est de la même
famille : un rayon qui passe exactement par une arête interne de la découpe
d'oreilles peut être manqué par les deux triangles, et la lumière traverse alors
le mur par un trou d'épingle. Invisible à l'arrêt, visible en mouchetures sur une
lightmap cuite, et le calculateur entier serait déjà construit dessus. Donc : le
plan, puis un test d'appartenance en deux dimensions dans le repère de la surface,
avec une règle de bord écrite.

**L'ensemble d'occulteurs est la cellule, ses voisines à un portail, et les
portails du bord de cet ensemble rendus opaques.** Une cellule est fermée : tout
rayon qui en sort traverse une surface ou un portail. En fermant le bord, la
région devient étanche et **rien d'extérieur ne peut contribuer** — ce qui est
précisément ce qui rend suffisante l'empreinte du cache décrite plus bas.
Conséquence assumée, et il faut l'écrire parce qu'elle se verra : **la lumière ne
tourne pas deux coins.** Un portail apparié n'occulte jamais ; « non solide » est
une notion de collision et n'entre pas ici, une grille projette son ombre.

**La sélection des lumières est géométrique, jamais par appartenance à une
cellule.** Pré-rejet par intersection de la sphère de la lumière avec la boîte
englobante de la cellule, puis `|v|² < r²` par luxel. Les lumières statiques
n'ont pas de champ de cellule, et un test d'appartenance à une cellule non
convexe est un comptage de traversées — un calcul de plus à rendre déterministe
pour un résultat que la géométrie donne gratuitement.

### L'atténuation, et le terme qui s'y ajoute

**L'atténuation est celle de l'étape 3, `(1 − d²/r²)²`**, et elle ne se
rediscute pas : le même mur éclairé par la même lampe doit rendre la même chose
cuit et dynamique, sans quoi basculer une source d'un mode à l'autre déplacerait
la scène.

**S'y ajoute un terme de Lambert, `max(0, N̂·L̂)`, et c'est un arbitrage.** Sans
lui, sol, mur et plafond autour d'une lampe sont également clairs, l'angle
disparaît et la cuisson perd son objet. Il ne coûte rien à l'ABI — la normale est
celle du plan de la surface, déjà dérivée au chargement —, contrairement à la
normale par sommet que l'étape 3 avait refusée et que l'étape 6 tranchera. Il
rapporte en outre le rejet le plus payant du calcul : `N̂·L̂ ≤ 0` rend le luxel noir
sans lancer un rayon. La normalisation passe par la table de racine inverse du
noyau : aucune libm, aucune approximation matérielle.

**Conséquence assumée jusqu'à l'étape 6** : une lampe cuite et la même lampe
dynamique ne rendent pas la même chose, la seconde ignorant l'orientation faute
de normale. Les deux usages diffèrent — le décor statique d'un côté, une source
portée de l'autre —, et l'écart se referme quand la normale par sommet arrive.

**L'ordre des opérations est figé.** Accumulation en `f32`, lumières dans l'ordre
de la section des lumières statiques, `total += canal × atténuation`, jamais de
`mul_add`. Suréchantillonnage **fixé à 2×2** à des décalages d'un quart de luxel,
sommés dans un ordre écrit, moyennés par une division par quatre — exacte. Puis
`v × 255 + 0,5`, borné par des comparaisons écrites, converti par `as`. **Le
nombre d'échantillons n'est pas un réglage** : configurable, il changerait
l'image, devrait entrer dans l'ABI et dans l'empreinte du cache. Le jour où il
faut le changer, c'est une constante de plus dans la révision du calcul.

**Les luxels hors du polygone se calculent comme les autres**, à leur position de
grille, sans test d'appartenance et sans autre dilatation que la gouttière de
l'atlas. Écartée : la dilatation par propagation, dont l'ordre de parcours
deviendrait contractuel pour un demi-luxel de gain. **À surveiller** : c'est le
coin sombre classique, et si la première capture le montre, la réponse est la
densité, pas l'algorithme.

### L'atlas d'une cellule

**C'est une texture ordinaire dont les sous-rectangles sont des puissances de deux
alignées sur leur propre taille.** Chaque surface reçoit un rectangle dont chaque
côté est la plus petite puissance de deux contenant son étendue plus une gouttière
d'un luxel, remplie par recopie du bord, et il est placé à un multiple de sa
propre taille.

C'est cet alignement qui rend l'atlas **exact au mipmap** : aucune réduction
2×2 ne traverse la frontière d'un sous-rectangle, si bien que la chaîne de
l'atlas est identique, texel pour texel, à celle que des textures par surface
auraient donnée. Le rangement en atlas devient alors un choix de mémoire sans
effet sur l'image. Écartée : une gouttière dimensionnée pour une chaîne complète,
qui coûterait la moitié du côté du rectangle en luxels de garde ; écartée aussi,
une chaîne tronquée, qui ne supprime pas le saignement mais le borne.

**Le rangement entre dans l'image par les coordonnées, donc il est
contractuel** : placement par classe de taille décroissante, à égalité par rang de
surface dans la cellule, premier emplacement libre balayé en lignes à
l'alignement du rectangle.

**Les coordonnées de lightmap se coupent en deux.** Le chargement dérive, par
sommet, des coordonnées **locales à la surface** ; la soumission y ajoute
l'origine du rectangle dans l'atlas, plus la gouttière et le demi-luxel. Une
addition dans la lecture d'un sommet, aucune allocation, et la carte reste
indépendante d'un rangement que le premier déplacement de sommet change. Le
demi-luxel n'est pas décoratif : le bilinéaire du rasteriseur retranche déjà un
demi-texel, et le centre du luxel `(0, 0)` est en `(0,5 ; 0,5)`.

**Un `Texture` par surface est écarté**, et c'est la table des textures du
contexte qui l'écarte : un balayage linéaire par lot et un plafond d'entrées, pour
des milliers de surfaces dont le contenu tient dans un seul atlas par cellule.

### Le cache de lightmaps

**Troisième genre du conteneur commun**, `LMAP`, avec le même en-tête et la même
table de sections que le maillage et la carte, et le même décodeur — déjà durci
contre un bloc hostile, déjà éprouvé par la troncature exhaustive et la mutation
aléatoire. Un format propre redemanderait ce durcissement et cette batterie de
tests, et c'est le genre de seconde implémentation dont l'oubli est silencieux.

**Un bloc pour le niveau, un enregistrement par cellule.** L'hôte range un fichier
à côté de sa carte, plutôt que N fichiers et une convention de nommage que le
moteur devrait décrire. Ce que le découpage par cellule apporterait vraiment —
l'acceptation partielle — vient de l'empreinte portée par chaque enregistrement.

Deux sections. La première porte les enregistrements de cellule,
longueur-préfixés comme les cellules d'une carte, **triés par identifiant, uniques
et pavant la section** : le moteur est le seul écrivain, la canonicité est donc
gratuite et rend deux caches comparables octet pour octet. Chacun porte
l'identifiant de la cellule, son empreinte, les côtés de son atlas, la position et
la longueur de ses luxels dans la seconde section, et le rectangle de chaque
surface avec son identifiant. La seconde porte les niveaux zéro des atlas bout à
bout, quatre octets par luxel dans l'ordre mémoire des pixels de sortie, alpha à
255 — c'est ce que le constructeur de texture attend, et l'hôte n'a jamais deux
ordres à tenir.

**Les mipmaps ne sont pas dans le cache** : ce serait le cache d'un cache, un tiers
d'octets de plus pour une valeur exactement dérivable. Ils se construisent à la
fin de l'appel qui calcule ou qui reprend.

**Ni somme de contrôle, ni compression**, pour les raisons déjà tranchées sur les
deux autres formats : une somme ne protège de rien face à un bloc hostile, et
l'intégrité de transport appartient à l'archive de l'hôte.

**L'empreinte est un FNV-1a 64 bits par cellule**, calculée sur des octets écrits
en petit-boutiste, tout par `to_bits` et sans une seule opération flottante, dans
cet ordre :

1. **la révision du calcul**, une constante du noyau incrémentée à *tout*
   changement de ce que ce document décrit — densité, nombre d'échantillons, forme
   de l'atténuation, terme de Lambert, portée de l'ensemble d'occulteurs,
   quantification. C'est le champ qu'on oublie, et son absence laisse un cache
   valide produire une image que la version suivante ne produit plus ;
2. **la cellule** : son identifiant, ses drapeaux, ses sommets dans l'ordre du
   fichier, puis chaque surface avec ses drapeaux, ses indices et les neuf
   flottants de son **repère de lightmap**, puis chaque portail avec ses points et
   l'identifiant de la cellule liée ou zéro ;
3. **chaque cellule voisine par portail apparié**, par identifiant croissant, par
   la même fonction : elles occultent et elles éclairent, donc elles comptent. Et
   comme l'ensemble d'occulteurs s'arrête là, cette empreinte est **prouvée
   suffisante** ;
4. **les lumières retenues**, celles dont la sphère coupe la boîte englobante de la
   cellule, dans l'ordre de leur section.

N'entrent pas : le repère de **texture**, le matériau, les entités, les noms.
Clause à écrire pour qu'on s'y fie : **réhabiller un décor ou déplacer un objet ne
périme aucune lightmap.** Écarté : hacher toutes les lumières de la carte, ce qui
ferait périmer le niveau entier au premier réglage d'une torche à l'autre bout.

**Une entrée dont l'empreinte ne concorde pas est écartée sans erreur**, et la
cellule reste sans lightmap ; le compte des entrées reprises le dit à l'hôte. Le
reste du contrat de reprise est dans [`abi.md`](abi.md).

## Documentation et commentaires

- **Toute déclaration a sa documentation** — fonctions, méthodes, types, champs
  publics, constantes, statiques, fonctions de test. Une ligne quand c'est
  évident, un paragraphe quand il y a un arbitrage à retrouver. Le lint
  `missing_docs` le fait refuser par `make lint`.
- **Les commentaires disent pourquoi**, jamais ce que dit la ligne suivante. Leur
  densité suit la difficulté : quatre lignes sur la règle top-left, rien sur la
  plomberie.
- **Langue** : identifiants en anglais, documentation et commentaires en français,
  documentation des éléments exportés en FFI en anglais. Le détail est dans
  `CONTRIBUTING.fr.md`, section « Langue ».
- **En-tête de fichier.** Tout fichier source — `.rs`, et dans les hôtes `.c`,
  `.cpp`, `.java`, `.js` — commence par :

  ```rust
  // Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
  // SPDX-License-Identifier: MIT OR Apache-2.0
  ```

  Côté Rust, il précède la doc de module `//!` et les attributs internes.
  `include/screengine.h` le reçoit par l'option `header` de
  `cbindgen.toml`, jamais par une édition.
- Pas de bannière, pas d'emoji, ni dans le code, ni dans les messages de commit.

## Formatage et lints

- **`rustfmt` sans configuration.** `make fmt` vérifie tout l'arbre, sans
  exclusion. Une configuration est une discussion de style de plus, pour un gain
  qui ne se mesure pas.
- **`clippy` avec `-D warnings`**, sur `--all-targets --all-features` : les tests
  et les exemples sont du code comme le reste.
- **Les lints se déclarent une fois**, dans `[workspace.lints]` du `Cargo.toml`
  racine, et chaque crate les hérite par `lints.workspace = true`. Ceux que ce
  document exige : `missing_docs`, `unsafe_op_in_unsafe_fn`,
  `clippy::undocumented_unsafe_blocks`. `unsafe_code` est refusé dans le noyau
  seul, par attribut de crate, puisque la couche FFI en a besoin partout.

## Tests

**Tout code livré part avec ses tests, dans le même commit.** Un lot sans test
n'est pas un lot plus petit, c'est un lot inachevé.

**Un test se vérifie en le faisant échouer une fois.** On casse le code, on voit
le test rougir, on répare. Un test qui n'a jamais échoué ne prouve pas qu'il
teste quelque chose.

### Où vivent les tests

- **Tests unitaires dans leur propre fichier** : `#[cfg(test)] mod tests;` dans le
  module, et `module/tests.rs` à côté. Sous-module et non fichier extérieur, ce
  qui leur garde l'accès aux éléments privés — sans quoi une fonction de bord ou
  un biais, qui ne sont pas publics, ne seraient testables que par leur effet.
  Ils peuvent déclarer `extern crate std;`.

  La forme usuelle en Rust met ce bloc à la fin du fichier testé. On l'en sort
  parce qu'un module finit toujours par porter plus de tests que de code, et
  qu'un fichier où il faut faire défiler trois cents lignes de tests pour relire
  vingt lignes de rasteriseur se relit mal.
- **La documentation d'un test dit ce que son nom ne dit pas** : le défaut qu'il
  attrape, et pourquoi il existe à côté de son voisin. `missing_docs` ne voit pas
  les fonctions privées d'un `mod tests`, d'où la cible `lint-doc-tests`, que
  `make lint` appelle.
- **Tests de la frontière** dans `crates/screengine-ffi/tests/` : ils appellent
  les fonctions exportées comme le ferait un hôte, pointeurs nuls et séquences
  invalides compris, et vérifient qu'aucune panique ne s'échappe. Ils restent du
  Rust appelé depuis du Rust : ni l'édition de liens, ni la disposition vue par
  un compilateur C, ni l'environnement flottant d'un vrai hôte n'y passent.
- **L'hôte C, dans `make test-abi`**, franchit réellement la frontière : lié à la
  bibliothèque statique, sans fenêtre, il vérifie refus, sentinelles autour du
  tampon, alignement et registre flottant, puis compare son empreinte du triangle
  à celle du chemin Rust (`screengine-conformance --print arete`). Il fait
  partie de `make test` ; sans compilateur C il saute en le disant, et ce saut
  est une erreur en intégration continue.
- **Les hôtes rendent aussi par tuiles** : une partie des tuiles, dans un ordre
  qui n'est pas celui des index, puis la fin qui complète, et l'empreinte doit
  être celle de la fin seule. L'hôte C++ les rend sur plusieurs threads.
- **L'hôte C++, dans `make test-cpp`**, se lie à la bibliothèque dynamique et
  reprend les mêmes contrôles. Il ajoute ce que le C statique ne voit pas : le
  header compilé en C++ — gardes `extern "C"`, assertions de disposition — et la
  preuve que `scg_abi_version` vient bien de la bibliothèque chargée. Même
  règle de saut.
- **L'hôte wasm, dans `make test-wasm`**, charge le module sous Node, sans
  fenêtre et sans aucun import. Il reprend les refus, les sentinelles et
  l'alignement, dans la mémoire linéaire, et ajoute ce que le web seul impose :
  les exports réels du `.wasm`, les constantes recopiées en JavaScript comparées
  au header, et une vue détachée par la croissance de la mémoire. Ni registre
  flottant ni état défaillant : wasm n'a pas le premier, et une panique y est un trap.
- **L'hôte Android, dans `make test-android`**, rend l'empreinte en deux paliers
  qui doivent concorder : `hosts/c/main.c` lié en statique pour les trois ABI,
  sans appareil — aarch64 et armv7 sous `qemu-user`, seul endroit où FPCR et
  FPSCR hostiles sont éprouvés —, puis, sur un émulateur, le même programme lié
  en dynamique et `Test.java` à travers JNI, sur une base de tampon désalignée.
  Voir [`construction.md`](construction.md), « Android ».
- **Les cibles `test-*` ont une forme commune** dans le `Makefile` : chaque
  hôte fournit `why-not`, `all` et `run`, et la règle compare son empreinte à
  celle du chemin Rust. `make test SANS=android` retire un hôte, par une
  exclusion écrite là où on l'appelle.
- **Conformance** dans `crates/screengine-conformance` : des scènes de référence,
  rendues sans fenêtre, dont le tampon est haché et comparé aux empreintes de
  `references/`.
- **Aucun test n'exige de fenêtre ni de GPU.** Les runners d'intégration continue
  n'ont pas d'écran ; un hôte à fenêtre n'est pas un test.

### Conformance

- **Le rasteriseur scalaire est la référence.** Une variante SIMD se valide contre
  son empreinte, jamais contre une capture prise avec elle-même. Une divergence
  est une variante fausse, pas une différence acceptable.
- **Une empreinte qui change est soit une régression, soit une évolution
  voulue.** Dans le second cas, `make conform-update`, et la mise à jour des
  références est un commit distinct, dont le message dit ce que le rendu fait
  désormais autrement. Jamais mêlée au lot qui l'a causée : le diff d'un fichier
  d'empreintes ne se relit pas.
- **Une option de rendu qui change l'image prend une scène, jamais une passe.**
  Les passes d'une scène ne diffèrent que par le découpage et doivent rendre la
  même empreinte : c'est tout leur objet. Un filtrage qui rend délibérément une
  autre image a donc besoin de sa propre référence, elle-même vérifiée dans les
  cinq passes, et sa géométrie se partage au texel près avec la scène qu'elle
  double — sans quoi une divergence ne serait plus attribuable à l'option.
- **Avant de figer une référence nouvelle ou de la mettre à jour, regarder
  l'image**, par `make conform-images`. Une empreinte dit qu'une image a changé,
  jamais qu'elle est juste : une scène rendue noire, à l'envers ou filtrée
  autrement qu'on croit se fige aussi bien qu'une autre.
- **La scène des arêtes partagées** — deux triangles qui partagent une arête se
  partagent ses pixels, sans trou ni recouvrement — se repasse à chaque
  modification du remplissage, à toutes les résolutions internes prévues. C'est
  le défaut le plus coûteux du projet : invisible à l'arrêt, visible en
  mouvement.
- **Chaque scène se rend en tuiles de 32, en tuiles de 64, en image entière, en
  tuiles dans un ordre mélangé à graine fixe et en tuiles réparties sur plusieurs
  threads**, et les cinq empreintes doivent être identiques. Une couture de
  tuile ne se voit que dans une configuration : sans ce contrôle, la conformance
  ne vaudrait que pour la sienne. L'image entière passe par l'API Rust du noyau,
  qui rend une région quelconque sans passer par la répartition ; l'ABI
  n'accepte que 32 et 64. Les tests du noyau font la même comparaison octet pour
  octet sur des scènes tirées au hasard, où les triangles se recouvrent.
- **Une scène, une référence**, `references/<scène>` : seize chiffres et un saut
  de ligne, comparés octet pour octet. Toutes les configurations se comparent à
  la même. Une référence absente fait échouer `--check`, jamais un « rien à
  comparer » qui laisserait la suite verte.
- **Une scène peut rendre plusieurs vues** — plusieurs angles, plusieurs
  résolutions internes —, et ses empreintes d'image s'enchaînent en une seule,
  dans l'ordre. Écartée : une référence par vue, qui nommerait la vue fautive
  mais porterait des dizaines de fichiers pour une scène ; la comparaison entre
  configurations se fait vue par vue et nomme déjà celle qui diverge. Une scène
  d'une seule vue garde l'empreinte de son image, sans enveloppe : c'est ce qui
  permet à un hôte de comparer la sienne au fichier versionné.
- **Une couture ne se lit pas dans une empreinte**, qui change aussi bien pour
  un trou que pour une teinte — et une référence prise sur un rendu troué le
  figerait. Elle se vérifie sans référence : la scène projetée étant convexe,
  son intersection avec une ligne de pixels est un segment, et un pixel de fond
  entre le premier et le dernier pixel peint est un trou. Ce contrôle attrape
  une couture de tuile ou une découpe qui mord ; il n'attrape pas la règle
  top-left, dont le biais ne départage que les centres de pixels tombant
  exactement sur l'arête — cas que seuls les tests du noyau atteignent, en les
  y plaçant.
- **Les hôtes se comparent au chemin Rust de leur plateforme**, et ce chemin à la
  référence versionnée : chaque plateforme d'intégration continue relie ainsi
  tous les hôtes qu'elle exécute au même fichier.
- **L'empreinte est FNV-1a 64 bits**, écrite en seize chiffres hexadécimaux
  minuscules. Elle hache la largeur puis la hauteur en `u32` petit-boutiste, puis
  la zone utile ligne par ligne, `largeur × 4` octets alpha compris ; le `stride`
  n'y entre pas. Chaque hôte la recalcule dans son langage : elle tient en dix
  lignes partout, et elle est native en PHP (`hash('fnv1a64')`). Écartés : xxHash, à
  réimplémenter en JavaScript et en Java ; SHA-256, asynchrone dans un
  navigateur, pour un détecteur de régression qui n'a rien à sécuriser.
- **L'environnement flottant de l'hôte ne change pas l'image.** L'hôte C démasque
  les exceptions, active le zéro forcé et l'arrondi vers le haut avant d'appeler
  le moteur, compare l'empreinte à celle du chemin Rust, et vérifie que son
  registre lui revient intact — MXCSR sous x86_64, FPCR sur aarch64 et FPSCR sur
  armv7, ces deux derniers par l'hôte Android sous `qemu-user`.
- Les empreintes sont comparées octet pour octet : `.gitattributes` les déclare
  binaires.

### Allocation

Un test de `screengine-conformance` installe un allocateur global qui compte les
allocations, crée un contexte, charge une scène complète — textures, mipmaps,
lightmaps —, puis vérifie qu'aucune image rendue ensuite n'alloue, la première
comprise. C'est la première qui compte : c'est là qu'un mipmap généré à la
demande ou un atlas agrandi se cacherait. Et c'est la seule preuve de l'invariant
qui ne dépende pas de l'attention du relecteur.

Il vit dans `crates/screengine-conformance/tests/allocation.rs`, binaire de test
à part : un allocateur global vaut pour tout le binaire. Le compte est par
thread, armé autour des images seulement, y compris dans les threads qui rendent
des tuiles — le harnais de test alloue sur ses propres threads pendant la
mesure. La scène est un quadrilatère, soumis sur trois images, **avec et sans
texture** : le chemin texturé remplit la table de textures du contexte, que le
chemin uni ne touche jamais, et une mesure qui l'ignorerait laisserait hors
d'elle ce que tout décor emprunte. Chaque ressource chargeable y entre avec son
étape, et se charge hors de la mesure — un chargement est un appel nommé, qui a
le droit d'allouer.

### Tests aléatoires

Pas de bibliothèque de tests par propriétés dans le noyau. Les tests qui tirent
des entrées au hasard utilisent un générateur écrit dans le test, avec une graine fixe
affichée en cas d'échec : un échec qui ne se rejoue pas n'a pas été trouvé.

### Décodage de fichiers

**Les fichiers d'épreuve s'écrivent en octets dans le test**, par un constructeur
qui pose les champs un à un ; aucun fichier binaire n'est versionné pour éprouver
le décodeur. Un binaire ne se relit pas en revue, et le format cassant une
douzaine de fois, chaque cassure obligerait à régénérer à la main ce que le test
sait produire. La raison décisive est ailleurs : **un fichier produit par
l'écrivain du projet rendrait le test tautologique** — il prouverait que
l'écrivain et le lecteur s'accordent, pas que l'un des deux est juste. Le
constructeur du test est le seul endroit où les décalages sont écrits deux fois,
et un désaccord y fait rougir.

Quatre épreuves, et la première est celle qui trouve :

- **tronquer un fichier valide à chacune de ses longueurs**, et exiger une erreur
  de format partout, sans une seule panique. C'est le contrôle exhaustif qui
  attrape la borne oubliée ;
- chaque compte porté à son maximum, des sections qui se recouvrent, un décalage
  au-delà de la fin, des identifiants dupliqués ou désordonnés, un indice égal à
  son compte ;
- **muter un fichier valide au hasard**, générateur et graine fixe comme
  ci-dessus, avec pour invariant « jamais de panique, toujours un succès ou une
  erreur de format ». C'est l'équivalent honnête d'un fuzzer dans un noyau qui
  n'admet aucune dépendance ;
- **une scène de conformance rendue depuis une ressource décodée.** Elle seule
  prouve que le décodeur rend la *bonne* ressource et pas seulement une ressource
  bien formée — et elle fait entrer le décodage dans la comparaison croisée entre
  cibles, que rien d'autre ne couvre.

Les bornes s'écrivent sur des valeurs choisies, jamais en comptant sur la cible :
`usize` fait 32 bits sur deux des quatre, la compilation sans `std` le vérifie
mais n'y exécute aucun test.

**Côté hôtes, un fichier binaire versionné, un seul, partagé par les quatre.**
Chacun doit charger une ressource pour prouver que le point d'entrée franchit
réellement la frontière ; leur faire recopier la disposition dans quatre langages
serait la même liste à quatre endroits, et ce qu'une liste recopiée coûte quand
elle diverge est connu du projet. **Le test Rust régénère ce fichier et le compare
octet pour octet**, sur le modèle du header : le binaire n'est alors jamais la
source de vérité, et un fichier périmé échoue franchement.
