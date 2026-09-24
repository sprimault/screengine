# Construction

Cibles, matrice de compilation, génération du header, liaisons. Toute question
du genre « pourquoi le `.so` Android ne se charge pas » se tranche ici.

**État : l'étape 3 est publiée en 0.3.0.** Ce document décrit la construction
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
| Node | 22 au minimum | exécute l'hôte wasm de `make test` et sert sa page ; aucun paquet npm. Présent sur les images d'intégration continue |
| NDK, SDK Android | NDK r29, build-tools 36.0.0, `android-36`, épinglés dans `hosts/android/Makefile` | l'hôte Android ; r28 au minimum pour les pages de 16 Ko. Fournis par `hosts/android/Dockerfile` |
| JDK | 17 | `javac` et les outils du SDK |
| `qemu-user` | celui de la distribution | exécute les tests aarch64 et armv7 de l'hôte Android sans appareil |

Les versions sont épinglées dans le `Makefile` et nulle part ailleurs.
`make tools` les installe, avec les cibles `thumbv7em-none-eabihf`,
`wasm32-unknown-unknown` et les trois cibles Android. `make lint` passe aussi
clippy sur `wasm32-unknown-unknown` et sur les trois cibles Android, là où un
`cfg` propre à une plateforme ne serait vérifié par rien d'autre ; la cible sans
`std`, elle, est couverte par `make nostd`, qui la compile. L'intégration
continue appelle `make tools` et lit les versions par `make print-CBINDGEN_VERSION`
plutôt que de les recopier.

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
| Windows x64 | `x86_64-pc-windows-msvc` | `.dll`, `.lib` | MSVC Build Tools | `screengine-play`, `hosts/c`, `hosts/cpp` | CI |
| Linux x64 | `x86_64-unknown-linux-gnu` | `.so`, `.a` | gcc ou clang | `hosts/c`, `hosts/cpp`, conformance | CI |
| Navigateur | `wasm32-unknown-unknown` | `.wasm` | cible rustup, Node | `hosts/web` | CI, `make test-wasm` sous Linux et Windows |
| Android arm64 | `aarch64-linux-android` | `.so`, `.a` | NDK, `qemu-user` | `hosts/android` | CI, `make test-android` sous Linux, sans appareil |
| Android armv7 | `armv7-linux-androideabi` | `.so`, `.a` | NDK, `qemu-user` | `hosts/android` | CI, `make test-android` sous Linux, sans appareil |
| Android x64 | `x86_64-linux-android` | `.so`, `.a` | NDK, SDK, émulateur | `hosts/android` | CI, `make test-android` sous Linux, sur émulateur par JNI |
| Sans `std` | `thumbv7em-none-eabihf` | `.rlib` du noyau seul | cible rustup | aucun | `make nostd`, CI |
| iOS, macOS | — | — | — | — | hors périmètre v1 |

**La cible sans `std` n'est pas une plateforme visée.** Elle existe parce qu'elle
n'a pas de `std` du tout : un `use std::` glissé dans le noyau y échoue à la
compilation, au lieu d'échouer au portage trois semaines plus tard. Elle est de
plus 32 bits, et révèle au même moment une hypothèse sur la largeur de `usize`.

