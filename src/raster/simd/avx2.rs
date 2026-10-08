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
    __m128i, __m256i, _mm_set_epi64x, _mm256_add_epi16, _mm256_add_epi32, _mm256_add_epi64,
    _mm256_and_si256, _mm256_blendv_epi8, _mm256_cmpgt_epi32, _mm256_i32gather_epi32,
    _mm256_loadu_si256, _mm256_min_epu16, _mm256_mullo_epi16, _mm256_or_si256, _mm256_set_epi32,
    _mm256_set_epi64x, _mm256_set1_epi32, _mm256_set1_epi64x, _mm256_shuffle_epi32,
    _mm256_sll_epi32, _mm256_sll_epi64, _mm256_slli_epi32, _mm256_srai_epi32, _mm256_srl_epi16,
    _mm256_srl_epi64, _mm256_srli_epi16, _mm256_srli_epi32, _mm256_srli_epi64, _mm256_storeu_si256,
    _mm256_sub_epi16, _mm256_sub_epi32, _mm256_xor_si256,
};

use super::{FlatRow, SampledRow};
use crate::math::fixed::UV_BITS;
use crate::raster::plane::GRADIENT_BITS;
use crate::texture::{HALF_TEXEL, WEIGHT_BITS};

/// Ce qui transporte l'ordre non signé dans l'ordre signé.
const SIGN: i32 = i32::MIN;

/// Lit huit texels aux huit indices d'un registre, par `vpgatherdd`.
///
/// **C'est ce qu'AVX2 a et que SSE2 n'a pas**, et c'est le poste que le chemin
/// à quatre voies ne pouvait pas prendre : là-bas les indices passent par la
/// pile et les lectures sont scalaires.
///
/// # Safety
///
/// Tous les indices doivent désigner un élément de `texels` : le `gather` lit
/// sans vérifier, donc un indice hors borne est un accès hors limites et non une
/// panique. La garantie vient du repli par masque, qui ramène chaque composante
/// sous la dimension de son niveau — et, pour une lightmap, du paramètre de type
/// qui empêche d'appeler cette fonction quand le segment n'en porte pas, auquel
/// cas la tranche est vide.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
unsafe fn gather8(texels: &[u32], index: __m256i) -> __m256i {
    // SAFETY: la précondition de cette fonction est exactement celle que
    // `_mm256_i32gather_epi32` demande — chaque indice, mis à l'échelle de
    // quatre octets, tombe dans la tranche.
    unsafe { _mm256_i32gather_epi32::<4>(texels.as_ptr().cast::<i32>(), index) }
}

/// Décalage à droite **arithmétique** de quatre `i64`.
///
/// `_mm256_srai_epi64` n'existe qu'en AVX512 : même émulation que son homologue
/// SSE2, et même raison de la garder — voir `super::sse2`, dont la
/// documentation dit aussi pourquoi aucun test ne peut la faire rougir.
///
/// Le brassage opère par voie de cent vingt-huit bits, ce qui est exactement ce
/// qu'on veut : il réplique le mot haut de chaque `i64` dans les deux moitiés du
/// registre d'un coup.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
unsafe fn shift_right_arithmetic(x: __m256i, n: u32) -> __m256i {
    // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
    unsafe {
        let logical = _mm256_srl_epi64(x, _mm_set_epi64x(0, i64::from(n)));
        let sign = _mm256_shuffle_epi32::<0b1111_0101>(_mm256_srai_epi32::<31>(x));
        let fill = _mm256_sll_epi64(sign, _mm_set_epi64x(0, i64::from(64 - n)));
        _mm256_or_si256(logical, fill)
    }
}

/// Rassemble les moitiés basses de deux groupes de quatre `i64` en huit `i32`.
///
/// Par la pile, comme [`fill_flat_row`] le fait déjà : les permutations qui
/// traversent les voies demanderaient trois instructions et un masque constant
/// pour le même résultat, et c'est la mesure qui dira si cela compte.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
unsafe fn narrow8(low: __m256i, high: __m256i) -> __m256i {
    let mut first = [0u64; 4];
    let mut second = [0u64; 4];
    // SAFETY: les deux écritures visent trente-deux octets de tableaux locaux de
    // quatre `u64`, donc exactement leur taille, sans exigence d'alignement.
    unsafe {
        _mm256_storeu_si256(first.as_mut_ptr().cast::<__m256i>(), low);
        _mm256_storeu_si256(second.as_mut_ptr().cast::<__m256i>(), high);
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
    }
}

