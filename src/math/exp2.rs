// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `2^x`, `log2 x` et la puissance qui s'en déduit, sans libm.
//!
//! Les trois suivent la méthode de la racine inverse : le nombre se décompose
//! par ses bits, ce qui reste tient dans un intervalle étroit où un polynôme
//! suffit, et la puissance de deux se rétablit exactement.
//!
//! **Aucune table.** Elle économiserait trois multiplications sur un calcul qui
//! tourne quelques centaines de fois au changement d'un réglage, jamais par
//! pixel, et coûterait une lecture mémoire, un indice et une seconde constante
//! à figer. Le polynôme seul atteint le demi-ulp.
//!
//! **Les polynômes s'évaluent en `f64`, et le calcul reste en `f64` de bout en
//! bout.** C'est permis ici et nulle part dans une image : hors image, un `f64`
//! est aussi déterministe qu'un `f32` — IEEE 754 impose les quatre opérations
//! au bit près —, et l'arrondi intermédiaire qu'il évite dans la puissance
//! dominerait l'erreur devant les deux polynômes réunis.

/// Coefficients de `2^r` sur `[-½, ½]`, minimax relatif de degré six.
///
/// Écartée : la série de Taylor du même degré, dont l'erreur est trois ordres
/// de grandeur plus grande pour exactement le même coût d'évaluation. Elle
/// garde son emploi dans les tests, où elle sert de référence sur un intervalle
/// que des carrés successifs ont réduit.
const EXP2_COEFFICIENTS: [f64; 7] = [
    1.0000000005541665,
    0.6931472057372677,
    0.24022646890634397,
    0.05550328776965264,
    0.009618488957089626,
    0.0013399931219164655,
    0.00015345812009089318,
];

/// Coefficients de `log2(1 + t)/t` sur `[√2/2 - 1, √2 - 1]`, minimax relatif de
/// degré huit.
///
/// C'est le quotient qui est approché, jamais le logarithme lui-même : la forme
/// factorisée `t·Q(t)` rend exactement zéro en `t = 0`, là où la forme
/// développée laisserait un résidu — et l'erreur relative y exploserait, `log2`
/// s'annulant en `x = 1`.
const LOG2_COEFFICIENTS: [f64; 9] = [
    1.4426950036524326,
    -0.721347346801592,
    0.48091064294107194,
    -0.36070368294257527,
    0.2879162483345373,
    -0.2389448187561814,
    0.21571560120145025,
    -0.20726976182338325,
    0.12583705125169542,
];

/// Au-delà, `2^x` déborde le `f32`.
const EXP2_OVERFLOW: f64 = 128.0;

/// En deçà, `2^x` tombe dans les dénormaux ou sous eux.
///
/// La borne est celle du plus petit `f32` **normal**, et non celle du plus petit
/// dénormal : un dénormal rendu ici traverserait un environnement flottant dont
/// le moteur ne fixe le réglage qu'à ses points d'entrée, et sa valeur
/// dépendrait du bit DAZ de l'hôte. Zéro ne dépend de personne.
const EXP2_UNDERFLOW: f64 = -126.0;

/// La fraction de mantisse au-delà de laquelle on bascule sur l'intervalle
/// haut : `√2 - 1`, lu en bits.
///
/// Centrer la mantisse sur `√2` plutôt que de la laisser dans `[1, 2)` divise
/// par deux l'étendue de `t`, donc le degré qu'il faut à précision égale.
const SQRT2_FRACTION: u32 = 0x35_04F3;

