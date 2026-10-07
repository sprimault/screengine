// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la variante SSE2 doit au rasteriseur scalaire.
//!
//! **L'égalité au bit près, et rien d'autre.** Une divergence est une variante
//! fausse, jamais une différence acceptable : ces cas comparent donc la variante
//! à l'expression du scalaire plutôt qu'à des valeurs écrites à la main, qui ne
//! diraient que ce qu'on y aurait mis.

use alloc::vec;
use alloc::vec::Vec;

use super::depths;
use crate::raster::plane::GRADIENT_BITS;

/// Ce que le chemin scalaire calcule, repris ici mot pour mot.
///
/// **Recopié plutôt que partagé, et c'est l'exception qui confirme la règle** :
/// la duplication entre le scalaire et ses variantes est voulue sur ce projet,
/// parce que le premier est la référence contre laquelle les secondes se
/// valident. Les fondre supprimerait la référence, et ce test ne comparerait
/// plus qu'une expression à elle-même.
fn scalar(depth: i64, depth_x: i64, count: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(count);
    let mut depth = depth;
    for _ in 0..count {
        out.push((depth >> GRADIENT_BITS) as u32);
        depth = depth.wrapping_add(depth_x);
    }
    out
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

/// Les deux chemins rendent les mêmes bits, sur des longueurs paires et impaires.
///
/// **Les deux parités comptent**, et c'est le défaut que la forme vectorielle
/// invite à écrire : la boucle avance de deux, donc un span de longueur impaire
/// laisse un pixel que seule la queue traite. Une variante qui l'oublierait
/// rendrait juste sur la moitié des lignes d'un triangle.
#[test]
fn les_deux_chemins_rendent_les_memes_bits() {
    let mut seed = Seed(0x5eed_1234_abcd_ef01);
    for count in 0..34usize {
        for _ in 0..8 {
            // Des profondeurs quelconques, pas des valeurs rondes : une pente
            // nulle ou une puissance de deux rendraient le décalage exact des
            // deux côtés sans rien éprouver de l'accumulation.
            let depth = seed.next() as i64;
            let depth_x = (seed.next() as i64) >> 20;
            let mut out = vec![0u32; count];
            depths(depth, depth_x, &mut out);
            assert_eq!(
                out,
                scalar(depth, depth_x, count),
                "divergence sur {count} pixels, depth={depth}, depth_x={depth_x}"
            );
        }
    }
}

/// Un span vide ne lit ni n'écrit rien.
///
/// Il arrive : le remplissage borne le span par la fenêtre, et une ligne du
/// triangle peut tomber entièrement hors de la tuile courante.
#[test]
fn un_span_vide_ne_touche_rien() {
    let mut out: [u32; 0] = [];
    depths(1 << 40, -7, &mut out);
}

/// Une pente négative descend comme le scalaire.
///
/// Écrit à part parce que le décalage est **logique** et non arithmétique : sur
/// une valeur dont le bit de poids fort est posé, les deux ne rendent pas la
/// même chose, et c'est le genre d'écart qu'un tirage aléatoire peut manquer si
/// ses valeurs restent petites.
#[test]
fn une_pente_negative_suit_le_scalaire() {
    let depth = i64::MAX / 2;
    let depth_x = -(1 << 30);
    let mut out = vec![0u32; 17];
    depths(depth, depth_x, &mut out);
    assert_eq!(out, scalar(depth, depth_x, 17));
}

/// L'accumulation enveloppe comme celle du scalaire.
///
/// Le remplissage borne la profondeur par `to_depth` avec une marge, donc ce cas
/// ne se produit pas sur un pixel couvert ; il est ici parce que les deux
/// chemins doivent rester comparables **partout**, et qu'une variante qui
/// saturerait là où l'autre enveloppe divergerait sans qu'aucune scène le dise.
#[test]
fn l_accumulation_enveloppe_comme_le_scalaire() {
    let depth = i64::MAX - 3;
    let depth_x = 1 << 40;
    let mut out = vec![0u32; 9];
    depths(depth, depth_x, &mut out);
    assert_eq!(out, scalar(depth, depth_x, 9));
}
