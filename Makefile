# Cibles en anglais, commentaires en français : convention des autres projets.

VERSION   ?= dev
SORTIE    ?= .tmp

# La cible qui prouve le no_std. Pas `--no-default-features`, qui ne prouve que
# le drapeau : un `use std::` passé au travers compile quand même, puisque std
# reste disponible pour la cible hôte. Une cible bare-metal n'a pas de std du
# tout, donc l'oubli échoue à la compilation plutôt qu'au portage.
CIBLE_NOSTD ?= thumbv7em-none-eabihf

# La cible du navigateur : exports C bruts, sans wasm-bindgen.
CIBLE_WASM ?= wasm32-unknown-unknown

# Les trois ABI Android. Chacune prend son module d'environnement flottant dans
# screengine-ffi, et armv7 est celle où un `cfg` faux passait sans bruit.
CIBLES_ANDROID ?= aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

# cargo install construit dans un répertoire temporaire du système et n'honore
# pas CARGO_TARGET_DIR. Sur un poste où ce répertoire est surveillé, la variable
# reçoit un --target-dir dans makefile.local ; ailleurs elle reste vide.
CARGO_INSTALL_FLAGS ?=

# Les hôtes construits par `make hosts`. Chacun exige son outillage — un
# compilateur C, php, la cible wasm, le NDK — et aucun poste ne les a tous.
# La liste se surcharge dans makefile.local plutôt que de faire échouer la
# cible sur ce qui manque.
HOSTS ?= c cpp web

# Les éditeurs de liens du NDK, par variables plutôt que par cargo-ndk : trois
# chemins ne justifient pas un outil de plus à épingler. armv7 porte un `a` que
# le triple Rust n'a pas. ANDROID_NDK_HOME vient de l'environnement ; le NDK est
# r28 au moins, qui aligne sur des pages de 16 Ko sans option.
#
# **Le niveau d'API se déclare ici et nulle part ailleurs.** Le Makefile de
# l'hôte et le manifeste le lisaient chacun de leur côté ; il est désormais
# exporté, et l'hôte vérifie que son manifeste porte la même valeur.
ANDROID_API ?= 21
export ANDROID_API
NDK_BIN      = $(ANDROID_NDK_HOME)/toolchains/llvm/prebuilt/linux-x86_64/bin
ANDROID_ENV  = CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$(NDK_BIN)/aarch64-linux-android$(ANDROID_API)-clang \
               CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER=$(NDK_BIN)/armv7a-linux-androideabi$(ANDROID_API)-clang \
               CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER=$(NDK_BIN)/x86_64-linux-android$(ANDROID_API)-clang

# La bibliothèque des trois ABI dans un profil : cdylib pour l'appareil,
# staticlib pour les exécutables de test lancés sans appareil.
android_build = for cible in $(CIBLES_ANDROID); do \
	  $(ANDROID_ENV) cargo build -p screengine-lib --profile $(1) --target $$cible || exit 1; \
	done

# makefile.local porte ce qui est propre au poste et n'est pas versionné. Inclus
# ici et non en tête : une affectation immédiate qui y référencerait une variable
# définie plus haut trouverait une chaîne vide.
#
# Passer par le Makefile plutôt que d'appeler cargo à la main : une commande
# tapée directement perd ces réglages, et l'écart ne se voit pas dans la sortie.
-include makefile.local

.PHONY: build lib lib-wasm lib-android run example web test native-libs fmt fmt-fix lint lint-doc-tests \
        lint-android-versions nostd msrv bench \
        conform conform-update conform-images header header-verif audit deny doc hosts host-c host-cpp host-web \
        host-android clean tools

build:
	cargo build --workspace

# Les bibliothèques publiées viennent de screengine-lib, qui leur donne le nom
# `screengine`. Elles passent par release-ffi et non par release : le premier
# est en panic = "unwind", sans quoi catch_unwind ne rattraperait rien et une
# panique traverserait la frontière C — comportement indéfini, pas plantage.
lib:
	cargo build -p screengine-lib --profile release-ffi

