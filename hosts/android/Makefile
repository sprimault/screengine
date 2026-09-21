# Hôte Android : JNI, test sans fenêtre, et une application pour l'écran.
#
# Appelé par le Makefile racine, qui construit la bibliothèque pour les trois ABI
# et passe PROFILE et OUT, comme pour hosts/c. `make -s why-not` écrit pourquoi
# l'hôte ne peut pas tourner ici, et rien quand il le peut.
#
# Le test a deux paliers, et `run` n'écrit qu'une empreinte, après avoir vérifié
# qu'ils la rendent tous :
#
#   1. hosts/c/main.c, lié en statique à la bibliothèque statique de chaque ABI,
#      exécuté sans appareil — x86_64 directement, aarch64 et armv7 sous
#      qemu-user. C'est ce qui éprouve FPCR et FPSCR, qu'un émulateur x86_64 ne
#      voit pas.
#   2. Sur un appareil ou un émulateur joignable par adb : le même main.c lié à
#      la bibliothèque dynamique, puis Test.java lancé par app_process, à travers
#      la couche JNI et l'ART réel.
#
# Java seul, sans Gradle : javac, d8, aapt2, zipalign et apksigner du SDK. Les
# versions sont celles du Dockerfile, qui les installe.

.DEFAULT_GOAL := all

PROFILE ?= release-ffi
OUT     ?= ../../.tmp/host-android
TARGET  := $(or $(CARGO_TARGET_DIR),../../target)
INCLUDE := ../../include

ANDROID_API  ?= 21
NDK_VERSION  ?= 29.0.14206865
BUILD_TOOLS  ?= 36.0.0
PLATFORM     ?= android-36
SYSTEM_IMAGE ?= system-images;android-36;default;x86_64
AVD          ?= screengine-x86_64

NDK_BIN     = $(ANDROID_NDK_HOME)/toolchains/llvm/prebuilt/linux-x86_64/bin
TOOLS       = $(ANDROID_HOME)/build-tools/$(BUILD_TOOLS)
ANDROID_JAR = $(ANDROID_HOME)/platforms/$(PLATFORM)/android.jar

# Ce que l'appareil reçoit, et où.
DEVICE_DIR := /data/local/tmp/screengine

ABIS := arm64-v8a armeabi-v7a x86_64

# Les scènes que chaque palier rend, dans l'ordre. Le Makefile racine en tient
# la liste et **passe ce nombre** : les deux ont divergé une fois, à l'ajout
# d'une scène, et l'échec n'est apparu qu'au bout d'une construction Android
# complète. La valeur ci-dessous ne sert qu'à un appel direct, hors du Makefile
# racine.
#
# Un seul nombre pour les cinq paliers : ceux qui rendraient un nombre
# différent de scènes ne se compareraient plus ligne à ligne, et la divergence
# passerait pour une égalité.
SCENE_COUNT ?= 3

triple_arm64-v8a   := aarch64-linux-android
triple_armeabi-v7a := armv7-linux-androideabi
triple_x86_64      := x86_64-linux-android

# L'éditeur de liens armv7 porte un `a` que le triple Rust n'a pas.
clang_arm64-v8a   := aarch64-linux-android$(ANDROID_API)-clang
clang_armeabi-v7a := armv7a-linux-androideabi$(ANDROID_API)-clang
clang_x86_64      := x86_64-linux-android$(ANDROID_API)-clang

# Un exécutable x86_64 statique tourne sous le noyau Linux de la machine. qemu
# est nommé et non laissé à binfmt_misc, que la machine peut avoir enregistré
# pour une architecture et pas pour l'autre.
qemu_arm64-v8a   := qemu-aarch64
qemu_armeabi-v7a := qemu-arm
qemu_x86_64      :=

# Les bibliothèques système de la bibliothèque statique Rust sous Android.
STATIC_LIBS := -lunwind -ldl -lm -lc

CFLAGS := -std=c11 -Wall -Wextra -Werror -O2 -I$(INCLUDE)