/// Le mélange de `texture::mix`, sur huit texels.
///
/// Même technique à deux voies que SSE2, dont la documentation porte la preuve
/// qu'aucun produit ne sort de ses seize bits.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
unsafe fn mix8(a: __m256i, b: __m256i, t: __m256i) -> __m256i {
    // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
    unsafe {
        let mask = _mm256_set1_epi32(0x00FF_00FF);
        let round = _mm256_set1_epi32(0x0080_0080);
        let wt = _mm256_or_si256(t, _mm256_slli_epi32::<16>(t));
        let inv = _mm256_sub_epi16(_mm256_set1_epi32(0x0100_0100), wt);
        let blend = |a: __m256i, b: __m256i| {
            let left = _mm256_mullo_epi16(_mm256_and_si256(a, mask), inv);
            let right = _mm256_mullo_epi16(_mm256_and_si256(b, mask), wt);
            let sum = _mm256_add_epi16(_mm256_add_epi16(left, right), round);
            _mm256_and_si256(_mm256_srli_epi16::<{ WEIGHT_BITS as i32 }>(sum), mask)
        };
        _mm256_or_si256(
            blend(a, b),
            _mm256_slli_epi32::<8>(blend(_mm256_srli_epi32::<8>(a), _mm256_srli_epi32::<8>(b))),
        )
    }
}

/// La combinaison de `light::modulate`, sur huit pixels.
///
/// **La saturation s'écrit par `_mm256_min_epu16`**, qu'AVX2 porte et que SSE2
/// n'a pas : là-bas il fallait passer par une soustraction saturante. Le
/// résultat est le même, l'écriture plus directe.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
unsafe fn modulate8(texel: __m256i, light: __m256i, shift: __m128i) -> __m256i {
    // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
    unsafe {
        let mask = _mm256_set1_epi32(0x00FF_00FF);
        let one = _mm256_set1_epi32(0x0001_0001);
        let cap = _mm256_set1_epi32(0x00FF_00FF);
        let combine = |t: __m256i, l: __m256i| {
            let product = _mm256_mullo_epi16(
                _mm256_and_si256(t, mask),
                _mm256_add_epi16(_mm256_and_si256(l, mask), one),
            );
            _mm256_min_epu16(_mm256_srl_epi16(product, shift), cap)
        };
        let low = combine(texel, light);
        let high = combine(_mm256_srli_epi32::<8>(texel), _mm256_srli_epi32::<8>(light));
        let green = _mm256_slli_epi32::<8>(_mm256_and_si256(high, _mm256_set1_epi32(0xFF)));
        let alpha = _mm256_and_si256(texel, _mm256_set1_epi32(0xFF00_0000u32 as i32));
        _mm256_or_si256(_mm256_or_si256(low, green), alpha)
    }
}

/// Les huit valeurs d'un attribut affine, et l'avancée d'un bloc.
///
/// Deux groupes de quatre `i64`, décalés de quatre pas : un registre de deux
/// cent cinquante-six bits ne tient que quatre `i64`.
struct Walk4 {
    low: __m256i,
    high: __m256i,
    wide: __m256i,
}

/// Les huit premières valeurs depuis `start`, par pas de `step`.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
unsafe fn walk4(start: i64, step: i64) -> Walk4 {
    let lane = |k: i64| start.wrapping_add(step.wrapping_mul(k));
    // SAFETY: ces intrinsèques ne touchent pas la mémoire.
    unsafe {
        Walk4 {
            low: _mm256_set_epi64x(lane(3), lane(2), lane(1), lane(0)),
            high: _mm256_set_epi64x(lane(7), lane(6), lane(5), lane(4)),
            wide: _mm256_set1_epi64x(step.wrapping_mul(8)),
        }
    }
}

/// Avance un attribut de huit pixels.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
unsafe fn advance(walk: &mut Walk4) {
    // SAFETY: additions enveloppantes, sans accès mémoire.
    unsafe {
        walk.low = _mm256_add_epi64(walk.low, walk.wide);
        walk.high = _mm256_add_epi64(walk.high, walk.wide);
    }
}

