// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'une ligne unie et d'un segment texturé en `simd128`.
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
//! Le chemin texturé en ajoute deux autres, et c'est tout ce qui le sépare de
//! celui de SSE2 :
//!
//! - **le décalage arithmétique de deux `i64` existe**, `i64x2_shr`, et son
//!   amplitude n'a pas à être une constante. SSE2 l'émule par un remplissage de
//!   signe explicite, faute d'avoir `_mm_srai_epi64` avant AVX512 ;
//! - **le minimum non signé sur seize bits existe**, `u16x8_min`, là où SSE2
//!   sature par le détour `x − subs(x, 255)` — `_mm_min_epu16` n'arrivant qu'en
//!   SSE4.1.
//!
//! **Le gather, en revanche, n'existe pas**, et c'est ce qui rend la colonne
//! SSE2 du relevé de `benches/remplissage.rs` le bon pronostic pour cette
//! variante : l'avantage d'AVX2 sur le cas dominant est son
//! `_mm256_i32gather_epi32`, pas sa largeur. Les quatre lectures sont donc
//! scalaires, par extraction de voie — sans tampon de pile, que SSE2 est seul à
//! devoir prendre.
//!
//! **Et son gain n'est pas mesuré.** Node ne rend aucune durée comparable à
//! celles du relevé, et il n'y a pas de machine wasm de référence dans ce
//! dépôt : ce que ce fichier prouve est sa **justesse**, par la passe `simd128`
//! de la conformance sous WASI et par les cas de son module de test.
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
    i16x8_add, i16x8_mul, i16x8_sub, i32x4_add, i32x4_shr, i32x4_splat, i32x4_sub, i64x2,
    i64x2_add, i64x2_shr, i64x2_splat, u16x8_min, u16x8_shr, u32x4, u32x4_extract_lane, u32x4_gt,
    u32x4_shl, u32x4_shr, u32x4_shuffle, u32x4_splat, v128, v128_and, v128_bitselect, v128_load,
    v128_or, v128_store,
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

/// Le mélange de `texture::mix`, sur quatre texels à la fois.
///
/// **La même technique à deux voies que le scalaire**, et c'est ce qui rend
/// l'égalité des bits mécanique plutôt qu'heureuse : R et B tiennent ensemble
/// dans `0x00FF00FF`, G et A dans le même masque une fois décalés, et aucun
/// produit ne sort de ses seize bits — `max(a, b) · 256 + 128` vaut au plus
/// 65 408. `i16x8_mul` traite donc huit canaux par instruction sans qu'un seul
/// bit traverse une frontière de voie.
///
/// `t` porte le poids de `b` sur huit bits, **par pixel** : c'est ce qui
/// distingue ce mélange d'une interpolation à poids constant, et ce qui oblige à
/// répliquer le poids dans les deux voies du mot.
#[inline]
fn mix_vec(a: v128, b: v128, t: v128) -> v128 {
    let mask = u32x4_splat(0x00FF_00FF);
    let round = u32x4_splat(0x0080_0080);
    let wt = v128_or(t, u32x4_shl(t, 16));
    let inv = i16x8_sub(u32x4_splat(0x0100_0100), wt);
    let blend = |a: v128, b: v128| {
        let left = i16x8_mul(v128_and(a, mask), inv);
        let right = i16x8_mul(v128_and(b, mask), wt);
        let sum = i16x8_add(i16x8_add(left, right), round);
        v128_and(u16x8_shr(sum, WEIGHT_BITS), mask)
    };
    v128_or(
        blend(a, b),
        u32x4_shl(blend(u32x4_shr(a, 8), u32x4_shr(b, 8)), 8),
    )
}

