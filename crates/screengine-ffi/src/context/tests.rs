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
        max_lines: 0,
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

/// Le champ encore réservé : c'est ce refus qui permettra de l'utiliser plus
/// tard sans casser une liaison déjà écrite.
///
/// Il n'en reste qu'un. Les deux autres ont servi comme la clause d'extension
/// l'annonçait — `max_triangles` à l'étape 1, `max_lines` à l'étape 8 —, chaque
/// fois sans toucher un décalage ni `SCG_ABI_VERSION`.
#[test]
fn refuse_un_champ_reserve_non_nul() {
    let mut config = sane();
    config.reserved2 = 1;
    assert_eq!(config.to_core().unwrap_err(), AbiError::RESERVED);
}

/// Les deux champs qui étaient réservés portent désormais une capacité, et ne
/// se refusent plus quand ils sont non nuls.
///
/// C'est l'usage prévu d'un champ réservé : un hôte de la version précédente
/// passait zéro, et zéro reste le défaut. Les deux sont éprouvés ensemble parce
/// que c'est la **même clause** qui les libère — les séparer aurait donné deux
/// tests dont la documentation se recopie, et le second n'aurait rien prouvé que
/// le premier ne prouve.
#[test]
fn les_champs_liberes_portent_leur_capacite() {
    let mut config = sane();
    config.max_triangles = 1_000;
    config.max_lines = 128;
    let core = config.to_core().expect("capacités choisies");
    assert_eq!(core.max_triangles, 1_000);
    assert_eq!(core.max_lines, 128);
}
