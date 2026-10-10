# Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Exécution des deux cibles Linux sur ARM, sans rien installer sur la machine :
# chaîne Rust et ses deux cibles, les deux chaînes C croisées avec leurs libc, et
# `qemu-user-static` pour exécuter ce qui en sort.
#
#   docker build -t screengine-linux-arm docker/linux-arm
#   docker run --rm --name screengine-linux-arm-test -u "$(id -u):$(id -g)" \
#     -v screengine-linux-arm-cache:/cache -v "$PWD":/work -w /work \
#     screengine-linux-arm sh -c 'make test-linux-arm && make conform-linux-arm'
#
# **Ce que ces deux cibles ont de propre est la bibliothèque C**, et c'est la
# seule raison pour laquelle elles existent dans la matrice : même jeu
# d'instructions et même largeur que les ABI Android, mais la glibc là où le NDK
# porte la bionic. Le noyau n'emploie aucune des deux ; la frontière passe par
# `std`, et rien ne le garantissait tant qu'on ne l'avait pas exécuté.
#
# **Hors de l'image d'Android, et pas par rangement** : un NDK n'est pas une
# glibc, et les paquets ci-dessous n'ont rien à voir avec un SDK. Les mêler
# ferait reconstruire l'image lourde d'Android — SDK, émulateur, image
# système — pour une montée qui ne concerne que les chaînes croisées.
#
# **Pas de `seccomp=unconfined` ici**, contrairement à l'image d'Android : c'est
# la bionic 32 bits qui appelle `personality` à son démarrage, et la glibc ne le
# fait pas. Vérifié en exécutant les deux cibles sous le profil par défaut.

FROM ubuntu:24.04

ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/cache/cargo \
    CARGO_TARGET_DIR=/cache/target \
    HOME=/cache/home \
    PATH=/opt/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# **Les `libc6-dev-*-cross` sont nommés explicitement**, et c'est ce qui a
# manqué au premier essai : `--no-install-recommends` les écarte, `gcc-*-cross`
# ne les entraîne donc pas, et l'édition de liens échoue sur `Scrt1.o` et
# `crti.o` introuvables — un message qui ne nomme pas le paquet qui manque.
#
# `gcc` et `libc6-dev` sans préfixe servent les binaires de la machine : la
# conformance les construit pour produire l'empreinte de référence.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl make gcc libc6-dev \
      gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
      gcc-arm-linux-gnueabihf libc6-dev-armhf-cross \
      qemu-user-static \
 && rm -rf /var/lib/apt/lists/*

# CARGO_HOME pointe vers le cache au lancement ; les binaires de rustup restent
# dans l'image, lisibles par l'utilisateur qu'on passe à `docker run -u`.
RUN curl -fsSL https://sh.rustup.rs | CARGO_HOME=/opt/cargo sh -s -- -y --no-modify-path \
      --profile minimal --default-toolchain stable \
      --target aarch64-unknown-linux-gnu,armv7-unknown-linux-gnueabihf \
 && chmod -R a+rX /opt/rustup /opt/cargo

# Le cache est un volume, ouvert à tous pour que l'utilisateur de `docker run -u`
# y crée ses sous-répertoires au premier lancement.
RUN mkdir -m 1777 /cache
VOLUME /cache
