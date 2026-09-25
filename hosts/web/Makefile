# Hôte wasm : un test sans fenêtre sous Node, et une page pour le navigateur.
#
# Appelé par le Makefile racine, qui construit le module et passe PROFILE et OUT,
# comme pour hosts/c. `make -s why-not` écrit pourquoi l'hôte ne peut pas
# tourner ici, et rien quand il le peut.
#
# Aucune dépendance npm : Node sert à exécuter le test et la page, rien à
# installer. Tout est recopié sous OUT, et c'est de là que Node lit et sert.

.DEFAULT_GOAL := all

PROFILE ?= release-wasm
OUT     ?= ../../.tmp/host-web
TARGET  := $(or $(CARGO_TARGET_DIR),../../target)
WASM    := $(TARGET)/wasm32-unknown-unknown/$(PROFILE)/screengine.wasm
HEADER  := $(abspath ../../include/screengine.h)
# Le maillage versionné que les quatre hôtes chargent.
MESH    := $(abspath ../../hosts/caisse.mesh)
NODE    ?= node
PORT    ?= 8080

SOURCES := screengine.js test.js page.js serve.js index.html package.json

why-not:
	@if ! command -v $(NODE) >/dev/null 2>&1; then \
	  echo "$(NODE) introuvable"; \
	elif ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then \
	  echo "cible wasm32-unknown-unknown absente, voir make tools"; \
	fi

.PHONY: all run serve why-not

# Recopié à chaque fois, sans règle sur les dates : quelques fichiers, et une
# copie jugée à jour à tort ferait tester une autre version que celle du dépôt.
all: $(WASM)
	@mkdir -p $(OUT)
	cp $(SOURCES) $(OUT)/
	cp $(WASM) $(OUT)/

# Écrit l'empreinte sur la sortie standard, les échecs sur la sortie d'erreur.
run: all
	@$(NODE) $(OUT)/test.js $(OUT)/screengine.wasm $(HEADER) $(MESH)

serve: all
	$(NODE) $(OUT)/serve.js $(OUT) $(PORT)
