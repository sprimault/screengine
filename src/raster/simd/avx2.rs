// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'une ligne unie en AVX2.
//!
//! **Quatre pixels par tour**, là où SSE2 en fait deux : un registre de deux
//! cent cinquante-six bits tient quatre profondeurs `i64`. Rien d'autre ne
//! change — même accumulation enveloppante, même décalage logique, même ordre.
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
//!   qu'à un seul endroit : [`super::span_depths`]. Un second appelant serait un
//!   second endroit où l'oublier.
//!
//! **Le danger qu'elle couvre est réel et muet** : une instruction AVX2 exécutée
//! sur un processeur qui ne la porte pas lève une instruction illégale, et sur
//! un système qui ne sauvegarde pas les registres larges, elle rend des valeurs
//! fausses par intermittence sans rien lever du tout. C'est ce second cas que
//! `has_avx2` interroge en trois temps plutôt qu'un.

#![allow(unsafe_code)]

#[cfg(target_arch = "x86")]
use core::arch::x86 as arch;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64 as arch;

use arch::{
    __m256i, _mm256_add_epi64, _mm256_set_epi64x, _mm256_set1_epi64x, _mm256_srli_epi64,
    _mm256_storeu_si256,
};

use crate::raster::plane::GRADIENT_BITS;

/// Les profondeurs d'une ligne, calculées quatre par quatre.
///
/// Même contrat que [`super::sse2::depths`] : `out` ressort porteur d'exactement
/// ce que le chemin scalaire aurait écrit, au bit près. Quatre voies parallèles
/// qui avancent chacune de quatre pas donnent les mêmes valeurs qu'une voie qui
/// en avance d'un, l'addition entière étant associative.
///
/// # Safety
///
/// Le processeur **et le système** doivent porter AVX2, ce que
/// [`super::x86::has_avx2`] établit en trois temps — le jeu d'instructions, la
/// lisibilité de `XGETBV`, et la sauvegarde des états `XMM` et `YMM`. Appelée
/// sans cette garantie, elle lève une instruction illégale, ou pire, calcule
/// faux sans rien signaler.
/// Les profondeurs d'une ligne, **si cette machine porte AVX2**.
///
/// Rend faux sans rien écrire quand elle ne le porte pas, à l'appelant de
/// retomber sur la référence.
///
/// **La vérification et l'appel vivent ici, dans le module qui porte le code
/// dangereux**, et non chez l'appelant : une précondition dont la garantie est
/// à l'étage au-dessus se perd au second appelant. C'est aussi ce qui laisse
/// `simd/mod.rs` entièrement sûr, sous le `deny(unsafe_code)` du crate.
///
/// **Elle est refaite à chaque appel plutôt que déduite du réglage.** Le
/// contexte a bien refusé `SCG_SIMD_AVX2` sur une machine qui ne le porte pas,
/// mais ce module n'a aucun moyen de le savoir, et une garantie qui vient
/// d'ailleurs n'en est pas une pour du code `unsafe`.
pub fn depths_if_available(depth: i64, depth_x: i64, out: &mut [u32]) -> bool {
    if !super::x86::has_avx2() {
        return false;
    }
    // SAFETY: `has_avx2` vient de répondre vrai, ce qui est exactement la
    // précondition de `depths` — jeu d'instructions présent, et système qui
    // sauvegarde les états `XMM` et `YMM`.
    unsafe { depths(depth, depth_x, out) };
    true
}

// **Même clause que le `cpuid`, et même raison** : à l'intérieur d'une fonction
// qui active `avx2`, les intrinsèques du jeu sont sûres sur une chaîne récente
// et `unsafe` sur le plancher déclaré. Sans ces blocs, `make msrv` ne compile
// plus ; avec eux, une chaîne récente les signale inutiles. `allow` et non
// `expect`, qui rougirait sur le plancher où ils sont bel et bien nécessaires.
//
// **À retirer le jour où `make msrv` passe sans eux**, condition qui se vérifie
// mécaniquement plutôt que de se chercher dans des notes de version. Seul
// `_mm256_storeu_si256` restera `unsafe` partout : il écrit par pointeur brut,
// ce qu'aucune fonctionnalité de cible ne rend sûr.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
pub unsafe fn depths(depth: i64, depth_x: i64, out: &mut [u32]) {
    let count = out.len();
    if count == 0 {
        return;
    }

    // Quatre voies, décalées d'un pas chacune ; le tour entier en avance de
    // quatre. `set_epi64x` range son premier argument dans la voie de poids
    // fort, d'où l'ordre inverse.
    //
    // SAFETY: ces intrinsèques ne touchent aucune mémoire ; la précondition de
    // la fonction couvre la disponibilité du jeu d'instructions.
    let mut quad = unsafe {
        _mm256_set_epi64x(
            depth.wrapping_add(depth_x.wrapping_mul(3)),
            depth.wrapping_add(depth_x.wrapping_mul(2)),
            depth.wrapping_add(depth_x),
            depth,
        )
    };
    // SAFETY: même garantie.
    let step = unsafe { _mm256_set1_epi64x(depth_x.wrapping_mul(4)) };

    let mut i = 0;
    // Le reste se traite après la boucle : écrire quatre valeurs là où moins
    // sont attendues déborderait de `out`.
    while i + 4 <= count {
        // SAFETY: le décalage et l'addition ne touchent pas la mémoire ;
        // l'écriture est **non alignée** et vise `scratch`, trente-deux octets
        // de pile dont l'alignement n'est pas garanti — jamais
        // `_mm256_store_si256`, qui l'exigerait.
        let shifted = unsafe { _mm256_srli_epi64::<{ GRADIENT_BITS as i32 }>(quad) };
        let mut scratch = [0u64; 4];
        // SAFETY: `scratch` porte trente-deux octets inscriptibles, ce que
        // l'écriture non alignée demande en tout et pour tout.
        unsafe { _mm256_storeu_si256(scratch.as_mut_ptr().cast::<__m256i>(), shifted) };
        for lane in 0..4 {
            out[i + lane] = scratch[lane] as u32;
        }
        // SAFETY: addition enveloppante sur quatre voies, sans accès mémoire.
        quad = unsafe { _mm256_add_epi64(quad, step) };
        i += 4;
    }

    if i < count {
        // Les voies portent déjà les profondeurs des pixels `i` à `i + 3` : on
        // les reprend plutôt que de les reconstruire depuis `depth`, ce qui
        // réintroduirait une multiplication là où le scalaire n'a que des
        // additions — et donc une occasion de diverger.
        let mut scratch = [0u64; 4];
        // SAFETY: même garantie que plus haut.
        unsafe { _mm256_storeu_si256(scratch.as_mut_ptr().cast::<__m256i>(), quad) };
        for (lane, slot) in out[i..].iter_mut().enumerate() {
            *slot = (scratch[lane] >> GRADIENT_BITS) as u32;
        }
    }
}

#[cfg(test)]
mod tests;
