// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pose le SONAME de la bibliothèque dynamique sous Linux et Android.
//!
//! rustc n'en pose aucun sur un cdylib. Sans lui, l'entrée DT_NEEDED d'un
//! programme lié prend ce que l'éditeur de liens a reçu — le nom du fichier avec
//! `-l`, mais un chemin relatif quand CMake passe la bibliothèque par son
//! chemin, que ni rpath ni runpath ne rattrapent. Pas de numéro de version dans
//! le nom : un APK ne sait pas porter `libscreengine.so.0`, et la compatibilité
//! se vérifie à l'exécution par `scg_abi_version`.

use std::env;

fn main() {
    if matches!(
        env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("linux" | "android")
    ) {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libscreengine.so");
    }
}
