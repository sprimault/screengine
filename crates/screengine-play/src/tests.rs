// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que les réglages de [`Play`] demandent au moteur.
//!
//! La boucle elle-même exige une fenêtre et ne se teste pas ici. La traduction
//! des réglages en [`Config`], si : c'est le seul endroit où un réglage peut
//! partir à zéro sans que rien ne rougisse.

use super::{Play, Scale};

/// Les réglages par défaut passent la résolution d'ouverture en plafond, et
/// laissent les deux budgets à `0` — ce que le moteur lit comme « prends le
/// défaut ».
#[test]
fn les_defauts_ne_plafonnent_rien() {
    let config = Play::new().config();

    assert_eq!(
        (config.width, config.height),
        (config.max_width, config.max_height)
    );
    assert_eq!(config.max_triangles, 0);
    assert_eq!(config.max_lines, 0);
}

/// Chaque réglage arrive dans le champ correspondant, et un plafond de
/// résolution ne déplace pas la résolution d'ouverture.
#[test]
fn chaque_reglage_arrive_dans_sa_configuration() {
    let config = Play::new()
        .resolution(320, 180)
        .max_resolution(640, 360)
        .tile_size(32)
        .max_triangles(40_000)
        .max_lines(4_096)
        .scale(Scale::Fill)
        .config();

    assert_eq!((config.width, config.height), (320, 180));
    assert_eq!((config.max_width, config.max_height), (640, 360));
    assert_eq!(config.tile_size, 32);
    assert_eq!(config.max_triangles, 40_000);
    assert_eq!(config.max_lines, 4_096);
}

/// Une capacité au-delà de ce que le moteur accepte est refusée par lui, pas
/// par cet étage : la configuration la porte telle quelle.
///
/// C'est la forme voulue — un seul endroit juge, et c'est celui qui alloue.
#[test]
fn une_capacite_demesuree_part_telle_quelle() {
    let config = Play::new().max_triangles(u32::MAX).config();

    assert_eq!(config.max_triangles, u32::MAX);
}
