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

.PHONY: build lib run test fmt lint nostd conform conform-update header header-verif \
        audit deny doc hosts host-c host-web host-android clean tools

build:
	cargo build --workspace

# La bibliothèque partagée passe par release-ffi et non par release : le premier
# est en panic = "unwind", sans quoi catch_unwind ne rattraperait rien et une
# panique traverserait la frontière C — comportement indéfini, pas plantage.
lib:
	cargo build -p screengine-ffi --profile release-ffi

run:
	cargo run -p screengine-host

PKG ?= --workspace
RUN ?=

test:
	cargo test $(PKG) $(if $(RUN),-- $(RUN))

# Doublon assumé avec les formatters de clippy : cargo fmt porte sur tout
# l'arbre, sans exclusion ni configuration, et reste vrai le jour où quelqu'un
# touche à la configuration de clippy.
fmt:
	cargo fmt --all --check

lint:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

# Le noyau seul, sur une cible sans std. Les autres crates en sont dispensés :
# l'hôte ouvre une fenêtre, la conformance écrit des fichiers, la couche FFI
# formate des messages d'erreur.
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

# Licences et provenance des dépendances. Le noyau n'en a aucune ; ce sont les
# hôtes et la conformance qui en portent, et ce sont elles qui voyagent dans les
# archives publiées.
deny:
	cargo deny check

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
CARGO_DENY_VERSION ?= 0.18.4

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
