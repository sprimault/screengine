// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la variante NEON doit au rasteriseur scalaire.
//!
//! **L'égalité au bit près sur les deux tampons**, et rien d'autre : une
//! divergence est une variante fausse, jamais une différence acceptable. Ces cas
//! comparent donc la variante à l'expression du scalaire plutôt qu'à des valeurs
//! écrites à la main, qui ne diraient que ce qu'on y aurait mis.
//!
//! **Ils ne tournent que sur `aarch64`**, donc sous `make test-arm` ou en
//! intégration continue, et par rien sur un poste x86 : `make lint` compile bien
//! ce module pour la cible, mais en `--lib`, ce qui laisse les tests dehors.

use alloc::vec;
use alloc::vec::Vec;

use super::fill_flat_row;
use crate::raster::plane::GRADIENT_BITS;
use crate::raster::simd::FlatRow;

/// Ce que le chemin scalaire écrit, repris ici mot pour mot.
///
/// Recopié plutôt que partagé, comme pour SSE2 et AVX2 : la duplication entre le
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
    let mut seed = Seed(0x0ead_5678_9abc_def1);
    for count in 0..34usize {
        for _ in 0..8 {
            // Des profondeurs quelconques, pas des valeurs rondes : une pente
            // nulle ou une puissance de deux rendraient le décalage exact des
            // deux côtés sans rien éprouver de l'accumulation. Un `start` tiré
            // sur soixante-quatre bits est négatif une fois sur deux, et c'est
            // ce qui couvre le décalage sur un bit de signe posé.
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

/// Le test de profondeur est **non signé**, et NEON le compare tel quel.
///
/// Écrit malgré `vcgtq_u32`, qui est exactement l'instruction voulue : c'est le
/// cas qui dirait qu'on a pris par erreur la forme signée, `vcgtq_s32`, dont le
/// nom ne diffère que d'une lettre et que rien d'autre ne distinguerait. Les
/// valeurs se placent exprès des deux côtés de `0x8000_0000`.
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
/// Le décalage de ce module est arithmétique, celui de SSE2 logique, et les deux
/// rendent les mêmes trente-deux bits bas : ceux-ci ne dépendent que des bits 12
/// à 43, que le signe n'atteint pas. Ce cas le vérifie au lieu de le supposer,
/// en traversant zéro — un `start` qui resterait positif ne dirait rien, le
/// signe n'entrant jamais en jeu.
#[test]
fn une_pente_negative_traverse_zero() {
    let mut seed = Seed(0x2468_ace0_1357_9bdf);
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

/// Le rétrécissement garde les trente-deux bits bas, et rien d'autre.
///
/// Propre à ce module : `vmovn_u64` tronque là où SSE2 et AVX2 recomposent par
/// la mémoire. Une erreur de voie — prendre les bits hauts, ou croiser les deux
/// moitiés — se verrait ici et nulle part dans un tirage, dont les valeurs ne
/// sont jamais placées aux frontières.
#[test]
fn le_retrecissement_garde_les_bits_bas() {
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
