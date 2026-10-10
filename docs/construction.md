# Construction

Cibles, matrice de compilation, génération du header, liaisons. Toute question
du genre « pourquoi le `.so` Android ne se charge pas » se tranche ici.

**État : version 1.1.0.** Ce document décrit la construction
telle qu'elle est ; les points que l'outillage réel doit encore confirmer sont
marqués **À vérifier**, et les choix restants **À trancher**.

## Passer par le `Makefile`

Toute construction passe par `make`, jamais par `cargo` en direct. Le `Makefile`
porte ce qu'une commande tapée à la main perd sans que la sortie le montre : le
profil de la bibliothèque partagée, la cible sans `std`, les versions épinglées
des outils.

Un `makefile.local` non versionné, inclus par le `Makefile`, porte ce qui est
propre à un poste : redirection de `CARGO_TARGET_DIR`, liste des hôtes
constructibles, cibles de confort. Il n'y entre rien dont le projet dépende.
Sans lui, un clone se construit dans `target/`.

## Outillage

| Outil | Version | Pourquoi cette version |
|---|---|---|
| Rust | stable, `rust-version = "1.85"` au minimum | édition 2024 |
| GNU Make | 4 | |
| `cbindgen` | `0.29.0`, épinglé | un générateur : une autre version produit un autre header, et `make header-verif` échouerait sur un dépôt propre |
| `cargo-deny` | `0.19.4`, épinglé | ses règles changent de sens d'une version à l'autre ; en deçà de 0.19.1, il ne lit pas les scores CVSS 4.0 de la base d'avis |
| `cargo-audit` | la dernière | il lit des avis publiés en continu ; l'épingler figerait ce qu'il sait lire |
| Node | 22, celui des images d'intégration continue | exécute l'hôte wasm de `make test` et sert sa page ; aucun paquet npm. Ce n'est pas un plancher que le code impose — il n'emploie que des modules ES et le préfixe `node:`, tous deux bien antérieurs — mais la version sur laquelle les tests tournent réellement |
| Go | 1.24 au minimum, celui du `go.mod` de l'hôte | l'hôte Go de `make test`, avec un compilateur C pour cgo — MinGW-w64 sous Windows, `cc` ailleurs. Aucun module tiers |
| NDK, SDK Android | NDK r29, build-tools 36.0.0, `android-36`, épinglés dans `hosts/android/Makefile` | l'hôte Android ; r28 au minimum pour les pages de 16 Ko. Fournis par `hosts/android/Dockerfile` |
| JDK | 17 | `javac` et les outils du SDK |
| `qemu-user` | celui de la distribution | exécute les tests aarch64 et armv7 de l'hôte Android sans appareil |

Les versions sont épinglées dans le `Makefile` et nulle part ailleurs.
`make tools` les installe, avec les cibles `thumbv7em-none-eabihf`,
`wasm32-unknown-unknown`, `i686-pc-windows-msvc` et les trois cibles Android.
`make lint` passe aussi clippy sur toutes celles-là **sauf la cible sans `std`**,
là où un `cfg` propre à une plateforme ne serait vérifié par rien d'autre ; celle
sans `std` est couverte par `make nostd`, qui la compile. Il y ajoute
`screengine-play` sur les deux cibles Linux ARM, et nulle part ailleurs. Il
entraîne en outre **trois** contrôles qui ne sont pas clippy : la documentation
des fonctions de test, que `missing_docs` ne voit pas, la concordance des
versions d'outillage Android, et celle de l'état annoncé avec la version du
paquet. L'intégration continue appelle `make tools`, qui installe les outils aux
versions épinglées ici.

**`make doc-verif` passe rustdoc en `-D warnings`, et aucune autre cible ne
l'entraîne** : un renvoi vers un élément qu'aucun `pub use` n'expose ne fait
rougir ni `lint` ni `test`, et c'est ainsi qu'une constante est restée
inatteignable une version entière. Elle est dans la liste d'avant-publication et
dans le job de vérification, au même titre que `header-verif`.

**`make lint` entraîne un troisième contrôle qui n'est pas clippy :
`etat-verif`**, la concordance de l'état annoncé avec la version du
`Cargo.toml`. Toute ligne d'un document versionné qui porte un numéro d'étape
**et** une version du dépôt annonce l'état courant, et doit donc porter celle du
paquet ; les datations — « Franchie, publiée en 0.4.0 », « ajoutée après la
0.8.1 » — n'en portent pas et restent dehors. Le compte des annonces est vérifié
lui aussi : un motif à moitié faux n'en perdrait qu'une partie, et le contrôle
passerait en ne surveillant plus qu'un document.

**`make print-<VARIABLE>` écrit une valeur et rien d'autre**, et c'est ainsi que
les workflows lisent ce qui vit dans le `Makefile` plutôt que de le recopier :
la cible wasm, les trois cibles Android, la liste des scènes que les hôtes
rendent. Les versions d'outils n'ont pas besoin d'y passer — `make tools` les
installe lui-même, et aucun workflow ne les nomme.

## Profils

| Profil | Pour quoi | Réglages |
|---|---|---|
| `dev` | développement | `opt-level = 3` pour le noyau et les dépendances, assertions conservées |
| `release` | binaires : exemples de l'étage d'accueil, conformance | `lto = "fat"`, `codegen-units = 1`, `panic = "abort"` |
| `release-ffi` | bibliothèque partagée et statique | hérite de `release`, `panic = "unwind"` |
| `ffi-test` | bibliothèques de `make test-abi`, `make test-cpp` et `make test-android` | hérite de `release`, `panic = "unwind"`, sans LTO, assertions conservées |
| `release-wasm` | module wasm publié | hérite de `release`, `panic = "abort"`, `strip = true` |
| `wasm-test` | module de `make test-wasm` | hérite de `ffi-test`, `panic = "abort"` |

- **Un rasteriseur logiciel non optimisé est inutilisable**, même pour déboguer :
  le profil de développement optimise le noyau, sans perdre `debug-assertions` ni
  `overflow-checks`.
- **La bibliothèque passe par `release-ffi`.** En `panic = "abort"`,
  `catch_unwind` ne rattrape rien, et une panique qui traverse la frontière C est
  un comportement indéfini. Ne pas unifier les deux profils pour simplifier le
  `Makefile` : `make lib` construit avec le bon.
- **Sauf sur wasm, qui passe par `release-wasm`.** La chaîne stable n'y déroule
  pas la pile, et `panic = "unwind"` y est ignoré sans avertissement : le profil
  écrit ce qui se passe vraiment. Voir « wasm » plus bas.
- **Les hôtes de `make test` se lient à `ffi-test`**, pas à l'artefact publié : la
  LTO complète ferait payer chaque modification du noyau à chaque `make test`.
  C'est admissible parce que l'image ne dépend pas du profil — une empreinte qui
  diffère entre les deux est un défaut du moteur.
