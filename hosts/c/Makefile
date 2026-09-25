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

endif

.PHONY: all run why-not

all: $(EXE)

# Écrit l'empreinte sur la sortie standard, les échecs sur la sortie d'erreur.
run: $(EXE)
	@$(EXE) "$(MESH)"
