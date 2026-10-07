// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la variante AVX2 doit au rasteriseur scalaire.
//!
//! **Chaque cas se saute quand la machine ne porte pas AVX2**, et c'est la
//! différence avec les cas de SSE2, qui tournent partout sur `x86_64`. Un test
//! qui s'ignore en silence est un test qui ne garde rien : celui-ci ne peut pas
//! faire autrement — la précondition est matérielle —, mais la conformance le
//! rattrape, sa passe AVX2 se sautant au même endroit et pour la même raison.

use alloc::vec;
use alloc::vec::Vec;

use super::depths_if_available;
use crate::raster::plane::GRADIENT_BITS;

/// Ce que le chemin scalaire calcule, repris ici mot pour mot.
///
/// Recopié plutôt que partagé, comme pour SSE2 : la duplication entre le
/// scalaire et ses variantes est voulue sur ce projet, parce que le premier est
/// la référence contre laquelle les secondes se valident.
fn scalar(depth: i64, depth_x: i64, count: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(count);
    let mut depth = depth;
    for _ in 0..count {
        out.push((depth >> GRADIENT_BITS) as u32);
        depth = depth.wrapping_add(depth_x);
    }
    out
}

/// Appelle la variante quand la machine la porte.
///
/// Rend `None` quand elle ne la porte pas, pour qu'un cas dise « sauté » plutôt
/// que « réussi ».
fn try_depths(depth: i64, depth_x: i64, count: usize) -> Option<Vec<u32>> {
    let mut out = vec![0u32; count];
    depths_if_available(depth, depth_x, &mut out).then_some(out)
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

/// Les deux chemins rendent les mêmes bits, sur les quatre restes possibles.
///
/// **Les quatre parités comptent**, et c'est le défaut que la forme vectorielle
/// invite à écrire : la boucle avance de quatre, donc une ligne dont la longueur
/// n'est pas un multiple de quatre laisse un à trois pixels que seule la queue
/// traite. Une variante qui l'oublierait rendrait juste sur un quart des lignes
/// d'un triangle — et les trois autres quarts se verraient.
#[test]
fn les_deux_chemins_rendent_les_memes_bits() {
    let mut seed = Seed(0xa1b2_c3d4_e5f6_0789);
    for count in 0..40usize {
        for _ in 0..8 {
            let depth = seed.next() as i64;
            let depth_x = (seed.next() as i64) >> 20;
            let Some(out) = try_depths(depth, depth_x, count) else {
                return;
            };
            assert_eq!(
                out,
                scalar(depth, depth_x, count),
                "divergence sur {count} pixels, depth={depth}, depth_x={depth_x}"
            );
        }
    }
}

/// AVX2 et SSE2 rendent les mêmes bits, puisque tous deux rendent ceux du
/// scalaire.
///
/// **Écrit bien que l'égalité avec le scalaire l'implique**, et pas par excès de
/// zèle : les deux variantes diffèrent par le nombre de voies, donc par la
/// manière dont les profondeurs de départ sont posées. Un décalage de voie qui
/// serait faux des deux côtés de la même façon passerait les deux cas
/// précédents ; il ne passerait pas celui-ci sur une longueur qui n'est multiple
/// ni de deux ni de quatre.
#[test]
fn avx2_et_sse2_s_accordent() {
    let mut seed = Seed(0x0f1e_2d3c_4b5a_6978);
    for count in [1usize, 3, 5, 7, 9, 11, 13, 17, 23] {
        let depth = seed.next() as i64;
        let depth_x = (seed.next() as i64) >> 20;
        let Some(wide) = try_depths(depth, depth_x, count) else {
            return;
        };
        let mut narrow = vec![0u32; count];
        crate::raster::simd::sse2::depths(depth, depth_x, &mut narrow);
        assert_eq!(
            wide, narrow,
            "divergence entre AVX2 et SSE2 sur {count} pixels"
        );
    }
}

/// Une pente négative descend comme le scalaire.
///
/// Le décalage est **logique** et non arithmétique : sur une valeur dont le bit
/// de poids fort est posé, les deux ne rendent pas la même chose, et un tirage
/// aléatoire aux valeurs modestes peut manquer l'écart.
#[test]
fn une_pente_negative_suit_le_scalaire() {
    let depth = i64::MAX / 2;
    let depth_x = -(1 << 30);
    let Some(out) = try_depths(depth, depth_x, 19) else {
        return;
    };
    assert_eq!(out, scalar(depth, depth_x, 19));
}

/// L'accumulation enveloppe comme celle du scalaire.
///
/// Le remplissage borne la profondeur avec une marge, donc ce cas ne se produit
/// pas sur un pixel couvert ; il est ici parce que les deux chemins doivent
/// rester comparables **partout**, et qu'une variante qui saturerait là où
/// l'autre enveloppe divergerait sans qu'aucune scène le dise.
#[test]
fn l_accumulation_enveloppe_comme_le_scalaire() {
    let depth = i64::MAX - 5;
    let depth_x = 1 << 40;
    let Some(out) = try_depths(depth, depth_x, 11) else {
        return;
    };
    assert_eq!(out, scalar(depth, depth_x, 11));
}
