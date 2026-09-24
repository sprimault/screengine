// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La garde contre la libm se tient dans les deux largeurs, ou ne se tient pas.
//!
//! `docs/rust.md` l'exige : la liste de `clippy.toml` couvre « en `f64` autant
//! qu'en `f32` ». Elle ne l'a pas fait — `f32::exp2` y a manqué le temps d'une
//! version, et rien ne l'a signalé puisqu'aucun appel n'existait encore. Le jour
//! où un `powf` se réécrit en `(e * log2(v)).exp2()` en `f32`, `make lint` passe
//! et l'`exp2f` de la cible entre dans une empreinte.
//!
//! Ce contrôle vit ici plutôt que dans le noyau, qui n'a pas `std` pour lire un
//! fichier, et plutôt que dans la frontière, parce que la liste protège les
//! trois crates dont les calculs entrent dans une empreinte.

/// Le fichier de lints, lu depuis la racine du dépôt.
const CLIPPY_TOML: &str = include_str!("../../../clippy.toml");

/// Les familles que la liste ne peut pas omettre.
///
/// La symétrie seule laisserait passer une liste également incomplète des deux
/// côtés — ce que l'audit a trouvé pour les hyperboliques et pour `log`. Cette
/// énumération est le « ni de leurs cousins » de `docs/rust.md`, qu'aucune règle
/// mécanique ne sait déplier.
const REQUIRED: &[&str] = &[
    "sqrt",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "atan2",
    "sinh",
    "cosh",
    "tanh",
    "asinh",
    "acosh",
    "atanh",
    "hypot",
    "cbrt",
    "exp",
    "exp2",
    "exp_m1",
    "ln",
    "ln_1p",
    "log",
    "log2",
    "log10",
    "powi",
    "powf",
    "mul_add",
    "floor",
    "ceil",
    "round",
    "round_ties_even",
    "trunc",
    "fract",
    "div_euclid",
    "rem_euclid",
    "min",
    "max",
    "clamp",
    "signum",
];

/// Les méthodes interdites d'une largeur donnée, dans l'ordre du fichier.
///
/// Lecture à la main plutôt que par un analyseur TOML : la conformance ne prend
/// pas une dépendance pour extraire ce qui suit un préfixe, et le format de ces
/// lignes est stable.
fn methods_of(width: &str) -> Vec<&'static str> {
    let needle = format!("\"{width}::");
    CLIPPY_TOML
        .lines()
        .filter_map(|line| {
            let rest = line.split_once(&needle)?.1;
            let (method, _) = rest.split_once('"')?;
            Some(method)
        })
        .collect()
}

/// Chaque méthode interdite en `f32` l'est aussi en `f64`, et réciproquement.
///
/// Le défaut attrapé est une asymétrie, pas une absence : une liste des deux
/// côtés peut être longue et fausse, et c'est ce qui la rend crédible à la
/// relecture.
#[test]
fn la_garde_contre_la_libm_est_symetrique() {
    let narrow = methods_of("f32");
    let wide = methods_of("f64");

    assert!(
        narrow.len() >= REQUIRED.len(),
        "la lecture de clippy.toml n'a trouvé que {} méthodes en f32 : le format des lignes a \
         changé, et ce test ne vérifie plus rien",
        narrow.len()
    );

    let missing_wide: Vec<&str> = narrow
        .iter()
        .copied()
        .filter(|m| !wide.contains(m))
        .collect();
    let missing_narrow: Vec<&str> = wide
        .iter()
        .copied()
        .filter(|m| !narrow.contains(m))
        .collect();

    assert!(
        missing_wide.is_empty() && missing_narrow.is_empty(),
        "clippy.toml est asymétrique — interdit en f32 mais pas en f64 : {missing_wide:?} ; \
         interdit en f64 mais pas en f32 : {missing_narrow:?}"
    );
}

/// Aucune famille de la libm ne manque à la liste.
#[test]
fn aucune_famille_de_la_libm_n_est_oubliee() {
    let present = methods_of("f32");
    let forgotten: Vec<&str> = REQUIRED
        .iter()
        .copied()
        .filter(|m| !present.contains(m))
        .collect();

    assert!(
        forgotten.is_empty(),
        "méthodes de la libm absentes de clippy.toml : {forgotten:?}"
    );
}