/// La combinaison de `light::modulate`, sur quatre pixels à la fois.
///
/// `shade` est le décalage du scalaire, `LIGHT_BITS` moins le sur-éclairement,
/// et il arrive en entier : `u16x8_shr` accepte une amplitude quelconque, là où
/// SSE2 doit la passer dans un registre.
///
/// **L'alpha ne se module pas**, et c'est le seul canal à part : il sort du
/// texel tel quel, là où les trois autres passent par `t · (l + 1) >> shade`
/// saturé. La voie haute porte G **et** A dans le même mot, d'où le masque qui
/// ne garde que le premier avant de rendre son octet à l'autre.
///
/// **La saturation est un minimum non signé**, `u16x8_min`, que SSE2 n'a pas
/// avant SSE4.1 et remplace par `x − subs(x, 255)`. Les deux rendent les mêmes
/// bits ; celui-ci dit ce qu'il fait.
#[inline]
fn modulate_vec(texel: v128, light: v128, shade: u32) -> v128 {
    let mask = u32x4_splat(0x00FF_00FF);
    let one = u32x4_splat(0x0001_0001);
    let cap = u32x4_splat(0x00FF_00FF);
    let combine = |t: v128, l: v128| {
        let product = i16x8_mul(v128_and(t, mask), i16x8_add(v128_and(l, mask), one));
        u16x8_min(u16x8_shr(product, shade), cap)
    };
    let low = combine(texel, light);
    let high = combine(u32x4_shr(texel, 8), u32x4_shr(light, 8));
    let green = u32x4_shl(v128_and(high, u32x4_splat(0x0000_00FF)), 8);
    let alpha = v128_and(texel, u32x4_splat(0xFF00_0000));
    v128_or(v128_or(low, green), alpha)
}

/// Lit quatre texels aux quatre indices d'un registre.
///
/// **`simd128` n'a pas de gather**, donc les quatre lectures sont scalaires.
/// Elles passent en revanche par l'extraction de voie, sans le tampon de pile
/// que SSE2 doit prendre faute d'instruction d'extraction d'un mot de
/// trente-deux bits.
///
/// Les indices sont dans les bornes par construction, chaque composante étant
/// repliée sous la dimension de son niveau : l'indexation vérifiée ne peut pas
/// échouer, et aucune longueur n'est avancée ici, donc elle n'ouvre aucune
/// fenêtre de dépliage.
#[inline]
fn gather(texels: &[u32], index: v128) -> v128 {
    u32x4(
        texels[u32x4_extract_lane::<0>(index) as usize],
        texels[u32x4_extract_lane::<1>(index) as usize],
        texels[u32x4_extract_lane::<2>(index) as usize],
        texels[u32x4_extract_lane::<3>(index) as usize],
    )
}

/// Les quatre valeurs d'un attribut affine, et l'avancée d'un bloc.
///
/// Deux voies de deux `i64`, décalées d'un pas : c'est la seule forme possible,
/// un registre de cent vingt-huit bits ne tenant que deux `i64`.
struct Walk2 {
    low: v128,
    high: v128,
    wide: v128,
}

impl Walk2 {
    /// Les quatre premières valeurs depuis `start`, par pas de `step`.
    fn new(start: i64, step: i64) -> Self {
        let lane = |k: i64| start.wrapping_add(step.wrapping_mul(k));
        Self {
            low: i64x2(lane(0), lane(1)),
            high: i64x2(lane(2), lane(3)),
            wide: i64x2_splat(step.wrapping_mul(4)),
        }
    }

    /// Avance de quatre pixels.
    #[inline]
    fn step(&mut self) {
        self.low = i64x2_add(self.low, self.wide);
        self.high = i64x2_add(self.high, self.wide);
    }
}

