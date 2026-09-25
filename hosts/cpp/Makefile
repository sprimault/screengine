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

# Le maillage versionné que les quatre hôtes chargent, passé en argument :
# l'exécutable s'exécute depuis un répertoire de sortie qui n'est pas celui du
# dépôt.
MESH    ?= ../../hosts/caisse.mesh

# Le décor que l'hôte de démonstration parcourt, et la racine de SDL3, que
# `makefile.local` porte : une dépendance système ne s'installe pas au même
# endroit d'un poste à l'autre, et le projet n'en impose aucun.
WORLD   ?= ../../hosts/couloir.world
SDL3    ?=

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

DEMO := $(OUT)/screengine-demo-cpp.exe

why-not-demo:
	@if [ -z "$(SDL3)" ]; then \
	  echo "SDL3 introuvable : definir SDL3 dans makefile.local"; \
	elif [ ! -f "$(SDL3)/lib/x64/SDL3.lib" ]; then \
	  echo "$(SDL3)/lib/x64/SDL3.lib introuvable"; \
	fi

# Deux DLL à côté de l'exécutable, celle du moteur et celle de SDL3 : même
# raison que ci-dessus, c'est le seul emplacement de recherche qui ne dépende
# ni du PATH ni du répertoire courant.
$(DEMO): demo.cpp $(IMPLIB) $(DLL) $(INCLUDE)/screengine.h build-demo.cmd
	@mkdir -p $(OUT)
	cp $(DLL) "$(SDL3)/lib/x64/SDL3.dll" $(OUT)/
	./build-demo.cmd "$$(cygpath -w $@)" "$$(cygpath -w $(OUT))" "$$(cygpath -w $(IMPLIB))" \
	  "$$(cygpath -w $(INCLUDE))" demo.cpp "$$(cygpath -w $(SDL3))"

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
	$(CXX) -std=c++17 -Wall -Wextra -Werror -O2 -pthread -I$(INCLUDE) main.cpp \
	  -L$(abspath $(TARGET)/$(PROFILE)) -Wl,-rpath,$(abspath $(TARGET)/$(PROFILE)) -lscreengine -ldl -o $@

DEMO := $(OUT)/screengine-demo-cpp

# SDL3 se trouve ici par pkg-config, la convention de la plateforme, plutôt que
# par un chemin que `makefile.local` porterait comme sous Windows.
why-not-demo:
	@pkg-config --exists sdl3 2>/dev/null || echo "sdl3 introuvable par pkg-config"

$(DEMO): demo.cpp $(TARGET)/$(PROFILE)/libscreengine.so $(INCLUDE)/screengine.h
	@mkdir -p $(OUT)
	$(CXX) -std=c++17 -Wall -Wextra -O2 -pthread -I$(INCLUDE) $$(pkg-config --cflags sdl3) demo.cpp \
	  -L$(abspath $(TARGET)/$(PROFILE)) -Wl,-rpath,$(abspath $(TARGET)/$(PROFILE)) -lscreengine \
	  $$(pkg-config --libs sdl3) -ldl -o $@

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
