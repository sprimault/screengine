// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'une ligne unie en `simd128`.
//!
//! **Quatre pixels par tour**, comme SSE2 et NEON : un registre de cent
//! vingt-huit bits ne tient que deux `i64`, donc les profondeurs se calculent
//! deux par deux, puis se rassemblent en quatre valeurs de trente-deux bits
//! pour la comparaison et l'écriture, qui sont le vrai travail.
//!
//! # Plus proche de NEON que de SSE2
//!
//! Les deux pièges de SSE2 n'en sont pas ici non plus : `u32x4_gt` compare en
//! non signé, et `v128_bitselect` sélectionne par bits. Et le rassemblement se
//! fait en registre, par un brassage qui prend les quatre moitiés basses d'un
//! coup — là où SSE2 passe par un tampon de pile.
//!
//! # Ce que `simd128` a de particulier
//!
//! **Il se décide à la compilation, et personne ne peut l'interroger.** Un
//! module compilé avec ce jeu ne se **charge** pas là où il manque : la
//! validation du module échoue avant la première instruction, donc il n'existe
//! aucun repli à l'exécution — ni par `scg_set_simd`, ni par rien d'autre.
//! C'est pourquoi l'activer est un choix de compatibilité du module publié, et
//! non une optimisation locale ; `docs/construction.md` porte le plancher que
//! cela fixe.
//!
//! **Aucune intrinsèque relâchée.** La proposition `relaxed-simd` offre des
//! formes moins chères dont le résultat dépend de la machine, ce qui est
//! exactement ce que le déterminisme du projet refuse. Rien ici n'en vient, et
//! le chemin est de toute façon entièrement entier.

#![allow(unsafe_code)]

use core::arch::wasm32::{
    i64x2, i64x2_add, i64x2_shr, i64x2_splat, u32x4_gt, u32x4_shuffle, u32x4_splat, v128,
    v128_bitselect, v128_load, v128_store,
};

use super::FlatRow;
use crate::raster::plane::GRADIENT_BITS;

/// Remplit une ligne unie : profondeurs, test strict, écriture masquée.
///
/// **Le résultat est celui du chemin scalaire, au bit près.** Le test est
/// strict — à profondeur égale, le triangle soumis le premier reste —, et c'est
/// ce qui rend l'égalité indépendante du découpage en tuiles.
pub fn fill_flat_row(row: FlatRow<'_>) {
    let FlatRow {
        color,
        depth,
        start,
        step,
        fill,
    } = row;
    let count = depth.len();

    let lane = |k: i64| start.wrapping_add(step.wrapping_mul(k));
    // Deux voies d'interpolation, décalées de deux pas, qui avancent de quatre
    // par tour. `i64x2` se construit depuis les valeurs, sans passer par la
    // mémoire comme le fait le chargement de NEON.
    let mut low = i64x2(lane(0), lane(1));
    let mut high = i64x2(lane(2), lane(3));
    let wide = i64x2_splat(step.wrapping_mul(4));
    let fill_v = u32x4_splat(fill);

    let mut i = 0;
    while i + 4 <= count {
        // Le décalage est **arithmétique**, comme celui du scalaire sur un
        // `i64` ; les trente-deux bits bas seraient les mêmes en logique, un
        // gradient restant sous 2⁶².
        //
        // Le brassage prend les voies 0 et 2 de chaque vecteur, c'est-à-dire la
        // moitié basse de chacun des quatre `i64` : les indices 0 à 3 désignent
        // le premier opérande, 4 à 7 le second.
        let shifted_low = i64x2_shr(low, GRADIENT_BITS);
        let shifted_high = i64x2_shr(high, GRADIENT_BITS);
        let zs = u32x4_shuffle::<0, 2, 4, 6>(shifted_low, shifted_high);

        // SAFETY: les deux chargements lisent seize octets **non alignés**, et
        // les quatre `u32` à partir de `i` les portent — la boucle garantit
        // `i + 4 <= count`. `v128_load` n'exige aucun alignement, ce qu'une
        // tranche ne garantirait pas. Les écritures rendent aux mêmes adresses.
        unsafe {
            let old = v128_load(depth[i..].as_ptr().cast::<v128>());
            let seen = v128_load(color[i..].as_ptr().cast::<v128>());
            // Le test du puits est `z > profondeur`, non signé, et c'est ce que
            // `u32x4_gt` compare. `v128_bitselect` prend son premier opérande
            // là où le masque a ses bits posés.
            let mask = u32x4_gt(zs, old);
            v128_store(
                depth[i..].as_mut_ptr().cast::<v128>(),
                v128_bitselect(zs, old, mask),
            );
            v128_store(
                color[i..].as_mut_ptr().cast::<v128>(),
                v128_bitselect(fill_v, seen, mask),
            );
        }

        low = i64x2_add(low, wide);
        high = i64x2_add(high, wide);
        i += 4;
    }

    // Le reste, en scalaire et dans les mêmes termes que la référence : écrire
    // quatre pixels là où moins sont attendus déborderait des tranches.
    let mut z = start.wrapping_add(step.wrapping_mul(i as i64));
    while i < count {
        let value = (z >> GRADIENT_BITS) as u32;
        if value > depth[i] {
            depth[i] = value;
            color[i] = fill;
        }
        z = z.wrapping_add(step);
        i += 1;
    }
}

#[cfg(test)]
mod tests;
