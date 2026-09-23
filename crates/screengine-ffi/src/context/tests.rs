// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La structure de configuration et sa disposition en mémoire.
//!
//! Les décalages se prouvent ici parce que `cbindgen` ne les vérifie pas : il
//! analyse la source sans jamais interroger `rustc`, et recopie l'ordre des
//! champs sans rien en savoir. Une liaison JavaScript, elle, les reproduit
//! octet par octet.

use super::*;

/// Une configuration d'ABI qui passe, dont les tests dérivent leurs
/// variantes.
fn sane() -> ScgContextConfig {
    ScgContextConfig {
        max_width: 640,
        max_height: 360,
        width: 640,
        height: 360,
        tile_size: 64,
        max_triangles: 0,
        reserved1: 0,
        reserved2: 0,
    }
}

/// Le cas nominal, qui garde les refus honnêtes : une conversion qui
/// rejetterait tout passerait le test suivant sans rien prouver.
#[test]
fn convertit_une_configuration_saine() {
    let config = sane().to_core().expect("configuration saine");
    assert_eq!(config.tile_size, 64);
}

/// Les deux champs encore réservés, un par un : c'est ce refus qui permettra
/// d'en utiliser un plus tard sans casser une liaison déjà écrite, et il ne
/// vaut que s'il couvre chacun d'eux.
#[test]
fn refuse_un_champ_reserve_non_nul() {
    for set in [
        (|c: &mut ScgContextConfig| c.reserved1 = 1) as fn(&mut ScgContextConfig),
        |c| c.reserved2 = 1,
    ] {
        let mut config = sane();
        set(&mut config);
        assert_eq!(config.to_core().unwrap_err(), AbiError::RESERVED);
    }
}

/// Le champ qui était réservé porte désormais la capacité, et ne se refuse
/// plus quand il est non nul.
///
/// C'est l'usage prévu d'un champ réservé : un hôte de la version précédente
/// passait zéro, et zéro reste le défaut.
#[test]
fn le_champ_de_capacite_ne_se_refuse_plus() {
    let mut config = sane();
    config.max_triangles = 1_000;
    assert_eq!(
        config.to_core().expect("capacité choisie").max_triangles,
        1_000
    );
}