- La conformance tourne en `release`, comme les binaires publiés. C'est pour cela
  qu'un débordement d'entier ne doit jamais dépendre du profil : voir
  [`rust.md`](rust.md), « Arithmétique et précision ».

## Artefacts

`screengine-lib` produit une bibliothèque dynamique (`cdylib`) et une
bibliothèque statique (`staticlib`), sous le nom `screengine`. Il ne contient
aucun code : il ré-exporte les points d'entrée de `screengine-ffi`, qui ne
produit plus qu'une rlib.

| Plateforme | Dynamique | Statique |
|---|---|---|
| Windows (MSVC) | `screengine.dll` et sa bibliothèque d'importation `screengine.dll.lib` | `screengine.lib` |
| Linux, Android | `libscreengine.so` | `libscreengine.a` |
| wasm | `screengine.wasm` | — |

**Un crate de plus, pour le nom.** Nommer `screengine` la bibliothèque de
`screengine-ffi` ferait entrer sa rlib en collision avec celle du noyau, qui
porte déjà ce nom. Écarté aussi : renommer les fichiers à l'empaquetage. Sous
Windows, la bibliothèque d'importation désigne la DLL par son nom d'origine et
devrait être régénérée, et l'archive ne contiendrait plus ce que rustc a
produit. Avec le crate, les noms sont natifs partout, et les hôtes du dépôt
chargent le même nom que les intégrateurs.

**Sous Linux et Android, la bibliothèque dynamique porte le SONAME
`libscreengine.so`**, posé par le `build.rs` de `screengine-lib` : rustc n'en pose
aucun, et sans lui l'entrée DT_NEEDED d'un programme prend ce que l'éditeur de
liens a reçu — un chemin relatif quand CMake passe la bibliothèque par son
chemin. Aucun numéro de version dans le nom : la compatibilité se vérifie à
l'exécution par `scg_abi_version`.

## Matrice

| Cible | Triple | Artefact | Outillage | Hôtes | Contrôle |
|---|---|---|---|---|---|
| Windows x64 | `x86_64-pc-windows-msvc` | `.dll`, `.lib` | MSVC Build Tools | `screengine-play`, `hosts/c`, `hosts/cpp`, `hosts/go` | CI |
| Windows x86 | `i686-pc-windows-msvc` | `.dll`, `.lib` | MSVC Build Tools | aucun | CI, `make conform-x86` |
| Linux x64 | `x86_64-unknown-linux-gnu` | `.so`, `.a` | gcc ou clang | `hosts/c`, `hosts/cpp`, `hosts/go`, conformance | CI |
| Linux x86 | `i686-unknown-linux-gnu` | `.so`, `.a` | gcc-multilib | aucun | CI, au tag seulement |
| Navigateur | `wasm32-unknown-unknown` | `.wasm` | cible rustup, Node | `hosts/web` | CI, `make test-wasm` sous Linux et Windows |
| wasm WASI | `wasm32-wasip1` | `.wasm` exécutable | cible rustup, Node | aucun | CI, `make test-wasi` et `make conform-wasi`, pour le noyau |
| Android arm64 | `aarch64-linux-android` | `.so`, `.a` | NDK, `qemu-user` | `hosts/android` | CI, `make test-android` sous Linux, sous `qemu-user` ; `make test-arm` et `make conform-arm` pour le noyau |
| Android armv7 | `armv7-linux-androideabi` | `.so`, `.a` | NDK, `qemu-user` | `hosts/android` | CI, `make test-android` sous Linux, sous `qemu-user` ; `make test-arm` et `make conform-arm` pour le noyau |
| Android x64 | `x86_64-linux-android` | `.so`, `.a` | NDK, SDK, émulateur | `hosts/android` | CI, `make test-android` sous Linux, sur émulateur par JNI |
| Linux arm64 | `aarch64-unknown-linux-gnu` | `.rlib` du noyau, de la frontière et de l'étage d'accueil | cible rustup, `gcc-aarch64-linux-gnu`, `qemu-user` | `screengine-play`, compilé et non exécuté | CI, `make test-linux-arm` et `make conform-linux-arm` sous `qemu-user` ; `make lint` et `make msrv` |
| Linux armv7 | `armv7-unknown-linux-gnueabihf` | `.rlib` du noyau, de la frontière et de l'étage d'accueil | cible rustup, `gcc-arm-linux-gnueabihf`, `qemu-user` | `screengine-play`, compilé et non exécuté | CI, `make test-linux-arm` et `make conform-linux-arm` sous `qemu-user` ; `make lint` et `make msrv` |
| Sans `std` | `thumbv7em-none-eabihf` | `.rlib` du noyau seul | cible rustup | aucun | `make nostd`, CI |
| iOS, macOS | — | — | — | — | hors périmètre v1 |

**La cible sans `std` n'est pas une plateforme visée.** Elle existe parce qu'elle
n'a pas de `std` du tout : un `use std::` glissé dans le noyau y échoue à la
compilation, au lieu d'échouer au portage trois semaines plus tard. Elle est de
plus 32 bits, et révèle au même moment une hypothèse sur la largeur de `usize`.

**iOS et macOS attendent que le reste soit stable.** Ils exigent un runner macOS,
et un cycle de retour lent depuis un poste Windows.

**Les deux cibles Linux sur ARM s'exécutent sous `qemu-user`.** Elles ont le jeu
d'instructions des ABI Android et une autre bibliothèque C — la glibc là où le
NDK porte la bionic —, ce qui est précisément la question qu'elles répondent : le
noyau n'emploie aucune des deux, mais la frontière passe par `std`.

**`make test-linux-arm` exécute le noyau et la frontière**, `make
conform-linux-arm` rejoue les vingt-neuf scènes contre les **mêmes** empreintes
versionnées — la raison qui donne sa forme à `conform-arm` et à `conform-x86`.
La frontière rend un test de moins sur armv7, et c'est attendu : celui du
registre flottant n'existe que sur `aarch64`, les fonctionnalités ARM 32 bits
étant instables sur chaîne stable.

**Pas de liaison statique ici**, contrairement aux cibles Android : celles-là
réclament `/system/bin/linker64`, qui n'existe que sur un appareil, là où une
glibc porte son éditeur de liens dynamique dans le sysroot croisé — que `-L`
donne à qemu. Et pas de `seccomp=unconfined` : c'est la bionic 32 bits qui
appelle `personality` à son démarrage, pas la glibc.

**Les `libc6-dev-*-cross` se nomment explicitement** quand on installe
l'outillage. Sans eux, le `gcc` croisé est présent mais l'édition de liens
échoue sur `Scrt1.o` et `crti.o` introuvables — un message qui ne nomme pas le
paquet qui manque. `docker/linux-arm/Dockerfile` fournit l'ensemble sous Linux,
et `make lint` et `make msrv` continuent de les compiler en `--lib`, sans rien
installer.

