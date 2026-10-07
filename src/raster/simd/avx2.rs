// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'une ligne unie en AVX2.
//!
//! **Huit pixels par tour**, là où SSE2 en fait quatre : les profondeurs se
//! calculent quatre par quatre — un registre de deux cent cinquante-six bits
//! tient quatre `i64` —, puis se rassemblent en huit valeurs de trente-deux bits
//! pour la comparaison et l'écriture.
//!
//! **Les deux pièges de SSE2 ne sont pas ceux d'ici.** AVX2 compare toujours en
//! signé, `_mm256_cmpgt_epi32`, donc le biais de `0x8000_0000` reste nécessaire ;
//! mais il porte `_mm256_blendv_epi8`, si bien que l'écriture masquée s'écrit
//! directement au lieu de passer par un et-ou-non.
//!
//! # Ce qui sépare ce module de son voisin SSE2
//!
//! **AVX2 n'est pas dans la base de `x86_64`**, quand SSE2 y est. Trois
//! conséquences, et elles donnent sa forme à tout ce fichier :
//!
//! - le module se compile **toujours** sur x86, sans `cfg(target_feature)` : on
//!   ne peut pas demander au compilateur ce que seule l'exécution sait ;
//! - la fonction porte `#[target_feature(enable = "avx2")]`, ce qui la rend
//!   `unsafe` à appeler — c'est le compilateur qui refuse de croire sur parole
//!   qu'un processeur porte ce jeu ;
//! - sa précondition est donc [`super::x86::has_avx2`], et elle n'est tenue
//!   qu'à un seul endroit : [`fill_flat_row_if_available`].
//!
//! **Le danger qu'elle couvre est réel et muet** : une instruction AVX2 exécutée
//! sur un processeur qui ne la porte pas lève une instruction illégale, et sur
//! un système qui ne sauvegarde pas les registres larges, elle rend des valeurs
//! fausses par intermittence sans rien lever du tout.

#![allow(unsafe_code)]

#[cfg(target_arch = "x86")]
use core::arch::x86 as arch;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64 as arch;

use arch::{
    __m256i, _mm256_add_epi64, _mm256_blendv_epi8, _mm256_cmpgt_epi32, _mm256_loadu_si256,
    _mm256_set_epi32, _mm256_set_epi64x, _mm256_set1_epi32, _mm256_set1_epi64x, _mm256_srli_epi64,
    _mm256_storeu_si256, _mm256_xor_si256,
};

use super::FlatRow;
use crate::raster::plane::GRADIENT_BITS;

/// Ce qui transporte l'ordre non signé dans l'ordre signé.
const SIGN: i32 = i32::MIN;

/// Remplit une ligne unie, **si cette machine porte AVX2**.
///
/// Rend faux sans rien écrire quand elle ne le porte pas, à l'appelant de
/// retomber sur la référence.
///
/// **La vérification et l'appel vivent ici, dans le module qui porte le code
/// dangereux**, et non chez l'appelant : une précondition dont la garantie est à
/// l'étage au-dessus se perd au second appelant. C'est aussi ce qui laisse
/// `simd/mod.rs` entièrement sûr, sous le `deny(unsafe_code)` du crate.
pub fn fill_flat_row_if_available(row: FlatRow<'_>) -> bool {
    if !super::x86::has_avx2() {
        return false;
    }
    // SAFETY: `has_avx2` vient de répondre vrai, ce qui est exactement la
    // précondition de `fill_flat_row` — jeu d'instructions présent, et système
    // qui sauvegarde les états `XMM` et `YMM`.
    unsafe { fill_flat_row(row) };
    true
}

