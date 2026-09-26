// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La traversée : quelles cellules une caméra voit, et par quelle fenêtre.
//!
//! **Deux fenêtres, qu'il ne faut pas confondre.** Celle de **propagation** vit
//! sur la pile, se réduit à chaque portail franchi, et décide des cellules
//! visitées : elle est propre à un chemin et ne s'agrège jamais. Celle de
//! **bornage** est ce que le remplissage recevra : elle est propre à une cellule,
//! union des fenêtres par lesquelles on l'a atteinte, ce qui garde une seule
//! soumission par cellule.
//!
//! Agréger la première ferait dégénérer la fenêtre à la taille de la cellule dès
//! que deux ouvertures écartées y mènent ; unir la seconde ne peut que
//! sur-dessiner, et le sur-dessin d'une boîte coûte quelques millièmes d'image.
//!
//! **Ce module ne dessine pas et ne soumet rien.** Il rend une liste de visites,
//! que la soumission relit dans l'ordre du fichier — l'ordre qui départage deux
//! surfaces coplanaires, et qui ne doit donc pas dépendre de la position de la
//! caméra.

use crate::format::World;
use crate::math::{Affine3, Projection};
use crate::raster::Rect;

use super::window::reduce;

/// La profondeur maximale de la pile de traversée.
///
/// Ce n'est pas un réglage de performance mais une **garantie de terminaison** :
/// on ne peut pas démontrer géométriquement qu'une fenêtre finit vide en
/// franchissant un cycle de cellules, donc la borne est ce qui l'assure. Elle ne
/// se configure pas — l'atteindre tronque l'image, et une image qui dépendrait
/// d'un champ de configuration échapperait à la conformance.
///
/// Soixante-quatre parce qu'une enfilade réelle en franchit quatre à huit, une
/// galerie dessinée exprès une douzaine, et qu'au-delà de vingt on ne traverse
/// plus un décor, on tourne en rond. Le coût d'une profondeur de plus est une
/// entrée de pile et zéro pixel.
pub(crate) const MAX_DEPTH: usize = 64;

/// Le nombre maximal de visites qu'une image peut porter.
///
/// Une visite est un couple (cellule, fenêtre) : la même cellule atteinte par
/// deux chemins en produit deux, qui fusionnent ensuite. La borne dimensionne la
/// liste à la création du contexte, parce que ni la carte — que le contexte ne
/// connaît pas alors — ni la soumission — où toute allocation est interdite — ne
/// peuvent la porter.
///
/// Quatre mille quatre-vingt-seize est hors d'atteinte d'un décor de cette
/// classe, où une image montre quelques dizaines de cellules. L'atteindre rend le
/// même statut que la profondeur : l'image est tronquée, et elle le dit.
pub(crate) const MAX_VISITS: usize = 4096;

/// Une cellule retenue par la traversée, et par quelle fenêtre la dessiner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Visit {
    /// L'index de la cellule dans la carte, qui est son rang dans le fichier.
    pub(crate) cell: u32,
    /// Le premier triangle qu'elle a préparé, rempli à la soumission.
    ///
    /// Nul pendant la traversée : c'est la soumission qui le sait, et c'est par
    /// lui que le remplissage retrouve la fenêtre d'un triangle.
    pub(crate) first_triangle: u32,
    /// La fenêtre de bornage, union des chemins qui ont mené ici.
    pub(crate) window: Rect,
}

/// Une entrée de la pile de traversée.
struct Frame {
    /// L'index de la cellule explorée.
    cell: u32,
    /// Sa fenêtre de propagation, propre à ce chemin.
    window: Rect,
    /// Le prochain portail à examiner.
    ///
    /// La récursion est dépliée : sans ce curseur, il faudrait empiler la liste
    /// des portails restants, ou rappeler la fonction et perdre la borne de
    /// profondeur au profit de la pile du thread.
    cursor: usize,
}

