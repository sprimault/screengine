// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la variante SSE2 doit au rasteriseur scalaire.
//!
//! **L'égalité au bit près sur les deux tampons**, et rien d'autre : une
//! divergence est une variante fausse, jamais une différence acceptable. Ces cas
//! comparent donc la variante à l'expression du scalaire plutôt qu'à des valeurs
//! écrites à la main, qui ne diraient que ce qu'on y aurait mis.

use alloc::vec;
use alloc::vec::Vec;

use super::fill_flat_row;
use crate::raster::plane::GRADIENT_BITS;
use crate::raster::simd::FlatRow;

/// Ce que le chemin scalaire écrit, repris ici mot pour mot.
///
/// **Recopié plutôt que partagé, et c'est l'exception qui confirme la règle** :
/// la duplication entre le scalaire et ses variantes est voulue sur ce projet,
/// parce que le premier est la référence contre laquelle les secondes se
/// valident. Les fondre supprimerait la référence, et ce test ne comparerait
/// plus qu'une expression à elle-même.
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
///
/// Pas de bibliothèque de tests par propriétés : le noyau n'admet aucune
/// dépendance, et un échec qui ne se rejoue pas n'a pas été trouvé.
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
/// **Un tampon vide ne prouverait rien** : tout pixel y passerait, et l'écriture
/// masquée serait un masque plein. Ce qui doit être vérifié est précisément le
/// mélange — des pixels qui passent, d'autres non, dans le même registre.
fn peuple(seed: &mut Seed, count: usize) -> (Vec<u32>, Vec<u32>) {
    let color = (0..count).map(|_| seed.next() as u32).collect();
    let depth = (0..count).map(|_| seed.next() as u32).collect();
    (color, depth)
}

/// Les deux chemins écrivent les mêmes bits, sur les quatre restes possibles.
///
/// **Les quatre parités comptent**, et c'est le défaut que la forme vectorielle
/// invite à écrire : la boucle avance de quatre, donc une ligne dont la longueur
/// n'est pas un multiple de quatre laisse un à trois pixels que seule la queue
/// traite. Une variante qui l'oublierait rendrait juste sur un quart des lignes
/// d'un triangle.
#[test]
fn les_deux_chemins_ecrivent_les_memes_bits() {
    let mut seed = Seed(0x5eed_1234_abcd_ef01);
    for count in 0..34usize {
        for _ in 0..8 {
            // Des profondeurs quelconques, pas des valeurs rondes : une pente
            // nulle ou une puissance de deux rendraient le décalage exact des
            // deux côtés sans rien éprouver de l'accumulation.
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

/// Le test de profondeur est **non signé**, et c'est le piège de SSE2.
///
/// `_mm_cmpgt_epi32` compare en signé : sans le biais appliqué aux deux côtés,
/// toute profondeur dont le bit de poids fort est posé — c'est-à-dire tout ce
/// qui est proche de la caméra — se comparerait à l'envers. Ce cas place
/// exprès des valeurs des deux côtés de `0x8000_0000`.
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

/// Une pente négative descend comme le scalaire.
///
/// Écrit à part parce que le décalage est **logique** et non arithmétique : sur
/// une valeur dont le bit de poids fort est posé, les deux ne rendent pas la
/// même chose, et c'est le genre d'écart qu'un tirage aléatoire peut manquer si
/// ses valeurs restent petites.
#[test]
fn une_pente_negative_suit_le_scalaire() {
    let mut seed = Seed(0x1357_9bdf_0246_8ace);
    let (mut color, mut depth) = peuple(&mut seed, 17);
    let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
    let (start, step) = (i64::MAX / 2, -(1 << 30));

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