/// Remplit une ligne unie : profondeurs, test strict, écriture masquée.
///
/// **Le résultat est celui du chemin scalaire, au bit près.** Le test est
/// strict — à profondeur égale, le triangle soumis le premier reste —, et c'est
/// ce qui rend l'égalité indépendante du découpage en tuiles.
///
/// # Safety
///
/// Le processeur **et le système** doivent porter AVX2, ce que
/// [`super::x86::has_avx2`] établit en trois temps — le jeu d'instructions, la
/// lisibilité de `XGETBV`, et la sauvegarde des états `XMM` et `YMM`.
// **Même clause que le `cpuid`, et même raison** : à l'intérieur d'une fonction
// qui active `avx2`, les intrinsèques du jeu sont sûres sur une chaîne récente
// et `unsafe` sur le plancher déclaré. `allow` et non `expect`, qui rougirait sur
// le plancher. À retirer le jour où `make msrv` passe sans eux.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
pub unsafe fn fill_flat_row(row: FlatRow<'_>) {
    let FlatRow {
        color,
        depth,
        start,
        step,
        fill,
    } = row;
    let count = depth.len();

    // Deux groupes de quatre voies, décalés de quatre pas, qui avancent de huit
    // par tour ; huit pixels se rassemblent à partir des deux.
    //
    // SAFETY: ces intrinsèques ne lisent ni n'écrivent de mémoire.
    let (mut low, mut high, wide) = unsafe {
        let lane = |k: i64| start.wrapping_add(step.wrapping_mul(k));
        (
            _mm256_set_epi64x(lane(3), lane(2), lane(1), lane(0)),
            _mm256_set_epi64x(lane(7), lane(6), lane(5), lane(4)),
            _mm256_set1_epi64x(step.wrapping_mul(8)),
        )
    };
    // SAFETY: même garantie.
    let (fill_v, sign_v) = unsafe { (_mm256_set1_epi32(fill as i32), _mm256_set1_epi32(SIGN)) };

    let mut i = 0;
    while i + 8 <= count {
        // SAFETY: décalages sans accès mémoire ; les deux rassemblements
        // prennent la moitié basse de chaque voie, qui porte la profondeur après
        // décalage.
        let zs = unsafe {
            let a = _mm256_srli_epi64::<{ GRADIENT_BITS as i32 }>(low);
            let b = _mm256_srli_epi64::<{ GRADIENT_BITS as i32 }>(high);
            let mut first = [0u64; 4];
            let mut second = [0u64; 4];
            _mm256_storeu_si256(first.as_mut_ptr().cast::<__m256i>(), a);
            _mm256_storeu_si256(second.as_mut_ptr().cast::<__m256i>(), b);
            _mm256_set_epi32(
                second[3] as i32,
                second[2] as i32,
                second[1] as i32,
                second[0] as i32,
                first[3] as i32,
                first[2] as i32,
                first[1] as i32,
                first[0] as i32,
            )
        };

        // SAFETY: les deux chargements lisent trente-deux octets **non
        // alignés**, et les huit `u32` à partir de `i` les portent — la boucle
        // garantit `i + 8 <= count`. Jamais la forme alignée, qu'une tranche ne
        // garantit pas.
        //
        // Le test du puits est `z > profondeur`, **non signé** ; AVX2 ne compare
        // qu'en signé, d'où le biais appliqué aux deux côtés. `blendv` prend le
        // second opérande là où le masque est posé.
        unsafe {
            let old = _mm256_loadu_si256(depth[i..].as_ptr().cast::<__m256i>());
            let seen = _mm256_loadu_si256(color[i..].as_ptr().cast::<__m256i>());
            let mask =
                _mm256_cmpgt_epi32(_mm256_xor_si256(zs, sign_v), _mm256_xor_si256(old, sign_v));
            _mm256_storeu_si256(
                depth[i..].as_mut_ptr().cast::<__m256i>(),
                _mm256_blendv_epi8(old, zs, mask),
            );
            _mm256_storeu_si256(
                color[i..].as_mut_ptr().cast::<__m256i>(),
                _mm256_blendv_epi8(seen, fill_v, mask),
            );
        }

        // SAFETY: additions enveloppantes sur quatre voies, sans accès mémoire.
        unsafe {
            low = _mm256_add_epi64(low, wide);
            high = _mm256_add_epi64(high, wide);
        }
        i += 8;
    }

    // Le reste, en scalaire et dans les mêmes termes que la référence : écrire
    // huit pixels là où moins sont attendus déborderait des tranches.
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
