// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'une ligne unie en SSE2.
//!
//! **Quatre pixels par tour**, et c'est la largeur du test, non celle de
//! l'interpolation : les profondeurs se calculent deux par deux — un registre de
//! cent vingt-huit bits ne tient que deux `i64` —, puis se rassemblent en quatre
//! valeurs de trente-deux bits pour la comparaison et l'écriture, qui sont le
//! vrai travail.
//!
//! **C'est la mesure qui a donné cette forme.** Une première version ne
//! calculait que les profondeurs et laissait le puits tester et écrire pixel par
//! pixel : elle était **plus lente que le scalaire**, l'aller-retour par un
//! tampon intermédiaire coûtant plus que l'addition épargnée.
//!
//! # Deux pièges de SSE2, et tous deux changent le résultat
//!
//! - **La comparaison d'entiers n'existe qu'en signé**, `_mm_cmpgt_epi32`, et la
//!   profondeur est un `u32` dont le bit de poids fort est posé dès qu'on est
//!   près de la caméra. Les deux côtés se biaisent donc de `0x8000_0000`, ce qui
//!   transporte l'ordre non signé dans l'ordre signé sans rien perdre ;
//! - **l'écriture masquée n'existe pas** — `_mm_blendv_epi8` est SSE4.1 —, d'où
//!   la forme `(nouveau & masque) | (ancien & !masque)`, qui écrit toujours les
//!   quatre pixels mais n'en change que ceux dont le test a réussi.
//!
//! Aucune intrinsèque fusionnée, relâchée ni approximative : il n'y en a pas
//! ici, le chemin étant entièrement entier.

#![allow(unsafe_code)]

#[cfg(target_arch = "x86")]
use core::arch::x86 as arch;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64 as arch;

use arch::{
    __m128i, _mm_add_epi64, _mm_and_si128, _mm_andnot_si128, _mm_cmpgt_epi32, _mm_loadu_si128,
    _mm_or_si128, _mm_set_epi32, _mm_set_epi64x, _mm_set1_epi32, _mm_set1_epi64x, _mm_srli_epi64,
    _mm_storeu_si128, _mm_xor_si128,
};

use super::FlatRow;
use crate::raster::plane::GRADIENT_BITS;

/// Ce qui transporte l'ordre non signé dans l'ordre signé.
const SIGN: i32 = i32::MIN;

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

    // Deux voies d'interpolation, décalées d'un pas, qui avancent de deux par
    // demi-tour ; quatre pixels se rassemblent à partir de deux demi-tours.
    //
    // SAFETY: ces intrinsèques ne lisent ni n'écrivent de mémoire.
    let (mut low, mut high, wide) = unsafe {
        (
            _mm_set_epi64x(start.wrapping_add(step), start),
            _mm_set_epi64x(
                start.wrapping_add(step.wrapping_mul(3)),
                start.wrapping_add(step.wrapping_mul(2)),
            ),
            _mm_set1_epi64x(step.wrapping_mul(4)),
        )
    };
    // SAFETY: même garantie.
    let (fill_v, sign_v) = unsafe { (_mm_set1_epi32(fill as i32), _mm_set1_epi32(SIGN)) };

    let mut i = 0;
    while i + 4 <= count {
        // SAFETY: décalages et additions sans accès mémoire ; les deux
        // rassemblements prennent la moitié basse de chaque voie, qui porte la
        // profondeur après décalage.
        let zs = unsafe {
            let a = _mm_srli_epi64::<{ GRADIENT_BITS as i32 }>(low);
            let b = _mm_srli_epi64::<{ GRADIENT_BITS as i32 }>(high);
            let mut lanes = [0u64; 2];
            let mut upper = [0u64; 2];
            _mm_storeu_si128(lanes.as_mut_ptr().cast::<__m128i>(), a);
            _mm_storeu_si128(upper.as_mut_ptr().cast::<__m128i>(), b);
            _mm_set_epi32(
                upper[1] as i32,
                upper[0] as i32,
                lanes[1] as i32,
                lanes[0] as i32,
            )
        };

        // SAFETY: `_mm_loadu_si128` lit seize octets **non alignés**, et les
        // quatre `u32` de `depth[i..i + 4]` les portent — la boucle garantit
        // `i + 4 <= count`. Jamais `_mm_load_si128`, dont l'alignement n'est
        // pas garanti par une tranche.
        let old = unsafe { _mm_loadu_si128(depth[i..].as_ptr().cast::<__m128i>()) };

        // Le test du puits est `z > profondeur`, **non signé**. SSE2 ne compare
        // qu'en signé, d'où le biais appliqué aux deux côtés : il préserve
        // l'ordre, et c'est exactement ce qu'on lui demande.
        //
        // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
        let (kept_depth, kept_color) = unsafe {
            let mask = _mm_cmpgt_epi32(_mm_xor_si128(zs, sign_v), _mm_xor_si128(old, sign_v));
            let seen = _mm_loadu_si128(color[i..].as_ptr().cast::<__m128i>());
            (
                _mm_or_si128(_mm_and_si128(mask, zs), _mm_andnot_si128(mask, old)),
                _mm_or_si128(_mm_and_si128(mask, fill_v), _mm_andnot_si128(mask, seen)),
            )
        };

        // SAFETY: les deux tranches portent au moins quatre `u32` à partir de
        // `i`, et l'écriture est non alignée pour la raison déjà dite.
        unsafe {
            _mm_storeu_si128(depth[i..].as_mut_ptr().cast::<__m128i>(), kept_depth);
            _mm_storeu_si128(color[i..].as_mut_ptr().cast::<__m128i>(), kept_color);
        }

        // SAFETY: additions enveloppantes sur deux voies, sans accès mémoire.
        unsafe {
            low = _mm_add_epi64(low, wide);
            high = _mm_add_epi64(high, wide);
        }
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
