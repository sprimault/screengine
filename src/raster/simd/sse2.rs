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
    _mm_or_si128, _mm_set_epi32, _mm_set_epi64x, _mm_set1_epi32, _mm_set1_epi64x,
    _mm_shuffle_epi32, _mm_sll_epi32, _mm_sll_epi64, _mm_srai_epi32, _mm_srl_epi64, _mm_srli_epi64,
    _mm_storeu_si128, _mm_xor_si128,
};

use super::{FlatRow, SampledRow};
use crate::math::fixed::UV_BITS;
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

/// Décalage à droite **arithmétique** de deux `i64`, que SSE2 n'a pas.
///
/// `_mm_srai_epi64` n'existe qu'en AVX512, et `_mm_srl_epi64` est logique : sur
/// une coordonnée de texture négative — elles le sont dès qu'un plaquage recule
/// l'origine —, il remonterait des zéros là où le scalaire remonte des uns. D'où
/// le remplissage de signe explicite.
///
/// **Aucun test ne peut le faire rougir, et il faut le dire.** Les coordonnées
/// tiennent dans un `i32` élargi, donc le décalage logique et l'arithmétique ont
/// les mêmes bits de poids faible, et le repli par masque jette précisément ceux
/// où ils diffèrent : la cassure volontaire l'a vérifié, y compris sur un niveau
/// de quarante. Ce code est donc juste **par construction et non par
/// couverture**, et il reste écrit ainsi pour ne pas dépendre d'une borne que
/// rien n'énonce ici — le jour où un attribut arriverait sur plus de trente-deux
/// bits, la forme logique divergerait en silence.
///
/// **`n` nul est le cas qui se traite tout seul** : le remplissage demande alors
/// un décalage de soixante-quatre, que SSE2 rend nul, et la partie logique vaut
/// déjà la valeur entière.
#[inline]
fn shift_right_arithmetic(x: __m128i, n: u32) -> __m128i {
    // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
    unsafe {
        let logical = _mm_srl_epi64(x, _mm_set_epi64x(0, i64::from(n)));
        // Le signe de chaque `i64` est le bit de poids fort de son mot haut :
        // on le réplique sur tout le mot, puis on prend les mots hauts des deux
        // voies — d'où le brassage (3, 3, 1, 1).
        let sign = _mm_shuffle_epi32::<0b1111_0101>(_mm_srai_epi32::<31>(x));
        let fill = _mm_sll_epi64(sign, _mm_set_epi64x(0, i64::from(64 - n)));
        _mm_or_si128(logical, fill)
    }
}

/// Rassemble les moitiés basses de deux paires de `i64` en quatre `i32`.
///
/// C'est la forme que [`fill_flat_row`] emploie pour les profondeurs, sortie ici
/// parce que le chemin texturé en a besoin trois fois — la profondeur et les
/// deux coordonnées.
#[inline]
fn narrow(low: __m128i, high: __m128i) -> __m128i {
    let mut lanes = [0u64; 2];
    let mut upper = [0u64; 2];
    // SAFETY: les deux écritures visent seize octets d'un tableau local de deux
    // `u64`, donc exactement sa taille, et `_mm_storeu_si128` n'exige aucun
    // alignement.
    unsafe {
        _mm_storeu_si128(lanes.as_mut_ptr().cast::<__m128i>(), low);
        _mm_storeu_si128(upper.as_mut_ptr().cast::<__m128i>(), high);
        _mm_set_epi32(
            upper[1] as i32,
            upper[0] as i32,
            lanes[1] as i32,
            lanes[0] as i32,
        )
    }
}

/// Les quatre valeurs d'un attribut affine, et l'avancée d'un bloc.
///
/// Deux voies de deux `i64`, décalées d'un pas : c'est la seule forme possible,
/// un registre de cent vingt-huit bits ne tenant que deux `i64`.
struct Walk2 {
    low: __m128i,
    high: __m128i,
    wide: __m128i,
}

impl Walk2 {
    /// Les quatre premières valeurs depuis `start`, par pas de `step`.
    fn new(start: i64, step: i64) -> Self {
        // SAFETY: ces intrinsèques ne touchent pas la mémoire.
        unsafe {
            Self {
                low: _mm_set_epi64x(start.wrapping_add(step), start),
                high: _mm_set_epi64x(
                    start.wrapping_add(step.wrapping_mul(3)),
                    start.wrapping_add(step.wrapping_mul(2)),
                ),
                wide: _mm_set1_epi64x(step.wrapping_mul(4)),
            }
        }
    }

    /// Avance de quatre pixels.
    #[inline]
    fn step(&mut self) {
        // SAFETY: additions enveloppantes sur deux voies, sans accès mémoire.
        unsafe {
            self.low = _mm_add_epi64(self.low, self.wide);
            self.high = _mm_add_epi64(self.high, self.wide);
        }
    }
}

