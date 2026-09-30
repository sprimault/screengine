// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le prédicat de recouvrement : cette boîte touche-t-elle cette surface ?
//!
//! **C'est l'autre moitié de l'oracle, et la seule qui ne partage aucune algèbre
//! avec le balayage.** Le chemin de force brute valide la traversée, mais une
//! formule de balayage fausse rendrait les deux chemins faux de la même façon et
//! l'égalité resterait verte. Celui-ci part d'ailleurs : des **axes séparateurs**
//! là où le balayage découpe un intervalle, et de la **triangulation** là où le
//! balayage teste le polygone.
//!
//! Que la triangulation convienne ici et pas au balayage n'est pas une
//! incohérence, c'est le fond de l'affaire. Un recouvrement est une question de
//! **volume**, et l'union des triangles est exactement le polygone ; un balayage
//! est une question de **surface du volume dilaté**, où chaque arête interne de
//! la découpe ajouterait un croc qui n'existe pas. Le premier ne voit pas la
//! découpe, le second la verrait.
//!
//! Les axes séparateurs exigent des convexes, ce qu'un triangle est toujours et
//! qu'une surface n'est pas : c'est la seconde raison de passer par elle.
//!
//! Il sert aussi au balayage lui-même, pour le seul cas qu'un segment ne peut pas
//! rendre : **la boîte déjà en intersection au départ**.

use crate::format::world::{Cell, Surface};
use crate::math::Vec3d;

use super::shape::support;

/// La pénétration d'une boîte dans une surface, le long de la normale du plan.
///
/// `None` quand elles ne se touchent pas. La valeur rendue est positive et vaut
/// ce qu'il faudrait reculer le long de la normale pour n'être plus en contact :
/// c'est elle qui départage deux surfaces quand la boîte part dans le solide,
/// **la moindre pénétration gagnant**.
pub(crate) fn penetration(
    cell: &Cell,
    surface: &Surface,
    half: Vec3d,
    centre: Vec3d,
) -> Option<f64> {
    let normal = Vec3d::from(cell.inward(surface));
    let anchor = Vec3d::from(cell.vertices[surface.first_vertex as usize].position);
    let square = normal.dot(normal);
    if square <= 0.0 {
        return None;
    }

    let first = surface.first_triangle as usize;
    let count = surface.triangle_count as usize;
    let mut touched = false;
    for triangle in &cell.triangles[first..first + count] {
        let corners = [
            Vec3d::from(cell.vertices[triangle[0] as usize].position),
            Vec3d::from(cell.vertices[triangle[1] as usize].position),
            Vec3d::from(cell.vertices[triangle[2] as usize].position),
        ];
        if overlaps_triangle(corners, half, centre) {
            touched = true;
            break;
        }
    }
    if !touched {
        return None;
    }

    // La profondeur se mesure sur la normale du plan, la seule direction qui ait
    // un sens pour une surface : le long des axes de la boîte, une face
    // rasante donnerait une profondeur énorme et sans rapport avec ce dont il
    // faut se dégager.
    let distance = normal.dot(centre - anchor);
    let reach = support(normal, half);
    let depth = (reach - abs(distance)) / length_of(square);
    Some(if depth < 0.0 { 0.0 } else { depth })
}

/// La boîte recouvre-t-elle ce triangle ?
///
/// Axes séparateurs : la normale du triangle, les trois axes de la boîte, et les
/// neuf produits croisés de ses arêtes par ces axes. Treize axes, et un seul qui
/// sépare suffit à conclure.
fn overlaps_triangle(corners: [Vec3d; 3], half: Vec3d, centre: Vec3d) -> bool {
    let v = [
        corners[0] - centre,
        corners[1] - centre,
        corners[2] - centre,
    ];
    let edges = [v[1] - v[0], v[2] - v[1], v[0] - v[2]];

    // Les trois axes de la boîte.
    for i in 0..3 {
        let (low, high) = span(&v, i);
        if low > half.axis(i) || high < -half.axis(i) {
            return false;
        }
    }

    // La normale du triangle.
    let normal = edges[0].cross(edges[1]);
    if normal != Vec3d::ZERO {
        let distance = normal.dot(v[0]);
        if abs(distance) > support(normal, half) {
            return false;
        }
    }

    // Les neuf produits croisés.
    for edge in &edges {
        for i in 0..3 {
            let mut axis = Vec3d::ZERO;
            set_axis(&mut axis, i, 1.0);
            let normal = edge.cross(axis);
            if normal == Vec3d::ZERO {
                continue;
            }
            let projections = [normal.dot(v[0]), normal.dot(v[1]), normal.dot(v[2])];
            let mut low = projections[0];
            let mut high = projections[0];
            for value in &projections[1..] {
                if *value < low {
                    low = *value;
                }
                if *value > high {
                    high = *value;
                }
            }
            let reach = support(normal, half);
            if low > reach || high < -reach {
                return false;
            }
        }
    }
    true
}

/// L'étendue des trois sommets sur un axe de la boîte.
fn span(v: &[Vec3d; 3], index: usize) -> (f64, f64) {
    let mut low = v[0].axis(index);
    let mut high = low;
    for point in &v[1..] {
        let value = point.axis(index);
        if value < low {
            low = value;
        }
        if value > high {
            high = value;
        }
    }
    (low, high)
}

/// La longueur d'un vecteur dont on a le carré, par une itération de Newton.
///
/// **Aucune racine de bibliothèque** : la table du noyau est en simple précision
/// et ne rendrait pas les mêmes bits. Une approximation par bissection des
/// exposants, affinée par Newton, suffit — la valeur ne sert qu'à convertir une
/// profondeur en unités de monde, jamais à décider d'un contact.
fn length_of(square: f64) -> f64 {
    if square <= 0.0 {
        return 1.0;
    }
    // Le point de départ prend la moitié de l'exposant, ce qui place l'estimation
    // à moins d'un facteur deux de la racine quel que soit l'ordre de grandeur.
    let bits = square.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mut guess = f64::from_bits((((exponent / 2) + 1023) as u64) << 52);
    // Six itérations. L'erreur relative de l'estimation initiale vaut jusqu'à 1
    // — la division entière de l'exposant tronque vers zéro, donc un exposant
    // impair négatif perd un cran —, et Newton la carre à chaque tour : quatre
    // ne suffisent pas sur les grands ordres de grandeur, où l'erreur restait à
    // 1,6 × 10⁻⁸. La sixième la met sous le dernier bit.
    for _ in 0..6 {
        guess = (guess + square / guess) * 0.5;
    }
    guess
}

/// La valeur absolue, écrite plutôt qu'empruntée à la bibliothèque du système.
fn abs(value: f64) -> f64 {
    if value < 0.0 { -value } else { value }
}

/// Pose une composante d'un vecteur, par son rang.
fn set_axis(v: &mut Vec3d, index: usize, value: f64) {
    match index {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
}

#[cfg(test)]
mod tests;