/// `2^x` en `f64`, le cœur que les trois fonctions publiques partagent.
fn exp2_raw(x: f64) -> f64 {
    // NaN d'abord et nommément : toute comparaison avec lui est fausse, et les
    // deux bornes ci-dessous le laisseraient filer jusqu'à la conversion.
    if x.is_nan() {
        return 0.0;
    }
    if x >= EXP2_OVERFLOW {
        return f64::from(f32::INFINITY);
    }
    if x <= EXP2_UNDERFLOW {
        return 0.0;
    }

    // `as` tronque vers zéro : le demi ajouté au signe de `x` donne l'arrondi
    // au plus proche. Pas de `round`, qui n'existe pas dans `core` et que
    // `clippy.toml` refuse de toute façon — l'arrondi s'écrit sur la
    // conversion, et le domaine est borné juste au-dessus.
    let half = if x < 0.0 { -0.5 } else { 0.5 };
    let k = (x + half) as i32;
    // Exact : `k` est le plus proche entier de `x`, donc `|x - k| ≤ ½`, et la
    // soustraction de deux nombres voisins ne perd aucun bit.
    let r = x - f64::from(k);

    let mut sum = EXP2_COEFFICIENTS[6];
    let mut i = 5;
    loop {
        sum = sum * r + EXP2_COEFFICIENTS[i];
        if i == 0 {
            break;
        }
        i -= 1;
    }

    // `k` tient dans `[-126, 127]`, les deux bornes ci-dessus s'en chargeant :
    // la puissance de deux est un `f32` normal, et le produit est exact.
    sum * f64::from(f32::from_bits(((127 + k) as u32) << 23))
}

/// `log2 x` en `f64`, pour un `x` dont l'appelant a déjà écarté les cas sans
/// logarithme réel fini.
fn log2_raw(x: f32) -> f64 {
    let bits = x.to_bits();
    let mut exponent = ((bits >> 23) & 0xFF) as i32 - 127;
    let mut fraction = bits & 0x7F_FFFF;

    // Un dénormal n'a pas d'exposant utilisable : on le remonte d'un facteur
    // connu et on retranche l'exposant correspondant. Sans cela sa mantisse
    // serait lue comme celle d'un nombre normal, et le résultat serait faux
    // d'un ordre de grandeur sans que rien ne le signale.
    if exponent == -127 {
        let scaled = (x * 16_777_216.0).to_bits();
        exponent = ((scaled >> 23) & 0xFF) as i32 - 127 - 24;
        fraction = scaled & 0x7F_FFFF;
    }

    // Mantisse centrée sur `√2` : au-delà, on la divise par deux et l'exposant
    // reprend l'unité.
    let high = fraction >= SQRT2_FRACTION;
    if high {
        exponent += 1;
    }
    let biased = if high { 126 } else { 127 };
    let t = f64::from(f32::from_bits(fraction | (biased << 23))) - 1.0;

    let mut sum = LOG2_COEFFICIENTS[8];
    let mut i = 7;
    loop {
        sum = sum * t + LOG2_COEFFICIENTS[i];
        if i == 0 {
            break;
        }
        i -= 1;
    }

    t * sum + f64::from(exponent)
}

/// `2^x`, à moins de 0,7 ulp.
///
/// Rend zéro pour un NaN — comme la racine inverse, et pour la même raison :
/// les fonctions de ce module refusent le NaN à l'entrée plutôt que de le
/// propager. Sature à l'infini au-dessus de 128, à zéro sous -126.
pub fn exp2(x: f32) -> f32 {
    exp2_raw(f64::from(x)) as f32
}

/// `log2 x`, à moins de 1,2 ulp.
///
/// Rend zéro pour tout ce qui n'a pas de logarithme réel fini — zéro, négatif,
/// NaN, infini —, et zéro exactement pour `x = 1`.
pub fn log2(x: f32) -> f32 {
    if x.is_nan() || x <= 0.0 || x == f32::INFINITY {
        return 0.0;
    }
    log2_raw(x) as f32
}

/// `x^g`, à moins de 0,8 ulp.
///
/// Le calcul ne repasse pas par le `f32` entre le logarithme et
/// l'exponentielle : cet arrondi-là dominerait l'erreur devant les deux
/// polynômes réunis, et se paierait d'un niveau entier après quantification sur
/// huit bits — ce qui est précisément l'usage prévu.
///
/// Rend un pour `g = 0`, zéro pour un `x` nul ou négatif, zéro pour un NaN.
pub fn powf(x: f32, g: f32) -> f32 {
    if x.is_nan() || g.is_nan() {
        return 0.0;
    }
    if g == 0.0 {
        return 1.0;
    }
    if x <= 0.0 || x == f32::INFINITY {
        return 0.0;
    }
    exp2_raw(f64::from(g) * log2_raw(x)) as f32
}

#[cfg(test)]
mod tests;
