// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la variante AVX2 doit au rasteriseur scalaire.
//!
//! **Chaque cas se saute quand la machine ne porte pas AVX2**, et c'est la
//! différence avec les cas de SSE2, qui tournent partout sur `x86_64`. Un test
//! qui s'ignore en silence ne garde rien : celui-ci ne peut pas faire
//! autrement — la précondition est matérielle —, mais la conformance le
//! rattrape, sa passe AVX2 se sautant au même endroit et l'annonçant en tête.

use alloc::vec;
use alloc::vec::Vec;

#[cfg(target_arch = "x86")]
use core::arch::x86::{__m256i, _mm_set_epi64x, _mm256_loadu_si256, _mm256_storeu_si256};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{__m256i, _mm_set_epi64x, _mm256_loadu_si256, _mm256_storeu_si256};

use super::fill_flat_row_if_available;
use crate::light::{MAX_OVERBRIGHT, modulate};
use crate::raster::plane::GRADIENT_BITS;
use crate::raster::simd::{FlatRow, LightmapRow, SampledRow};
use crate::raster::triangle::dither_offsets;
use crate::texture::mix;

/// Ce que le chemin scalaire écrit, repris ici mot pour mot.
///
/// Recopié plutôt que partagé, comme pour SSE2 : la duplication entre le
/// scalaire et ses variantes est voulue, le premier étant la référence contre
/// laquelle les secondes se valident.
fn scalar(color: &mut [u32], depth: &mut [u32], start: i64, step: i64, fill: u32) {
    let mut z = start;
    for i in 0..depth.len() {
        let value = (z >> GRADIENT_BITS) as u32;
        if value > depth[i] {
            depth[i] = value;
            color[i] = fill;
        }
        z = z.wrapping_add(step);
    }
}

/// Un générateur à graine fixe, écrit ici comme le noyau l'exige.
struct Seed(u64);

impl Seed {
    /// Le prochain mot, par un xorshift dont la graine est écrite dans le test.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Un tampon de profondeurs déjà peuplé, pour que le test ait à trancher.
fn peuple(seed: &mut Seed, count: usize) -> (Vec<u32>, Vec<u32>) {
    let color = (0..count).map(|_| seed.next() as u32).collect();
    let depth = (0..count).map(|_| seed.next() as u32).collect();
    (color, depth)
}

/// Les deux chemins écrivent les mêmes bits, sur les huit restes possibles.
///
/// **Les huit parités comptent**, et c'est le défaut que la forme vectorielle
/// invite à écrire : la boucle avance de huit, donc une ligne dont la longueur
/// n'est pas un multiple de huit laisse un à sept pixels que seule la queue
/// traite.
#[test]
fn les_deux_chemins_ecrivent_les_memes_bits() {
    let mut seed = Seed(0xa1b2_c3d4_e5f6_0789);
    for count in 0..40usize {
        for _ in 0..8 {
            let start = seed.next() as i64;
            let step = (seed.next() as i64) >> 20;
            let fill = seed.next() as u32;
            let (mut color, mut depth) = peuple(&mut seed, count);
            let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());

            if !fill_flat_row_if_available(FlatRow {
                color: &mut color,
                depth: &mut depth,
                start,
                step,
                fill,
            }) {
                return;
            }
            scalar(&mut attendu_color, &mut attendu_depth, start, step, fill);

            assert_eq!(depth, attendu_depth, "profondeurs, {count} pixels");
            assert_eq!(color, attendu_color, "couleurs, {count} pixels");
        }
    }
}

/// Le test de profondeur est **non signé**, et AVX2 compare en signé lui aussi.
///
/// Même piège que SSE2, et même biais : sans lui, toute profondeur proche de la
/// caméra se comparerait à l'envers.
#[test]
fn la_comparaison_reste_non_signee() {
    let bornes = [0u32, 1, 0x7FFF_FFFF, 0x8000_0000, 0x8000_0001, u32::MAX];
    for &ancien in &bornes {
        for &neuf in &bornes {
            let mut color = vec![0u32; 8];
            let mut depth = vec![ancien; 8];
            let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
            let start = i64::from(neuf) << GRADIENT_BITS;

            if !fill_flat_row_if_available(FlatRow {
                color: &mut color,
                depth: &mut depth,
                start,
                step: 0,
                fill: 0xAABB_CCDD,
            }) {
                return;
            }
            scalar(
                &mut attendu_color,
                &mut attendu_depth,
                start,
                0,
                0xAABB_CCDD,
            );

            assert_eq!(depth, attendu_depth, "ancien={ancien:#x} neuf={neuf:#x}");
            assert_eq!(color, attendu_color, "ancien={ancien:#x} neuf={neuf:#x}");
        }
    }
}

