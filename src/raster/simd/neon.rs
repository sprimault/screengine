// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'une ligne unie et d'un segment texturé en NEON.
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
//! Le chemin texturé en ajoute deux autres :
//!
//! - **le décalage par une amplitude variable existe**, et il n'a pas de forme à
//!   part : `vshlq_s64` d'une amplitude **négative** est le décalage
//!   arithmétique à droite d'un `i64`, que SSE2 émule par un remplissage de
//!   signe faute d'avoir `_mm_srai_epi64` avant AVX512. La même forme sert aux
//!   mots de trente-deux et de seize bits ;
//! - **le minimum non signé sur seize bits existe**, `vminq_u16`, là où SSE2
//!   sature par le détour `x − subs(x, 255)`.
//!
//! **Le gather, en revanche, n'existe pas**, et c'est ce qui rend la colonne
//! SSE2 du relevé de `benches/remplissage.rs` le bon pronostic pour cette
//! variante : l'avantage d'AVX2 sur le cas dominant est son
//! `_mm256_i32gather_epi32`, pas sa largeur. Les quatre lectures sont donc
//! scalaires, par extraction de voie — sans tampon de pile, que SSE2 est seul à
//! devoir prendre.
//!
//! **Et son gain n'est pas mesuré.** `qemu-user` ne rend aucune durée
//! comparable à celles du relevé, et ce dépôt n'a pas de machine ARM : ce que ce
//! fichier prouve est sa **justesse**, par la passe NEON de la conformance sous
//! `make conform-arm` et par les cas de son module de test.
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
    int32x4_t, int64x2_t, uint32x4_t, vaddq_s32, vaddq_s64, vaddq_u16, vandq_u32, vbslq_u32,
    vcgtq_u32, vcombine_u32, vdupq_n_s16, vdupq_n_s32, vdupq_n_s64, vdupq_n_u16, vdupq_n_u32,
    vgetq_lane_u32, vld1q_s64, vld1q_u32, vminq_u16, vmovn_u64, vmulq_u16, vorrq_u32,
    vreinterpretq_s32_u32, vreinterpretq_u16_u32, vreinterpretq_u32_s32, vreinterpretq_u32_u16,
    vreinterpretq_u64_s64, vshlq_n_u32, vshlq_s64, vshlq_u16, vshlq_u32, vshrq_n_s32, vshrq_n_s64,
    vshrq_n_u16, vshrq_n_u32, vst1q_u32, vsubq_s32, vsubq_u16,
};

use super::{FlatRow, SampledRow};
use crate::math::fixed::UV_BITS;
use crate::raster::plane::GRADIENT_BITS;
use crate::texture::{HALF_TEXEL, WEIGHT_BITS};

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

/// Le décalage arithmétique à droite de deux `i64`, par une amplitude variable.
///
/// **`vshlq_s64` d'une amplitude négative est un décalage à droite**, et il est
/// arithmétique parce que l'opérande est signé : une coordonnée de texture
/// négative — elles le sont dès qu'un plaquage recule l'origine — y remonte des
/// uns comme le scalaire. C'est l'instruction `SSHL`, et c'est ce qui dispense
/// ce fichier du remplissage de signe que SSE2 doit écrire.
///
/// L'amplitude reste sous soixante-quatre, comme pour le scalaire, dont le `>>`
/// sur un `i64` a la même précondition.
#[inline]
fn shift_right(x: int64x2_t, n: u32) -> int64x2_t {
    // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
    unsafe { vshlq_s64(x, vdupq_n_s64(-i64::from(n))) }
}

/// Le mélange de `texture::mix`, sur quatre texels à la fois.
///
/// **La même technique à deux voies que le scalaire**, et c'est ce qui rend
/// l'égalité des bits mécanique plutôt qu'heureuse : R et B tiennent ensemble
/// dans `0x00FF00FF`, G et A dans le même masque une fois décalés, et aucun
/// produit ne sort de ses seize bits — `max(a, b) · 256 + 128` vaut au plus
/// 65 408. `vmulq_u16` traite donc huit canaux par instruction sans qu'un seul
/// bit traverse une frontière de voie.
///
/// `t` porte le poids de `b` sur huit bits, **par pixel** : c'est ce qui
/// distingue ce mélange d'une interpolation à poids constant, et ce qui oblige à
/// répliquer le poids dans les deux voies du mot.
#[inline]
fn mix_vec(a: uint32x4_t, b: uint32x4_t, t: uint32x4_t) -> uint32x4_t {
    // SAFETY: aucune de ces intrinsèques ne touche la mémoire ; les
    // réinterprétations ne changent que le type vu, jamais les bits.
    unsafe {
        let mask = vdupq_n_u32(0x00FF_00FF);
        let round = vreinterpretq_u16_u32(vdupq_n_u32(0x0080_0080));
        let wt = vorrq_u32(t, vshlq_n_u32::<16>(t));
        let wt16 = vreinterpretq_u16_u32(wt);
        let inv = vsubq_u16(vreinterpretq_u16_u32(vdupq_n_u32(0x0100_0100)), wt16);
        let blend = |a: uint32x4_t, b: uint32x4_t| {
            let left = vmulq_u16(vreinterpretq_u16_u32(vandq_u32(a, mask)), inv);
            let right = vmulq_u16(vreinterpretq_u16_u32(vandq_u32(b, mask)), wt16);
            let sum = vaddq_u16(vaddq_u16(left, right), round);
            vandq_u32(
                vreinterpretq_u32_u16(vshrq_n_u16::<{ WEIGHT_BITS as i32 }>(sum)),
                mask,
            )
        };
        vorrq_u32(
            blend(a, b),
            vshlq_n_u32::<8>(blend(vshrq_n_u32::<8>(a), vshrq_n_u32::<8>(b))),
        )
    }
}