# Le module wasm, par son propre profil : la cible n'a pas de dépliage sur une
# chaîne stable, et le panic = "unwind" de release-ffi y serait ignoré sans
# avertissement. `cargo rustc` plutôt que `cargo build`, pour ne produire que le
# cdylib : la bibliothèque statique n'a rien à faire sur cette cible.
lib-wasm:
	cargo rustc -p screengine-lib --profile release-wasm --target $(CIBLE_WASM) --crate-type cdylib

lib-android:
	@$(call android_build,release-ffi)

# Les exemples de l'étage d'accueil, qui ouvrent une fenêtre. `make run` lance
# le plus petit ; `make example EXAMPLE=nom` en choisit un autre.
EXAMPLE ?= hello

run: example

example:
	cargo run -p screengine-play --example $(EXAMPLE)

PKG ?= --workspace
RUN ?=

test:
	cargo test $(PKG) $(if $(RUN),-- $(RUN))
	@for hote in $(filter-out $(SANS),$(TEST_HOSTS)); do $(MAKE) test-$$hote || exit 1; done

# Les hôtes qui franchissent réellement la frontière, chacun comparant son
# empreinte à celle du chemin Rust sur la même scène, qu'il décrit lui-même
# dans son langage et soumet par l'ABI. Les tests de screengine-ffi
# appellent les fonctions exportées depuis Rust : ils ne voient ni l'édition de
# liens, ni la disposition vue par un autre compilateur, ni l'environnement d'un
# vrai hôte.
#
#   abi   l'hôte C, lié à la bibliothèque statique
#   cpp   l'hôte C++, lié à la bibliothèque dynamique : le header compilé en C++
#         et le chargement dynamique réel
#   wasm     l'hôte web sous Node, sans fenêtre : le module instancié sans
#            import, la mémoire linéaire et scg_buffer_alloc
#   android  les trois ABI sans appareil, x86_64 sur un émulateur par JNI
#
# Chaque hôte garde dans son Makefile `why-not`, `all` et `run`, avec PROFILE et
# OUT ; ce qui suit ne connaît que son répertoire, son profil et la commande qui
# construit ce qu'il charge.
#
# SANS retire un hôte de `make test`, et c'est la seule façon de le faire : une
# exclusion écrite là où on l'appelle, qui se lit dans le workflow. Un outil
# manquant, lui, reste une erreur en intégration continue.
TEST_HOSTS   := abi cpp wasm android
SANS         ?=
TEST_TARGETS := $(addprefix test-,$(TEST_HOSTS)) $(addsuffix -run,$(addprefix test-,$(TEST_HOSTS)))
.PHONY: $(TEST_TARGETS)

host_dir_abi      := c
host_name_abi     := C
host_profile_abi  := ffi-test
host_build_abi     = cargo build -p screengine-lib --profile ffi-test

host_dir_cpp      := cpp
host_name_cpp     := C++
host_profile_cpp  := ffi-test
host_build_cpp     = cargo build -p screengine-lib --profile ffi-test

host_dir_wasm     := web
host_name_wasm    := wasm
host_profile_wasm := wasm-test
host_build_wasm    = cargo rustc -p screengine-lib --profile wasm-test --target $(CIBLE_WASM) --crate-type cdylib

host_dir_android     := android
host_name_android    := Android
host_profile_android := ffi-test
host_build_android    = $(call android_build,ffi-test)

HOST_OUT = $(abspath $(SORTIE))/host-$(host_dir_$*)

# Les scènes que chaque hôte décrit dans son langage, dans l'ordre où il écrit
# leurs empreintes. `arete` relie les hôtes au chemin Rust depuis l'étape 0 ;
# `texture` y ajoute le seul chemin que la première n'emprunte pas, celui du
# remplissage texturé ; `texture-bilineaire` reprend la même géométrie et ne
# change que le filtrage, donc c'est `scg_set_filter` seul qu'elle éprouve de
# bout en bout. Une scène ajoutée ici est une scène à écrire dans les quatre
# hôtes, et c'est voulu : c'est ce qui rend leur comparaison possible.
HOST_SCENES := arete texture texture-bilineaire

