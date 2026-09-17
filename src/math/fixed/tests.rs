// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le seul arrondi du pipeline, éprouvé sur les deux signes.
//!
//! C'est là que les flottants s'arrêtent : un biais introduit ici se retrouve
//! dans toutes les empreintes de conformance, et rien en aval ne le rattrape.

use super::*;

/// Le repère de base : un pixel vaut seize sous-pixels, dans les deux sens.
#[test]
fn convertit_un_pixel_entier() {
    assert_eq!(to_subpixel(0.0), 0);
    assert_eq!(to_subpixel(1.0), 16);
    assert_eq!(to_subpixel(-1.0), -16);
}

/// Le cas qui distingue l'arrondi de la troncature : un demi-sous-pixel de
/// part et d'autre de zéro, que `as` ramènerait tous les deux à zéro.
#[test]
fn arrondit_le_demi_pas_en_s_ecartant_de_zero() {
    let half = 0.5 / SUBPIXEL_SCALE as f32;
    assert_eq!(to_subpixel(half), 1);
    assert_eq!(to_subpixel(-half), -1);
}

/// L'arrondi est symétrique : un objet centré sur l'origine le reste.
#[test]
fn l_arrondi_est_symetrique() {
    for step in 0..1000 {
        let v = step as f32 * 0.013;
        assert_eq!(to_subpixel(-v), -to_subpixel(v), "à v = {v}");
    }
}

/// L'erreur ne dépasse jamais un demi-sous-pixel, là où la troncature
/// atteindrait le sous-pixel entier.
#[test]
fn l_erreur_reste_sous_le_demi_pas() {
    for step in -500..500 {
        let v = step as f32 * 0.0137;
        let exact = v * SUBPIXEL_SCALE as f32;
        let error = (to_subpixel(v) as f32 - exact).abs();
        assert!(error <= 0.5, "erreur {error} à v = {v}");
    }
}

/// La borne sur laquelle les pires cas des fonctions de bord sont calculés :
/// 4096 pixels font exactement 2¹⁶ sous-pixels, donc un écart entre sommets
/// tient dans 2¹⁷ et leur produit dans 2³⁴.
#[test]
fn la_bande_de_garde_tient_dans_ses_bornes() {
    assert_eq!(to_subpixel(4096.0), 1 << 16);
}

/// La profondeur reste à la marge des bornes aux deux extrémités, et ce qui
/// ne devrait jamais arriver — zéro, négatif, NaN, au-delà de un — y est
/// ramené par comparaison, pas par la saturation d'une conversion.
#[test]
fn la_profondeur_reste_dans_la_marge() {
    let (low, high) = (DEPTH_MARGIN, u32::MAX - DEPTH_MARGIN);
    for (depth, expected) in [
        (0.0, low),
        (-1.0, low),
        (f32::NAN, low),
        (1.0e-12, low),
        (1.0, high),
        (7.0, high),
        (f32::INFINITY, high),
        (0.5, 1 << 31),
        (0.25, 1 << 30),
    ] {
        assert_eq!(to_depth(depth), expected, "{depth}");
    }
    // Le plus grand `f32` sous un donne 2³² − 256 : la conversion ne déborde
    // pas, et la valeur, sous la marge haute, passe telle quelle.
    let below_one = f32::from_bits(1.0f32.to_bits() - 1);
    assert_eq!(to_depth(below_one), u32::MAX - 255);
}