/// La combinaison de `light::modulate`, sur quatre pixels à la fois.
///
/// `shade` est le décalage du scalaire, `LIGHT_BITS` moins le sur-éclairement,
/// et il passe par la même forme que les autres décalages variables : une
/// amplitude négative donnée à `vshlq_u16`.
///
/// **L'alpha ne se module pas**, et c'est le seul canal à part : il sort du
/// texel tel quel, là où les trois autres passent par `t · (l + 1) >> shade`
/// saturé. La voie haute porte G **et** A dans le même mot, d'où le masque qui
/// ne garde que le premier avant de rendre son octet à l'autre.
///
/// **La saturation est un minimum non signé**, `vminq_u16`, que SSE2 n'a pas
/// avant SSE4.1 et remplace par `x − subs(x, 255)`. Les deux rendent les mêmes
/// bits ; celui-ci dit ce qu'il fait.
#[inline]
fn modulate_vec(texel: uint32x4_t, light: uint32x4_t, shade: u32) -> uint32x4_t {
    // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
    unsafe {
        let mask = vdupq_n_u32(0x00FF_00FF);
        let one = vdupq_n_u16(1);
        let cap = vdupq_n_u16(0x00FF);
        let down = vdupq_n_s16(-(shade as i16));
        let combine = |t: uint32x4_t, l: uint32x4_t| {
            let product = vmulq_u16(
                vreinterpretq_u16_u32(vandq_u32(t, mask)),
                vaddq_u16(vreinterpretq_u16_u32(vandq_u32(l, mask)), one),
            );
            vminq_u16(vshlq_u16(product, down), cap)
        };
        let low = combine(texel, light);
        let high = combine(vshrq_n_u32::<8>(texel), vshrq_n_u32::<8>(light));
        let green = vshlq_n_u32::<8>(vandq_u32(
            vreinterpretq_u32_u16(high),
            vdupq_n_u32(0x0000_00FF),
        ));
        let alpha = vandq_u32(texel, vdupq_n_u32(0xFF00_0000));
        vorrq_u32(vorrq_u32(vreinterpretq_u32_u16(low), green), alpha)
    }
}

/// Lit quatre texels aux quatre indices d'un registre.
///
/// **NEON n'a pas de gather**, donc les quatre lectures sont scalaires. Elles
/// passent en revanche par l'extraction de voie, sans le tampon de pile que SSE2
/// doit prendre faute d'instruction d'extraction d'un mot de trente-deux bits.
///
/// Les indices sont dans les bornes par construction, chaque composante étant
/// repliée sous la dimension de son niveau : l'indexation vérifiée ne peut pas
/// échouer, et aucune longueur n'est avancée ici, donc elle n'ouvre aucune
/// fenêtre de dépliage.
#[inline]
fn gather(texels: &[u32], index: uint32x4_t) -> uint32x4_t {
    // SAFETY: les quatre extractions ne touchent pas la mémoire, et le
    // chargement lit seize octets d'un tableau local de quatre `u32`, donc
    // exactement sa taille ; `vld1q_u32` n'exige aucun alignement au-delà de
    // celui que `u32` donne déjà.
    unsafe {
        let four = [
            texels[vgetq_lane_u32::<0>(index) as usize],
            texels[vgetq_lane_u32::<1>(index) as usize],
            texels[vgetq_lane_u32::<2>(index) as usize],
            texels[vgetq_lane_u32::<3>(index) as usize],
        ];
        vld1q_u32(four.as_ptr())
    }
}