# Sans l'outillage de l'hôte, la cible saute et dit pourquoi. En intégration
# continue (CI défini), le même saut est une erreur : un contrôle qui ne tourne
# pas sans que personne le voie est pire que pas de contrôle.
#
# Messages sans accents : la console Windows les reçoit dans une autre page de
# code que celle du Makefile. --no-print-directory explicite sur chaque appel
# d'un hôte : GNU Make 4.3 ne le déduit pas de -s, et la ligne « Entering
# directory » se retrouverait dans la raison de why-not ou dans l'empreinte.
$(addprefix test-,$(TEST_HOSTS)): test-%:
	@reason=$$($(MAKE) -s --no-print-directory -C hosts/$(host_dir_$*) why-not); \
	if [ -z "$$reason" ]; then \
	  $(MAKE) test-$*-run; \
	elif [ -n "$$CI" ]; then \
	  echo "test-$* impossible en integration continue : $$reason"; exit 1; \
	else \
	  echo "test-$* saute : $$reason"; \
	fi

$(addsuffix -run,$(addprefix test-,$(TEST_HOSTS))): test-%-run:
	$(host_build_$*)
	@mkdir -p $(HOST_OUT)
	@: > $(HOST_OUT)/rust.txt
	@for scene in $(HOST_SCENES); do \
	  cargo run -q -p screengine-conformance --release -- --print $$scene >> $(HOST_OUT)/rust.txt || exit 1; \
	done
	$(MAKE) -s --no-print-directory -C hosts/$(host_dir_$*) PROFILE=$(host_profile_$*) OUT=$(HOST_OUT) all
	$(MAKE) -s --no-print-directory -C hosts/$(host_dir_$*) PROFILE=$(host_profile_$*) OUT=$(HOST_OUT) \
	  SCENE_COUNT=$(words $(HOST_SCENES)) run > $(HOST_OUT)/host.txt
	@rust=$$(tr -d '\r' < $(HOST_OUT)/rust.txt); host=$$(tr -d '\r' < $(HOST_OUT)/host.txt); \
	if [ -z "$$host" ]; then \
	  echo "test-$* : l'hote $(host_name_$*) n'a rien ecrit"; exit 1; \
	fi; \
	i=1; for scene in $(HOST_SCENES); do \
	  r=$$(echo "$$rust" | sed -n "$$i p"); h=$$(echo "$$host" | sed -n "$$i p"); \
	  if [ "$$r" != "$$h" ]; then \
	    echo "test-$* : scene $$scene, hote $(host_name_$*) '$$h', chemin Rust '$$r'"; exit 1; \
	  fi; \
	  i=$$((i + 1)); \
	done; \
	echo "test-$* : $(words $(HOST_SCENES)) empreinte(s) identiques au chemin Rust"

# Les bibliothèques système que réclame la bibliothèque statique sur ce poste.
# Elles sont figées dans hosts/c : on relance ceci quand la liaison de l'hôte C
# casse sur un symbole introuvable après une montée de Rust.
native-libs:
	cargo rustc -p screengine-lib --profile ffi-test --crate-type staticlib -- --print native-static-libs

# Doublon assumé avec les formatters de clippy : cargo fmt porte sur tout
# l'arbre, sans exclusion ni configuration, et reste vrai le jour où quelqu'un
# touche à la configuration de clippy.
fmt:
	cargo fmt --all --check

# Applique ce que `fmt` refuse. Une cible plutôt qu'un `cargo fmt` tapé à la
# main : le diff de rustfmt se relit dans `git diff`, alors qu'un reformatage
# reproduit à la main dérive au deuxième essai.
fmt-fix:
	cargo fmt --all

# Le noyau et la frontière passent aussi clippy sur les cibles qu'aucune machine
# de développement n'exécute : leurs `cfg` propres ne sont vérifiés par aucune
# autre commande, et une cible sans module flottant y échoue sur le
# `compile_error!` de screengine-ffi.
# Les versions du NDK et du SDK sont épinglées dans le Makefile de l'hôte, et
# le Dockerfile les répète pour construire l'image de la machine Linux. Il ne
# peut pas lire un Makefile, et le dire en commentaire n'a jamais empêché deux
# valeurs de diverger : une montée faite d'un seul côté produirait une image qui
# construit avec un NDK et une CI qui en attend un autre, ce qui ne se voit
# qu'au premier défaut propre à une version.
lint-android-versions:
	@for nom in NDK_VERSION BUILD_TOOLS PLATFORM SYSTEM_IMAGE; do \
	  fait=$$(sed -n "s/^$$nom *?= *//p" hosts/android/Makefile); \
	  attendu=$$(sed -n "s/^ARG $$nom=//p" hosts/android/Dockerfile); \
	  if [ "$$fait" != "$$attendu" ]; then \
	    echo "$$nom : $$fait dans hosts/android/Makefile, $$attendu dans son Dockerfile"; \
	    exit 1; \
	  fi; \
	done

