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
  screengine-play/          src/, examples/
  screengine-conformance/   src/, references/
```

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
| `screengine-play` | étage d'accueil : fenêtre, entrées, boucle à pas fixe, mise à l'échelle | oui | `winit`, `softbuffer` | non |
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
  lib.rs  error.rs  buffer.rs  context.rs   ce qui existe avant tout domaine
  math/       étape 1   vecteurs, matrices, quaternions, tables, virgule fixe
  raster/     étape 1   clipping, fonctions de bord, profondeur, tuiles ; simd/ à l'étape 9
  texture/    étape 2   mipmaps, filtrage
  light/      étape 3   lightmaps, brouillard, post-traitement de tuile
  format/     étape 4   maillage et carte, versionnés
  world/      étape 5   cellules, portails, traversée
  collide/    étape 7   balayage de boîte contre les cellules
```

**`raster/simd/` est le seul endroit du noyau qui autorise `unsafe`**, et le
scalaire qui lui sert de référence reste dans `raster/` : les fondre supprimerait
la référence.

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
  Darwin, le CRT de MSVC. Trigonométrie et racine inverse passent par les tables
  et polynômes du noyau. Le `no_std` en écarte la plupart d'office ; la règle vaut
  aussi dans les crates qui ont `std`, dès qu'un résultat entre dans une
  empreinte.
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
- **Vecteur colonne, `M·v`, stockage par colonnes.** Le noyau travaille en
  `Affine3`, 3×4 : trois colonnes puis la translation. La projection n'est pas
  une matrice, c'est `(sx, sy, near)` appliqué à part, avec un plan lointain
  infini : `z_c = near` et `w_c = z_vue` donnent directement la profondeur
  `near/w`. Une composition se lit `parent.product(local)`, la droite
  s'appliquant d'abord.
- **Chaque somme s'écrit de gauche à droite, dans l'ordre des colonnes** :
  `((m0·x + m3·y) + m6·z) + m9`. Jamais d'arbre équilibré `(a+b)+(c+d)`, ni de
  boucle générique : c'est l'ordre qu'un chemin SIMD reproduit en accumulant
  colonne par colonne, et un test compare les bits à l'expression écrite.
- **Orientations en quaternions**, normalisés à la réception plutôt qu'exigés
  unitaires, interpolés par `nlerp` : pour dix degrés par pas, l'écart au
  sphérique reste sous le millième de degré, sans arc cosinus.
- **Face avant en sens antihoraire** dans les données. Le retournement que
  produit l'axe Y de l'écran, vers le bas, se traite dans le signe des fonctions
  de bord, sans permuter de sommets.
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
| Bande de garde | ±4096 pixels, soit ±2¹⁶ en 28.4 | au-delà, le sommet est clippé en espace homogène |
| Fonctions de bord | `i64` | un écart entre sommets atteint 2¹⁷ en 28.4, un produit 2³⁴ : `i32` déborde dès que la bande de garde sert |
| Règle top-left | biais de −1 sur les arêtes ni hautes ni gauches | deux triangles partageant une arête se partagent ses pixels, sans trou ni recouvrement |
| Profondeur | `u32`, `near/w` en 0.32, saturé | plus grand est plus proche ; `1/w` est affine en espace écran, donc s'interpole exactement. Test strict : à égalité, le premier triangle soumis reste |
| Attributs (`1/w`, `u/w`, `v/w`, lightmap) | `i64`, 32.32 | équations de plan établies à la mise en place du triangle, par division entière |
| Coordonnées de texture après division | `i32`, 16.16 | textures en puissance de deux, repli par masque |
| Poids du bilinéaire | 8 bits, tirés des bits fractionnaires | mélange entier, arrondi `(… + 128) >> 8` |

- **Tout s'évalue en coordonnées globales.** Une fonction de bord ou un attribut
  en un pixel se calcule à partir des sommets et de la position du pixel dans
  l'image, jamais d'une valeur arrondie propre à la tuile. Rebaser les valeurs à
  l'origine d'une tuile est permis, parce qu'une translation entière est exacte.
- **La division de perspective a lieu aux multiples de 16 de l'abscisse dans
  l'image**, même quand ce point tombe hors du triangle ou de la tuile ; entre
  deux, l'interpolation est affine. Un segment qui commencerait au bord de la
  tuile ou du triangle donnerait une texture différente selon le découpage.
- **Le niveau de mipmap se choisit par segment de 16 pixels**, sur la même grille,
  à partir de la dérivée entière des coordonnées de texture ; le logarithme se
  prend par `leading_zeros`.
- **Le tramage ordonné** ajoute aux coordonnées 16.16, avant troncature, un
  décalage sous-texel tiré d'une table fixe indexée par la position du pixel dans
  l'image. C'est le filtrage par défaut ; le bilinéaire est le niveau au-dessus.
  La table et son motif sont figés à l'étape 2, et testés.
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
  la capacité reçue, répartition par tuile pour la résolution interne maximale,
  pile de matrices, tampons de clipping. Changer de résolution sous ce maximum
  n'alloue rien.
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
  `.cpp`, `.kt`, `.js` — commence par :

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
  à celle du chemin Rust (`screengine-conformance --print triangle`). Il fait
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
mesure. Aujourd'hui la scène est le triangle en dur,
sur trois images ; chaque ressource chargeable y entrera avec son étape.

### Tests aléatoires

Pas de bibliothèque de tests par propriétés dans le noyau. Les tests qui tirent
des entrées au hasard utilisent un générateur écrit dans le test, avec une graine fixe
affichée en cas d'échec : un échec qui ne se rejoue pas n'a pas été trouvé.
