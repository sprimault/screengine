// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le balayage de force brute : toutes les cellules, toutes leurs surfaces.
//!
//! **Ce n'est pas un outil de test, c'est la moitié d'un oracle.** Le chemin
//! rapide suit les portails et s'arrête à une borne ; celui-ci ne suit rien et ne
//! s'arrête jamais. Sur une carte bien formée, les deux doivent rendre
//! exactement les mêmes bits, et cette égalité est le seul contrôle qui attrape
//! une traversée trop étroite — exactement comme le chemin brut de la traversée
//! de rendu attrape une fenêtre trop étroite à l'étape 5.
//!
//! Ce qu'il n'attrape pas, et c'est pourquoi il ne suffit pas seul : **une
//! formule de balayage fausse rend les deux chemins faux de la même façon**, et
//! l'égalité reste verte. C'est le prédicat statique qui ferme ce cas, et il ne
//! partage aucune algèbre avec ce qui précède.
//!
//! Il n'a pas de borne de cellules : il les visite toutes, donc il ne rend jamais
//! de résultat tronqué. Une comparaison avec le chemin rapide sur un décor qui
//! atteint la borne comparerait deux choses différentes, et c'est au décor de
//! validation de rester en deçà.

use crate::format::World;
use crate::math::Vec3d;

use super::{Best, Hit, grown, start_solid, sweep_cell};

/// Balaie une boîte contre **toutes** les surfaces solides de la carte.
///
/// La cellule de départ n'entre pas : il n'y a pas de traversée à amorcer. C'est
/// aussi ce qui fait de ce chemin une référence indépendante — il ne peut pas se
/// tromper de cellule, n'en connaissant aucune.
pub(crate) fn sweep_brute(world: &World, half: Vec3d, from: Vec3d, to: Vec3d) -> Hit {
    // **La dilatation s'applique ici aussi**, et l'oublier a été le premier
    // défaut que l'égalité des deux chemins ait attrapé : sans elle, l'oracle
    // comparait deux boîtes de tailles différentes et divergeait de la marge.
    let grown_half = grown(half);
    let mut best = Best::new(to);

    // Dans l'ordre du fichier, qui est le troisième critère de départage : deux
    // contacts au même instant et de même famille se tranchent par lui, et un
    // parcours d'un autre ordre rendrait une autre normale.
    for cell in world.cells() {
        sweep_cell(cell, grown_half, from, to, &mut best);
        start_solid(cell, half, from, &mut best);
    }
    best.hit
}
