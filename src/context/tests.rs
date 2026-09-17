// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que le contexte accepte, et ce qu'il refuse.
//!
//! Les refus comptent autant que le cas nominal : une configuration invalide qui
//! passerait donnerait des tampons incohérents, et le défaut ne se verrait qu'au
//! premier remplissage.

use super::*;

/// Une configuration qui passe, dont les tests dérivent leurs variantes.
fn sane() -> Config {
    Config {
        max_width: 640,
        max_height: 360,
        width: 640,
        height: 360,
        tile_size: 64,
    }
}

/// Le cas nominal, qui garde les tests de refus honnêtes : sans lui, une
/// validation qui refuserait tout passerait tous les autres.
#[test]
fn accepte_une_configuration_saine() {
    let ctx = Context::new(sane()).expect("configuration saine");
    assert_eq!(ctx.resolution(), (640, 360));
}

/// Une largeur nulle donnerait des tampons vides et une boucle de
/// remplissage qui ne s'exécute jamais, sans que rien ne le signale.
#[test]
fn refuse_une_dimension_nulle() {
    let mut config = sane();
    config.width = 0;
    assert_eq!(
        Context::new(config).unwrap_err(),
        Error::InvalidArgument(Argument::Resolution)
    );
}

/// Les tampons sont dimensionnés pour le maximum : une résolution initiale
/// au-delà déborderait dès la première image.
#[test]
fn refuse_une_resolution_initiale_au_dela_du_maximum() {
    let mut config = sane();
    config.width = config.max_width + 1;
    assert_eq!(
        Context::new(config).unwrap_err(),
        Error::InvalidArgument(Argument::Resolution)
    );
}

/// La borne n'est pas un confort : au-delà, les pires cas des formats en
/// virgule fixe cessent d'être vrais et une fonction de bord déborde avant
/// que quoi que ce soit d'autre ne le signale.
#[test]
fn refuse_un_maximum_au_dela_de_la_borne_des_formats() {
    let mut config = sane();
    config.max_width = MAX_RESOLUTION + 1;
    config.width = MAX_RESOLUTION + 1;
    assert_eq!(
        Context::new(config).unwrap_err(),
        Error::InvalidArgument(Argument::Resolution)
    );
}

/// Une taille intermédiaire compilerait et rendrait une image : elle se
/// refuse ici, parce qu'aucun chemin de rendu ne sera écrit pour elle.
#[test]
fn refuse_une_taille_de_tuile_hors_liste() {
    let mut config = sane();
    config.tile_size = 48;
    assert_eq!(
        Context::new(config).unwrap_err(),
        Error::InvalidArgument(Argument::TileSize)
    );
}

/// Le premier des trois messages d'erreur du premier jour. La frontière C
/// ne peut pas faire ce contrôle, faute de recevoir la longueur du tampon.
#[test]
fn refuse_un_stride_plus_court_que_la_largeur() {
    let mut ctx = Context::new(sane()).expect("configuration saine");
    let mut pixels = [0u8; 4];
    assert_eq!(
        ctx.frame_end(&mut pixels, 639).unwrap_err(),
        Error::InvalidArgument(Argument::Stride)
    );
}

/// Un appelant Rust porte la longueur avec sa tranche : c'est le seul
/// contrôle qui distingue ce chemin de celui de la frontière C.
#[test]
fn refuse_un_tampon_trop_court_pour_son_stride() {
    let mut ctx = Context::new(sane()).expect("configuration saine");
    let mut pixels = [0u8; 4];
    assert_eq!(
        ctx.frame_end(&mut pixels, 640).unwrap_err(),
        Error::InvalidArgument(Argument::BufferLength)
    );
}