/// Rassemble les moitiés basses de deux paires de `i64` en quatre `u32`.
///
/// `vmovn_u64` tronque chaque `u64` à ses trente-deux bits bas, `vcombine_u32`
/// assemble les deux moitiés : aucun aller-retour par la mémoire, que SSE2 doit
/// faire.
#[inline]
fn narrow(low: int64x2_t, high: int64x2_t) -> uint32x4_t {
    // SAFETY: rétrécissements et assemblage sans accès mémoire.
    unsafe {
        vcombine_u32(
            vmovn_u64(vreinterpretq_u64_s64(low)),
            vmovn_u64(vreinterpretq_u64_s64(high)),
        )
    }
}

/// Les quatre valeurs d'un attribut affine, et l'avancée d'un bloc.
///
/// Deux voies de deux `i64`, décalées d'un pas : c'est la seule forme possible,
/// un registre de cent vingt-huit bits ne tenant que deux `i64`.
struct Walk2 {
    low: int64x2_t,
    high: int64x2_t,
    wide: int64x2_t,
}

impl Walk2 {
    /// Les quatre premières valeurs depuis `start`, par pas de `step`.
    fn new(start: i64, step: i64) -> Self {
        let lane = |k: i64| start.wrapping_add(step.wrapping_mul(k));
        let first = [lane(0), lane(1)];
        let second = [lane(2), lane(3)];
        // SAFETY: les deux chargements lisent seize octets d'un tableau de deux
        // `i64`, qui les porte ; `vdupq_n_s64` ne touche pas la mémoire.
        unsafe {
            Self {
                low: vld1q_s64(first.as_ptr()),
                high: vld1q_s64(second.as_ptr()),
                wide: vdupq_n_s64(step.wrapping_mul(4)),
            }
        }
    }