/// Rassemble les moitiés basses de deux paires de `i64` en quatre `u32`.
///
/// Le brassage prend les voies 0 et 2 de chaque vecteur, c'est-à-dire la moitié
/// basse de chacun des quatre `i64` : les indices 0 à 3 désignent le premier
/// opérande, 4 à 7 le second.
#[inline]
fn narrow(low: v128, high: v128) -> v128 {
    u32x4_shuffle::<0, 2, 4, 6>(low, high)
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
    let du_low = i64x2(i64::from(dither[0][0]), i64::from(dither[0][1]));
    let du_high = i64x2(i64::from(dither[0][2]), i64::from(dither[0][3]));
    let dv_low = i64x2(i64::from(dither[1][0]), i64::from(dither[1][1]));
    let dv_high = i64x2(i64::from(dither[1][2]), i64::from(dither[1][3]));
    let mask_x = u32x4_splat(width - 1);
    let mask_y = u32x4_splat(height - 1);
    let log_width = width.trailing_zeros();

    // Le décalage de niveau puis celui du format, tous deux arithmétiques et
    // dans cet ordre : le tramage s'ajoute **entre** les deux, comme le scalaire
    // le fait, faute de quoi il serait divisé par le niveau et s'éteindrait.
    let coord = |walk: &Walk2, d_low: v128, d_high: v128| {
        let fold = |value: v128, d: v128| i64x2_shr(i64x2_add(i64x2_shr(value, shift), d), UV_BITS);
        narrow(fold(walk.low, d_low), fold(walk.high, d_high))
    };

    // La lightmap se lit **toujours en bilinéaire**, quel que soit le filtrage,
    // et ne porte donc aucun tramage : son décalage de niveau est le seul à
    // s'appliquer avant la lecture. Voir `Crawl::sample` et `Texture::bilinear`.
    let mask_lx = u32x4_splat(lightmap.size.0 - 1);
    let mask_ly = u32x4_splat(lightmap.size.1 - 1);
    let log_lwidth = lightmap.size.0.trailing_zeros();
    let half = i32x4_splat(HALF_TEXEL);
    let weight_mask = u32x4_splat(0xFF);
    let lcoord = |walk: &Walk2| {
        // Le demi-texel se retranche **après** le décalage de niveau et avant la
        // séparation en partie entière et poids, comme `Texture::bilinear`.
        let raw = narrow(
            i64x2_shr(walk.low, lightmap.shift),
            i64x2_shr(walk.high, lightmap.shift),
        );
        i32x4_sub(raw, half)
    };

    let mut i = 0;
    while i + 4 <= count {
        // Logique et non arithmétique : la profondeur est positive, `to_depth`
        // la bornant avec sa marge, et c'est ce que le chemin uni fait déjà —
        // les trente-deux bits bas sont les mêmes des deux façons, un gradient
        // restant sous 2⁶².
        let zs = narrow(
            i64x2_shr(z.low, GRADIENT_BITS),
            i64x2_shr(z.high, GRADIENT_BITS),
        );
        let tx = coord(&u, du_low, du_high);
        let ty = coord(&v, dv_low, dv_high);

        // L'adressage : repli par masque sur chaque composante, puis
        // `y · largeur + x` par un décalage, la largeur étant une puissance de
        // deux.
        let index = v128_or(
            u32x4_shl(v128_and(ty, mask_y), log_width),
            v128_and(tx, mask_x),
        );

        let texel_v = gather(texels, index);

        // La lightmap : quatre voisins par pixel, leurs deux poids, trois
        // mélanges, puis la combinaison. Seize lectures par bloc, contre quatre
        // pour la texture — c'est le prix du bilinéaire, et il est entièrement
        // dans le gather qu'aucun des deux jeux de cette largeur ne porte.
        let shaded = if LIT {
            let lx = lcoord(&lu);
            let ly = lcoord(&lv);
            let one = i32x4_splat(1);
            let floor_x = i32x4_shr(lx, UV_BITS);
            let floor_y = i32x4_shr(ly, UV_BITS);
            let x0 = v128_and(floor_x, mask_lx);
            let x1 = v128_and(i32x4_add(floor_x, one), mask_lx);
            let y0 = v128_and(floor_y, mask_ly);
            let y1 = v128_and(i32x4_add(floor_y, one), mask_ly);
            let wu = v128_and(i32x4_shr(lx, UV_BITS - WEIGHT_BITS), weight_mask);
            let wv = v128_and(i32x4_shr(ly, UV_BITS - WEIGHT_BITS), weight_mask);
            let line = |y: v128, x: v128| v128_or(u32x4_shl(y, log_lwidth), x);
            let rows0 = mix_vec(
                gather(lightmap.texels, line(y0, x0)),
                gather(lightmap.texels, line(y0, x1)),
                wu,
            );
            let rows1 = mix_vec(
                gather(lightmap.texels, line(y1, x0)),
                gather(lightmap.texels, line(y1, x1)),
                wu,
            );
            modulate_vec(texel_v, mix_vec(rows0, rows1, wv), lightmap.shade)
        } else {
            texel_v
        };

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
                v128_bitselect(shaded, seen, mask),
            );
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