/// Les huit coordonnées d'un attribut texturé : niveau, tramage, format.
///
/// **Un seul registre de tramage pour les deux moitiés**, et c'est une
/// conséquence du motif : sa période est de quatre en `x`, donc le cinquième
/// pixel d'un bloc de huit porte le décalage du premier.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
unsafe fn coord8(walk: &Walk4, shift: u32, dither: __m256i) -> __m256i {
    // SAFETY: additions sans accès mémoire ; les décalages portent leur propre
    // garantie.
    unsafe {
        let a = shift_right_arithmetic(
            _mm256_add_epi64(shift_right_arithmetic(walk.low, shift), dither),
            UV_BITS,
        );
        let b = shift_right_arithmetic(
            _mm256_add_epi64(shift_right_arithmetic(walk.high, shift), dither),
            UV_BITS,
        );
        narrow8(a, b)
    }
}

/// Les huit coordonnées d'une lightmap, demi-texel retranché.
///
/// Pas de tramage : une lightmap se lit toujours en bilinéaire, quel que soit le
/// filtrage.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
unsafe fn lcoord8(walk: &Walk4, shift: u32, half: __m256i) -> __m256i {
    // SAFETY: soustraction et décalages sans accès mémoire.
    unsafe {
        let raw = narrow8(
            shift_right_arithmetic(walk.low, shift),
            shift_right_arithmetic(walk.high, shift),
        );
        _mm256_sub_epi32(raw, half)
    }
}

/// Remplit les blocs de huit pixels d'un segment texturé, **si cette machine
/// porte AVX2**.
///
/// Rend `None` sans rien écrire quand elle ne le porte pas, à l'appelant de
/// retomber sur SSE2 ou sur la référence.
pub fn fill_sampled_row_if_available<const LIT: bool>(row: SampledRow<'_>) -> Option<usize> {
    if !super::x86::has_avx2() {
        return None;
    }
    // SAFETY: `has_avx2` vient de répondre vrai, ce qui est la précondition de
    // `fill_sampled_row` — jeu d'instructions présent, et système qui sauvegarde
    // les états `XMM` et `YMM`.
    Some(unsafe { fill_sampled_row::<LIT>(row) })
}