**iOS et macOS attendent que le reste soit stable.** Ils exigent un runner macOS,
et un cycle de retour lent depuis un poste Windows.

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
  Écarté : TypeScript, que Node exécute désormais sans compilation mais qu'un
  navigateur ne lit pas — la page exigerait alors un compilateur, donc npm.
  La page a été vue dans un navigateur avant la 0.0.0 ; l'intégration continue
  n'en fait tourner que le test sous Node.

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
- **Le test a deux paliers**, et `make test-android` exige que leurs cinq
  empreintes soient identiques :
  1. `hosts/c/main.c`, lié en statique à la bibliothèque statique de chaque ABI,
     exécuté sans appareil — x86_64 directement, aarch64 et armv7 sous
     `qemu-user`. C'est le seul palier qui éprouve FPCR et FPSCR : un émulateur
     x86_64 ne voit que MXCSR.
  2. Sur un émulateur ou un appareil joignable par `adb` : le même `main.c` lié
     à la bibliothèque dynamique, puis `Test.java` lancé par `app_process`, à
     travers la couche JNI et l'ART, sur un tampon direct dont la base est
     décalée d'un octet.

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
- **PHP** : l'extension FFI. `FFI::cdef` ignore les lignes de préprocesseur sans
  les évaluer, mais ne connaît pas les assertions statiques du `trailer` : une
  liaison coupe le header à `#endif  /* SCREENGINE_H */`, et lit les constantes
  `SCG_*` dans les `#define` du même fichier. Constaté sous PHP 8.4 ; aucun hôte
  PHP ne le vérifie en continu.
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
| vérification | Linux | `fmt`, `lint`, `nostd`, `header-verif`, `deny` |
| tests | Linux et Windows | `test`, hôtes C, C++ et wasm compris, `conform` ; l'hôte Android sous Linux seulement, émulateur démarré, et retiré sous Windows par `SANS=android` |
| audit | Linux | `audit`, dans un job à part : un avis publié en amont n'est pas un défaut de la PR en cours |

Tout passe par le `Makefile`, et les outils par `make tools`. Les actions sont
épinglées au SHA.

La protection de branche exige les contrôles par leur nom : un workflow modifié
peut rendre vert un contrôle qui ne vérifie plus rien. C'est pour cela que
`.github/workflows/` se discute avant d'être modifié.

**Les empreintes de référence sont versionnées**, et chaque plateforme les compare
aux mêmes fichiers : Windows et Linux se comparent ainsi entre eux sans étape
dédiée. Une conformance qui ne tournerait que sur une plateforme ne comparerait
rien. wasm et Android n'exécutent pas la conformance en intégration continue ;
leurs empreintes se comparent par leurs hôtes.

## Publication

- **Chaque étape franchie est publiée** : l'étape N porte la version `0.N.0`.
- **La section du `CHANGELOG` est la condition de la publication.** Une section
  absente l'arrête ; les notes publiées sont celles qui ont été relues en pull
  request.
- **`.github/workflows/release.yml`**, sur un tag `v*` : vérifie que le tag et la
  version du `Cargo.toml` concordent, lit la section du `CHANGELOG`, repasse les
  tests et la conformance — un tag posé sur un commit rouge ne publie pas —,
  construit par `make lib` — `make lib-wasm` et `make lib-android` pour les deux
  autres —, puis crée la
  release en brouillon. Les notes se relisent avant de publier.
- **L'hôte Android se teste une fois par publication**, émulateur démarré, sur
  l'entrée qui publie sa bibliothèque. Les autres entrées le retirent par
  `SANS=android` : Windows n'a pas d'émulateur, et le repasser sous Linux ne
  vérifierait rien de plus.
- **Une archive par cible** — `windows_x64`, `linux_x64`, `wasm32`, `android`,
  cette dernière rangée en `lib/<abi>/` comme `jniLibs/` —,
  `screengine_<tag>_<cible>`, contenant le header, `LICENSE-MIT`,
  `LICENSE-APACHE`, `THIRD-PARTY-NOTICES`, et les bibliothèques suivantes ; un
  `SHA256SUMS` calculé sur les archives, et une attestation de provenance
  vérifiable par `gh attestation verify`.

  | Archive | Ce qu'elle emporte |
  |---|---|
  | `windows_x64` | `screengine.dll`, sa bibliothèque d'importation, `screengine.lib` |
  | `linux_x64` | `libscreengine.so` et `libscreengine.a` |
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
- **L'archive est éprouvée avant d'être publiée**, sous Windows et Linux : un
  hôte C lié à la bibliothèque statique et un hôte C++ lié à la dynamique,
  contre le seul contenu du paquet, doivent rendre l'empreinte de référence ;
  sous Linux, le SONAME et l'entrée DT_NEEDED se vérifient aussi.
- **Un tag `vX.Y.Z-essai.N` éprouve le workflow** sans occuper le vrai tag : même
  section du `CHANGELOG`, release en brouillon marquée préversion, à supprimer
  avec le tag.
- Les notes d'une version qui ne rend encore rien disent ce qu'elle ne fait pas.
