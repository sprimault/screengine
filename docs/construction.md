# Construction

Cibles, matrice de compilation, génération du header, liaisons. Toute question
du genre « pourquoi le `.so` Android ne se charge pas » se tranche ici.

**État : l'étape 0 n'est pas livrée.** Ce document décrit ce que la construction
doit être. Ce qui est déjà en place est dit comme tel ; les points que l'outillage
réel doit encore confirmer sont marqués **À vérifier**, et les choix restants
**À trancher**.

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
| `cargo-deny` | `0.18.4`, épinglé | ses règles changent de sens d'une version à l'autre |
| `cargo-audit` | la dernière | il lit des avis publiés en continu ; l'épingler figerait ce qu'il sait lire |

Les versions sont épinglées dans le `Makefile` et nulle part ailleurs.
`make tools` les installe, avec la cible `thumbv7em-none-eabihf`. L'intégration
continue appelle `make tools` et lit les versions par `make print-CBINDGEN_VERSION`
plutôt que de les recopier.

## Profils

| Profil | Pour quoi | Réglages |
|---|---|---|
| `dev` | développement | `opt-level = 3` pour le noyau et les dépendances, assertions conservées |
| `release` | binaires : hôte, conformance | `lto = "fat"`, `codegen-units = 1`, `panic = "abort"` |
| `release-ffi` | bibliothèque partagée et statique | hérite de `release`, `panic = "unwind"` |

- **Un rasteriseur logiciel non optimisé est inutilisable**, même pour déboguer :
  le profil de développement optimise le noyau, sans perdre `debug-assertions` ni
  `overflow-checks`.
- **La bibliothèque passe par `release-ffi`.** En `panic = "abort"`,
  `catch_unwind` ne rattrape rien, et une panique qui traverse la frontière C est
  un comportement indéfini. Ne pas unifier les deux profils pour simplifier le
  `Makefile` : `make lib` construit avec le bon.
- La conformance tourne en `release`, comme les binaires publiés. C'est pour cela
  qu'un débordement d'entier ne doit jamais dépendre du profil : voir
  [`rust.md`](rust.md), « Arithmétique et précision ».

## Artefacts

`screengine-ffi` produit une bibliothèque dynamique (`cdylib`) et une bibliothèque
statique (`staticlib`).

| Plateforme | Dynamique | Statique |
|---|---|---|
| Windows (MSVC) | `screengine_ffi.dll` et sa bibliothèque d'importation `screengine_ffi.dll.lib` | `screengine_ffi.lib` |
| Linux, Android | `libscreengine_ffi.so` | `libscreengine_ffi.a` |
| wasm | `screengine_ffi.wasm` | — |

**À trancher — C1** : le nom publié. Le nom de bibliothèque suit celui du crate,
d'où `screengine_ffi`. Le renommer en `screengine` dans le `Cargo.toml` de la
couche FFI entre en collision avec le noyau, qui porte déjà ce nom et dont elle
dépend. Recommandation : garder `screengine_ffi` dans l'arbre de construction, et
renommer en `screengine` à l'empaquetage. Sous Linux et Android, Rust ne pose pas
de `SONAME` et le renommage du fichier suffit. Sous Windows, la bibliothèque
d'importation garde le nom de la DLL d'origine : elle se régénère, ou la
distribution Windows s'en tient à la liaison statique et au chargement
dynamique par nom.

## Matrice

| Cible | Triple | Artefact | Outillage | Hôtes | Contrôle |
|---|---|---|---|---|---|
| Windows x64 | `x86_64-pc-windows-msvc` | `.dll`, `.lib` | MSVC Build Tools | `screengine-host`, `hosts/c`, `hosts/php` | CI |
| Linux x64 | `x86_64-unknown-linux-gnu` | `.so`, `.a` | gcc ou clang | `hosts/php`, conformance | CI |
| Navigateur | `wasm32-unknown-unknown` | `.wasm` | cible rustup | `hosts/web` | CI, avec son hôte |
| Android arm64 | `aarch64-linux-android` | `.so` | NDK | `hosts/android` | CI, avec son hôte |
| Android armv7 | `armv7-linux-androideabi` | `.so` | NDK | `hosts/android` | CI, avec son hôte |
| Android x64 | `x86_64-linux-android` | `.so` | NDK | émulateur | CI, avec son hôte |
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

- **L'hôte C se lie à la bibliothèque statique.** La liaison réclame les
  bibliothèques système dont dépend `std`. Leur liste exacte se lit, plutôt que
  de se deviner, par :

  ```
  cargo rustc -p screengine-ffi --profile release-ffi --crate-type staticlib -- --print native-static-libs
  ```

- **Rust se lie au CRT dynamique (`/MD`).** Un hôte C compilé en `/MT` échoue à
  l'édition de liens sur des symboles en double, ou pire, se lie avec deux tas
  distincts.
- **L'hôte PHP charge la DLL**, par FFI : voir « Liaisons ».

### Linux

- Sert la conformance croisée : les empreintes produites par la glibc et par le
  CRT de MSVC doivent être identiques. Un écart désigne un appel à la libm, une
  hypothèse sur l'alignement ou un chemin SIMD sélectionné différemment — jamais
  une différence acceptable.
- Sert aussi l'hôte PHP, dont l'outillage s'installe plus simplement sous Linux.

### wasm