/// Remplit les blocs de huit pixels d'un segment texturé, et rend leur compte.
///
/// **Le résultat est celui du chemin scalaire, au bit près**, et celui de SSE2
/// par conséquent : les deux variantes reproduisent la même arithmétique, et la
/// conformance exige l'égalité des trois.
///
/// # Safety
///
/// Le processeur **et le système** doivent porter AVX2 — voir
/// [`fill_flat_row`], dont la clause est la même.
#[allow(unused_unsafe)]
#[target_feature(enable = "avx2")]
pub unsafe fn fill_sampled_row<const LIT: bool>(row: SampledRow<'_>) -> usize {
    let SampledRow {
        color,
        depth,
        start,
        step,
        uv,
        uv_step,
        shift,
        texels,
        size,
        dither,
        lightmap,
        ..
    } = row;
    let count = depth.len();
    let (width, height) = size;

    // SAFETY: tout ce bloc n'appelle que des intrinsèques sans accès mémoire, et
    // les quatre marches, qui n'en font pas davantage.
    let (mut z, mut u, mut v, mut lu, mut lv) = unsafe {
        (
            walk4(start, step),
            walk4(uv[0], uv_step[0]),
            walk4(uv[1], uv_step[1]),
            walk4(lightmap.uv[0], lightmap.uv_step[0]),
            walk4(lightmap.uv[1], lightmap.uv_step[1]),
        )
    };

    // SAFETY: même garantie ; les constantes du segment sont posées une fois.
    let (du, dv, mask_x, mask_y, log_width, sign_v) = unsafe {
        (
            _mm256_set_epi64x(
                i64::from(dither[0][3]),
                i64::from(dither[0][2]),
                i64::from(dither[0][1]),
                i64::from(dither[0][0]),
            ),
            _mm256_set_epi64x(
                i64::from(dither[1][3]),
                i64::from(dither[1][2]),
                i64::from(dither[1][1]),
                i64::from(dither[1][0]),
            ),
            _mm256_set1_epi32((width - 1) as i32),
            _mm256_set1_epi32((height - 1) as i32),
            _mm_set_epi64x(0, i64::from(width.trailing_zeros())),
            _mm256_set1_epi32(SIGN),
        )
    };
    // SAFETY: même garantie, pour les constantes de la lightmap.
    let (mask_lx, mask_ly, log_lwidth, shade, half, weight_mask, one) = unsafe {
        (
            _mm256_set1_epi32((lightmap.size.0 - 1) as i32),
            _mm256_set1_epi32((lightmap.size.1 - 1) as i32),
            _mm_set_epi64x(0, i64::from(lightmap.size.0.trailing_zeros())),
            _mm_set_epi64x(0, i64::from(lightmap.shade)),
            _mm256_set1_epi32(HALF_TEXEL),
            _mm256_set1_epi32(0xFF),
            _mm256_set1_epi32(1),
        )
    };

    let mut i = 0;
    while i + 8 <= count {
        // SAFETY: tout ce bloc n'accède à la mémoire que par `gather8`, dont la
        // précondition est tenue par le repli : chaque composante est ramenée
        // sous la dimension de son niveau par un `and`, donc chaque indice
        // désigne un texel de la tranche. Pour la lightmap, `LIT` empêche d'y
        // entrer quand la tranche est vide.
        let shaded = unsafe {
            let tx = coord8(&u, shift, du);
            let ty = coord8(&v, shift, dv);
            let index = _mm256_or_si256(
                _mm256_sll_epi32(_mm256_and_si256(ty, mask_y), log_width),
                _mm256_and_si256(tx, mask_x),
            );
            let texel = gather8(texels, index);
            if LIT {
                let lx = lcoord8(&lu, lightmap.shift, half);
                let ly = lcoord8(&lv, lightmap.shift, half);
                let x0 = _mm256_and_si256(_mm256_srai_epi32::<{ UV_BITS as i32 }>(lx), mask_lx);
                let x1 = _mm256_and_si256(
                    _mm256_add_epi32(_mm256_srai_epi32::<{ UV_BITS as i32 }>(lx), one),
                    mask_lx,
                );
                let y0 = _mm256_and_si256(_mm256_srai_epi32::<{ UV_BITS as i32 }>(ly), mask_ly);
                let y1 = _mm256_and_si256(
                    _mm256_add_epi32(_mm256_srai_epi32::<{ UV_BITS as i32 }>(ly), one),
                    mask_ly,
                );
                let wu = _mm256_and_si256(
                    _mm256_srai_epi32::<{ (UV_BITS - WEIGHT_BITS) as i32 }>(lx),
                    weight_mask,
                );
                let wv = _mm256_and_si256(
                    _mm256_srai_epi32::<{ (UV_BITS - WEIGHT_BITS) as i32 }>(ly),
                    weight_mask,
                );
                let line =
                    |y: __m256i, x: __m256i| _mm256_or_si256(_mm256_sll_epi32(y, log_lwidth), x);
                let rows0 = mix8(
                    gather8(lightmap.texels, line(y0, x0)),
                    gather8(lightmap.texels, line(y0, x1)),
                    wu,
                );
                let rows1 = mix8(
                    gather8(lightmap.texels, line(y1, x0)),
                    gather8(lightmap.texels, line(y1, x1)),
                    wu,
                );
                modulate8(texel, mix8(rows0, rows1, wv), shade)
            } else {
                texel
            }
        };

        // Le test et l'écriture masquée. AVX2 porte `blendv`, donc le mélange
        // s'écrit directement ; la comparaison reste signée, d'où le biais.
        //
        // SAFETY: les deux lectures et les deux écritures portent trente-deux
        // octets à partir de `i`, que la boucle garantit disponibles, et sans
        // exigence d'alignement.
        unsafe {
            let zs = narrow8(
                _mm256_srli_epi64::<{ GRADIENT_BITS as i32 }>(z.low),
                _mm256_srli_epi64::<{ GRADIENT_BITS as i32 }>(z.high),
            );
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
                _mm256_blendv_epi8(seen, shaded, mask),
            );
        }

        // SAFETY: les quatre avancées n'accèdent pas à la mémoire.
        unsafe {
            advance(&mut z);
            advance(&mut u);
            advance(&mut v);
            if LIT {
                advance(&mut lu);
                advance(&mut lv);
            }
        }
        i += 8;
    }
    i
}

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