**`screengine-play` entre dans la boucle de `make lint`** sur ces deux cibles, en
`--lib` : X11, Wayland et xkbcommon passant par `dlopen`, la compilation n'exige
aucune bibliothèque système. C'est la seule paire de cibles où il y entre — un
Raspberry Pi est la seule machine de la matrice où quelqu'un ouvrirait une
fenêtre. Il reste hors de `make msrv`, qui ne répond que du noyau et de la
frontière : le plancher de `winit`, de `softbuffer` et de `png` n'est pas sous le
contrôle du projet.

**À vérifier** : l'ouverture d'une fenêtre sur un appareil ARM. Ce que la boucle
prouve est que l'étage d'accueil compile, jamais qu'un compositeur l'accepte — et
l'exécution ci-dessus ne couvre que le noyau et la frontière, qui n'ouvrent
rien.

**Les deux cibles bureau 32 bits ne portent aucun hôte, et sont éprouvées par la
conformance seule.** Lier `hosts/c` ou `hosts/cpp` contre elles demanderait une
seconde chaîne C pour cette architecture sur chaque runner, pour vérifier une
édition de liens que le 64 bits vérifie déjà ; ce qu'elles ont de propre, c'est
la largeur du pointeur, et c'est une empreinte qui le dit. `make conform-x86`
rejoue donc les scènes sur `CIBLE_X86` **contre les mêmes références** — des
références propres au 32 bits ne diraient que « i686 est reproductible avec
lui-même ».

Une seule des deux tourne à chaque PR, celle de Windows, parce qu'elle s'y lie
sans rien installer. Une divergence viendrait de l'architecture et non du
système : les deux couples diraient la même chose, et le second coûterait
`gcc-multilib` sur le runner Linux. L'archive `linux_x86` est donc construite et
sa conformance jouée **au tag**, où le paquet l'installe de toute façon.

Le risque propre à x86 32 bits est la **x87**, dont les registres à 80 bits
arrondissent deux fois. Rust l'évite en activant SSE2 sur ces cibles —
`rustc --print cfg --target i686-pc-windows-msvc` le liste —, et les empreintes
le confirment : chaque scène de conformance y rend la même empreinte que sur
toutes les autres cibles.

## Par cible

### Windows

- **L'hôte C se compile avec MSVC**, `cl -MD -std:c17`. MinGW est écarté : il
  ne se lie pas à une bibliothèque statique produite pour la cible MSVC.
  `hosts/c/build.cmd` trouve `cl` dans le PATH, sinon par `vswhere` et
  `vcvars64`, et place les outils MSVC devant le `link.exe` de Git Bash.
- **L'environnement MSVC ne se charge que dans `build.cmd`**, jamais pour tout
  un shell Git Bash. Quand il est chargé, rustc cherche `link.exe` par le PATH
  au lieu de le trouver lui-même, et tombe sur le `link` de Git : toute la
  compilation Rust échoue. C'est pourquoi l'intégration continue n'utilise pas
  d'action d'invite développeur.
- **`-std:c17` n'est pas un confort.** Sans lui, `cl` ne définit pas
  `__STDC_VERSION__`, et les `_Static_assert` de disposition du header ne sont
  pas compilés — sans rien signaler. L'hôte C s'arrête sur un `#error` dans ce
  cas.
- **L'hôte C se lie à la bibliothèque statique.** La liaison réclame les
  bibliothèques système dont dépend `std` — aujourd'hui `kernel32 ntdll userenv
  ws2_32 dbghelp`. Elles sont figées dans `hosts/c`, et `make native-libs` en
  relit la liste quand une montée de Rust la change : une liste périmée échoue
  bruyamment à l'édition de liens, jamais en silence.
- **Rust se lie au CRT dynamique (`/MD`).** Un hôte C compilé en `/MT` échoue à
  l'édition de liens sur des symboles en double, ou pire, se lie avec deux tas
  distincts.
- **Deux bibliothèques statiques Rust ne cohabitent pas dans un même binaire.**
  Chacune embarque sa bibliothèque standard : un intégrateur qui lie déjà une
  autre bibliothèque Rust en statique aura des symboles en double, et doit
  passer par la bibliothèque dynamique.
- **L'hôte C++ se lie à la DLL** par sa bibliothèque d'importation
  `screengine.dll.lib`, avec `cl -MD -std:c++17`, et sans aucune
  bibliothèque système : c'est la DLL qui les porte. La DLL est copiée à côté de
  l'exécutable, seul emplacement de recherche qui ne dépende ni du PATH ni du
  répertoire courant. L'hôte vérifie que `scg_abi_version` vient bien du module
  `screengine.dll` : `&scg_abi_version` y désigne le thunk d'import de
  l'exécutable, pas la fonction.
- **`windows.h` définit une macro `small`**, héritée de `rpcndr.h`. Un
  intégrateur C++ qui nomme ainsi une variable obtient une erreur de syntaxe
  sans rapport apparent avec le moteur.
- **`windows.h` définit aussi `near` et `far`**, macros vides héritées de la
  mémoire segmentée 16 bits. Aucun champ ni paramètre de l'ABI ne porte ces
  noms — le plan proche de `ScgCamera` s'appelle `near_plane` pour cela : un
  champ nommé `near` disparaîtrait dans toute unité de compilation qui inclut
  `windows.h` avant le header, et l'erreur ne désignerait pas la cause.

### Linux

- Sert la conformance croisée : les empreintes produites par la glibc et par le
  CRT de MSVC doivent être identiques. Un écart désigne un appel à la libm, une
  hypothèse sur l'alignement ou un chemin SIMD sélectionné différemment — jamais
  une différence acceptable.
- **L'hôte C++ se compile avec `c++ -std=c++17 -pthread`**, lié par
  `-L… -lscreengine` : l'éditeur de liens y préfère la bibliothèque partagée
  à l'archive voisine. Un `rpath` vers le répertoire de construction la retrouve
  à l'exécution, sans `LD_LIBRARY_PATH`. `-pthread` sert ses tuiles rendues sur
  plusieurs threads.
- **L'hôte C se compile avec `cc -std=c11`**, lié à `libscreengine.a` puis
  à `-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc`. `gcc_s` porte le dépliage, sans
  lequel `catch_unwind` ne rattraperait rien.
- **`screengine-play` s'y compile sans paquet système** : X11, Wayland et
  xkbcommon sont chargés par `dlopen`, et seule l'exécution les exige. **À
  vérifier** : l'ouverture de la fenêtre sous X11 et sous Wayland. Sans les
  décorations dessinées de `winit`, retirées pour leurs dépendances, une fenêtre
  sous un compositeur Wayland qui n'en fournit pas — GNOME — n'a pas de barre de
  titre.

### wasm

- **Cible `wasm32-unknown-unknown`, exports C bruts.** Le JavaScript de l'hôte
  instancie le module et appelle les fonctions `scg_` directement.
- **Pas de `wasm-bindgen`.** Il fabriquerait une seconde frontière, propre à
  JavaScript, à côté de l'ABI C : deux contrats à maintenir, et un hôte web qui
  n'éprouverait plus celui que les autres utilisent.
- **Le module est compilé avec `simd128`, et cela fixe un plancher.** Les
  instructions vectorielles sont dans le binaire, donc un navigateur qui ne
  porte pas ce jeu **ne charge pas le module** : sa validation échoue avant la
  première instruction, et aucun réglage — `scg_set_simd` compris — ne peut y
  rattraper quoi que ce soit. Le plancher est **Chrome 91, Firefox 89 et
  Safari 16.4**, le dernier arrivé datant de mars 2023.

  Écarté : publier deux modules, avec et sans, et les départager par une
  détection côté JavaScript. C'est la forme juste si un navigateur plus ancien
  devient un besoin, mais elle double un artefact de la matrice et des archives
  pour une population à qui un moteur logiciel ne rendrait de toute façon pas
  une image fluide.
- **`scg_buffer_alloc` est obligatoire**, et toute vue sur la mémoire se recrée
  après chaque appel : voir [`abi.md`](abi.md), « Ce qu'un auteur de liaison doit
  savoir ».
- **`make lib-wasm`** construit le module par `cargo rustc --crate-type cdylib`,
  pour ne pas produire la bibliothèque statique, et le dépose dans
  `target/wasm32-unknown-unknown/release-wasm/screengine.wasm`. Il
  s'instancie sans aucun import et exporte `memory` avec les fonctions `scg_`.
- **Une panique est un trap.** Constaté avec Rust 1.98 : la bibliothèque
  standard de la cible est précompilée en `panic = "abort"`, et un profil en
  `panic = "unwind"` se construit sans erreur ni avertissement, mais une panique
  rattrapée par `catch_unwind` finit quand même en `RuntimeError: unreachable`.
  Seule une chaîne nightly avec `-Zbuild-std` déroulerait la pile. D'où le
  profil `release-wasm`, et le crochet de `screengine-ffi` qui écrit le texte de
  la panique avant le trap ; ce qu'en fait l'hôte est dans [`abi.md`](abi.md),
  « Après une panique ».
- **wasm n'a pas d'environnement flottant à fixer.** Sa spécification impose
  l'arrondi au plus proche et un sous-dépassement graduel, sans mode qui les
  change : la frontière y passe par son module neutre.
- **`make lint` passe aussi clippy sur la cible**, pour le noyau et la couche
  FFI : un `cfg` propre à wasm n'est vérifié par aucune autre commande.
- **L'hôte est en JavaScript, sans paquet npm.** `hosts/web/screengine.js` est
  commun au test et à la page, sans API de Node ni du DOM. `make test-wasm` le
  lance sous Node, sans fenêtre ; `make web` sert la page sur
  `http://127.0.0.1:8080/`, puisque `fetch` ne lit pas un `.wasm` en `file://`.
  **La page charge `salles.world` et `caisse.mesh`, et les parcourt au
  clavier** : c'est l'hôte de démonstration du web, et le patron des autres — un
  canvas, des événements, une boucle, et aucune bibliothèque de fenêtrage à
  démêler de ce qu'il montre du moteur.
  Écarté : TypeScript, que Node exécute désormais sans compilation mais qu'un
  navigateur ne lit pas — la page exigerait alors un compilateur, donc npm.
  La page a été vue dans un navigateur avant la 0.0.0 ; l'intégration continue
  n'en fait tourner que le test sous Node.
