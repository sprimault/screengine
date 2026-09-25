// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La triangulation d'un polygone plan simple, par découpe d'oreilles.
//!
//! **Ce n'est pas la compilation de cartes que le projet écarte.** Celle-ci
//! découpe une surface *contre d'autres surfaces*, ce qui lie chaque face à ses
//! voisines et rend l'édition à chaud impraticable ; une triangulation est
//! locale à un polygone, ne crée aucun sommet, et se refait pour cette surface
//! seule quand elle change.
//!
//! Exiger la convexité éviterait tout ceci, et renverrait la subdivision d'une
//! face en L — que la moindre extrusion produit — à un éditeur qui n'existe pas
//! encore.

use crate::math::Vec3;

/// Le plus grand nombre de sommets qu'une surface peut porter.
///
/// Le chargement alloue sur ce plafond et la découpe est quadratique : soixante
/// quatre sommets font au pire quatre mille tours, ce qui se paie une fois au
/// chargement et jamais par image.
pub(crate) const MAX_POLYGON: usize = 64;

/// Les deux axes du plan sur lesquels projeter, choisis par la composante
/// dominante de la normale.
///
/// Projeter sur le plan le moins incliné est ce qui garde l'aire des triangles
/// loin de zéro : un mur vertical projeté sur le sol s'écraserait en segment, et
/// chaque test d'oreille deviendrait un départage d'arrondis.
fn dominant_axes(normal: Vec3) -> (usize, usize) {
    let (x, y, z) = (abs(normal.x), abs(normal.y), abs(normal.z));
    if x >= y && x >= z {
        (1, 2)
    } else if y >= z {
        (2, 0)
    } else {
        (0, 1)
    }
}

/// La valeur absolue, sans passer par la bibliothèque mathématique du système.
///
/// `f32::abs` vit dans `std` : le noyau ne l'a pas, et une implémentation par
/// masque de bit serait la même partout — celle-ci l'est aussi, et se lit.
fn abs(value: f32) -> f32 {
    if value < 0.0 { -value } else { value }
}

/// Une composante d'un vecteur, par son rang.
fn axis(v: Vec3, index: usize) -> f32 {
    match index {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// La normale d'un polygone plan, par la somme de Newell.
///
/// Newell plutôt qu'un produit vectoriel sur les trois premiers sommets : ces
/// trois-là peuvent être alignés, auquel cas le produit est nul et l'orientation
/// perdue, alors que la somme porte sur toutes les arêtes. Elle se somme dans
/// l'ordre des sommets, qui est celui du fichier : l'ordre d'opérations d'une
/// valeur dérivée entre dans le rendu, donc il est contractuel.
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

/// Le double de l'aire signée du triangle `a b c`, dans le plan `(i, j)`.
fn cross(a: Vec3, b: Vec3, c: Vec3, i: usize, j: usize) -> f32 {
    let (ax, ay) = (axis(a, i), axis(a, j));
    let (bx, by) = (axis(b, i), axis(b, j));
    let (cx, cy) = (axis(c, i), axis(c, j));
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

/// Vrai si `p` est dans le triangle `a b c`, bords compris.
fn inside(a: Vec3, b: Vec3, c: Vec3, p: Vec3, i: usize, j: usize, winding: f32) -> bool {
    let sign = |value: f32| if winding < 0.0 { -value } else { value };
    sign(cross(a, b, p, i, j)) >= 0.0
        && sign(cross(b, c, p, i, j)) >= 0.0
        && sign(cross(c, a, p, i, j)) >= 0.0
}

/// Triangule `points`, ou rend `None` si le polygone n'est pas triangulable.
///
/// Rend `points.len() - 2` triplets d'indices dans `points`, dans l'ordre de
/// découpe. L'orientation des triangles est celle du polygone : une surface
/// écrite à l'envers reste à l'envers, et c'est au fichier de la poser à
/// l'endroit.
///
/// `None` pour moins de trois sommets, plus de [`MAX_POLYGON`], une normale
/// nulle — tous les sommets alignés — ou un polygone dont aucun sommet n'est une
/// oreille, ce qui arrive quand il se recoupe lui-même. **Un polygone dégénéré
/// n'est pas une atteinte à la mémoire** : c'est un décor faux, et le décodeur
/// le refuse comme tel.
pub(crate) fn triangulate(points: &[Vec3], out: &mut [[u32; 3]]) -> Option<usize> {
    if points.len() < 3 || points.len() > MAX_POLYGON {
        return None;
    }
    let normal = newell(points);
    if normal.x == 0.0 && normal.y == 0.0 && normal.z == 0.0 {
        return None;
    }
    let (i, j) = dominant_axes(normal);
    // Le signe de l'aire projetée dit dans quel sens tourne le polygone vu de
    // ce plan ; tous les tests d'oreille s'y rapportent.
    let winding = axis(normal, 3 - i - j);

    // Les sommets encore en lice, par leur indice d'origine.
    let mut ring = [0u32; MAX_POLYGON];
    for (slot, index) in ring[..points.len()].iter_mut().enumerate() {
        *index = slot as u32;
    }
    let mut count = points.len();
    let mut written = 0;

    // Chaque tour retire au plus une oreille ; sans oreille sur un tour complet,
    // le polygone n'en a plus, et il se recoupe.
    let mut at = 0;
    let mut since = 0;
    while count > 3 {
        if since >= count {
            return None;
        }
        let (prev, tip, next) = (
            ring[(at + count - 1) % count],
            ring[at],
            ring[(at + 1) % count],
        );
        let (a, b, c) = (
            points[prev as usize],
            points[tip as usize],
            points[next as usize],
        );

        let convex = if winding < 0.0 {
            cross(a, b, c, i, j) <= 0.0
        } else {
            cross(a, b, c, i, j) >= 0.0
        };
        let empty = convex
            && !ring[..count].iter().any(|&other| {
                other != prev
                    && other != tip
                    && other != next
                    && inside(a, b, c, points[other as usize], i, j, winding)
            });

        if empty {
            out[written] = [prev, tip, next];
            written += 1;
            for slot in at..count - 1 {
                ring[slot] = ring[slot + 1];
            }
            count -= 1;
            if at >= count {
                at = 0;
            }
            since = 0;
        } else {
            at = (at + 1) % count;
            since += 1;
        }
    }

    out[written] = [ring[0], ring[1], ring[2]];
    Some(written + 1)
}

#[cfg(test)]
mod tests;
