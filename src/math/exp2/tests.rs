// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `2^x`, `log2 x` et la puissance, contre des références écrites ici.
//!
//! **La référence n'est pas la libm de la machine**, et ce n'est pas une
//! coquetterie de doctrine. IEEE 754 impose `sqrt` correctement arrondi, ce qui
//! permet à la racine inverse de se comparer à celle de la plateforme ; il
//! n'impose rien de tel pour l'exponentielle ni le logarithme, que musl, la
//! glibc, Darwin et le CRT de MSVC ont le droit de calculer différemment. Une
//! borne mesurée contre l'une d'elles ne voudrait rien dire ailleurs.
//!
//! Les deux références ci-dessous n'emploient que les quatre opérations, sont
//! lentes et manifestement justes, et tiennent environ vingt bits sous l'ulp du
//! `f32` — assez pour arbitrer une erreur qui s'exprime en dixièmes d'ulp.

extern crate std;

use std::vec::Vec;

use super::*;
use crate::testing::fnv1a;

/// La borne exigée de `exp2`, en ulp du `f32`.
///
/// Serrée exprès au-dessus des 0,53 mesurés : une borne molle laisserait passer
/// un Horner repassé en `f32`, qui triple l'erreur sans rien casser d'autre.
const EXP2_TOLERANCE: f64 = 0.7;

/// La borne exigée de `log2`, en ulp du `f32`.
const LOG2_TOLERANCE: f64 = 1.2;

/// `2^k` en `f64`, exact, pour un `k` d'exposant valide.
fn scale(k: i32) -> f64 {
    f64::from_bits(((1023 + k) as u64) << 52)
}

/// `2^x` de référence : réduction par carrés successifs, puis Taylor.
///
/// L'argument est divisé par soixante-quatre avant d'entrer dans la série, ce
/// qui rend huit termes largement suffisants, et six élévations au carré
/// rendent le facteur. Mesurée à 3·10⁻¹⁴ d'erreur relative, soit vingt bits
/// sous l'ulp du `f32`.
fn exp2_reference(x: f64) -> f64 {
    let half = if x < 0.0 { -0.5 } else { 0.5 };
    let k = (x + half) as i32;
    let u = (x - f64::from(k)) * core::f64::consts::LN_2 / 64.0;

    let mut term = 1.0;
    let mut sum = 1.0;
    let mut n = 1;
    while n <= 8 {
        term = term * u / f64::from(n);
        sum += term;
        n += 1;
    }
    let mut i = 0;
    while i < 6 {
        sum *= sum;
        i += 1;
    }
    sum * scale(k)
}

/// `log2 x` de référence : le logarithme bit à bit.
///
/// À chaque tour la mantisse est élevée au carré ; si elle franchit deux, le
/// bit courant du développement binaire vaut un et on la ramène. Soixante tours
/// donnent bien plus que les vingt-quatre bits du `f32`.
fn log2_reference(x: f64) -> f64 {
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 0x7FF) as i32 - 1023;
    let mut mantissa = f64::from_bits((bits & 0xF_FFFF_FFFF_FFFF) | (1023u64 << 52));

    let mut fraction = 0.0;
    let mut weight = 0.5;
    let mut i = 0;
    while i < 60 {
        mantissa *= mantissa;
        if mantissa >= 2.0 {
            mantissa *= 0.5;
            fraction += weight;
        }
        weight *= 0.5;
        i += 1;
    }
    f64::from(exponent) + fraction
}

/// L'écart entre une valeur rendue et sa référence, en ulp du `f32`.
fn ulp_error(got: f32, exact: f64) -> f64 {
    if got.is_infinite() || exact == 0.0 {
        return 0.0;
    }
    let above = f32::from_bits(got.to_bits() + 1);
    let ulp = f64::from(above) - f64::from(got);
    if ulp == 0.0 {
        return 0.0;
    }
    let error = f64::from(got) - exact;
    let error = if error < 0.0 { -error } else { error };
    error / ulp
}

