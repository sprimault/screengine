# Cibles en anglais, commentaires en français : convention des autres projets.

VERSION   ?= dev
SORTIE    ?= .tmp

# La cible qui prouve le no_std. Pas `--no-default-features`, qui ne prouve que
# le drapeau : un `use std::` passé au travers compile quand même, puisque std
# reste disponible pour la cible hôte. Une cible bare-metal n'a pas de std du
# tout, donc l'oubli échoue à la compilation plutôt qu'au portage.
CIBLE_NOSTD ?= thumbv7em-none-eabihf

# cargo install construit dans un répertoire temporaire du système et n'honore
# pas CARGO_TARGET_DIR. Sur un poste où ce répertoire est surveillé, la variable
# reçoit un --target-dir dans makefile.local ; ailleurs elle reste vide.
CARGO_INSTALL_FLAGS ?=

# Les hôtes construits par `make hosts`. Chacun exige son outillage — un
# compilateur C, php, la cible wasm, le NDK — et aucun poste ne les a tous.
# La liste se surcharge dans makefile.local plutôt que de faire échouer la
# cible sur ce qui manque.
HOSTS ?= c web

# makefile.local porte ce qui est propre au poste et n'est pas versionné. Inclus
# ici et non en tête : une affectation immédiate qui y référencerait une variable
# définie plus haut trouverait une chaîne vide.
#
# Passer par le Makefile plutôt que d'appeler cargo à la main : une commande
# tapée directement perd ces réglages, et l'écart ne se voit pas dans la sortie.
-include makefile.local

.PHONY: build lib run example test test-abi test-abi-run test-cpp test-cpp-run native-libs fmt lint lint-doc-tests nostd conform conform-update header \
        header-verif audit deny doc hosts host-c host-web host-android clean tools

build:
	cargo build --workspace

# La bibliothèque partagée passe par release-ffi et non par release : le premier
# est en panic = "unwind", sans quoi catch_unwind ne rattraperait rien et une
# panique traverserait la frontière C — comportement indéfini, pas plantage.
lib:
	cargo build -p screengine-ffi --profile release-ffi

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
	$(MAKE) test-abi
	$(MAKE) test-cpp

# L'hôte C franchit réellement la frontière, lié à la bibliothèque statique, et
# son empreinte doit être celle du chemin Rust. Les tests de screengine-ffi
# appellent les fonctions exportées depuis Rust : ils ne voient ni l'édition de
# liens, ni la disposition vue par un compilateur C, ni l'environnement
# flottant d'un vrai hôte.
#
# Sans compilateur C, la cible saute et dit pourquoi. En intégration continue
# (CI défini), le même saut est une erreur : un contrôle qui ne tourne pas sans
# que personne le voie est pire que pas de contrôle.
ABI_OUT = $(abspath $(SORTIE))/host-c

test-abi:
	@reason=$$($(MAKE) -s --no-print-directory -C hosts/c why-not); \
	if [ -z "$$reason" ]; then \
	  $(MAKE) test-abi-run; \
	elif [ -n "$$CI" ]; then \
	  echo "test-abi impossible en integration continue : $$reason"; exit 1; \
	else \
	  echo "test-abi saute : $$reason"; \
	fi

# Messages sans accents : la console Windows les reçoit dans une autre page de
# code que celle du Makefile.
#
# --no-print-directory explicite sur chaque appel de hosts/c : GNU Make 4.3 ne
# le déduit pas de -s, et la ligne « Entering directory » se retrouverait dans
# la raison de why-not ou dans le fichier d'empreinte.
test-abi-run:
	cargo build -p screengine-ffi --profile ffi-test
	@mkdir -p $(ABI_OUT)
	cargo run -q -p screengine-conformance --release -- --print triangle > $(ABI_OUT)/rust.txt
	$(MAKE) -s --no-print-directory -C hosts/c PROFILE=ffi-test OUT=$(ABI_OUT) all
	$(MAKE) -s --no-print-directory -C hosts/c PROFILE=ffi-test OUT=$(ABI_OUT) run > $(ABI_OUT)/c.txt
	@rust=$$(tr -d '\r' < $(ABI_OUT)/rust.txt); c=$$(tr -d '\r' < $(ABI_OUT)/c.txt); \
	if [ -n "$$c" ] && [ "$$c" = "$$rust" ]; then \
	  echo "test-abi : empreinte $$c, identique au chemin Rust"; \
	else \
	  echo "test-abi : hote C '$$c', chemin Rust '$$rust'"; exit 1; \
	fi

