# Hôte C++, sans fenêtre, lié à la bibliothèque dynamique.
#
# Appelé par le Makefile racine, qui construit la bibliothèque et passe PROFILE
# et OUT, comme pour hosts/c. `make -s why-not` écrit pourquoi l'hôte ne peut
# pas se construire ici, et rien quand il le peut.

.DEFAULT_GOAL := all

PROFILE ?= release-ffi
OUT     ?= ../../.tmp/host-cpp
TARGET  := $(or $(CARGO_TARGET_DIR),../../target)
INCLUDE := ../../include

ifeq ($(OS),Windows_NT)

IMPLIB  := $(TARGET)/$(PROFILE)/screengine.dll.lib
DLL     := $(TARGET)/$(PROFILE)/screengine.dll
EXE     := $(OUT)/screengine-cpp.exe
VSWHERE := /c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe

why-not:
	@command -v cl >/dev/null 2>&1 && exit 0; \
	[ -x "$(VSWHERE)" ] && [ -n "$$("$(VSWHERE)" -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)" ] && exit 0; \
	echo "cl introuvable, ni dans le PATH ni par vswhere"

# La DLL est copiée à côté de l'exécutable : c'est le premier endroit où Windows
# la cherche, et le seul qui ne dépende ni du PATH ni du répertoire courant.
$(EXE): main.cpp $(IMPLIB) $(DLL) $(INCLUDE)/screengine.h build.cmd
	@mkdir -p $(OUT)
	cp $(DLL) $(OUT)/
	./build.cmd "$$(cygpath -w $@)" "$$(cygpath -w $(OUT))" "$$(cygpath -w $(IMPLIB))" "$$(cygpath -w $(INCLUDE))" main.cpp

else

CXX := $(if $(filter default,$(origin CXX)),c++,$(CXX))
EXE := $(OUT)/screengine-cpp

why-not:
	@command -v $(CXX) >/dev/null 2>&1 || echo "$(CXX) introuvable"

# -L et -l plutôt que le chemin du .so : l'éditeur de liens y préfère la
# bibliothèque partagée à l'archive voisine, comme chez un intégrateur. Le rpath
# la retrouve à l'exécution sans LD_LIBRARY_PATH, par le SONAME
# `libscreengine.so` que screengine-lib pose.
$(EXE): main.cpp $(TARGET)/$(PROFILE)/libscreengine.so $(INCLUDE)/screengine.h
	@mkdir -p $(OUT)
	$(CXX) -std=c++17 -Wall -Wextra -Werror -O2 -I$(INCLUDE) main.cpp \
	  -L$(abspath $(TARGET)/$(PROFILE)) -Wl,-rpath,$(abspath $(TARGET)/$(PROFILE)) -lscreengine -ldl -o $@

endif

.PHONY: all run why-not

all: $(EXE)

# Écrit l'empreinte sur la sortie standard, les échecs sur la sortie d'erreur.
run: $(EXE)
	@$(EXE)