/// Les deux références s'accordent avec la plateforme.
///
/// Sans ce contrôle, un défaut **dans la référence** rendrait tous les tests de
/// précision verts en mesurant l'écart d'une erreur à elle-même. C'est le seul
/// endroit où la libm entre, et elle n'y sert qu'à se faire confirmer.
#[test]
#[allow(clippy::disallowed_methods)]
fn les_references_s_accordent_avec_la_plateforme() {
    let mut x = -100.0f64;
    while x < 100.0 {
        let ecart = exp2_reference(x) / x.exp2() - 1.0;
        assert!(
            ecart < 1.0e-12 && ecart > -1.0e-12,
            "exp2 de référence en {x}"
        );
        x += 0.37;
    }

    let mut y = 1.0e-30f64;
    while y < 1.0e30 {
        let ecart = log2_reference(y) - y.log2();
        assert!(
            ecart < 1.0e-12 && ecart > -1.0e-12,
            "log2 de référence en {y}"
        );
        y *= 7.9;
    }
}

/// Les identités que rien n'approche : elles ne dépendent d'aucune référence,
/// et une erreur de reconstruction d'exposant les casse avant toute mesure.
#[test]
fn les_identites_exactes_tiennent() {
    assert_eq!(exp2(0.0), 1.0);
    assert_eq!(log2(1.0), 0.0);
    assert!(log2(1.0).is_sign_positive(), "log2(1) doit être +0");

    for k in -120..=120 {
        let power = f32::from_bits(((127 + k) as u32) << 23);
        assert_eq!(exp2(k as f32), power, "2^{k}");
        assert_eq!(log2(power), k as f32, "log2(2^{k})");
    }

    // `2^k` étant exact, décaler l'argument d'une unité double le résultat au
    // bit près : c'est la preuve que le polynôme et l'exposant ne se mêlent
    // jamais.
    let mut x = -30.0f32;
    while x < 30.0 {
        assert_eq!(exp2(x + 1.0), 2.0 * exp2(x), "exp2({x} + 1)");
        x += 0.125;
    }
}

/// Ce que les deux fonctions refusent, et ce qu'elles rendent alors.
#[test]
fn les_cas_limites_rendent_ce_qui_est_documente() {
    assert_eq!(exp2(f32::NAN), 0.0);
    assert_eq!(exp2(f32::INFINITY), f32::INFINITY);
    assert_eq!(exp2(f32::NEG_INFINITY), 0.0);
    assert_eq!(exp2(128.0), f32::INFINITY);
    assert_eq!(exp2(-126.0), 0.0);

    assert_eq!(log2(f32::NAN), 0.0);
    assert_eq!(log2(0.0), 0.0);
    assert_eq!(log2(-1.0), 0.0);
    assert_eq!(log2(f32::INFINITY), 0.0);

    assert_eq!(powf(2.0, 0.0), 1.0);
    assert_eq!(powf(0.0, 2.0), 0.0);
    assert_eq!(powf(f32::NAN, 2.0), 0.0);
    assert_eq!(powf(2.0, f32::NAN), 0.0);
}

/// Un dénormal en entrée de `log2` a son exposant remonté avant lecture.
///
/// Sans ce traitement, sa mantisse serait lue comme celle d'un nombre normal et
/// le résultat serait faux de dizaines d'unités — sans erreur, sans panique, et
/// sans que la borne de précision des autres tests le voie : ils ne balaient
/// que des nombres normaux.
#[test]
fn log2_lit_un_denormal() {
    let denormals = [
        f32::from_bits(1),
        f32::from_bits(0x40_0000),
        f32::from_bits(0x7F_FFFF),
    ];
    for x in denormals {
        let error = ulp_error(log2(x), log2_reference(f64::from(x)));
        assert!(error <= LOG2_TOLERANCE, "{x:e} : {error} ulp");
    }
}

