# Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Construction et test de l'hôte Android sous Linux, sans rien installer sur la
# machine : chaîne Rust et ses trois cibles, NDK, SDK, émulateur x86_64 et
# qemu-user pour les deux ABI ARM.
#
#   docker build -t screengine-android hosts/android
#   docker run --rm --name screengine-android-test \
#     --device /dev/kvm --group-add "$(stat -c %g /dev/kvm)" -u "$(id -u):$(id -g)" \
#     --security-opt seccomp=unconfined \
#     -v screengine-android-cache:/cache -v "$PWD":/work -w /work \
#     screengine-android sh -c 'make -C hosts/android emulator && make test-android'
#
# seccomp=unconfined parce que bionic 32 bits appelle personality(0xffffffff) à
# son démarrage : le profil par défaut de Docker le refuse, et l'exécutable armv7
# s'arrête en SIGABRT avant d'avoir rien testé.
#
# Les versions sont épinglées ici comme dans hosts/android/Makefile, et doivent
# rester les mêmes : c'est celui-ci qui décide ce que l'intégration continue
# attend du runner. Le cache — registre Cargo, cibles, AVD — vit dans un volume,
# pour qu'une image se reconstruise sans perdre des minutes de compilation.

FROM ubuntu:24.04

ARG CMDLINE_TOOLS=16111833
ARG NDK_VERSION=29.0.14206865
ARG BUILD_TOOLS=36.0.0
ARG PLATFORM=android-36
ARG SYSTEM_IMAGE=system-images;android-36;default;x86_64

ENV ANDROID_HOME=/opt/android-sdk \
    ANDROID_NDK_HOME=/opt/android-sdk/ndk/${NDK_VERSION} \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/cache/cargo \
    CARGO_TARGET_DIR=/cache/target \
    ANDROID_USER_HOME=/cache/android \
    HOME=/cache/home \
    PATH=/opt/cargo/bin:/opt/android-sdk/cmdline-tools/latest/bin:/opt/android-sdk/platform-tools:/opt/android-sdk/emulator:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# gcc pour lier les binaires Rust de la machine — la conformance, qui donne
# l'empreinte de référence ; le reste, pour l'émulateur, qui charge ces
# bibliothèques même sans fenêtre.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl unzip make gcc libc6-dev openjdk-17-jdk-headless qemu-user \
      libpulse0 libgl1 libnss3 libxcomposite1 libxcursor1 libxdamage1 libxi6 \
      libxtst6 libxkbfile1 libasound2t64 libdrm2 libbsd0 libxrandr2 libgbm1 \
 && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /opt/android-sdk/cmdline-tools \
 && curl -fsSL -o /tmp/tools.zip "https://dl.google.com/android/repository/commandlinetools-linux-${CMDLINE_TOOLS}_latest.zip" \
 && unzip -q /tmp/tools.zip -d /opt/android-sdk/cmdline-tools \
 && mv /opt/android-sdk/cmdline-tools/cmdline-tools /opt/android-sdk/cmdline-tools/latest \
 && rm /tmp/tools.zip \
 && yes | sdkmanager --licenses > /dev/null \
 && sdkmanager --install "platform-tools" "emulator" "build-tools;${BUILD_TOOLS}" \
      "platforms;${PLATFORM}" "ndk;${NDK_VERSION}" "${SYSTEM_IMAGE}" > /dev/null \
 && chmod -R a+rX /opt/android-sdk

# CARGO_HOME pointe vers le cache au lancement ; les binaires de rustup restent
# dans l'image, lisibles par l'utilisateur qu'on passe à `docker run -u`.
RUN curl -fsSL https://sh.rustup.rs | CARGO_HOME=/opt/cargo sh -s -- -y --no-modify-path \
      --profile minimal --default-toolchain stable \
      --target aarch64-linux-android,armv7-linux-androideabi,x86_64-linux-android \
 && chmod -R a+rX /opt/rustup /opt/cargo

# Le cache est un volume. Un volume neuf prend le contenu et les droits du
# répertoire de l'image : vidé de ce que sdkmanager y a écrit en root pendant la
# construction, et ouvert à tous, pour que l'utilisateur de `docker run -u` y
# crée ses sous-répertoires au premier lancement.
RUN rm -rf /cache && mkdir -m 1777 /cache
VOLUME /cache
