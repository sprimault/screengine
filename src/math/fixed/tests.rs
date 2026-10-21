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

/// `to_texel` arrondit au plus proche, demi-pas écarté de zéro, et c'est sur
/// les négatifs que ça compte : la conversion `as` tronque vers zéro, si bien
/// qu'une texture aurait une couture le long de son origine, sur une ligne que
/// rien d'autre ne distingue.
#[test]
fn la_coordonnee_de_texture_arrondit_au_plus_proche() {
    let unit = 1 << TEXEL_BITS;
    for (value, expected) in [
        (0.0, 0),
        (1.0, unit),
        (-1.0, -unit),
        (0.5, unit / 2),
        (-0.5, -unit / 2),
    ] {
        assert_eq!(to_texel(value), expected, "{value}");
    }

    // Un quart de pas de part et d'autre de zéro : les deux doivent s'arrondir
    // du même côté de leur propre entier, donc s'opposer exactement.
    let quart = 0.25 / unit as f32;
    assert_eq!(to_texel(quart), -to_texel(-quart));
    let trois_quarts = 0.75 / unit as f32;
    assert_eq!(to_texel(trois_quarts), 1);
    assert_eq!(to_texel(-trois_quarts), -1);
}

/// La borne est écrite, jamais laissée à la saturation de `as` : un sommet
/// engendré par le découpage retombe parfois quelques ulp sous le plan proche,
/// donc porte une profondeur à peine au-dessus de un.
#[test]
fn la_coordonnee_de_texture_reste_dans_son_format() {
    let limit = (MAX_TEXEL_COORD as i32) << TEXEL_BITS;
    for (value, expected) in [
        (MAX_TEXEL_COORD, limit),
        (-MAX_TEXEL_COORD, -limit),
        (MAX_TEXEL_COORD * 2.0, limit),
        (f32::INFINITY, limit),
        (f32::NEG_INFINITY, -limit),
        (f32::NAN, 0),
    ] {
        assert_eq!(to_texel(value), expected, "{value}");
    }
    // Vingt-sept bits avec le signe : cinq de marge dans l'`i32`, et le 14.12
    // littéralement vrai.
    assert!(limit <= 1 << 26);
}

/// [`rounded`] rend les bonnes valeurs sur tout son domaine et au-delà.
///
/// **Ce que ce cas ne peut pas distinguer, et il faut le dire** : le bornage
/// écrit de la saturation de `as`. Sur cette cible les deux rendent les mêmes
/// bits — `as` sature déjà à zéro depuis un négatif et depuis `NaN` —, et c'est
/// vérifié plutôt que supposé : en remplaçant le corps par
/// `(value * scale + 0.5) as u32).min(max)`, ce cas reste vert. La règle du
/// bornage écrit tient donc par la **portabilité**, que seul un chemin vectoriel
/// mettrait en défaut, et aucun test scalaire ne la gardera.
///
/// Ce qu'il garde réellement : le demi-pas, le maximum, et les deux bornes.
/// Chacune de ces trois mutations le fait rougir.
#[test]
fn l_arrondi_rend_les_bonnes_valeurs_sur_tout_son_domaine() {
    // Le domaine nominal, demi-pas compris : `0,5/255` est le point qui bascule,
    // et un canal plein doit rendre le maximum exactement.
    for (value, expected) in [(0.0, 0), (1.0, 255), (0.5, 128), (0.002, 1)] {
        assert_eq!(rounded(value, 255.0, 255), expected, "{value}");
    }

    // Hors domaine, des deux côtés, et `NaN`.
    for value in [-0.001, -1.0, f32::NEG_INFINITY, f32::NAN] {
        assert_eq!(rounded(value, 255.0, 255), 0, "{value} sous la borne");
    }
    for value in [1.001, 2.0, f32::INFINITY] {
        assert_eq!(rounded(value, 255.0, 255), 255, "{value} au-dessus");
    }
}

/// L'échelle est un paramètre, et c'est tout ce qui distinguait les trois sites.
///
/// Une valeur déjà pondérée prend `1.0`, un canal de zéro à un prend son
/// maximum, et le brouillard une échelle qui n'est pas celle d'un octet : les
/// trois doivent rendre la même chose pour la même fraction du plein.
#[test]
fn l_echelle_et_le_maximum_se_donnent_separement() {
    // La moitié du plein, à trois échelles.
    assert_eq!(rounded(127.5, 1.0, 255), 128);
    assert_eq!(rounded(0.5, 255.0, 255), 128);
    assert_eq!(rounded(0.5, 4096.0, 4096), 2048);

    // Le maximum borne, même quand l'échelle le dépasse : c'est lui qui décide,
    // pas le produit.
    assert_eq!(rounded(1.0, 4096.0, 255), 255);
}