/// `exp2` tient sa borne sur tout le domaine, exposants extrêmes compris.
///
/// L'erreur ne dépend que de la partie fractionnaire — le facteur `2^k` est
/// exact —, mais le balayage couvre aussi les exposants : c'est la
/// reconstruction des bits qu'il éprouve là, pas le polynôme.
#[test]
fn exp2_tient_sa_borne() {
    let mut worst = 0.0f64;
    let steps = 1 << 20;
    for i in 0..=steps {
        // Tout le domaine utile, des deux côtés de zéro.
        let x = -125.0 + 253.0 * (i as f32) / (steps as f32);
        let error = ulp_error(exp2(x), exp2_reference(f64::from(x)));
        if error > worst {
            worst = error;
        }
    }
    assert!(
        worst <= EXP2_TOLERANCE,
        "{worst} ulp, borne {EXP2_TOLERANCE}"
    );
}

/// `log2` tient sa borne, et l'erreur ne dépend que de la mantisse.
#[test]
fn log2_tient_sa_borne() {
    let mut worst = 0.0f64;
    let steps = 1 << 20;
    for i in 0..=steps {
        // Une mantisse entière, à exposant fixe : c'est elle que le polynôme
        // voit.
        let bits = 127u32 << 23 | (((i as u64 * 0x7F_FFFF) / steps as u64) as u32);
        let x = f32::from_bits(bits);
        let error = ulp_error(log2(x), log2_reference(f64::from(x)));
        if error > worst {
            worst = error;
        }
    }
    for k in -126..=127 {
        let x = f32::from_bits(((127 + k) as u32) << 23 | 0x2A_AAAA);
        let error = ulp_error(log2(x), log2_reference(f64::from(x)));
        if error > worst {
            worst = error;
        }
    }
    assert!(
        worst <= LOG2_TOLERANCE,
        "{worst} ulp, borne {LOG2_TOLERANCE}"
    );
}

/// La puissance ne perd pas un niveau après quantification sur huit bits.
///
/// C'est l'usage prévu, et la seule mesure qui compte pour une table de
/// correction : deux voies qui diffèrent d'un demi-ulp rendent le même octet,
/// deux voies qui diffèrent d'un niveau se voient sur un aplat.
#[test]
fn la_puissance_quantifiee_est_juste() {
    for gamma in 1..=30 {
        let g = gamma as f32 / 10.0;
        // Zéro à part : la référence décompose son argument par ses bits, et
        // le logarithme d'un zéro n'existe pas plus pour elle que pour la
        // fonction mesurée.
        assert_eq!(powf(0.0, g), 0.0, "zéro à la puissance {g}");
        for i in 1..=255u32 {
            let x = i as f32 / 255.0;
            let exact = exp2_reference(f64::from(g) * log2_reference(f64::from(x)));
            let got = f64::from(powf(x, g));
            let level = |v: f64| (v * 255.0 + 0.5) as i32;
            assert_eq!(
                level(got),
                level(exact),
                "({x}, {g}) : {got} contre {exact}"
            );
        }
    }
}

/// Les bits que les trois fonctions rendent sont figés.
///
/// La borne en ulp dit que le résultat est bon ; elle ne dit pas qu'il est le
/// **même** sur toutes les cibles, et c'est lui qui entrera dans les empreintes
/// de conformance par la table de post-traitement. Un coefficient retouché doit
/// changer ce test, pas une image chez quelqu'un d'autre.
#[test]
fn l_empreinte_des_fonctions_est_figee() {
    let mut bytes = Vec::new();
    for i in 0..4096u32 {
        let x = -64.0 + 128.0 * (i as f32) / 4096.0;
        bytes.extend_from_slice(&exp2(x).to_bits().to_le_bytes());

        let y = f32::from_bits(((i % 254 + 1) << 23) | ((i * 2053) % 0x80_0000));
        bytes.extend_from_slice(&log2(y).to_bits().to_le_bytes());
        bytes.extend_from_slice(&powf(i as f32 / 4096.0, 2.2).to_bits().to_le_bytes());
    }
    assert_eq!(fnv1a(bytes), FINGERPRINT);
}

/// L'empreinte de `l_empreinte_des_fonctions_est_figee`.
const FINGERPRINT: u64 = 5_895_963_650_951_908_999;
