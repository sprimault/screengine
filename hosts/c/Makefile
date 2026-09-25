# Hôte C, sans fenêtre, lié à la bibliothèque statique.
#
# Appelé par le Makefile racine, qui construit la bibliothèque et passe :
#   PROFILE  le profil Cargo de la bibliothèque (ffi-test pour test-abi)
#   OUT      le répertoire de sortie, sous .tmp/ : aucun exécutable ne s'écrit
#            ni ne se lance ailleurs sur un poste surveillé par l'antivirus
#
# `make -s why-not` écrit pourquoi l'hôte ne peut pas se construire ici, et
# rien quand il le peut : c'est ce qui permet à test-abi de sauter en le disant.

.DEFAULT_GOAL := all

PROFILE ?= release-ffi
OUT     ?= ../../.tmp/host-c
TARGET  := $(or $(CARGO_TARGET_DIR),../../target)
INCLUDE := ../../include

# Le maillage que l'hôte charge, versionné et partagé par les quatre hôtes. Il
# est passé en argument plutôt qu'écrit dans le programme : celui-ci s'exécute
# depuis un répertoire de sortie qui n'est pas celui du dépôt.
MESH    ?= ../../hosts/caisse.mesh

# Le décor que l'hôte de démonstration parcourt, et la racine de SDL3, que
# `makefile.local` porte : une dépendance système ne s'installe pas au même
# endroit d'un poste à l'autre, et le projet n'en impose aucun.
WORLD   ?= ../../hosts/couloir.world
SDL3    ?=

ifeq ($(OS),Windows_NT)

LIB := $(TARGET)/$(PROFILE)/screengine.lib
EXE := $(OUT)/screengine-abi.exe
VSWHERE := /c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe

# MinGW est écarté même quand il est présent : il ne se lie pas à une
# bibliothèque statique produite pour la cible MSVC.
why-not:
	@command -v cl >/dev/null 2>&1 && exit 0; \
	[ -x "$(VSWHERE)" ] && [ -n "$$("$(VSWHERE)" -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)" ] && exit 0; \
	echo "cl introuvable, ni dans le PATH ni par vswhere"

$(EXE): main.c $(LIB) $(INCLUDE)/screengine.h build.cmd
	@mkdir -p $(OUT)
	./build.cmd "$$(cygpath -w $@)" "$$(cygpath -w $(OUT))" "$$(cygpath -w $(LIB))" "$$(cygpath -w $(INCLUDE))" main.c

DEMO := $(OUT)/screengine-demo.exe

why-not-demo:
	@if [ -z "$(SDL3)" ]; then \
	  echo "SDL3 introuvable : definir SDL3 dans makefile.local"; \
	elif [ ! -f "$(SDL3)/lib/x64/SDL3.lib" ]; then \
	  echo "$(SDL3)/lib/x64/SDL3.lib introuvable"; \
	fi

$(DEMO): demo.c $(LIB) $(INCLUDE)/screengine.h build-demo.cmd
	@mkdir -p $(OUT)
	./build-demo.cmd "$$(cygpath -w $@)" "$$(cygpath -w $(OUT))" "$$(cygpath -w $(LIB))" \
	  "$$(cygpath -w $(INCLUDE))" demo.c "$$(cygpath -w $(SDL3))"
	# La DLL à côté de l'exécutable : c'est le seul emplacement de recherche qui
	# ne dépende ni du PATH ni du répertoire courant.
	cp "$(SDL3)/lib/x64/SDL3.dll" $(OUT)/

else

CC  ?= cc
LIB := $(TARGET)/$(PROFILE)/libscreengine.a
EXE := $(OUT)/screengine-abi

# Les bibliothèques système que réclame une bibliothèque statique Rust sous
# Linux, celles que rend `make native-libs`. gcc_s porte le dépliage, sans
# lequel catch_unwind ne rattraperait rien.
LIBS := -lgcc_s -lutil -lrt -lpthread -lm -ldl -lc

why-not:
	@command -v $(CC) >/dev/null 2>&1 || echo "$(CC) introuvable"

$(EXE): main.c $(LIB) $(INCLUDE)/screengine.h
	@mkdir -p $(OUT)
	$(CC) -std=c11 -Wall -Wextra -Werror -O2 -I$(INCLUDE) main.c $(LIB) $(LIBS) -o $@

DEMO := $(OUT)/screengine-demo

# SDL3 se trouve ici par pkg-config, la convention de la plateforme, plutôt que
# par un chemin que `makefile.local` porterait comme sous Windows.
why-not-demo:
	@pkg-config --exists sdl3 2>/dev/null || echo "sdl3 introuvable par pkg-config"

$(DEMO): demo.c $(LIB) $(INCLUDE)/screengine.h
	@mkdir -p $(OUT)
	$(CC) -std=c11 -Wall -Wextra -O2 -I$(INCLUDE) $$(pkg-config --cflags sdl3) demo.c \
	  $(LIB) $$(pkg-config --libs sdl3) $(LIBS) -o $@

endif

.PHONY: all run why-not why-not-demo demo demo-run

all: $(EXE)

# L'hôte de démonstration : il ouvre une fenêtre, donc il ne fait partie
# d'aucun contrôle — `make test` ne le construit pas, et rien ne le lance en
# intégration continue.
demo: $(DEMO)

demo-run: $(DEMO)
	@$(DEMO) "$(WORLD)" "$(MESH)"

# Écrit l'empreinte sur la sortie standard, les échecs sur la sortie d'erreur.
run: $(EXE)
	@$(EXE) "$(MESH)"