- **`make test-wasi` et `make conform-wasi` exécutent le noyau sur une cible
  wasm**, par `hosts/web/wasi-run.mjs` et le `node:wasi` de Node. Rien n'y
  tournait : `make lint` compile pour `wasm32-unknown-unknown` sans rien
  exécuter, et `make test-wasm` éprouve la bibliothèque à travers l'ABI, pas le
  noyau.

  **La cible n'est pas celle qu'on publie**, et c'est WASI qui fait la
  différence : `wasm32-unknown-unknown` n'a ni arguments, ni sortie standard, ni
  système de fichiers, donc un binaire de test n'y a aucun moyen de dire ce
  qu'il a trouvé et la conformance ne peut pas lire ses références. Le code
  vectoriel et l'arithmétique sont identiques entre les deux — même
  architecture, mêmes `target_feature` —, et le noyau ne touche pas au système :
  c'est le compromis de `qemu` sur Android, où les tests tournent sur l'ABI
  publiée mais sans appareil.

  **Deux choses que cette cible ne peut pas éprouver**, et toutes deux se
  sautent en le disant plutôt que d'échouer : la passe `threads` de la
  conformance, Node n'ayant pas wasi-threads, et le cas du noyau qui compte sur
  `catch_unwind` — une panique est un trap sur wasm, ce que `docs/abi.md` décrit
  déjà comme la raison pour laquelle l'état défaillant n'y est pas observé.

  **En release, et les assertions de debug actives.** Le module de test en
  profil `dev` fait segfauter Node au bout d'une centaine de cas, à un rang qui
  varie d'une exécution à l'autre, et élargir la pile wasm n'y change rien ;
  `-C debug-assertions=on` garde les douze `debug_assert!` du noyau, qui ne
  dépendent pas du profil. Et `--nocapture`, parce qu'un trap arrête le
  processus : sans lui, un échec ne nomme que le test commencé.

### Go

- **Lié par cgo à la bibliothèque dynamique**, et non à la statique comme l'hôte
  C : celle-ci réclame ses bibliothèques système, que cgo passerait en drapeaux
  d'édition de liens à tenir à jour dans le `Makefile` de l'hôte. Le chargement
  dynamique est de plus ce qu'un intégrateur fera.
- **Sous Windows, MinGW-w64 se lie directement à la DLL produite par MSVC** —
  `-lscreengine.dll`, sans `gendef` ni `dlltool`. C'est ce qui ouvre Windows à cet
  hôte là où l'hôte C y reste sur MSVC, faute de pouvoir lier une **statique**
  MSVC depuis MinGW.
- **Aucun pointeur vers le tas de Go ne traverse la frontière.** Tout ce que le
  moteur lit ou écrit passe par `calloc` : le langage déplace ses objets, et une
  adresse prise sur l'un d'eux cesse d'être valide sans prévenir. Le moteur
  ne conserve rien au-delà d'un appel, mais s'en remettre à cette clause pour une
  écriture de tampon serait tenir l'invariant du moteur pour une garantie du
  langage.
- **C'est ce que cet hôte éprouve que les quatre autres n'éprouvent pas** : que
  l'ABI se consomme depuis un langage qui déplace ses objets. C et C++ n'en
  déplacent aucun,
  JavaScript et Java ne passent jamais de pointeur.
- Aucun module tiers, et le cache de Go va sous `.tmp/` comme le reste : ailleurs
  il tomberait dans le profil de l'utilisateur.

### Android

- **Trois ABI** : `arm64-v8a` pour les appareils, `armeabi-v7a` pour les anciens,
  `x86_64` pour l'émulateur. armv7 est la cible la plus susceptible de révéler un
  défaut d'alignement ou une hypothèse 64 bits.
