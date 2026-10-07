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

use super::fill_flat_row_if_available;
use crate::raster::plane::GRADIENT_BITS;
use crate::raster::simd::FlatRow;

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

/// Une pente négative descend comme le scalaire.
#[test]
fn une_pente_negative_suit_le_scalaire() {
    let mut seed = Seed(0x2468_ace0_1357_9bdf);
    let (mut color, mut depth) = peuple(&mut seed, 19);
    let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
    let (start, step) = (i64::MAX / 2, -(1 << 30));

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