JAVA_SOURCES := $(wildcard src/screengine/host/*.java)

.PHONY: all run why-not apk emulator native dex

# print-<VARIABLE> écrit une valeur et rien d'autre : l'intégration continue lit
# les versions ici plutôt que de les recopier.
print-%:
	@echo '$($*)'

why-not:
	@if [ -z "$(ANDROID_NDK_HOME)" ] || [ ! -d "$(NDK_BIN)" ]; then \
	  echo "NDK introuvable, ANDROID_NDK_HOME non defini ou incomplet"; \
	elif [ ! -f "$(ANDROID_JAR)" ] || [ ! -x "$(TOOLS)/d8" ]; then \
	  echo "SDK incomplet : $(PLATFORM) ou build-tools $(BUILD_TOOLS) absents de ANDROID_HOME"; \
	elif ! command -v javac >/dev/null 2>&1; then \
	  echo "javac introuvable"; \
	elif ! command -v qemu-aarch64 >/dev/null 2>&1 || ! command -v qemu-arm >/dev/null 2>&1; then \
	  echo "qemu-aarch64 ou qemu-arm introuvable"; \
	elif ! rustup target list --installed 2>/dev/null | grep -qx armv7-linux-androideabi; then \
	  echo "cibles Rust Android absentes, voir make tools"; \
	elif [ "$$(adb get-state 2>/dev/null)" != "device" ]; then \
	  echo "aucun appareil ni emulateur joignable par adb"; \
	fi

# Palier 1 : un exécutable statique par ABI.
$(OUT)/%/screengine-abi-static: ../c/main.c $(INCLUDE)/screengine.h FORCE
	@mkdir -p $(@D)
	$(NDK_BIN)/$(clang_$*) $(CFLAGS) -static ../c/main.c \
	  $(TARGET)/$(triple_$*)/$(PROFILE)/libscreengine.a $(STATIC_LIBS) -o $@

# Palier 2 : la couche JNI et l'exécutable dynamique, pour l'ABI de l'émulateur
# comme pour l'APK. L'entrée DT_NEEDED prend le SONAME `libscreengine.so` que
# screengine-lib pose, et l'appareil retrouve la bibliothèque à côté.
$(OUT)/%/libscreengine_jni.so: jni.c $(INCLUDE)/screengine.h FORCE
	@mkdir -p $(@D)
	cp $(TARGET)/$(triple_$*)/$(PROFILE)/libscreengine.so $(@D)/
	$(NDK_BIN)/$(clang_$*) $(CFLAGS) -shared -fPIC -fvisibility=hidden -Wl,-soname,libscreengine_jni.so \
	  jni.c -L$(@D) -lscreengine -ljnigraphics -o $@

$(OUT)/%/screengine-abi: ../c/main.c $(INCLUDE)/screengine.h $(OUT)/%/libscreengine_jni.so FORCE
	$(NDK_BIN)/$(clang_$*) $(CFLAGS) ../c/main.c -L$(@D) -lscreengine -o $@

FORCE:

native: $(foreach abi,$(ABIS),$(OUT)/$(abi)/screengine-abi-static $(OUT)/$(abi)/libscreengine_jni.so) \
        $(OUT)/x86_64/screengine-abi

# --release 8 : le bytecode que d8 accepte pour toute API minimale. L'encodage
# est écrit : sans locale, javac lit les sources en US-ASCII.
dex: $(JAVA_SOURCES)
	@rm -rf $(OUT)/classes && mkdir -p $(OUT)/classes $(OUT)/dex
	javac --release 8 -encoding UTF-8 -Xlint:-options -classpath $(ANDROID_JAR) -d $(OUT)/classes $(JAVA_SOURCES)
	$(TOOLS)/d8 --min-api $(ANDROID_API) --lib $(ANDROID_JAR) --output $(OUT)/dex $$(find $(OUT)/classes -name '*.class')

# L'APK n'est pas lancé par le test, mais construit par lui : une application qui
# ne se construit plus se découvre ici plutôt que le jour où on la réclame.
# Alignement sur des pages de 16 Ko, signature par une clé de débogage.
apk: native dex
	@rm -rf $(OUT)/apk && mkdir -p $(OUT)/apk/lib
	$(TOOLS)/aapt2 link --manifest AndroidManifest.xml -I $(ANDROID_JAR) \
	  --min-sdk-version $(ANDROID_API) -o $(OUT)/apk/base.apk
	cp $(OUT)/dex/classes.dex $(OUT)/apk/
	for abi in $(ABIS); do \
	  mkdir -p $(OUT)/apk/lib/$$abi && cp $(OUT)/$$abi/*.so $(OUT)/apk/lib/$$abi/; \
	done
	cd $(OUT)/apk && jar uf base.apk classes.dex lib
	$(TOOLS)/zipalign -f -P 16 4 $(OUT)/apk/base.apk $(OUT)/apk/aligned.apk
	@[ -f $(OUT)/debug.keystore ] || keytool -genkeypair -keystore $(OUT)/debug.keystore -storepass android \
	  -alias debug -keypass android -keyalg RSA -validity 10000 -dname CN=debug >/dev/null 2>&1
	$(TOOLS)/apksigner sign --ks $(OUT)/debug.keystore --ks-pass pass:android \
	  --out $(OUT)/screengine.apk $(OUT)/apk/aligned.apk

all: native dex apk

# Écrit l'empreinte sur la sortie standard, les échecs sur la sortie d'erreur. Une
# empreinte par palier et par ABI, qui doivent toutes être identiques.
run: all
	@set -e; rm -f $(OUT)/paliers.txt; \
	$(foreach abi,$(ABIS),h=$$($(qemu_$(abi)) $(OUT)/$(abi)/screengine-abi-static | tr -d '\r'); \
	  echo "$(abi) statique : $$(echo $$h)" >&2; echo "$$h" >> $(OUT)/paliers.txt; ) \
	adb shell rm -rf $(DEVICE_DIR) >/dev/null; \
	adb shell mkdir -p $(DEVICE_DIR) >/dev/null; \
	adb push $(OUT)/x86_64/libscreengine.so $(OUT)/x86_64/libscreengine_jni.so \
	  $(OUT)/x86_64/screengine-abi $(OUT)/dex/classes.dex $(DEVICE_DIR)/ >/dev/null; \
	h=$$(adb shell "LD_LIBRARY_PATH=$(DEVICE_DIR) $(DEVICE_DIR)/screengine-abi" | tr -d '\r'); \
	echo "x86_64 appareil, C : $$(echo $$h)" >&2; echo "$$h" >> $(OUT)/paliers.txt; \
	h=$$(adb shell "LD_LIBRARY_PATH=$(DEVICE_DIR) CLASSPATH=$(DEVICE_DIR)/classes.dex app_process $(DEVICE_DIR) screengine.host.Test $(DEVICE_DIR)" | tr -d '\r'); \
	echo "x86_64 appareil, JNI : $$(echo $$h)" >&2; echo "$$h" >> $(OUT)/paliers.txt; \
	scenes=$$(head -1 $(OUT)/paliers.txt >/dev/null && awk 'END {print NR}' $(OUT)/paliers.txt); \
	if [ "$$scenes" -ne $$(( 5 * $(SCENE_COUNT) )) ]; then \
	  echo "attendu $(SCENE_COUNT) empreinte(s) par palier, reçu $$scenes lignes pour cinq paliers" >&2; \
	  exit 1; \
	fi; \
	for i in $$(seq 1 $(SCENE_COUNT)); do \
	  distinct=$$(awk -v n=$(SCENE_COUNT) -v i=$$i 'NR % n == i % n' $(OUT)/paliers.txt | sort -u); \
	  if [ $$(printf '%s\n' "$$distinct" | wc -l) -ne 1 ]; then \
	    echo "scene $$i : empreintes divergentes entre paliers ou ABI" >&2; \
	    cat $(OUT)/paliers.txt >&2; exit 1; \
	  fi; \
	  echo "$$distinct"; \
	done

# Démarre l'émulateur sans fenêtre et attend la fin de son démarrage. Pour une
# machine Linux avec KVM ; l'intégration continue passe par son action.
#
# ANDROID_AVD_HOME explicite : avdmanager crée l'AVD sous ANDROID_USER_HOME, que
# l'émulateur ne lit pas, et il ne le trouverait pas. `-read-only` : l'émulateur
# n'écrit rien dans l'AVD, et un conteneur tué ne laisse pas de verrou qui
# refuserait le lancement suivant. Le démarrage est borné — un
# émulateur qui ne démarre pas laisserait `adb wait-for-device` attendre sans fin.
EMULATOR_TIMEOUT ?= 300

emulator:
	@set -e; export ANDROID_AVD_HOME="$${ANDROID_USER_HOME:-$$HOME/.android}/avd"; \
	mkdir -p $(OUT) "$$ANDROID_AVD_HOME"; \
	avdmanager list avd -c 2>/dev/null | grep -qx $(AVD) || \
	  echo no | avdmanager create avd -n $(AVD) -k "$(SYSTEM_IMAGE)" --force >/dev/null; \
	rm -f "$$ANDROID_AVD_HOME/$(AVD).avd/"*.lock; \
	emulator -avd $(AVD) -read-only -no-window -no-audio -no-boot-anim -no-snapshot -gpu swiftshader_indirect \
	  > $(OUT)/emulator.log 2>&1 & \
	timeout $(EMULATOR_TIMEOUT) sh -c 'adb wait-for-device && \
	  until [ "$$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d "\r")" = 1 ]; do sleep 2; done' \
	  || { echo "l'emulateur n'a pas demarre en $(EMULATOR_TIMEOUT) s :" >&2; tail -20 $(OUT)/emulator.log >&2; exit 1; }
