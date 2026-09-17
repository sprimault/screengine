// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La racine inverse, sans libm ni instruction approximative.
//!
//! Le nombre se décompose exactement par ses bits : une mantisse ramenée dans
//! [1, 4) et un exposant pair. Une petite table estime la racine inverse de la
//! mantisse à moins de 1 %, deux itérations de Newton en nombre fixe l'amènent
//! sous la précision du `f32`, et l'exposant se rétablit par une puissance de
//! deux, exacte. Ni `sqrt`, que la règle ne rouvre pas fonction par fonction,
//! ni `rsqrtps` ou `vrsqrte`, dont les bits varient selon le fondeur.

/// En deçà, un carré de longueur est traité comme nul.
///
/// Le plus petit `f32` normal vaut environ 1,2·10⁻³⁸ : huit ordres de grandeur
/// de marge garantissent qu'aucun dénormal n'entre dans un calcul, quel que soit
/// le réglage DAZ/FTZ que la frontière a neutralisé.
pub const NEGLIGIBLE: f32 = 1.0e-30;

/// Sous-intervalles de la table, par moitié [1, 2) et [2, 4) de la mantisse.
const HALF: usize = 32;

/// `1/√m` au milieu de chaque sous-intervalle : les 32 premiers sur [1, 2), les
/// 32 suivants sur [2, 4).
static ESTIMATE: [f32; 2 * HALF] = estimates();

/// Calcule la table, à la compilation, par Newton en `f64` à partir d'une
/// estimation grossière : le calcul flottant constant est exact au bit près.
const fn estimates() -> [f32; 2 * HALF] {
    let mut table = [0.0f32; 2 * HALF];
    let mut i = 0;
    while i < 2 * HALF {
        let (base, width) = if i < HALF { (1.0, 1.0) } else { (2.0, 2.0) };
        let slot = (i % HALF) as f64 + 0.5;
        let m = base + width * slot / HALF as f64;
        let mut y = 0.7;
        let mut n = 0;
        while n < 40 {
            y = y * (1.5 - 0.5 * m * y * y);
            n += 1;
        }
        table[i] = y as f32;
        i += 1;
    }
    table
}

/// `1/√x`, ou zéro si `x` n'est pas un nombre fini au-dessus de
/// [`NEGLIGIBLE`] — zéro, négatif, NaN ou infini compris.
pub fn rsqrt(x: f32) -> f32 {
    // NaN d'abord et nommément : toute comparaison avec lui est fausse, et un
    // test `x <= NEGLIGIBLE` seul le laisserait passer.
    if x.is_nan() || x <= NEGLIGIBLE || x == f32::INFINITY {
        return 0.0;
    }

    let bits = x.to_bits();
    let exponent = ((bits >> 23) & 0xFF) as i32 - 127;
    let fraction = bits & 0x7F_FFFF;
    // Un exposant impair passe une unité dans la mantisse, qui tombe alors dans
    // [2, 4) : l'exposant qui reste est pair, sa moitié exacte.
    let odd = (exponent & 1) as u32;
    let mantissa = f32::from_bits(fraction | ((127 + odd) << 23));
    let even = exponent - odd as i32;

    let slot = odd as usize * HALF + (fraction >> (23 - 5)) as usize;
    let mut y = ESTIMATE[slot];
    // Newton sous forme de correction, `y + y·(½ − ½m·y²)`, et non la forme
    // usuelle `y·(3/2 − ½m·y²)` : là, `3/2 − ε` s'arrondit à l'ulp de 3/2 et
    // l'erreur finale dépasse deux ulp de `y` ; ici, le résidu est petit et
    // s'ajoute à `y` sans perdre ses bits. L'ordre des opérations est écrit une
    // fois : une variante SIMD le reproduit, elle ne le simplifie pas.
    let half = 0.5 * mantissa;
    y = y + y * (0.5 - (half * y) * y);
    y = y + y * (0.5 - (half * y) * y);

    // `even / 2` tient dans [-50, 64] pour un `x` dans l'intervalle admis : la
    // puissance de deux est un `f32` normal, et le produit est exact.
    let scale = f32::from_bits(((127 - even / 2) as u32) << 23);
    y * scale
}

#[cfg(test)]
mod tests;