/// AVX2 et SSE2 écrivent les mêmes bits, puisque tous deux écrivent ceux du
/// scalaire.
///
/// **Écrit bien que l'égalité avec le scalaire l'implique**, et pas par excès de
/// zèle : les deux variantes diffèrent par le nombre de voies, donc par la
/// manière dont les profondeurs de départ sont posées. Un décalage de voie faux
/// des deux côtés de la même façon passerait les cas précédents ; il ne
/// passerait pas celui-ci sur une longueur qui n'est multiple ni de quatre ni de
/// huit.
#[test]
fn avx2_et_sse2_s_accordent() {
    let mut seed = Seed(0x0f1e_2d3c_4b5a_6978);
    for count in [1usize, 3, 5, 7, 9, 11, 13, 17, 23] {
        let start = seed.next() as i64;
        let step = (seed.next() as i64) >> 20;
        let fill = seed.next() as u32;
        let (mut wide_color, mut wide_depth) = peuple(&mut seed, count);
        let (mut narrow_color, mut narrow_depth) = (wide_color.clone(), wide_depth.clone());

        if !fill_flat_row_if_available(FlatRow {
            color: &mut wide_color,
            depth: &mut wide_depth,
            start,
            step,
            fill,
        }) {
            return;
        }
        crate::raster::simd::sse2::fill_flat_row(FlatRow {
            color: &mut narrow_color,
            depth: &mut narrow_depth,
            start,
            step,
            fill,
        });

        assert_eq!(wide_depth, narrow_depth, "profondeurs, {count} pixels");
        assert_eq!(wide_color, narrow_color, "couleurs, {count} pixels");
    }
}

/// Une profondeur **vraiment négative** descend comme le scalaire.
///
/// Comme pour SSE2 : la valeur de départ traverse zéro, faute de quoi le bit de
/// signe resterait libre et les deux décalages seraient identiques par
/// construction.
#[test]
fn une_pente_negative_traverse_zero() {
    let mut seed = Seed(0x2468_ace0_1357_9bdf);
    let (mut color, mut depth) = peuple(&mut seed, 19);
    let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
    // Huit pixels au-dessus de zéro, onze au-dessous : la bascule tombe au
    // milieu d'un registre, pas sur sa frontière.
    let (start, step) = (8 * (1 << 38), -(1 << 38));

    if !fill_flat_row_if_available(FlatRow {
        color: &mut color,
        depth: &mut depth,
        start,
        step,
        fill: 0x1234_5678,
    }) {
        return;
    }
    scalar(
        &mut attendu_color,
        &mut attendu_depth,
        start,
        step,
        0x1234_5678,
    );

    assert_eq!(depth, attendu_depth);
    assert_eq!(color, attendu_color);
}

/// Huit mots dans un registre large, et le retour.
fn charge8(v: [u32; 8]) -> __m256i {
    // SAFETY: trente-deux octets lus dans un tableau local de huit `u32`, sans
    // exigence d'alignement.
    unsafe { _mm256_loadu_si256(v.as_ptr().cast::<__m256i>()) }
}

/// L'inverse de [`charge8`].
fn extrait8(r: __m256i) -> [u32; 8] {
    let mut v = [0u32; 8];
    // SAFETY: même garantie, en écriture.
    unsafe { _mm256_storeu_si256(v.as_mut_ptr().cast::<__m256i>(), r) };
    v
}

/// Les deux briques arithmétiques rendent les bits de leurs scalaires.
///
/// **Les mêmes cas que SSE2, sur huit voies**, et pour la même raison : une
/// divergence d'un bit se voit ici sur le canal fautif, là où une image ne
/// dirait qu'une empreinte différente. Les bornes du poids et les trois
/// sur-éclairements sont joués explicitement.
#[test]
fn les_deux_briques_rendent_les_bits_du_scalaire() {
    if !crate::raster::simd::x86::has_avx2() {
        return;
    }
    let mut seed = Seed(0x7a6b_5c4d_3e2f_1009);
    let mut huit = || {
        let mut v = [0u32; 8];
        for slot in &mut v {
            *slot = seed.next() as u32;
        }
        v
    };

    for _ in 0..400 {
        let (a, b) = (huit(), huit());
        let t = huit().map(|w| w & 0xFF);
        // SAFETY: `has_avx2` a répondu vrai en tête du cas.
        let obtenu = extrait8(unsafe { super::mix8(charge8(a), charge8(b), charge8(t)) });
        for k in 0..8 {
            assert_eq!(obtenu[k], mix(a[k], b[k], t[k]), "melange, voie {k}");
        }
    }

    for overbright in 0..=MAX_OVERBRIGHT {
        // SAFETY: `_mm_set_epi64x` ne touche pas la mémoire.
        let shift = unsafe { _mm_set_epi64x(0, i64::from(8 - overbright)) };
        for _ in 0..300 {
            let (texel, light) = (huit(), huit());
            // SAFETY: même garantie que ci-dessus.
            let obtenu =
                extrait8(unsafe { super::modulate8(charge8(texel), charge8(light), shift) });
            for k in 0..8 {
                assert_eq!(
                    obtenu[k],
                    modulate(texel[k], light[k], overbright),
                    "combinaison, voie {k}, ob={overbright}"
                );
            }
        }
    }
}

