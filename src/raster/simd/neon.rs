// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'une ligne unie en NEON.
//!
//! **Quatre pixels par tour**, comme SSE2 : un registre de cent vingt-huit bits
//! ne tient que deux `i64`, donc les profondeurs se calculent deux par deux,
//! puis se rassemblent en quatre valeurs de trente-deux bits pour la
//! comparaison et l'écriture, qui sont le vrai travail.
//!
//! # Les deux pièges de SSE2 n'en sont pas ici
//!
//! C'est tout ce qui sépare ce fichier de son voisin, et il est plus court pour
//! cette seule raison :
//!
//! - **la comparaison non signée existe**, `vcgtq_u32`, et la profondeur est un
//!   `u32` dont le bit de poids fort est posé près de la caméra. Pas de biais de
//!   `0x8000_0000` à appliquer aux deux côtés ;
//! - **la sélection par bits existe**, `vbslq_u32`, là où SSE2 écrit
//!   `(nouveau & masque) | (ancien & !masque)` faute de mélange avant SSE4.1.
//!
//! **Et le rassemblement se fait en registre**, par `vmovn_u64` qui rétrécit
//! deux `u64` en deux `u32` puis `vcombine_u32` qui les assemble. SSE2 passe là
//! par un tampon de pile, n'ayant pas d'instruction de rétrécissement.
//!
//! # Ni détection, ni `target_feature`
//!
//! **NEON est dans la base d'`aarch64`**, comme SSE2 dans celle de `x86_64` :
//! le `cfg` du module suffit, et aucune fonction n'a à activer un jeu que la
//! cible porte déjà. C'est AVX2 qui est l'exception du dépôt, pas l'inverse.
//!
//! **ARM 32 bits reste dehors**, ses fonctionnalités étant instables sur chaîne
//! stable : `armv7` retombe donc sur le scalaire, et c'est ce que `conform-arm`
//! y joue.
//!
//! Aucune intrinsèque fusionnée, relâchée ni approximative : il n'y en a pas
//! ici, le chemin étant entièrement entier.

#![allow(unsafe_code)]

use core::arch::aarch64::{
    vaddq_s64, vbslq_u32, vcgtq_u32, vcombine_u32, vdupq_n_s64, vdupq_n_u32, vld1q_s64, vld1q_u32,
    vmovn_u64, vreinterpretq_u64_s64, vshrq_n_s64, vst1q_u32,
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
    let first = [lane(0), lane(1)];
    let second = [lane(2), lane(3)];

    // Deux voies d'interpolation, décalées de deux pas, qui avancent de deux par
    // demi-tour ; quatre pixels se rassemblent à partir de deux demi-tours.
    //
    // SAFETY: les deux chargements lisent seize octets d'un tableau de deux
    // `i64`, qui les porte ; `vdupq_n_s64` ne touche pas la mémoire.
    let (mut low, mut high, wide) = unsafe {
        (
            vld1q_s64(first.as_ptr()),
            vld1q_s64(second.as_ptr()),
            vdupq_n_s64(step.wrapping_mul(4)),
        )
    };
    // SAFETY: aucun accès mémoire.
    let fill_v = unsafe { vdupq_n_u32(fill) };

    let mut i = 0;
    while i + 4 <= count {
        // Le décalage est **arithmétique**, comme celui du scalaire sur un
        // `i64` ; le logique rendrait les mêmes trente-deux bits bas, un
        // gradient restant sous 2⁶² et le décalage ne différant qu'au-delà du
        // bit cinquante-deux. C'est pourquoi SSE2 s'en tire avec `srli`.
        //
        // SAFETY: décalages et rétrécissements sans accès mémoire.
        let zs = unsafe {
            let a = vshrq_n_s64::<{ GRADIENT_BITS as i32 }>(low);
            let b = vshrq_n_s64::<{ GRADIENT_BITS as i32 }>(high);
            vcombine_u32(
                vmovn_u64(vreinterpretq_u64_s64(a)),
                vmovn_u64(vreinterpretq_u64_s64(b)),
            )
        };

        // SAFETY: les deux chargements lisent seize octets, et les quatre `u32`
        // à partir de `i` les portent — la boucle garantit `i + 4 <= count`.
        // Les écritures les rendent aux mêmes adresses. NEON n'exige aucun
        // alignement de ces formes, ce qu'une tranche ne garantirait pas.
        //
        // Le test du puits est `z > profondeur`, non signé, et c'est exactement
        // ce que `vcgtq_u32` compare. `vbslq_u32` prend son deuxième opérande là
        // où le masque a ses bits posés.
        unsafe {
            let old = vld1q_u32(depth[i..].as_ptr());
            let seen = vld1q_u32(color[i..].as_ptr());
            let mask = vcgtq_u32(zs, old);
            vst1q_u32(depth[i..].as_mut_ptr(), vbslq_u32(mask, zs, old));
            vst1q_u32(color[i..].as_mut_ptr(), vbslq_u32(mask, fill_v, seen));
        }

        // SAFETY: additions enveloppantes sur deux voies, sans accès mémoire.
        unsafe {
            low = vaddq_s64(low, wide);
            high = vaddq_s64(high, wide);
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
