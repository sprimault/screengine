// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la variante `simd128` doit au rasteriseur scalaire.
//!
//! **L'égalité au bit près sur les deux tampons**, et rien d'autre : une
//! divergence est une variante fausse, jamais une différence acceptable. Ces cas
//! comparent donc la variante à l'expression du scalaire plutôt qu'à des valeurs
//! écrites à la main, qui ne diraient que ce qu'on y aurait mis.
//!
//! **Ils ne tournent que sous `make test-wasi`**, donc par Node, et par rien
//! d'autre : `make lint` compile ce module pour la cible publiée mais en
//! `--lib`, ce qui laisse les tests dehors.

use alloc::vec;
use alloc::vec::Vec;

use super::fill_flat_row;
use crate::raster::plane::GRADIENT_BITS;
use crate::raster::simd::FlatRow;

/// Ce que le chemin scalaire écrit, repris ici mot pour mot.
///
/// Recopié plutôt que partagé, comme pour les trois autres variantes : la
/// duplication entre le scalaire et ses variantes est voulue, le premier étant
/// la référence contre laquelle les secondes se valident.
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
///
/// Un tampon vide ne prouverait rien : tout pixel y passerait, et la sélection
/// par bits verrait un masque plein. Ce qui doit être vérifié est le mélange,
/// des pixels qui passent et d'autres non dans le même registre.
fn peuple(seed: &mut Seed, count: usize) -> (Vec<u32>, Vec<u32>) {
    let color = (0..count).map(|_| seed.next() as u32).collect();
    let depth = (0..count).map(|_| seed.next() as u32).collect();
    (color, depth)
}

/// Les deux chemins écrivent les mêmes bits, sur les quatre restes possibles.
///
/// **Les quatre parités comptent** : la boucle avance de quatre, donc une ligne
/// dont la longueur n'est pas un multiple de quatre laisse un à trois pixels que
/// seule la queue traite. Une variante qui l'oublierait rendrait juste sur un
/// quart des lignes d'un triangle.
#[test]
fn les_deux_chemins_ecrivent_les_memes_bits() {
    let mut seed = Seed(0x128d_4321_fedc_ba98);
    for count in 0..34usize {
        for _ in 0..8 {
            // Des profondeurs quelconques, pas des valeurs rondes : une pente
            // nulle ou une puissance de deux rendraient le décalage exact des
            // deux côtés sans rien éprouver de l'accumulation. Un `start` tiré
            // sur soixante-quatre bits est négatif une fois sur deux.
            let start = seed.next() as i64;
            let step = (seed.next() as i64) >> 20;
            let fill = seed.next() as u32;
            let (mut color, mut depth) = peuple(&mut seed, count);
            let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());

            fill_flat_row(FlatRow {
                color: &mut color,
                depth: &mut depth,
                start,
                step,
                fill,
            });
            scalar(&mut attendu_color, &mut attendu_depth, start, step, fill);

            assert_eq!(depth, attendu_depth, "profondeurs, {count} pixels");
            assert_eq!(color, attendu_color, "couleurs, {count} pixels");
        }
    }
}

/// Le test de profondeur est **non signé**, et `simd128` le compare tel quel.
///
/// Écrit malgré `u32x4_gt`, qui est l'instruction voulue : c'est le cas qui
/// dirait qu'on a pris par erreur la forme signée, `i32x4_gt`, dont le nom ne
/// diffère que d'une lettre. Les valeurs se placent exprès des deux côtés de
/// `0x8000_0000`.
#[test]
fn la_comparaison_reste_non_signee() {
    let bornes = [0u32, 1, 0x7FFF_FFFF, 0x8000_0000, 0x8000_0001, u32::MAX];
    for &ancien in &bornes {
        for &neuf in &bornes {
            let mut color = vec![0u32; 4];
            let mut depth = vec![ancien; 4];
            let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
            // Pente nulle : les quatre pixels portent la même profondeur, et
            // c'est la comparaison seule qu'on éprouve.
            let start = i64::from(neuf) << GRADIENT_BITS;

            fill_flat_row(FlatRow {
                color: &mut color,
                depth: &mut depth,
                start,
                step: 0,
                fill: 0xAABB_CCDD,
            });
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

/// Une ligne vide ne lit ni n'écrit rien.
///
/// Elle arrive : le remplissage borne le span par la fenêtre, et une ligne du
/// triangle peut tomber entièrement hors de la tuile courante.
#[test]
fn une_ligne_vide_ne_touche_rien() {
    let mut color: [u32; 0] = [];
    let mut depth: [u32; 0] = [];
    fill_flat_row(FlatRow {
        color: &mut color,
        depth: &mut depth,
        start: 1 << 40,
        step: -7,
        fill: 0,
    });
}

/// Une profondeur **vraiment négative** descend comme le scalaire.
///
/// La valeur de départ traverse zéro, faute de quoi le bit de signe resterait
/// libre et le décalage arithmétique ne se distinguerait pas du logique.
#[test]
fn une_pente_negative_traverse_zero() {
    let mut seed = Seed(0x9bdf_1357_ace0_2468);
    let (mut color, mut depth) = peuple(&mut seed, 17);
    let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
    // Huit pixels au-dessus de zéro, neuf au-dessous : la bascule tombe au
    // milieu d'un registre, pas sur sa frontière.
    let step = -(1 << 38);
    let start = 8 * (1 << 38);

    fill_flat_row(FlatRow {
        color: &mut color,
        depth: &mut depth,
        start,
        step,
        fill: 0x1234_5678,
    });
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

/// Le brassage prend les moitiés basses, et dans l'ordre des pixels.
///
/// Propre à ce module : `u32x4_shuffle` désigne ses voies par huit indices, et
/// toute erreur — prendre les moitiés hautes, ou croiser les deux vecteurs —
/// reste une suite plausible. Des profondeurs dont les bits hauts sont posés la
/// rendent visible, là où un tirage place rarement ses valeurs aux frontières.
#[test]
fn le_brassage_garde_les_bits_bas_dans_l_ordre() {
    let hauts = [
        0x0000_0001_FFFF_FFFFu64,
        0xFFFF_FFFF_0000_0000,
        0xDEAD_BEEF_CAFE_BABE,
        0x8000_0000_8000_0000,
    ];
    for &brut in &hauts {
        let start = (brut as i64) << GRADIENT_BITS;
        let mut color = vec![0u32; 7];
        let mut depth = vec![0u32; 7];
        let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());

        fill_flat_row(FlatRow {
            color: &mut color,
            depth: &mut depth,
            start,
            step: 1 << GRADIENT_BITS,
            fill: 0x00FF_00FF,
        });
        scalar(
            &mut attendu_color,
            &mut attendu_depth,
            start,
            1 << GRADIENT_BITS,
            0x00FF_00FF,
        );

        assert_eq!(depth, attendu_depth, "brut={brut:#x}");
        assert_eq!(color, attendu_color, "brut={brut:#x}");
    }
}