- **armv7 ne se reconnaît pas à ses `target_feature`.** Les fonctionnalités ARM
  32 bits sont instables, et leur `cfg` n'est jamais vrai sur une chaîne stable :
  `rustc --print cfg --target armv7-linux-androideabi` n'en liste aucune. La
  frontière choisit donc son module flottant par l'ABI — `target_os = "android"`
  ou `target_abi = "eabihf"` —, et une cible qu'aucun module ne couvre échoue à
  la compilation plutôt que de passer sans fixer son environnement.
- **Le NDK fournit l'éditeur de liens**, passé à Cargo par les variables
  `CARGO_TARGET_<TRIPLE>_LINKER` du `Makefile`, et `make lib-android` construit
  les trois ABI. Écarté : `cargo-ndk`, un outil de plus à épingler pour trois
  chemins ; et `.cargo/config.toml`, local au poste. L'éditeur de liens fixe le
  niveau d'API — 21, par `ANDROID_API` — et celui d'armv7 s'appelle
  `armv7a-linux-androideabi21-clang`, avec un `a` que le triple Rust n'a pas.
  NDK r29, épinglé dans `hosts/android/Makefile` ; r28 au minimum, qui aligne
  sur des pages de 16 Ko sans option.
- **Le dépliage n'exige rien de plus** : la bibliothèque standard lie la
  libunwind du NDK d'elle-même, et `release-ffi` s'applique tel quel.
- **La couche JNI est en C**, dans `hosts/android/jni.c`, compilée par le NDK en
  une bibliothèque séparée, `libscreengine_jni.so`, liée dynamiquement à
  `libscreengine.so` — c'est la bibliothèque publiée qui est chargée, pas une
  copie. Elle n'exporte que `JNI_OnLoad`, qui enregistre les méthodes par
  `RegisterNatives` : un nom ou une signature fausse fait échouer le chargement
  au lieu du premier appel. Écarté : le crate `jni`, qui ferait entrer une
  dépendance et du code propre à Android dans l'arbre Rust pour de la
  conversion.
- **Le bitmap s'écrit sans copie**, par `AndroidBitmap_lockPixels`. Android y
  donne le `stride` en octets par ligne ; l'ABI l'attend en pixels, d'où la
  division par quatre. `Bitmap.setPixels(int[])` est écarté : il échange rouge et
  bleu, voir [`abi.md`](abi.md).
- **Java seul, sans Gradle.** `javac`, `d8`, `aapt2`, `zipalign` et `apksigner`
  du SDK, pilotés par le `Makefile` de l'hôte. Kotlin exigerait son compilateur,
  et Gradle un cache de dépendances, pour un hôte de quelques centaines de
  lignes.
- **`make test-arm` exécute les tests du noyau sur les deux ABI ARM**, sous
  `qemu-user` et sans appareil. Ils n'y avaient jamais tourné, seulement
  compilé : `make lint` y passe clippy et `make nostd` prouve le bare-metal, mais
  ni l'un ni l'autre n'exécute quoi que ce soit, et seul l'hôte C y rendait une
  empreinte. La virgule fixe, les tables, le balayage et la cuisson n'avaient
  donc jamais rendu un verdict sur une autre architecture.

  **Les binaires de test se lient en statique**, par
  `-C target-feature=+crt-static`, et sans cela rien ne démarre : un binaire
  Android dynamique réclame `/system/bin/linker64`, qui n'existe que sur un
  appareil, et `qemu` s'arrête sur un interpréteur introuvable. C'est la raison
  pour laquelle l'hôte C du premier palier est lui aussi lié en statique.

  **Et `qemu-arm` réclame que le filtre d'appels système soit levé** : il appelle
  `personality` pour émuler un espace 32 bits, ce que le réglage par défaut d'un
  conteneur refuse. L'image d'Android fournit le reste — NDK, cibles rustup,
  `qemu` —, et la cible saute en le disant quand l'un manque. **En intégration
  continue, ce saut est une erreur** : le job qui l'appelle installe tout ce
  qu'elle demande, donc un saut n'y signale pas un poste démuni mais une étape
  d'installation cassée, et un vert l'aurait caché.
- **`make conform-arm` rejoue la conformance sur les mêmes deux ABI**, contre
  les **mêmes** empreintes versionnées, pour la raison qui donne sa forme à
  `make conform-x86` : des références propres à ARM ne diraient que « ARM est
  reproductible avec lui-même ». C'est le seul endroit où les vingt-neuf scènes
  se comparent hors de x86 — les hôtes n'en portent que quatorze, et par le seul
  chemin de remplissage que leur machine résout.

  Elle partage avec `make test-arm` sa détection d'outillage et son
  environnement d'exécution, dans une recette unique : deux copies de cette
  détection finiraient par diverger, et un `qemu` oublié d'un côté y ferait
  sauter en silence la cible qui en dépend.
- **Le test a deux paliers**, et `make test-android` exige que leurs cinq
  empreintes soient identiques. **La cible entière demande un appareil ou un
  émulateur joignable par `adb`** : le premier palier s'en passe, mais il ne se
  lance pas seul, et sans `adb` la cible s'annonce impossible plutôt que de
  rendre un demi-résultat.
  1. `hosts/c/main.c`, lié en statique à la bibliothèque statique de chaque ABI,
     exécuté sans appareil — x86_64 directement, aarch64 et armv7 sous
     `qemu-user`. C'est le seul palier qui éprouve FPCR et FPSCR : un émulateur
     x86_64 ne voit que MXCSR.
  2. Sur un émulateur ou un appareil joignable par `adb` : le même `main.c` lié
     à la bibliothèque dynamique, puis `Test.java` lancé par `app_process`, à
     travers la couche JNI et l'ART, sur un tampon direct dont la base est
     décalée d'un octet.

  **Le maillage est poussé sur l'appareil** avec les bibliothèques, et chaque
  palier reçoit son chemin : les trois exécutions du premier le lisent depuis le
  dépôt — elles tournent sur la machine —, les deux du second depuis
  `/data/local/tmp`. Le maillage voyage avec les quatre autres fichiers de
  données — le décor, celui de collision, la liste des balayages et celle des
  rayons —, et ce sont les seuls que ces hôtes ouvrent,
  et c'est ce qui éprouve le chargement d'une ressource à travers le pont JNI.

  L'APK n'est pas lancé par le test, mais construit par lui.
- **`hosts/android/Dockerfile`** fournit tout cela sous Linux avec KVM : chaîne
  Rust, NDK, SDK, émulateur x86_64 et `qemu-user`. L'image se nomme
  `screengine-android`, et le cache — registre Cargo, cibles, AVD — vit dans un
  volume. En intégration continue, le job `tests` Linux installe les mêmes
  versions et démarre l'émulateur par son action ; Windows retire l'hôte par
  `make test SANS=android`, une exclusion écrite plutôt qu'un saut.