    /// Avance de quatre pixels.
    #[inline]
    fn step(&mut self) {
        // SAFETY: additions enveloppantes sur deux voies, sans accès mémoire.
        unsafe {
            self.low = vaddq_s64(self.low, self.wide);
            self.high = vaddq_s64(self.high, self.wide);
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
///
/// `LIT` dit si le segment porte une lightmap. Un paramètre de type et non un
/// champ lu : le segment qui n'en porte pas ne compile pas la lecture, là où un
/// drapeau examiné par bloc l'aurait payée à chaque tour.
pub fn fill_sampled_row<const LIT: bool>(row: SampledRow<'_>) -> usize {
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

    let mut z = Walk2::new(start, step);
    let mut u = Walk2::new(uv[0], uv_step[0]);
    let mut v = Walk2::new(uv[1], uv_step[1]);
    let mut lu = Walk2::new(lightmap.uv[0], lightmap.uv_step[0]);
    let mut lv = Walk2::new(lightmap.uv[1], lightmap.uv_step[1]);

    // Le tramage et les deux masques de repli sont constants sur le segment :
    // voir `SampledRow::dither` pour la période de quatre qui le permet.
    let du0 = [i64::from(dither[0][0]), i64::from(dither[0][1])];
    let du1 = [i64::from(dither[0][2]), i64::from(dither[0][3])];
    let dv0 = [i64::from(dither[1][0]), i64::from(dither[1][1])];
    let dv1 = [i64::from(dither[1][2]), i64::from(dither[1][3])];
    // SAFETY: les quatre chargements lisent chacun seize octets d'un tableau de
    // deux `i64` ; les duplications ne touchent pas la mémoire.
    let (du_low, du_high, dv_low, dv_high, mask_x, mask_y, log_width) = unsafe {
        (
            vld1q_s64(du0.as_ptr()),
            vld1q_s64(du1.as_ptr()),
            vld1q_s64(dv0.as_ptr()),
            vld1q_s64(dv1.as_ptr()),
            vdupq_n_u32(width - 1),
            vdupq_n_u32(height - 1),
            vdupq_n_s32(width.trailing_zeros() as i32),
        )
    };

    // Le décalage de niveau puis celui du format, tous deux arithmétiques et
    // dans cet ordre : le tramage s'ajoute **entre** les deux, comme le scalaire
    // le fait, faute de quoi il serait divisé par le niveau et s'éteindrait.
    let coord = |walk: &Walk2, d_low: int64x2_t, d_high: int64x2_t| {
        // SAFETY: additions sans accès mémoire ; les décalages passent par
        // `shift_right`, qui porte sa propre garantie.
        unsafe {
            let fold = |value: int64x2_t, d: int64x2_t| {
                shift_right(vaddq_s64(shift_right(value, shift), d), UV_BITS)
            };
            narrow(fold(walk.low, d_low), fold(walk.high, d_high))
        }
    };

    // La lightmap se lit **toujours en bilinéaire**, quel que soit le filtrage,
    // et ne porte donc aucun tramage : son décalage de niveau est le seul à
    // s'appliquer avant la lecture. Voir `Crawl::sample` et `Texture::bilinear`.
    //
    // SAFETY: aucune de ces duplications ne touche la mémoire.
    let (mask_lx, mask_ly, log_lwidth, half, weight_mask) = unsafe {
        (
            vdupq_n_u32(lightmap.size.0 - 1),
            vdupq_n_u32(lightmap.size.1 - 1),
            vdupq_n_s32(lightmap.size.0.trailing_zeros() as i32),
            vdupq_n_s32(HALF_TEXEL),
            vdupq_n_u32(0xFF),
        )
    };
    let lcoord = |walk: &Walk2| {
        // Le demi-texel se retranche **après** le décalage de niveau et avant la
        // séparation en partie entière et poids, comme `Texture::bilinear`.
        //
        // SAFETY: soustraction et réinterprétation sans accès mémoire.
        unsafe {
            let raw = narrow(
                shift_right(walk.low, lightmap.shift),
                shift_right(walk.high, lightmap.shift),
            );
            vsubq_s32(vreinterpretq_s32_u32(raw), half)
        }
    };

    let mut i = 0;
    while i + 4 <= count {
        // Le décalage est arithmétique, comme celui du scalaire sur un `i64` ;
        // les trente-deux bits bas seraient les mêmes en logique, un gradient
        // restant sous 2⁶².
        //
        // SAFETY: deux décalages par constante, sans accès mémoire.
        let zs = unsafe {
            narrow(
                vshrq_n_s64::<{ GRADIENT_BITS as i32 }>(z.low),
                vshrq_n_s64::<{ GRADIENT_BITS as i32 }>(z.high),
            )
        };
        let tx = coord(&u, du_low, du_high);
        let ty = coord(&v, dv_low, dv_high);

        // L'adressage : repli par masque sur chaque composante, puis
        // `y · largeur + x` par un décalage, la largeur étant une puissance de
        // deux.
        //
        // SAFETY: aucune de ces intrinsèques ne touche la mémoire.
        let index = unsafe {
            vorrq_u32(
                vshlq_u32(vandq_u32(ty, mask_y), log_width),
                vandq_u32(tx, mask_x),
            )
        };

        let texel_v = gather(texels, index);

        // La lightmap : quatre voisins par pixel, leurs deux poids, trois
        // mélanges, puis la combinaison. Seize lectures par bloc, contre quatre
        // pour la texture — c'est le prix du bilinéaire, et il est entièrement
        // dans le gather qu'aucun des deux jeux de cette largeur ne porte.
        let shaded = if LIT {
            let lx = lcoord(&lu);
            let ly = lcoord(&lv);
            // SAFETY: décalages, additions et masques sans accès mémoire ; les
            // lectures passent par `gather`, qui porte sa propre garantie.
            let (x0, x1, y0, y1, wu, wv) = unsafe {
                let one = vdupq_n_s32(1);
                let floor = |c: int32x4_t| vshrq_n_s32::<{ UV_BITS as i32 }>(c);
                let weight = |c: int32x4_t| {
                    vandq_u32(
                        vreinterpretq_u32_s32(vshrq_n_s32::<{ (UV_BITS - WEIGHT_BITS) as i32 }>(c)),
                        weight_mask,
                    )
                };
                (
                    vandq_u32(vreinterpretq_u32_s32(floor(lx)), mask_lx),
                    vandq_u32(vreinterpretq_u32_s32(vaddq_s32(floor(lx), one)), mask_lx),
                    vandq_u32(vreinterpretq_u32_s32(floor(ly)), mask_ly),
                    vandq_u32(vreinterpretq_u32_s32(vaddq_s32(floor(ly), one)), mask_ly),
                    weight(lx),
                    weight(ly),
                )
            };
            // SAFETY: les quatre adressages ne touchent pas la mémoire.
            let (rows0, rows1) = unsafe {
                let line = |y: uint32x4_t, x: uint32x4_t| vorrq_u32(vshlq_u32(y, log_lwidth), x);
                (
                    mix_vec(
                        gather(lightmap.texels, line(y0, x0)),
                        gather(lightmap.texels, line(y0, x1)),
                        wu,
                    ),
                    mix_vec(
                        gather(lightmap.texels, line(y1, x0)),
                        gather(lightmap.texels, line(y1, x1)),
                        wu,
                    ),
                )
            };
            modulate_vec(texel_v, mix_vec(rows0, rows1, wv), lightmap.shade)
        } else {
            texel_v
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
            vst1q_u32(color[i..].as_mut_ptr(), vbslq_u32(mask, shaded, seen));
        }

        z.step();
        u.step();
        v.step();
        if LIT {
            lu.step();
            lv.step();
        }
        i += 4;
    }
    i
}

#[cfg(test)]
mod tests;