/// Explore le monde depuis une cellule et remplit la liste des visites.
///
/// Rend `true` quand l'exploration a été tronquée — profondeur ou nombre de
/// visites atteints. L'image reste alors complète de tout ce qui a été atteint,
/// et ce qui manque commence une cellule plus loin.
///
/// `visits` est vidée à l'entrée et triée à la sortie : ses entrées sortent dans
/// l'ordre des index de cellules, qui est celui du fichier. Les fenêtres d'une
/// même cellule y sont déjà fusionnées.
pub(crate) fn traverse(
    world: &World,
    start: u32,
    window: Rect,
    view: Affine3,
    projection: &Projection,
    visits: &mut alloc::vec::Vec<Visit>,
) -> bool {
    visits.clear();
    let cells = world.cells();
    if start as usize >= cells.len() || window.width == 0 || window.height == 0 {
        return false;
    }

    let mut stack: [Frame; MAX_DEPTH] = core::array::from_fn(|_| Frame {
        cell: 0,
        window: Rect::EMPTY,
        cursor: 0,
    });
    let mut depth = 1;
    stack[0] = Frame {
        cell: start,
        window,
        cursor: 0,
    };
    let mut truncated = !push_visit(visits, start, window);

    while depth > 0 {
        let top = depth - 1;
        let cell = &cells[stack[top].cell as usize];
        if stack[top].cursor >= cell.portals.len() {
            depth -= 1;
            continue;
        }

        let portal = &cell.portals[stack[top].cursor];
        stack[top].cursor += 1;

        // Un portail non apparié est un mur : il n'y a rien derrière lui.
        let Some((next, _)) = portal.link else {
            continue;
        };

        // **Une cellule déjà sur le chemin courant ne se reprend pas.** C'est
        // l'élagage exact et gratuit des cycles : il se lit en parcourant la
        // pile, au plus soixante-quatre comparaisons d'entiers, sans rien
        // allouer et sans table indexée par cellule. Une cellule atteinte par un
        // autre chemin reste légitime, et c'est ce qui garde l'élimination fine.
        if stack[..depth].iter().any(|frame| frame.cell == next) {
            continue;
        }

        let reduced = reduce(stack[top].window, &portal.points, view, projection);
        // Une fenêtre vide coupe la branche : c'est le terminateur normal de la
        // traversée, et de loin le plus fréquent.
        if reduced.width == 0 || reduced.height == 0 {
            continue;
        }

        if !push_visit(visits, next, reduced) {
            truncated = true;
            continue;
        }

        if depth == MAX_DEPTH {
            // La cellule est dessinée — sa visite vient d'être enregistrée —,
            // seuls ses portails ne seront pas dépliés : ce qui manque commence
            // donc une cellule plus loin que la borne.
            truncated = true;
            continue;
        }
        stack[depth] = Frame {
            cell: next,
            window: reduced,
            cursor: 0,
        };
        depth += 1;
    }

    merge(visits);
    truncated
}

/// Enregistre une visite, ou dit que la liste est pleine.
fn push_visit(visits: &mut alloc::vec::Vec<Visit>, cell: u32, window: Rect) -> bool {
    if visits.len() == visits.capacity() {
        return false;
    }
    visits.push(Visit {
        cell,
        first_triangle: 0,
        window,
    });
    true
}

/// Trie les visites par cellule et fusionne les fenêtres d'une même cellule.
///
/// Le tri rend l'ordre du fichier, que la soumission exige : c'est lui qui
/// départage deux surfaces coplanaires, et en ordre de traversée il dépendrait de
/// la position de la caméra. La fusion se lit en une passe adjacente, comme
/// l'unicité des identifiants du décodeur, et compacte en place — l'écriture
/// reste toujours derrière la lecture.
fn merge(visits: &mut alloc::vec::Vec<Visit>) {
    visits.sort_unstable_by_key(|visit| visit.cell);

    let mut write = 0;
    for read in 0..visits.len() {
        if write > 0 && visits[write - 1].cell == visits[read].cell {
            let union = cover(visits[write - 1].window, visits[read].window);
            visits[write - 1].window = union;
            continue;
        }
        visits[write] = visits[read];
        write += 1;
    }
    visits.truncate(write);
}

/// Le plus petit rectangle qui contient les deux.
///
/// Une union de rectangles n'est pas un rectangle : celui-ci est leur enveloppe,
/// donc il sur-dessine. C'est admissible ici et nulle part ailleurs — une fenêtre
/// de bornage trop large rend exactement la même image, seule une fenêtre trop
/// étroite troue.
fn cover(a: Rect, b: Rect) -> Rect {
    if a.width == 0 || a.height == 0 {
        return b;
    }
    if b.width == 0 || b.height == 0 {
        return a;
    }
    let x = if a.x < b.x { a.x } else { b.x };
    let y = if a.y < b.y { a.y } else { b.y };
    let right = {
        let (ar, br) = (a.x + a.width, b.x + b.width);
        if ar > br { ar } else { br }
    };
    let bottom = {
        let (ab, bb) = (a.y + a.height, b.y + b.height);
        if ab > bb { ab } else { bb }
    };
    Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

#[cfg(test)]
mod tests;