#### Pourquoi le `.so` ne se charge pas

Dans l'ordre où les causes se rencontrent :

1. **`dlopen failed: library "…" not found`** — la bibliothèque n'est pas dans le
   bon répertoire de `jniLibs/` pour l'ABI de l'appareil, ou le nom passé à
   `System.loadLibrary` porte le préfixe `lib` ou l'extension `.so`, qu'il ne
   doit pas porter.
2. **`… is 32-bit instead of 64-bit`** ou **`has unexpected e_machine`** — une
   bibliothèque d'une autre architecture a été copiée dans le répertoire d'une
   ABI.
3. **`cannot locate symbol "…"`** sur un symbole de la libc — la bibliothèque a été
   liée pour un niveau d'API plus récent que celui de l'appareil. L'éditeur de
   liens choisi fixe ce niveau.
4. **Refus sur un appareil à pages de 16 Ko** — la bibliothèque est alignée sur des
   pages de 4 Ko. Le NDK r29 aligne les ABI 64 bits sur 16 Ko sans option,
   constaté sur `arm64-v8a` et `x86_64` par `llvm-readelf -l` ; `armeabi-v7a`
   reste à 4 Ko, et c'est attendu : les pages de 16 Ko n'existent que sur les
   appareils 64 bits. Avec un NDK antérieur à r28, l'alignement se demande à
   l'éditeur de liens (`-Wl,-z,max-page-size=16384`). L'APK se vérifie par
   `zipalign -c -P 16`.
5. **`JNI_OnLoad` rend une erreur au chargement** — `RegisterNatives` n'a pas
   trouvé la classe ou une méthode : nom de paquet, de classe, de méthode ou
   signature mal reproduit dans `jni.c`. Sans `RegisterNatives`, le même défaut
   n'apparaîtrait qu'au premier appel, en `UnsatisfiedLinkError`.

## Hôtes de démonstration

Les hôtes de `hosts/` ont deux rôles, et tous ne portent pas les deux : celui qui
éprouve la frontière sans fenêtre — `make test` le construit et compare son
empreinte — et celui qui montre le moteur. Le second ouvre une fenêtre, donc il
n'est dans aucun contrôle : ni `make test`, ni l'intégration continue ne le
construisent.

**`hosts/go` n'a que le premier**, et c'est cohérent avec ce qu'il existe pour
éprouver : un langage qui déplace ses objets se vérifie sur le passage de
pointeurs, pas sur une fenêtre, et son rôle est le serveur — où il n'y en a pas.

- **`make demo-c` et `make demo-cpp`** chargent la carte et le maillage
  versionnés, et s'y déplacent au clavier — flèches ou ZQSD, Échap pour
  fermer. Ils suivent la page web pas à pas, parce qu'elle n'a aucune
  bibliothèque de fenêtrage à démêler de ce qu'elle montre du moteur.
- **SDL3 y fournit la fenêtre, la texture et les événements**, rien de plus :
  le moteur rend dans un bloc d'octets, et l'hôte le remonte. Une autre
  bibliothèque de fenêtrage se substituerait à SDL sans toucher aux appels
  `scg_`.
- **Sa racine vient de `makefile.local` sous Windows** — la variable `SDL3`,
  pointant sur l'archive `SDL3-devel-VC` dépliée —, **de `pkg-config` ailleurs**,
  qui est la convention de la plateforme. Sans elle, la cible dit ce qui manque
  plutôt que d'échouer sur un `#include`.
- **`build-demo.cmd` compile en `-W4` sans `-WX`**, contrairement aux deux
  autres scripts : les en-têtes de SDL3 ne sont pas écrits pour le niveau
  d'avertissement du projet, et une dépendance tierce n'a pas à l'être.
- **Les deux DLL sont copiées à côté de l'exécutable** pour l'hôte C++, celle du
  moteur et celle de SDL3, pour la raison déjà donnée plus haut.
- **Vus sous Windows** avec SDL3 3.4.16, **et sous Linux** avec le SDL3 du
  gestionnaire de paquets, `libsdl3-dev` 3.4.2 : les deux s'y lient par
  `pkg-config`, ouvrent leur fenêtre et rendent la même image. Vérifié sur un
  serveur X virtuel, en capturant l'écran — un binaire qui démarre sans rien
  afficher passerait un contrôle qui ne regarde que son code de sortie.
- **Android a la sienne, `DemoActivity`** : une `SurfaceView`, deux zones
  tactiles — moitié gauche pour avancer et reculer, moitié droite pour tourner —,
  et le décor rangé dans les ressources de l'APK par `aapt2 link -A`, puisqu'une
  application ne lit pas le dépôt. C'est elle que l'icône lance ; l'activité à
  image fixe reste exportée et se lance par intention explicite.
- **Les damiers des hôtes suivent la densité de plaquage de la carte**, 256
  texels par unité de monde aux murs et 128 au sol : 512 et 256 texels de côté,
  cases de 128 et 32. Un damier de 64, la valeur d'avant, donnait des cases de
  quelques centimètres que le mipmap ramenait à un aplat dès le deuxième
  panneau. Le maillage des caisses garde le sien, son plaquage lui étant propre.
- **Chaque zone tactile est relative à son point de pose.** Une origine fixe au
  centre de la zone a été essayée sur un appareil et ne tient pas en main : le
  pouce ne tombe jamais deux fois au même endroit, et il faudrait dessiner un
  repère. Le manche relatif s'en passe.
- **La cadence affichée est celle de l'écran**, `unlockCanvasAndPost` attendant
  le balayage : elle est plafonnée et ne dit rien du coût d'une image. Le temps
  passé dans le moteur est mesuré et affiché à côté, et c'est lui qui se compare
  d'une étape à l'autre.
- **Le pont JNI rend l'image par tuiles sous un seul verrou du bitmap.**
  `frameBitmap` enchaîne `scg_frame_begin`, les tuiles et `scg_frame_end` côté
  C : appeler chaque tuile depuis Java verrouillerait et déverrouillerait le
  bitmap une cinquantaine de fois par image. Le verrou est pris avant
  `scg_frame_begin`, sans quoi un échec entre les deux laisserait l'image
  ouverte et le contexte refuserait tout ensuite.
- **L'APK se construit là où le SDK est installé**, et s'installe par `adb`
  depuis le poste où l'appareil est branché. **À vérifier** : aucun contrôle
  n'éprouve la démonstration Android en continu — celui du pont JNI tourne sur
  un émulateur sans fenêtre.

## Header

- **`include/screengine.h` est généré** par `cbindgen`, à partir de
  `screengine-ffi` seul, et versionné : généré parce qu'écrit à la main il
  divergerait des signatures, versionné parce qu'un intégrateur doit pouvoir le
  lire sans installer `cbindgen`.
- **`make header`** le régénère. **`make header-verif`** le régénère dans `.tmp/`
  et échoue sur tout écart avec le fichier versionné. L'intégration continue passe
  le second ; un développeur le passe avant de pousser, pour ne pas découvrir
  l'écart après avoir perdu le contexte.
