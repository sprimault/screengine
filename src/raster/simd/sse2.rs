// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'une ligne unie en SSE2.
//!
//! **Deux pixels par tour, et non quatre.** La profondeur interpolée est un
//! `i64` avant son décalage de gradient : un registre de cent vingt-huit bits
//! n'en tient que deux. La ramener en trente-deux bits plus tôt donnerait quatre
//! pixels par tour et **d'autres bits** que le scalaire, ce qui est la seule
//! chose que cette variante n'a pas le droit de faire.
//!
//! **Rien ici ne décide de ce qui est écrit**, et c'est ce qui rend la variante
//! vérifiable : elle reproduit l'expression du scalaire, opération pour
//! opération, sur deux pixels à la fois. Le reste du remplissage — mise en place
//! du triangle, fonctions de bord, parcours des lignes — lui est commun.
//!
//! Aucune intrinsèque fusionnée, relâchée ni approximative : il n'y en a pas
//! ici, le chemin étant entièrement entier.

#![allow(unsafe_code)]

#[cfg(target_arch = "x86")]
use core::arch::x86 as arch;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64 as arch;

use arch::{
    __m128i, _mm_add_epi64, _mm_set_epi64x, _mm_set1_epi64x, _mm_srli_epi64, _mm_storeu_si128,
};

use crate::raster::plane::GRADIENT_BITS;

/// Les profondeurs d'une ligne, calculées deux par deux.
///
/// Écrites dans `out`, qui porte exactement `count` valeurs. L'appelant les
/// consomme ensuite pixel par pixel : ce qui se vectorise est leur **calcul**,
/// pas le test ni l'écriture, qui lisent et écrivent un tampon dont ce module ne
/// connaît ni la disposition ni les bornes.
///
/// **Le résultat est celui du scalaire, au bit près**, et c'est vérifiable ligne
/// à ligne : l'accumulation est la même addition enveloppante, dans le même
/// ordre, et le décalage le même décalage logique. Deux voies parallèles qui
/// avancent chacune de deux pas donnent les mêmes valeurs qu'une voie qui en
/// avance d'un, l'addition entière étant associative.
///
/// # Safety
///
/// Rien n'est exigé de l'appelant : les écritures passent par `out`, dont la
/// longueur borne la boucle, et les chargements sont non alignés.
pub fn depths(depth: i64, depth_x: i64, out: &mut [u32]) {
    let count = out.len();
    if count == 0 {
        return;
    }

    // Deux voies : la première part de `depth`, la seconde un pas plus loin, et
    // chacune avance de deux pas par tour.
    //
    // SAFETY: `_mm_set_epi64x` et `_mm_set1_epi64x` ne lisent aucune mémoire.
    let (mut pair, step) = unsafe {
        (
            _mm_set_epi64x(depth.wrapping_add(depth_x), depth),
            _mm_set1_epi64x(depth_x.wrapping_mul(2)),
        )
    };

    let mut i = 0;
    // Le reste impair se traite après la boucle, en scalaire : écrire deux
    // valeurs là où une seule est attendue déborderait de `out`.
    while i + 2 <= count {
        // SAFETY: `_mm_srli_epi64` et `_mm_add_epi64` ne touchent pas la
        // mémoire ; `_mm_storeu_si128` écrit seize octets **non alignés**, et
        // `scratch` reçoit une adresse de pile dont l'alignement n'est pas
        // garanti — d'où la forme non alignée, jamais `_mm_store_si128`.
        let shifted = unsafe { _mm_srli_epi64::<{ GRADIENT_BITS as i32 }>(pair) };
        let mut scratch = [0u64; 2];
        // SAFETY: `scratch` porte seize octets inscriptibles, ce que l'écriture
        // non alignée demande en tout et pour tout.
        unsafe { _mm_storeu_si128(scratch.as_mut_ptr().cast::<__m128i>(), shifted) };
        out[i] = scratch[0] as u32;
        out[i + 1] = scratch[1] as u32;
        // SAFETY: addition enveloppante sur deux voies, sans accès mémoire.
        pair = unsafe { _mm_add_epi64(pair, step) };
        i += 2;
    }

    if i < count {
        // La voie de rang pair porte la valeur du pixel `i` : c'est elle qu'on
        // reprend, et non une reconstruction à partir de `depth`, qui
        // réintroduirait une multiplication là où le scalaire n'a que des
        // additions.
        let mut scratch = [0u64; 2];
        // SAFETY: même garantie que plus haut — seize octets inscriptibles.
        unsafe { _mm_storeu_si128(scratch.as_mut_ptr().cast::<__m128i>(), pair) };
        out[i] = (scratch[0] >> GRADIENT_BITS) as u32;
    }
}

#[cfg(test)]
mod tests;
