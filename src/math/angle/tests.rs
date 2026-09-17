// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La table trigonométrique, ses symétries et la conversion depuis les radians.

extern crate std;

use super::*;
use crate::testing::{Rng, fnv1a};

/// La table figée à la compilation est celle que la même fonction calcule à
/// l'exécution, bit pour bit : sur chaque cible où ce test tourne, le calcul
/// constant et le calcul de la cible s'accordent.
#[test]
fn la_table_constante_est_celle_de_l_execution() {
    let runtime = std::hint::black_box(quarter_sine as fn() -> [f32; STEPS + 1])();
    for (i, (a, b)) in QUARTER_SINE.iter().zip(runtime.iter()).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "entrée {i}");
    }
}

/// L'empreinte des bits de la table. Un générateur modifié change le rendu de
/// toute rotation : il doit le faire en changeant ce test, pas en silence.
#[test]
fn l_empreinte_de_la_table_est_figee() {
    let bytes = QUARTER_SINE.iter().flat_map(|v| v.to_bits().to_le_bytes());
    assert_eq!(fnv1a(bytes), TABLE_FINGERPRINT);
}

/// L'empreinte attendue de la table.
const TABLE_FINGERPRINT: u64 = 4_871_351_292_124_514_140;

/// Les bornes du quart de cercle sont exactes : le sinus d'un angle droit vaut
/// un, sans quoi une rotation d'un quart de tour ne serait pas une permutation
/// d'axes.
#[test]
fn les_angles_droits_sont_exacts() {
    assert_eq!(Angle(0).sin().to_bits(), 0.0f32.to_bits());
    assert_eq!(Angle::QUARTER.sin(), 1.0);
    assert_eq!(Angle::QUARTER.cos(), 0.0);
    assert_eq!(Angle(0).cos(), 1.0);
    assert_eq!(Angle(2 * QUARTER).sin(), 0.0);
    assert_eq!(Angle(3 * QUARTER).sin(), -1.0);
}

/// Les symétries sont exactes au bit près, parce qu'elles se lisent dans la
/// même entrée de table : c'est ce que l'angle binaire apporte.
#[test]
fn les_symetries_sont_exactes() {
    let mut rng = Rng::new(3);
    for _ in 0..100_000 {
        let a = Angle(rng.next() as u32);
        let opposite = Angle(a.0.wrapping_neg());
        assert_eq!(opposite.sin(), -a.sin(), "sin(-a) à {}", a.0);
        let supplement = Angle((2 * QUARTER).wrapping_sub(a.0));
        assert_eq!(supplement.sin(), a.sin(), "sin(π - a) à {}", a.0);
    }
}

/// La table interpolée reste sous le millionième de la fonction exacte. La
/// libm de la plateforme ne sert ici que de mesure, avec une tolérance : ses
/// bits ne sont une référence nulle part.
#[test]
#[allow(clippy::disallowed_methods)]
fn l_interpolation_reste_sous_le_millionieme() {
    let mut rng = Rng::new(5);
    for _ in 0..100_000 {
        let a = Angle(rng.next() as u32);
        let radians = a.0 as f64 * (2.0 * PI / 4_294_967_296.0);
        let error = (a.sin() as f64 - radians.sin()).abs();
        assert!(error < 1.0e-6, "écart {error} à {}", a.0);
    }
}

/// Les radians se réduisent à un tour avant toute conversion entière : les
/// multiples de π/2, négatifs compris, tombent sur leur quart de tour à
/// quelques unités près.
#[test]
fn les_radians_se_reduisent_a_un_tour() {
    let near = |radians: f32, expected: u32| {
        let got = Angle::from_radians(radians).0;
        let distance = got.wrapping_sub(expected).min(expected.wrapping_sub(got));
        assert!(distance <= 1 << 9, "{radians} rad : {got} pour {expected}");
    };
    let half_pi = (PI / 2.0) as f32;
    near(0.0, 0);
    near(half_pi, QUARTER);
    near(-half_pi, 3 * QUARTER);
    near(4.0 * half_pi, 0);
    near(-7.0 * half_pi, QUARTER);
    near(1000.0 * half_pi, 0);
}

/// Ce qu'aucune réduction ne rend sensé donne l'angle nul, au lieu de la valeur
/// de saturation d'une conversion, qui changerait selon le jeu d'instructions.
#[test]
fn les_valeurs_hors_de_portee_donnent_l_angle_nul() {
    for radians in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.0e9, -3.0e30] {
        assert_eq!(Angle::from_radians(radians), Angle(0), "{radians}");
    }
}