lint: lint-doc-tests lint-android-versions
	cargo clippy --workspace --all-targets --all-features -- -D warnings
	for cible in $(CIBLE_WASM) $(CIBLES_ANDROID); do \
	  cargo clippy -p screengine -p screengine-ffi --lib --target $$cible -- -D warnings || exit 1; \
	done

# missing_docs ne voit pas les fonctions privées d'un `mod tests`, alors que la
# règle du projet ne fait pas d'exception pour elles. Sans ce contrôle, la
# documentation des tests se dégrade sans que rien ne le dise : on en écrit
# quelques-unes, puis plus, et personne ne s'en aperçoit à la relecture.
#
# Les fichiers par find et non par git ls-files : un arbre sans .git, comme une
# archive, ne donnerait aucun fichier, et awk lirait alors son entrée standard
# au lieu d'échouer — le contrôle attendrait indéfiniment, ou ne vérifierait
# rien.
lint-doc-tests:
	@awk '/#\[test\]/ { if (prev !~ /\/\/\//) { print FILENAME ":" FNR ": #[test] sans documentation"; bad = 1 } } \
	      FILENAME ~ /tests\.rs$$|\/tests\// && /^(fn|const|struct|enum|static|type) / { \
	        if (prev !~ /\/\/\// && prev !~ /^[[:space:]]*#\[/) { \
	          print FILENAME ":" FNR ": declaration de test sans documentation"; bad = 1 } } \
	      { prev = $$0 } END { exit bad }' \
	  $$(find src crates -name '*.rs') < /dev/null \
	  || (echo "Chaque declaration d un module de test porte sa documentation, aides comprises." && exit 1)

# Le noyau seul, sur une cible sans std. Les autres crates en sont dispensés :
# l'étage d'accueil ouvre une fenêtre, la conformance écrit des fichiers, la
# couche FFI formate des messages d'erreur.
nostd:
	cargo build -p screengine --no-default-features --target $(CIBLE_NOSTD)

# La version minimale déclarée, compilée pour de bon.
#
# `rust-version` est un engagement que `CONTRIBUTING` reprend et qu'un
# intégrateur lit avant de choisir sa chaîne — et que rien ne vérifiait : tout
# se construit ici avec une chaîne récente, et une fonction stabilisée depuis
# 1.85 passerait sans bruit jusqu'à ce que quelqu'un ouvre le dépôt avec la
# chaîne annoncée. Le noyau et la frontière suffisent : ce sont eux qu'un
# intégrateur construit, l'étage d'accueil et la conformance portant des
# dépendances dont le plancher leur appartient.
# La version se lit dans le `Cargo.toml` au moment de l'appel, plutôt que
# d'être recopiée ici : c'est lui qui fait foi pour Cargo, et une copie de plus
# se serait mise à mentir comme celles que ce dépôt vient de supprimer.
# La référence de performance, prise avant que l'étape 3 touche au remplissage.
#
# **Hors de la liste fixe, délibérément.** Une durée dépend de la charge de la
# machine : en faire un contrôle le rendrait rouge pour des raisons étrangères
# au code, et on finirait par relever son seuil jusqu'à ce qu'il ne mesure plus
# rien. Rien n'échoue ici — on lit les chiffres et on les compare à ceux que le
# fichier de mesure porte en commentaire.
bench:
	cargo bench -p screengine

msrv:
	@version=$$(grep '^rust-version = ' Cargo.toml | cut -d'"' -f2); \
	echo "version minimale declaree : $$version"; \
	rustup toolchain install "$$version" --profile minimal --no-self-update && \
	cargo "+$$version" build -p screengine -p screengine-ffi --all-targets

