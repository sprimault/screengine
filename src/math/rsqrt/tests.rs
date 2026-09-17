// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La racine inverse, balayée sur toutes les mantisses.

extern crate std;

use super::*;
use crate::testing::fnv1a;

/// La table d'estimation figée à la compilation est celle de l'exécution.
#[test]
fn la_table_constante_est_celle_de_l_execution() {
    let runtime = std::hint::black_box(estimates as fn() -> [f32; 2 * HALF])();
    for (i, (a, b)) in ESTIMATE.iter().zip(runtime.iter()).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "entrée {i}");
    }
}

/// Toutes les mantisses, pour un exposant pair et un impair : l'erreur relative
/// ne dépend que de la parité de l'exposant, donc ces deux balayages couvrent
/// tous les nombres admis. L'empreinte des 2²⁴ résultats est figée : c'est elle
/// qui garantit les mêmes bits partout.
///
/// La précision se mesure contre la racine carrée de la plateforme, que la
/// norme impose correctement arrondie. La borne est d'un ulp et demi, pas d'un
/// seul : sans multiplication fusionnée, que le déterminisme interdit, le
/// résidu `½ − ½m·y²` hérite de l'arrondi de deux produits, et l'écart mesuré
/// atteint 1,36 ulp au bord du premier sous-intervalle. Aucune normale ni
/// aucune direction de caméra n'en dépend à ce point.
#[test]
#[allow(clippy::disallowed_methods)]
fn toutes_les_mantisses_restent_sous_l_ulp_et_demi() {
    let mut hash = fnv1a([]);
    let mut worst = 0.0f64;
    for exponent in [127u32, 128] {
        for fraction in 0..1u32 << 23 {
            let x = f32::from_bits((exponent << 23) | fraction);
            let y = rsqrt(x);
            let exact = 1.0 / (x as f64).sqrt();
            let ulp = (f32::from_bits(y.to_bits() + 1) - y) as f64;
            let error = (y as f64 - exact).abs() / ulp;
            if error > worst {
                worst = error;
            }
            for byte in y.to_bits().to_le_bytes() {
                hash = (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    assert!(worst <= 1.5, "écart maximal de {worst} ulp");
    assert_eq!(hash, SWEEP_FINGERPRINT);
}

/// L'empreinte attendue du balayage.
const SWEEP_FINGERPRINT: u64 = 4_403_909_289_154_917_219;

/// Les puissances de quatre ont une racine inverse exacte, et l'exposant se
/// rétablit sans arrondi, aux deux extrémités de l'intervalle admis.
#[test]
fn les_puissances_de_quatre_sont_exactes() {
    assert_eq!(rsqrt(1.0), 1.0);
    assert_eq!(rsqrt(4.0), 0.5);
    assert_eq!(rsqrt(0.25), 2.0);
    let power = |k: i32| f32::from_bits(((127 + k) as u32) << 23);
    assert_eq!(rsqrt(power(-98)), power(49));
    assert_eq!(rsqrt(power(126)), power(-63));
}

/// Ce qui n'a pas de racine inverse finie et exploitable rend zéro, jamais un
/// NaN ni un infini qui se propageraient dans la transformation des sommets.
#[test]
fn les_cas_limites_rendent_zero() {
    for x in [
        0.0,
        -0.0,
        -1.0,
        NEGLIGIBLE,
        1.0e-31,
        f32::from_bits(1),
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ] {
        assert_eq!(rsqrt(x).to_bits(), 0.0f32.to_bits(), "{x}");
    }
    assert!(rsqrt(f32::MAX) > 0.0);
}