- **Cible `wasm32-unknown-unknown`, exports C bruts.** Le JavaScript de l'hôte
  instancie le module et appelle les fonctions `scg_` directement.
- **Pas de `wasm-bindgen`.** Il fabriquerait une seconde frontière, propre à
  JavaScript, à côté de l'ABI C : deux contrats à maintenir, et un hôte web qui
  n'éprouverait plus celui que les autres utilisent.
- **`scg_buffer_alloc` est obligatoire**, et toute vue sur la mémoire se recrée
  après chaque appel : voir [`abi.md`](abi.md), « Ce qu'un auteur de liaison doit
  savoir ».
- **À vérifier — C2 : les paniques sur wasm.** Au moment de l'écriture, la chaîne
  stable ne déroule pas la pile sur `wasm32-unknown-unknown` : la bibliothèque
  standard y est précompilée en `panic = "abort"`, une panique y est un trap, et
  `catch_unwind` n'y rattrape rien. Le profil `release-ffi` ne s'y applique donc
  probablement pas tel quel. À établir contre la version de Rust en usage au
  lot 7 : si c'est confirmé, le web a son propre profil, et `abi.md` dit qu'une
  panique y rend l'instance inutilisable.

### Android

- **Trois ABI** : `arm64-v8a` pour les appareils, `armeabi-v7a` pour les anciens,
  `x86_64` pour l'émulateur. armv7 est la cible la plus susceptible de révéler un
  défaut d'alignement ou une hypothèse 64 bits.
- **Le NDK fournit l'éditeur de liens.** Le niveau d'API minimal est celui de
  l'éditeur choisi (`aarch64-linux-android21-clang` pour l'API 21), et Rust exige
  un NDK récent — r25 au minimum depuis Rust 1.68.
- **À trancher — C3** : le passage par JNI. La couche FFI n'exporte que l'ABI C,
  et une méthode `native` Java réclame un symbole `Java_<paquet>_<classe>_<méthode>`.
  Recommandation : une couche JNI mince écrite en C dans `hosts/android`,
  compilée par le NDK et liée à `libscreengine_ffi.so`. Un crate `jni` ferait
  entrer une dépendance et du code propre à Android dans l'arbre Rust, pour ce
  qui n'est que de la conversion.
- **À trancher — C4** : l'outil de construction côté Cargo — `cargo-ndk`, ou la
  configuration des éditeurs de liens du NDK dans `.cargo/config.toml`.

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
   pages de 4 Ko. Les NDK récents alignent sur 16 Ko par défaut ; avec un plus
   ancien, l'alignement se demande à l'éditeur de liens
   (`-Wl,-z,max-page-size=16384`). **À vérifier** au lot 8 contre le NDK en usage.
5. **`UnsatisfiedLinkError: No implementation found for …`** — la bibliothèque est
   chargée, mais la méthode `native` ne trouve pas son symbole JNI : nom de
   paquet, de classe ou de méthode mal reproduit dans la couche JNI, ou méthode
   jamais enregistrée.

## Header

- **`include/screengine.h` est généré** par `cbindgen`, à partir de
  `screengine-ffi` seul, et versionné : généré parce qu'écrit à la main il
  divergerait des signatures, versionné parce qu'un intégrateur doit pouvoir le
  lire sans installer `cbindgen`.
- **`make header`** le régénère. **`make header-verif`** le régénère dans `.tmp/`
  et échoue sur tout écart avec le fichier versionné. L'intégration continue passe
  le second ; un développeur le passe avant de pousser, pour ne pas découvrir
  l'écart après avoir perdu le contexte.
- **`cbindgen.toml`**, à la racine, s'écrit au lot 3 de l'étape 0. Il fixe au
  minimum : le langage C, une garde d'inclusion, la recopie de la documentation,
  et l'en-tête de licence par l'option `header`.
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
- **PHP** : l'extension FFI. **À trancher — C5** : `FFI::cdef` ne comprend pas les
  directives du préprocesseur — `#include`, `#define`, gardes d'inclusion. Le
  header généré ne se passe donc pas tel quel. Recommandation : une seconde
  sortie de `cbindgen`, sans directives, générée et vérifiée comme la première,
  plutôt qu'un filtrage à la main dans l'hôte, qui divergerait en silence.
- **JavaScript** : instancie le `.wasm`, alloue par `scg_buffer_alloc`, écrit les
  structures octet par octet selon les décalages du header — d'où l'absence de
  remplissage implicite exigée par `abi.md`.
- **Java / Kotlin** : par la couche JNI de C3.

## Intégration continue

`.github/workflows/ci.yml`, sur chaque push et chaque pull request vers `master`,
et chaque semaine pour l'audit :

| Job | Plateforme | Contrôles |
|---|---|---|
| vérification | Linux | `fmt`, `lint`, `nostd`, `header-verif`, `deny` |
| tests | Linux et Windows | `test`, `conform` |
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
  construit par `make lib`, puis crée la release en brouillon. Les notes se
  relisent avant de publier.
- **Une archive par cible**, `screengine_<tag>_<cible>`, contenant la
  bibliothèque, le header, `LICENSE` et `THIRD-PARTY-NOTICES` ; un `SHA256SUMS`
  calculé sur les archives, et une attestation de provenance vérifiable par
  `gh attestation verify`.
- Les notes d'une version qui ne rend encore rien disent ce qu'elle ne fait pas.
