# Hôte Go, sans fenêtre, lié à la bibliothèque dynamique par cgo.
#
# Appelé par le Makefile racine, qui construit la bibliothèque et passe :
#   PROFILE  le profil Cargo de la bibliothèque (ffi-test pour test-go)
#   OUT      le répertoire de sortie, sous .tmp/ : aucun exécutable ne s'écrit
#            ni ne se lance ailleurs sur un poste surveillé par l'antivirus
#
# **La dynamique et non la statique**, contrairement à l'hôte C : une statique
# Rust réclame ses bibliothèques système, que cgo passerait dans des drapeaux
# d'édition de liens à tenir à jour ici. L'hôte C++ se lie déjà ainsi, et le
# chargement dynamique depuis un programme Go est ce qu'un intégrateur fera.
#
# `make -s why-not` écrit pourquoi l'hôte ne peut pas se construire ici, et rien
# quand il le peut : c'est ce qui permet à test-go de sauter en le disant.

.DEFAULT_GOAL := all

PROFILE ?= release-ffi
OUT     ?= ../../.tmp/host-go
TARGET  := $(or $(CARGO_TARGET_DIR),../../target)
INCLUDE ?= ../../include

# Les cinq fichiers de données que les scènes chargent, versionnés et partagés
# par les cinq hôtes.
MESH      ?= ../../hosts/caisse.mesh
WORLD     ?= ../../hosts/salles.world
COLLISION ?= ../../hosts/collision.world
SWEEPS    ?= ../../hosts/collision.sweeps
PICKS     ?= ../../hosts/selection.picks

# Go n'est pas toujours dans le PATH — sous Windows son répertoire
# d'installation n'y entre pas pour un shell POSIX. `makefile.local` le pose
# quand c'est le cas.
GO ?= go

# Le cache et le répertoire de modules vont sous OUT, comme tout ce que la
# chaîne écrit : ailleurs, ils tomberaient dans le profil de l'utilisateur, que
# l'antivirus surveille.
GOFLAGS_ENV = GOCACHE=$(abspath $(OUT))/cache GOMODCACHE=$(abspath $(OUT))/mod GOPATH=$(abspath $(OUT))/path

ifeq ($(OS),Windows_NT)

LIB     := $(TARGET)/$(PROFILE)/screengine.dll
LIBDIR  := $(TARGET)/$(PROFILE)
EXE     := $(OUT)/screengine-go.exe
# MinGW se lie directement à la bibliothèque d'importation produite par MSVC,
# sans passer par gendef ni dlltool : mesuré, et c'est ce qui ouvre Windows à
# cet hôte là où l'hôte C y reste sur MSVC et sa statique.
#
# Pas de rpath : Windows cherche la DLL à côté de l'exécutable, où la recette la
# recopie.
LDLIB   := -lscreengine.dll
RPATH   :=

why-not:
	@if ! command -v $(GO) >/dev/null 2>&1; then \
	  echo "$(GO) introuvable, definir GO dans makefile.local"; \
	elif ! command -v gcc >/dev/null 2>&1; then \
	  echo "gcc introuvable : cgo en a besoin sous Windows"; \
	fi

else

LIB     := $(TARGET)/$(PROFILE)/libscreengine.so
LIBDIR  := $(TARGET)/$(PROFILE)
EXE     := $(OUT)/screengine-go
LDLIB   := -lscreengine
# `$ORIGIN` est résolu par l'éditeur de liens dynamique, jamais par le shell, et
# il traverse donc deux expansions : `$$` que Make rend simple, puis `\$` que le
# shell laisse passer. Des apostrophes ne suffiraient pas — placées dans la
# valeur, elles se retrouvent à l'intérieur des guillemets de CGO_LDFLAGS, où
# elles sont littérales et ne protègent plus rien. Le symptôme est un RUNPATH
# vide, que seul `readelf -d` montre.
RPATH   := -Wl,-rpath,\$$ORIGIN

why-not:
	@if ! command -v $(GO) >/dev/null 2>&1; then \
	  echo "$(GO) introuvable, definir GO dans makefile.local"; \
	elif ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1; then \
	  echo "aucun compilateur C : cgo en a besoin"; \
	fi

endif

SOURCES := main.go screengine.go scenes.go textures.go world.go go.mod

# La bibliothèque recopiée à côté de l'exécutable, comme une cible et non comme
# un `cp` glissé dans la recette de celui-ci : une copie qui ne se fait qu'en
# reconstruisant autre chose manque dès que l'exécutable est à jour et que le
# répertoire de sortie a été vidé — et l'exécutable ne trouve plus sa
# bibliothèque à l'exécution.
LIBCOPY := $(OUT)/$(notdir $(LIB))

.PHONY: all run why-not

# `-rpath $ORIGIN` sous Linux, rien sous Windows qui cherche déjà là : les deux
# disent la même chose — la bibliothèque se trouve à côté de l'exécutable, et non
# dans un répertoire de construction que l'intégrateur n'aura pas.
$(EXE): $(SOURCES) $(LIB) $(INCLUDE)/screengine.h
	@mkdir -p $(OUT)
	$(GOFLAGS_ENV) \
	CGO_CFLAGS="-I$(abspath $(INCLUDE))" \
	CGO_LDFLAGS="-L$(abspath $(LIBDIR)) $(LDLIB) $(RPATH)" \
	  $(GO) build -o $(abspath $(EXE)) .

$(LIBCOPY): $(LIB)
	@mkdir -p $(OUT)
	cp $(LIB) $(OUT)/

all: $(EXE) $(LIBCOPY)

# Écrit l'empreinte sur la sortie standard, les échecs sur la sortie d'erreur.
run: all
	@$(EXE) "$(MESH)" "$(WORLD)" "$(COLLISION)" "$(SWEEPS)" "$(PICKS)"