/// Remplit les blocs de quatre pixels d'un segment texturé, et rend leur
/// compte.
///
/// **Le résultat est celui du chemin scalaire, au bit près**, et rien ici n'est
/// une approximation : les coordonnées restent en `i64` comme là-bas, le
/// tramage vient de la même table, le repli est le même masque, et le test de
/// profondeur est le même test strict. Ce qui change est le nombre de pixels
/// traités par instruction.
///
/// **Le reste du segment n'est pas traité** : voir
/// [`super::fill_sampled_row`], qui dit pourquoi l'appelant le finit lui-même.
pub fn fill_sampled_row(row: SampledRow<'_>) -> usize {
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
        ..
    } = row;
    let count = depth.len();
    let (width, height) = size;

    let mut z = Walk2::new(start, step);
    let mut u = Walk2::new(uv[0], uv_step[0]);
    let mut v = Walk2::new(uv[1], uv_step[1]);

    // Le tramage et les deux masques de repli sont constants sur le segment :
    // voir `SampledRow::dither` pour la période de quatre qui le permet.
    //
    // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
    let (du_low, du_high, dv_low, dv_high, mask_x, mask_y, log_width, sign_v) = unsafe {
        (
            _mm_set_epi64x(i64::from(dither[0][1]), i64::from(dither[0][0])),
            _mm_set_epi64x(i64::from(dither[0][3]), i64::from(dither[0][2])),
            _mm_set_epi64x(i64::from(dither[1][1]), i64::from(dither[1][0])),
            _mm_set_epi64x(i64::from(dither[1][3]), i64::from(dither[1][2])),
            _mm_set1_epi32((width - 1) as i32),
            _mm_set1_epi32((height - 1) as i32),
            _mm_set_epi64x(0, i64::from(width.trailing_zeros())),
            _mm_set1_epi32(SIGN),
        )
    };

    // Le décalage de niveau puis celui du format, tous deux en `i64` et dans
    // cet ordre : le tramage s'ajoute **entre** les deux, comme le scalaire le
    // fait, faute de quoi il serait divisé par le niveau et s'éteindrait.
    let coord = |walk: &Walk2, d_low: __m128i, d_high: __m128i| {
        // SAFETY: additions sans accès mémoire ; les décalages sont ceux de
        // `shift_right_arithmetic`, qui porte sa propre garantie.
        unsafe {
            let low = shift_right_arithmetic(
                _mm_add_epi64(shift_right_arithmetic(walk.low, shift), d_low),
                UV_BITS,
            );
            let high = shift_right_arithmetic(
                _mm_add_epi64(shift_right_arithmetic(walk.high, shift), d_high),
                UV_BITS,
            );
            narrow(low, high)
        }
    };

    let mut i = 0;
    while i + 4 <= count {
        // Logique et non arithmétique : la profondeur est positive, `to_depth`
        // la bornant avec sa marge, et c'est ce que le chemin uni fait déjà.
        //
        // SAFETY: deux décalages sans accès mémoire.
        let (zl, zh) = unsafe {
            (
                _mm_srli_epi64::<{ GRADIENT_BITS as i32 }>(z.low),
                _mm_srli_epi64::<{ GRADIENT_BITS as i32 }>(z.high),
            )
        };
        let zs = narrow(zl, zh);
        let tx = coord(&u, du_low, du_high);
        let ty = coord(&v, dv_low, dv_high);

        // L'adressage : repli par masque sur chaque composante, puis
        // `y · largeur + x` par un décalage, la largeur étant une puissance de
        // deux. `_mm_mullo_epi32` est SSE4.1 et n'aurait rien apporté ici.
        //
        // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
        let index = unsafe {
            _mm_or_si128(
                _mm_sll_epi32(_mm_and_si128(ty, mask_y), log_width),
                _mm_and_si128(tx, mask_x),
            )
        };

        let mut ids = [0i32; 4];
        // SAFETY: seize octets dans un tableau local de quatre `i32`, sans
        // exigence d'alignement.
        unsafe { _mm_storeu_si128(ids.as_mut_ptr().cast::<__m128i>(), index) };
        // **Les quatre lectures sont scalaires, et c'est ce que SSE2 impose** :
        // il n'a pas de `gather`. Les indices sont dans les bornes par
        // construction — chaque composante est repliée sous la dimension de son
        // niveau —, donc l'indexation vérifiée ne peut pas échouer ; elle est
        // gardée parce qu'elle ne coûte que ce test et qu'aucune longueur n'est
        // avancée ici, donc aucune fenêtre de dépliage ne s'ouvre.
        //
        // SAFETY: `_mm_set_epi32` ne touche pas la mémoire.
        let texel_v = unsafe {
            _mm_set_epi32(
                texels[ids[3] as usize] as i32,
                texels[ids[2] as usize] as i32,
                texels[ids[1] as usize] as i32,
                texels[ids[0] as usize] as i32,
            )
        };

        // Le test et l'écriture masquée, dans les termes de `fill_flat_row` :
        // biais de signe parce que SSE2 ne compare qu'en signé, et
        // `(nouveau & masque) | (ancien & !masque)` faute de `_mm_blendv_epi8`.
        //
        // SAFETY: les deux lectures prennent seize octets à partir de `i`, que
        // la boucle garantit disponibles, et sans exigence d'alignement.
        let (kept_depth, kept_color) = unsafe {
            let old = _mm_loadu_si128(depth[i..].as_ptr().cast::<__m128i>());
            let seen = _mm_loadu_si128(color[i..].as_ptr().cast::<__m128i>());
            let mask = _mm_cmpgt_epi32(_mm_xor_si128(zs, sign_v), _mm_xor_si128(old, sign_v));
            (
                _mm_or_si128(_mm_and_si128(mask, zs), _mm_andnot_si128(mask, old)),
                _mm_or_si128(_mm_and_si128(mask, texel_v), _mm_andnot_si128(mask, seen)),
            )
        };

        // SAFETY: mêmes seize octets que les lectures ci-dessus.
        unsafe {
            _mm_storeu_si128(depth[i..].as_mut_ptr().cast::<__m128i>(), kept_depth);
            _mm_storeu_si128(color[i..].as_mut_ptr().cast::<__m128i>(), kept_color);
        }

        z.step();
        u.step();
        v.step();
        i += 4;
    }
    i
}

#[cfg(test)]
mod tests;