- **`cbindgen.toml`**, à la racine, fixe le langage C, la garde d'inclusion, la
  recopie de la documentation et l'en-tête de licence par l'option `header`. Il
  n'inclut que `stddef.h` et `stdint.h` : aucun `bool` ne traverse la frontière.
- **Le header se compile en C++.** `cpp_compat` l'entoure de gardes
  `extern "C"` ; sans elles, un programme C++ cherche des symboles au nom décoré
  et échoue à l'édition de liens.
- **`cbindgen` ne prouve aucun décalage.** Il analyse la source syntaxiquement et
  n'interroge jamais `rustc` : il ne connaît ni taille ni alignement, et recopie
  l'ordre des champs tel qu'il le lit. Or une liaison JavaScript reproduit ces
  décalages octet par octet. Ils se prouvent donc ailleurs — un test de
  `screengine-ffi` sur `offset_of!`, et des assertions statiques sur `sizeof` et
  `offsetof` injectées par l'option `trailer` de `cbindgen.toml` — une seule
  liste, en `_Static_assert` pour C11 et en `static_assert` pour C++11 —,
  compilées par les hôtes C et C++. Sous MSVC, la norme C++ se lit dans
  `_MSVC_LANG` : `__cplusplus` y reste à 199711 sans `/Zc:__cplusplus`. C'est le seul contrôle qui échoue quand une structure change de
  disposition sans que personne ne l'ait voulu.
- **Le header est en LF**, déclaré dans `.gitattributes` : une conversion en CRLF
  sur un clone Windows ferait échouer `make header-verif` sans qu'une ligne de
  code ait bougé.
- **`hosts/caisse.mesh` suit le même modèle**, pour les données : un maillage
  versionné que les cinq hôtes chargent, engendré par la conformance et
  réécrit par **`make mesh`**. Un test de la conformance le compare octet pour
  octet à ce que le générateur écrit, si bien qu'un fichier périmé échoue là,
  franchement, au lieu de faire diverger les empreintes des hôtes sans dire
  pourquoi. Il est déclaré binaire dans `.gitattributes`. Aucun hôte ne réécrit
  sa disposition : leur faire poser ces octets dans chaque langage serait la même
  liste autant de fois qu'il y a d'hôtes.
- **`hosts/collision.world` et `hosts/collision.sweeps` suivent ce modèle**, pour
  le balayage. Le premier est un décor ; le second porte la **question** — la
  liste des boîtes et des trajets que chaque hôte rejoue, huit octets de magie,
  un compte, puis neuf flottants par balayage, octet de poids faible en tête.

  **Il n'a pas de numéro de version**, parce que ce n'est pas un format du
  moteur : celui-ci ne le lit jamais, et le fichier est livré dans le même commit
  que les hôtes qui le lisent. La magie sert à refuser franchement un mauvais
  chemin, pas à négocier une évolution.

  Sans ce fichier, chaque hôte réengendrerait la liste dans son langage, et son
  empreinte prouverait en plus que chaque hôte a su reporter la même
  règle géométrique — la moins intéressante des deux propriétés, et celle qui
  ferait échouer le contrôle.
- **Les images de `crates/screengine-play/assets/` sont produites pour le
  projet et publiées sous sa double licence**, MIT ou Apache 2.0, comme le
  reste du dépôt. Aucune ne provient d'une œuvre existante, et c'est une
  contrainte et non une préférence : un moteur de cette famille a pour
  tentation permanente de reprendre les données des titres qui l'ont inspiré,
  qui ne sont pas redistribuables.

  Chaque fichier porte en plus ses champs `Author`, `Copyright` et `License`
  dans ses métadonnées PNG, pour qu'une image séparée du dépôt garde sa
  provenance. **Cette phrase-ci fait foi, pas ces champs** : des métadonnées se
  réécrivent sans laisser de trace, un fichier versionné se relit dans
  l'historique.
- **Sa documentation est en anglais.** Ce qu'un auteur de liaison ne peut pas
  ignorer y figure, fonction par fonction.

## Liaisons

Les liaisons vivent dans des dépôts séparés et ne contiennent que de la
conversion de types ; les conditions d'admission sont dans `CONTRIBUTING.fr.md`.
Les hôtes de `hosts/` sont des démonstrations de portabilité, pas des liaisons
publiées. Ce qui suit est ce que chaque langage impose au chargement.

- **C** : inclut le header, se lie à la bibliothèque statique ou dynamique.
- **C++** : inclut le même header, que `cpp_compat` entoure de gardes
  `extern "C"`, et se lie de préférence à la bibliothèque dynamique.
- **Les liaisons à FFI déclaratif** — celles qui lisent le header au lieu de le
  compiler — n'évaluent pas ses lignes de préprocesseur et ne connaissent pas
  les assertions statiques du `trailer` : elles coupent le header à
  `#endif  /* SCREENGINE_H */` et recopient les constantes `SCG_*` à la main.
  C'est pour elles que `abi.md` exige qu'un code inconnu se ramène à sa
  catégorie : elles auront toujours des constantes en retard.
- **JavaScript** : instancie le `.wasm`, alloue par `scg_buffer_alloc`, écrit les
  structures octet par octet selon les décalages du header — d'où l'absence de
  remplissage implicite exigée par `abi.md`.
- **Java / Kotlin** : par la couche JNI décrite plus haut, section « Android ».
  Elle est en C, n'exporte que `JNI_OnLoad`, et enregistre ses méthodes par
  `RegisterNatives`.

## Intégration continue

`.github/workflows/ci.yml`, sur chaque push et chaque pull request vers `master`,
et chaque semaine pour l'audit :

| Job | Plateforme | Contrôles |
|---|---|---|
| vérification | Linux | `fmt`, `lint`, `nostd`, `header-verif`, `doc-verif`, `msrv`, `deny` |
| tests | Linux et Windows | `test`, hôtes C, C++, wasm et Go compris, `conform`, puis `test-wasi` et `conform-wasi` sous Node ; sous Linux seulement, l'hôte Android émulateur démarré puis `test-arm` et `conform-arm` sous `qemu-user` ; l'hôte Android retiré sous Windows par `SANS=android` |
| audit | Linux | `audit`, dans un job à part : un avis publié en amont n'est pas un défaut de la PR en cours |

**`make msrv` construit le noyau et la frontière avec la chaîne que
`rust-version` annonce**, et il est dans ce job plutôt que dans la liste fixe
d'avant-publication : il installe une chaîne, donc il télécharge à froid, et ce
coût se paie une fois ici. Sans lui, une fonction stabilisée après le plancher
déclaré passerait inaperçue jusqu'à ce qu'un intégrateur ouvre le dépôt avec la
chaîne annoncée.

Tout passe par le `Makefile`, et les outils par `make tools`. Les actions sont
épinglées au SHA.

