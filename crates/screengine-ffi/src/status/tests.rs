// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les codes de retour, vus comme une liaison les traite.
//!
//! Un code publié ne change jamais de sens : ces tests figent la plage et
//! l'unicité, pas les valeurs elles-mêmes, qui sont dans le header.

use super::*;

/// La catégorie d'un code est `(-code) / 100`, et les codes généraux sont
/// tous dans la catégorie 0.
#[test]
fn les_codes_generaux_sont_dans_la_premiere_plage() {
    for code in [
        SCG_ERR_NULL,
        SCG_ERR_INVALID_ARGUMENT,
        SCG_ERR_OUT_OF_MEMORY,
        SCG_ERR_INVALID_STATE,
        SCG_ERR_PANIC,
        SCG_ERR_POISONED,
    ] {
        assert!(code < 0, "un code d'erreur est négatif");
        assert_eq!((-code) / 100, 0, "code {code} hors de la plage générale");
    }
}

/// Deux erreurs distinctes ne partagent jamais un code : un code publié ne
/// change pas de sens, et deux sens pour un code reviendrait au même.
#[test]
fn chaque_erreur_du_noyau_a_son_code() {
    let errors = [
        Error::InvalidArgument(Argument::Resolution),
        Error::OutOfMemory,
    ];
    for (i, a) in errors.iter().enumerate() {
        for b in &errors[i + 1..] {
            assert_ne!(code_of(*a), code_of(*b));
        }
    }
}

/// L'inverse du précédent : les arguments refusés partagent un code, et
/// seul le message les distingue. Deux messages identiques laisseraient
/// l'intégrateur chercher lequel de ses arguments est en cause.
#[test]
fn chaque_argument_refuse_a_son_message() {
    let arguments = [
        Argument::Resolution,
        Argument::TileSize,
        Argument::Stride,
        Argument::BufferLength,
    ];
    for (i, a) in arguments.iter().enumerate() {
        let error = Error::InvalidArgument(*a);
        assert_eq!(code_of(error), SCG_ERR_INVALID_ARGUMENT);
        for b in &arguments[i + 1..] {
            assert_ne!(message_of(error), message_of(Error::InvalidArgument(*b)));
        }
    }
}