# Rejoue les scènes de référence et compare les empreintes. Une divergence est
# soit une régression, soit une évolution volontaire du rendu — dans le second
# cas, conform-update, et dans un commit séparé du lot qui l'a causée.
conform:
	cargo run -p screengine-conformance --release -- --check

conform-update:
	cargo run -p screengine-conformance --release -- --update

# Écrit chaque vue en image, dans .tmp/conformance-images. Une empreinte ne dit
# pas si l'image est juste, seulement si elle a changé : avant de figer une
# référence nouvelle ou de la mettre à jour, on regarde.
conform-images:
	cargo run -p screengine-conformance --release -- --dump $(SORTIE)/conformance-images

# Le header est généré et versionné : généré parce qu'écrit à la main il
# divergerait des signatures, versionné parce qu'un intégrateur doit pouvoir le
# lire sans installer cbindgen.
header:
	cbindgen --config cbindgen.toml --crate screengine-ffi --output include/screengine.h

# Ce que passe l'intégration continue. Un écart signale une édition manuelle du
# header ou une régénération oubliée, et les deux se paient chez celui qui
# intègre, pas ici.
header-verif:
	@mkdir -p $(SORTIE)
	@cbindgen --config cbindgen.toml --crate screengine-ffi --output $(SORTIE)/screengine.h
	@diff -u include/screengine.h $(SORTIE)/screengine.h || { \
	  echo "include/screengine.h diverge : lancer make header"; \
	  exit 1; \
	}

audit:
	cargo audit

# Licences et provenance des dépendances. Le noyau et la couche FFI n'en ont
# aucune ; ce sont l'étage d'accueil et la conformance qui en portent.
#
# --workspace indispensable : le noyau est le paquet racine, et sans ce drapeau
# cargo-deny ne contrôle que lui — licences comprises, sans rien signaler.
deny:
	cargo deny --workspace check

doc:
	cargo doc --workspace --no-deps --open

hosts: $(addprefix host-,$(HOSTS))

host-c: lib
	$(MAKE) -C hosts/c

host-cpp: lib
	$(MAKE) -C hosts/cpp

host-web: lib-wasm
	$(MAKE) -C hosts/web

# La page du navigateur, servie en local : `fetch` ne lit pas un .wasm en
# file://. PORT se choisit sur la ligne de commande.
web: lib-wasm
	$(MAKE) -C hosts/web serve

host-android: lib-android
	$(MAKE) -C hosts/android apk

clean:
	cargo clean
	rm -rf $(SORTIE)

# Les versions sont épinglées ici et nulle part ailleurs : le workflow appelle
# make tools plutôt que de réécrire ses cargo install, et l'action qui lance le
# lint lit CBINDGEN_VERSION par print-%, faute de quoi les deux définitions
# divergent sans que rien ne le signale.
#
# cbindgen est épinglable parce que c'est un générateur : une version différente
# produit un header différent, donc header-verif échouerait sur un dépôt propre.
CBINDGEN_VERSION   ?= 0.29.0
# 0.19.4 et pas en deçà : les versions antérieures à 0.19.1 ne lisent pas les
# scores CVSS 4.0 de la base d'avis et échouent sur toute la base, et 0.19.4
# corrige la lecture des avis sous Windows.
CARGO_DENY_VERSION ?= 0.19.4

# print-<VARIABLE> écrit la valeur d'une variable et rien d'autre, pour que
# l'intégration continue lise l'épinglage plutôt que de le recopier.
print-%:
	@echo $($*)

tools:
	cargo install cbindgen --version $(CBINDGEN_VERSION) --locked $(CARGO_INSTALL_FLAGS)
	cargo install cargo-deny --version $(CARGO_DENY_VERSION) --locked $(CARGO_INSTALL_FLAGS)
	# @latest délibérément : cargo-audit ne signale pas des règles mais des
	# avis, et son intérêt est de connaître les derniers. L'épingler figerait
	# ce qu'il sait lire des avis publiés depuis.
	cargo install cargo-audit --locked $(CARGO_INSTALL_FLAGS)
	rustup target add $(CIBLE_NOSTD) $(CIBLE_WASM) $(CIBLES_ANDROID)
