// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La forme de l'atténuation, et ce que la somme de plusieurs lumières donne.

use super::*;
use crate::scene::Color;

/// Une lumière blanche de rayon `radius`, placée à l'origine de la vue.
fn white(radius: f32) -> Placed {
    let light = Light {
        position: Vec3::ZERO,
        radius,
        color: Color::new(0xFF, 0xFF, 0xFF, 0xFF),
    };
    Placed::new(&light, Vec3::ZERO).expect("rayon valide")
}

/// Un point à `distance` du centre, sur un axe quelconque.
fn away(distance: f32) -> Vec3 {
    Vec3::new(distance, 0.0, 0.0)
}

/// Au centre, la lumière donne sa couleur pleine ; au-delà du rayon, rien.
#[test]
fn l_attenuation_va_du_plein_au_neant() {
    let light = white(10.0);
    assert_eq!(light.contribution(Vec3::ZERO), [1.0, 1.0, 1.0]);
    assert_eq!(light.contribution(away(10.0)), [0.0; 3]);
    assert_eq!(light.contribution(away(1000.0)), [0.0; 3]);
}

/// **L'atténuation s'annule au bord avec une dérivée nulle.**
///
/// C'est la raison du carré, et le défaut qu'il évite est visible : une
/// atténuation en `1 − d²/r²` seule s'annule avec une pente franche, ce qui
/// dessine un anneau net à la limite de portée. Le critère est que la valeur
/// juste avant le bord soit très petite devant celle de mi-parcours — une
/// pente franche y laisserait une marche.
#[test]
fn l_attenuation_s_annule_en_douceur_au_bord() {
    let light = white(10.0);
    let at = |d: f32| light.contribution(away(d))[0];

    let bord = at(9.9);
    let milieu = at(5.0);
    assert!(
        milieu > 0.5,
        "{milieu} à mi-parcours, la portée est trop courte"
    );
    // `1 − d²/r²` vaut ici 0,0199 ; son carré, 0,000396. Le seuil sépare
    // nettement les deux formes.
    assert!(bord < 0.001, "{bord} au bord : la pente n'est pas nulle");
}

/// L'atténuation décroît sans jamais remonter, et reste dans `[0, 1]`.
#[test]
fn l_attenuation_decroit_de_facon_monotone() {
    let light = white(10.0);
    let mut precedent = f32::INFINITY;
    for step in 0..=200 {
        let value = light.contribution(away(step as f32 * 0.1))[0];
        assert!((0.0..=1.0).contains(&value), "{value} hors de [0, 1]");
        assert!(value <= precedent, "remontée à {step}");
        precedent = value;
    }
}

/// Une lumière colorée ne porte que sa teinte : un canal nul le reste, quelle
/// que soit la distance.
#[test]
fn chaque_canal_porte_la_teinte_de_sa_lumiere() {
    let light = Light {
        position: Vec3::ZERO,
        radius: 10.0,
        color: Color::new(0xFF, 0x80, 0x00, 0xFF),
    };
    let placed = Placed::new(&light, Vec3::ZERO).expect("rayon valide");
    let at_center = placed.contribution(Vec3::ZERO);
    assert_eq!(at_center[0], 1.0);
    assert!(
        (at_center[1] - 128.0 / 255.0).abs() < 1.0e-6,
        "{at_center:?}"
    );
    assert_eq!(at_center[2], 0.0);
}

/// **La somme sature après l'addition, jamais avant.**
///
/// Deux lumières faibles de teintes différentes doivent mélanger leurs
/// couleurs. Saturer chacune séparément les rendrait indiscernables dès
/// qu'une troisième, forte, domine — et c'est précisément le mélange qu'on
/// veut voir.
#[test]
fn la_somme_sature_apres_l_addition() {
    let rouge = Light {
        position: Vec3::ZERO,
        radius: 10.0,
        color: Color::new(0xFF, 0, 0, 0xFF),
    };
    let bleu = Light {
        position: Vec3::ZERO,
        radius: 10.0,
        color: Color::new(0, 0, 0xFF, 0xFF),
    };
    let lights = [
        Placed::new(&rouge, Vec3::ZERO).expect("rayon valide"),
        Placed::new(&bleu, Vec3::ZERO).expect("rayon valide"),
    ];
    // Au centre, chacune est pleine : le résultat est magenta saturé, et non
    // un rouge qui aurait mangé le bleu.
    assert_eq!(sum(&lights, Vec3::ZERO), [1.0, 0.0, 1.0]);

    // À mi-parcours, aucune n'est pleine : la somme reste sous la saturation
    // et garde les deux teintes.
    let milieu = sum(&lights, away(5.0));
    assert!(milieu[0] > 0.0 && milieu[0] < 1.0, "{milieu:?}");
    assert_eq!(milieu[0], milieu[2], "les deux teintes ont divergé");
}

/// Deux lumières blanches qui se superposent saturent, plutôt que de rendre
/// une valeur au-delà de l'unité que la conversion en octets enroulerait.
#[test]
fn deux_lumieres_pleines_saturent() {
    let lights = [white(10.0), white(10.0)];
    assert_eq!(sum(&lights, Vec3::ZERO), [1.0, 1.0, 1.0]);
}

/// Sans lumière, la somme est nulle : l'appelant n'a pas de cas à distinguer.
#[test]
fn aucune_lumiere_ne_donne_rien() {
    assert_eq!(sum(&[], Vec3::ZERO), [0.0; 3]);
}

/// Un rayon nul, négatif ou non fini est refusé, et une position non finie
/// aussi : la première ferait diviser par zéro, la seconde empoisonnerait
/// chaque sommet du lot.
#[test]
fn une_lumiere_sans_etendue_est_refusee() {
    for radius in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
        let light = Light {
            position: Vec3::ZERO,
            radius,
            color: Color::new(0xFF, 0xFF, 0xFF, 0xFF),
        };
        assert!(
            Placed::new(&light, Vec3::ZERO).is_none(),
            "rayon {radius} accepté"
        );
    }
    let light = Light {
        position: Vec3::ZERO,
        radius: 1.0,
        color: Color::new(0xFF, 0xFF, 0xFF, 0xFF),
    };
    assert!(Placed::new(&light, Vec3::new(f32::NAN, 0.0, 0.0)).is_none());
}

/// La distance se mesure dans les trois dimensions, pas sur un seul axe.
///
/// Sans ce contrôle, une atténuation qui ne lirait que `x` passerait tous les
/// tests qui précèdent, puisqu'ils placent leurs points sur cet axe.
#[test]
fn la_distance_est_celle_des_trois_axes() {
    let light = white(10.0);
    // Trois points à la même distance, sur trois axes différents.
    let attendu = light.contribution(Vec3::new(6.0, 0.0, 0.0));
    assert_eq!(light.contribution(Vec3::new(0.0, 6.0, 0.0)), attendu);
    assert_eq!(light.contribution(Vec3::new(0.0, 0.0, 6.0)), attendu);
    // Et la diagonale : 3² + 4² = 5².
    assert_eq!(
        light.contribution(Vec3::new(3.0, 4.0, 0.0)),
        light.contribution(Vec3::new(5.0, 0.0, 0.0))
    );
}
