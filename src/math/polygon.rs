// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce dont un calcul sur un polygone plan a besoin et que [`Vec3`] ne porte pas.
//!
//! Trois fonctions, extraites le jour où elles ont eu leur troisième appelant :
//! la triangulation par découpe d'oreilles, les contrôles de repère du
//! chargement et le calcul des lightmaps les écrivaient chacun de son côté, à
//! l'expression près. Elles rendent donc exactement les mêmes bits qu'avant, ce
//! qui est la condition pour qu'aucune empreinte ne bouge.
//!
//! Ce qui **n'est pas** ici, et c'est délibéré : le choix des deux axes sur
//! lesquels projeter. Il paraît commun aux mêmes appelants et ne l'est pas —
//! voir `format::ears::dominant_axes` et `world::bake::plane_axes`, qui disent
//! chacune ce qui interdit de les fondre.

use super::Vec3;

/// La normale de Newell d'un polygone, sommée dans l'ordre de ses sommets.
///
/// Newell plutôt qu'un produit vectoriel sur les trois premiers sommets : ces
/// trois-là peuvent être alignés, auquel cas le produit est nul et l'orientation
/// perdue, alors que la somme porte sur toutes les arêtes. L'ordre des sommets
/// est celui du fichier, et l'ordre d'opérations d'une valeur dérivée entre dans
/// le rendu : il est contractuel.
///
/// Le vecteur rendu n'est pas unitaire — sa longueur vaut le double de l'aire du
/// polygone —, et c'est l'appelant qui décide s'il le normalise.
pub(crate) fn newell(points: &[Vec3]) -> Vec3 {
    let mut normal = Vec3::ZERO;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        normal.x += (a.y - b.y) * (a.z + b.z);
        normal.y += (a.z - b.z) * (a.x + b.x);
        normal.z += (a.x - b.x) * (a.y + b.y);
    }
    normal
}

/// Une composante d'un vecteur, par son rang.
///
/// Les deux axes d'un plan de projection se désignent par leur rang, et non par
/// un champ : c'est ce qui permet à une même boucle de servir les trois plans.
pub(crate) fn axis(v: Vec3, index: usize) -> f32 {
    match index {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// La valeur absolue.
///
/// `f32::abs` vit dans `std`, que le noyau n'a pas. Une implémentation par
/// masque de bit serait la même partout — celle-ci l'est aussi, et se lit.
pub(crate) fn abs(value: f32) -> f32 {
    if value < 0.0 { -value } else { value }
}

#[cfg(test)]
mod tests;