# L'hôte C++, lié à la bibliothèque dynamique : le header compilé en C++ et le
# chargement dynamique réel, que l'hôte C lié en statique ne voit pas. Même
# règle de saut que test-abi.
#
# Ces deux cibles reprennent test-abi presque ligne à ligne. C'est la deuxième
# occurrence : notée, pas extraite ; la troisième — l'hôte wasm — décidera de la
# forme commune.
CPP_OUT = $(abspath $(SORTIE))/host-cpp

test-cpp:
	@reason=$$($(MAKE) -s --no-print-directory -C hosts/cpp why-not); \
	if [ -z "$$reason" ]; then \
	  $(MAKE) test-cpp-run; \
	elif [ -n "$$CI" ]; then \
	  echo "test-cpp impossible en integration continue : $$reason"; exit 1; \
	else \
	  echo "test-cpp saute : $$reason"; \
	fi

test-cpp-run:
	cargo build -p screengine-ffi --profile ffi-test
	@mkdir -p $(CPP_OUT)
	cargo run -q -p screengine-conformance --release -- --print triangle > $(CPP_OUT)/rust.txt
	$(MAKE) -s --no-print-directory -C hosts/cpp PROFILE=ffi-test OUT=$(CPP_OUT) all
	$(MAKE) -s --no-print-directory -C hosts/cpp PROFILE=ffi-test OUT=$(CPP_OUT) run > $(CPP_OUT)/cpp.txt
	@rust=$$(tr -d '\r' < $(CPP_OUT)/rust.txt); cpp=$$(tr -d '\r' < $(CPP_OUT)/cpp.txt); \
	if [ -n "$$cpp" ] && [ "$$cpp" = "$$rust" ]; then \
	  echo "test-cpp : empreinte $$cpp, identique au chemin Rust"; \
	else \
	  echo "test-cpp : hote C++ '$$cpp', chemin Rust '$$rust'"; exit 1; \
	fi

# Les bibliothèques système que réclame la bibliothèque statique sur ce poste.
# Elles sont figées dans hosts/c : on relance ceci quand la liaison de l'hôte C
# casse sur un symbole introuvable après une montée de Rust.
native-libs:
	cargo rustc -p screengine-ffi --profile ffi-test --crate-type staticlib -- --print native-static-libs

# Doublon assumé avec les formatters de clippy : cargo fmt porte sur tout
# l'arbre, sans exclusion ni configuration, et reste vrai le jour où quelqu'un
# touche à la configuration de clippy.
fmt:
	cargo fmt --all --check

lint: lint-doc-tests
	cargo clippy --workspace --all-targets --all-features -- -D warnings

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
	@awk '/#\[test\]/ { if (prev !~ /\/\/\//) { print FILENAME ":" FNR ": #[test] sans documentation"; bad = 1 } } { prev = $$0 } END { exit bad }' \
	  $$(find src crates -name '*.rs') < /dev/null \
	  || (echo "Chaque fonction de test porte sa documentation, comme toute declaration." && exit 1)

# Le noyau seul, sur une cible sans std. Les autres crates en sont dispensés :
# l'étage d'accueil ouvre une fenêtre, la conformance écrit des fichiers, la
# couche FFI formate des messages d'erreur.
nostd:
	cargo build -p screengine --no-default-features --target $(CIBLE_NOSTD)

# Rejoue les scènes de référence et compare les empreintes. Une divergence est
# soit une régression, soit une évolution volontaire du rendu — dans le second
# cas, conform-update, et dans un commit séparé du lot qui l'a causée.
conform:
	cargo run -p screengine-conformance --release -- --check

conform-update:
	cargo run -p screengine-conformance --release -- --update

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

host-web: lib
	$(MAKE) -C hosts/web

host-android: lib
	$(MAKE) -C hosts/android

clean:
	cargo clean
	rm -rf $(SORTIE) dist

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
	rustup target add $(CIBLE_NOSTD)