/// Les deux variantes échantillonnent les mêmes bits, éclairées ou non.
///
/// **AVX2 se compare ici à SSE2 et non au scalaire**, et la transitivité suffit :
/// SSE2 est validé bit à bit contre le scalaire dans son propre fichier, sur les
/// mêmes régimes — coordonnées des deux côtés de zéro, niveaux de zéro à
/// quarante, les quatre restes. Ce que ce cas ajoute est propre à la largeur :
/// un bloc de huit là où l'autre en fait quatre.
///
/// La comparaison ne porte que sur les pixels qu'AVX2 a traités : SSE2 en traite
/// au moins autant, un multiple de quatre étant toujours atteint avant un
/// multiple de huit.
#[test]
fn les_deux_largeurs_echantillonnent_les_memes_bits() {
    if !crate::raster::simd::x86::has_avx2() {
        return;
    }
    let mut seed = Seed(0x5150_4140_3130_2120);
    let texels: Vec<u32> = (0..64 * 32).map(|i| i | 0xFF00_0000).collect();
    let atlas_pixels: Vec<u8> = (0..16 * 16 * 4).map(|i| (i * 11) as u8).collect();
    let atlas = crate::texture::Texture::load(16, 16, &atlas_pixels).expect("lightmap valide");

    for count in [8usize, 12, 16, 23, 32] {
        for lit in [false, true] {
            let start = (seed.next() >> 1) as i64;
            let step = (seed.next() as i64) >> 20;
            let uv = [(seed.next() as i64) >> 28, (seed.next() as i64) >> 28];
            let uv_step = [(seed.next() as i64) >> 44, (seed.next() as i64) >> 44];
            let luv = [(seed.next() as i64) >> 30, (seed.next() as i64) >> 30];
            let luv_step = [(seed.next() as i64) >> 48, (seed.next() as i64) >> 48];
            let (x0, y) = (seed.next() as i32 & 0x3FF, seed.next() as i32 & 0x3FF);
            let dither = [
                [
                    dither_offsets(x0, y)[0],
                    dither_offsets(x0 + 1, y)[0],
                    dither_offsets(x0 + 2, y)[0],
                    dither_offsets(x0 + 3, y)[0],
                ],
                [
                    dither_offsets(x0, y)[1],
                    dither_offsets(x0 + 1, y)[1],
                    dither_offsets(x0 + 2, y)[1],
                    dither_offsets(x0 + 3, y)[1],
                ],
            ];
            let (mut wide_color, mut wide_depth) = peuple(&mut seed, count);
            let (mut narrow_color, mut narrow_depth) = (wide_color.clone(), wide_depth.clone());

            // Une macro et non une closure : celle-ci rendrait une structure
            // dont la durée de vie vient de ses arguments, ce qu'une closure ne
            // sait pas annoncer. L'expansion a lieu dans cette portée, donc les
            // variables du cas s'y lisent telles quelles.
            macro_rules! decrire {
                ($color:expr, $depth:expr) => {
                    SampledRow {
                        color: $color,
                        depth: $depth,
                        start,
                        step,
                        uv,
                        uv_step,
                        shift: 0,
                        texels: &texels,
                        size: (64, 32),
                        dither,
                        x0,
                        y,
                        lit,
                        lightmap: if lit {
                            LightmapRow {
                                uv: luv,
                                uv_step: luv_step,
                                shift: 0,
                                texels: atlas.level_texels(0),
                                size: atlas.level_size(0),
                                shade: 8,
                            }
                        } else {
                            LightmapRow::NONE
                        },
                    }
                };
            }

            let large = if lit {
                super::fill_sampled_row_if_available::<true>(decrire!(
                    &mut wide_color,
                    &mut wide_depth
                ))
            } else {
                super::fill_sampled_row_if_available::<false>(decrire!(
                    &mut wide_color,
                    &mut wide_depth
                ))
            };
            let Some(large) = large else { return };
            assert_eq!(large, count & !7, "pixels traités, {count}");

            let etroit = if lit {
                crate::raster::simd::sse2::fill_sampled_row::<true>(decrire!(
                    &mut narrow_color,
                    &mut narrow_depth
                ))
            } else {
                crate::raster::simd::sse2::fill_sampled_row::<false>(decrire!(
                    &mut narrow_color,
                    &mut narrow_depth
                ))
            };
            assert!(etroit >= large, "SSE2 en traite au moins autant");

            assert_eq!(
                wide_depth[..large],
                narrow_depth[..large],
                "profondeurs, {count} px, lit={lit}"
            );
            assert_eq!(
                wide_color[..large],
                narrow_color[..large],
                "couleurs, {count} px, lit={lit}"
            );
        }
    }
}