**Chaque job porte un plafond de temps**, et celui de publication aussi. Le
défaut de GitHub est de six heures : une étape qui pend ne rouge pas, elle se
tait, et il faut qu'un humain aille voir. Constaté sur une installation réseau,
un `apt-get update` arrêté en silence pendant trente-deux minutes alors que le
job entier tourne en **onze**, émulateur Android compris. Les plafonds sont
calibrés dessus, avec de quoi absorber un runner lent : les atteindre signale
une panne, jamais une charge.

**Un plafond trop large ne sert à rien**, et c'est ce qui donne sa forme à
celui de l'étape d'installation Android : à quarante minutes sur le job, le
blocage de trente-deux serait passé dessous sans rien dire. L'étape qui
télécharge en porte donc un propre, bien plus court — ce qui nomme la panne au
lieu de laisser le job expirer sans raison affichée.

La protection de branche exige les contrôles par leur nom : un workflow modifié
peut rendre vert un contrôle qui ne vérifie plus rien. C'est pour cela que
`.github/workflows/` se discute avant d'être modifié.

**Les empreintes de référence sont versionnées**, et chaque plateforme les compare
aux mêmes fichiers : Windows et Linux se comparent ainsi entre eux sans étape
dédiée. Une conformance qui ne tournerait que sur une plateforme ne comparerait
rien. Les deux ABI ARM d'Android la jouent sous `qemu-user` et wasm sous WASI,
l'ABI Android x86_64 restant couverte par son hôte sur émulateur.

## Publication

- **Chaque étape franchie est publiée** : l'étape N porte la version `0.N.0`.
- **La section du `CHANGELOG` est la condition de la publication.** Une section
  absente l'arrête ; les notes publiées sont celles qui ont été relues en pull
  request.
- **`.github/workflows/release.yml`**, sur un tag `v*` : vérifie que le tag et la
  version du `Cargo.toml` concordent, lit la section du `CHANGELOG`, repasse les
  tests et la conformance — un tag posé sur un commit rouge ne publie pas —,
  construit par `make lib` — `make lib-wasm` et `make lib-android` pour les deux
  autres —, puis crée la release **en brouillon**.

  **Le tag ne publie donc pas.** Le brouillon est voulu : c'est le moment où les
  notes se relisent, une dernière fois, telles qu'un intégrateur les lira. La
  release devient visible par un geste explicite — `gh release edit vX.Y.Z
  --draft=false`, ou le bouton de l'interface —, et tant qu'il n'est pas fait,
  les archives existent sans que personne ne puisse les trouver. C'est la
  dernière étape de la publication, pas une formalité oubliée.
- **L'hôte Android se teste une fois par publication**, émulateur démarré, sur
  l'entrée qui publie sa bibliothèque. Les autres entrées le retirent par
  `SANS=android` : Windows n'a pas d'émulateur, et le repasser sous Linux ne
  vérifierait rien de plus.
- **Une archive par cible** — `windows_x64`, `windows_x86`, `linux_x64`,
  `linux_x86`, `wasm32`, `android`, cette dernière rangée en `lib/<abi>/` comme
  `jniLibs/` —,
  `screengine_<tag>_<cible>`, contenant le header, `LICENSE-MIT`,
  `LICENSE-APACHE`, `THIRD-PARTY-NOTICES`, et les bibliothèques suivantes ; un
  `SHA256SUMS` calculé sur les archives, et une attestation de provenance
  vérifiable par `gh attestation verify`.

  | Archive | Ce qu'elle emporte |
  |---|---|
  | `windows_x64`, `windows_x86` | `screengine.dll`, sa bibliothèque d'importation, `screengine.lib` |
  | `linux_x64`, `linux_x86` | `libscreengine.so` et `libscreengine.a` |
  | `wasm32` | `screengine.wasm` |
  | `android` | `libscreengine.so` par ABI, **et rien d'autre** |

  **L'archive Android n'emporte pas les bibliothèques statiques**, que la
  matrice ci-dessus dit pourtant produites — elle décrit ce que la cible sort,
  pas ce qu'on distribue. Une statique Rust embarque la bibliothèque standard :
  celle de Windows fait trois mégaoctets et demi en release, et il en faudrait
  trois de plus ici, une par ABI, pour un artefact dont l'usage est `jniLibs/`,
  qui ne prend que des `.so`. Qui veut lier en statique sur Android construit
  depuis un clone, comme pour un correctif entre deux versions.
- **`THIRD-PARTY-NOTICES` couvre ce que les bibliothèques embarquent** sans que
  le projet en dépende : la bibliothèque standard de Rust, `compiler-builtins`
  et son libm, et la libunwind de LLVM sur Android. Un seul fichier pour toutes
  les cibles. Écarté : `cargo-about`, qui ne lit que le graphe de Cargo — vide
  ici — et ne verrait rien de la bibliothèque standard.
- **L'archive est éprouvée avant d'être publiée**, par `make test-archive
  PAQUET=<répertoire décompressé>` : la cible déduit de `lib/` les hôtes que ce
  paquet permet de lier — C contre la statique, C++ contre la dynamique, Node
  contre le module wasm —, les construit contre le seul contenu du paquet et
  compare leurs empreintes à celles du chemin Rust. Sous Linux, le SONAME et
  l'entrée `DT_NEEDED` se vérifient en plus. L'archive Android n'a pas d'hôte à
  lier : son architecture, son SONAME et son alignement de pages se lisent dans
  l'en-tête ELF, sans rien exécuter.

  La cible vit dans le `Makefile` et non dans le workflow **parce que la liste
  des scènes y vit déjà** : recopiée là où elle ne tourne qu'au tag, elle a
  divergé deux fois, et les deux fois l'échec est apparu une fois la version
  posée.
- **`make paquet` monte le paquet, et c'est la même disposition pour tous** :
  `lib/`, `include/`, les deux licences et `THIRD-PARTY-NOTICES`, une entrée
  `abi:triple` rangeant sa bibliothèque sous `lib/<abi>/`. Elle descend dans le
  `Makefile` pour la raison qui y a déjà fait descendre `test-archive` — une
  disposition écrite au seul endroit qui ne tourne qu'au tag n'est vérifiée que
  par la publication elle-même.
- **`make test-paquet` monte les deux paquets de la machine et les éprouve**,
  hors de toute publication : le natif puis celui de wasm. Deux et non un, parce
  qu'aucune archive ne mêle le module wasm aux bibliothèques natives, et qu'un
  paquet qui les mêlerait ferait éprouver une disposition que personne ne
  télécharge. Elle reste hors de la liste d'avant-publication : elle construit en
  `release-ffi`, dont la LTO complète se paierait à chaque passage.
- **Le tag se pose directement**, `vX.Y.Z`, sans tag d'essai préalable : le
  brouillon est déjà le moment où l'on relit avant de rendre visible, et ce que
  le workflow ferait de plus est ce que la liste fixe a vérifié avant le commit.
- Les notes d'une version qui ne rend encore rien disent ce qu'elle ne fait pas.
