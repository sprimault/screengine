// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le rangement des lightmaps d'une cellule dans une seule image.
//!
//! **Un atlas par cellule, et non une texture par surface.** C'est la table des
//! textures du contexte qui l'impose : elle se parcourt linéairement à chaque lot
//! et plafonne à 65 535 entrées, si bien que deux mille surfaces y feraient deux
//! millions de comparaisons par image pour un contenu qui tient dans une seule
//! image par cellule.
//!
//! **Les sous-rectangles sont des puissances de deux alignées sur leur propre
//! taille**, et c'est ce qui rend l'atlas *exact au mipmap* : aucune réduction 2×2
//! ne traverse la frontière d'un sous-rectangle, donc la chaîne de l'atlas est
//! identique, texel pour texel, à celle que des textures séparées auraient
//! donnée. Le rangement devient alors un choix de mémoire sans effet sur l'image
//! — ce qui n'est vrai d'aucun autre rangement.
//!
//! Écartée : une gouttière dimensionnée pour une chaîne complète, qui coûterait la
//! moitié du côté de chaque rectangle en luxels de garde. Écartée aussi, une
//! chaîne tronquée : elle ne supprime pas le saignement, elle le borne.

// Ce module range, il n'éclaire pas : son premier appelant est le calcul des
// lightmaps, qui remplira les rectangles posés ici. Il se livre avant lui parce
// qu'il se vérifie sur des propriétés — alignement, recouvrement, gouttière — que
// personne ne saurait juger sur une image, alors qu'un atlas faux sous un
// éclairage plausible ne se verrait pas.
#![allow(dead_code)]

use alloc::vec::Vec;

use crate::buffer::reserved;
use crate::error::{Error, Malformation, Result};
use crate::format::world::Cell;

/// La gouttière autour de chaque surface, en luxels.
///
/// Un luxel de chaque côté, rempli par recopie du bord : c'est ce que le
/// bilinéaire du rasteriseur va chercher quand il échantillonne le bord d'une
/// surface, et sans lui il irait chercher le voisin dans l'atlas.
pub(crate) const GUTTER: u32 = 1;

/// Le côté maximal d'un atlas, en luxels.
pub(crate) const MAX_ATLAS: u32 = 1024;

/// Où la lightmap d'une surface est rangée dans l'atlas de sa cellule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Slot {
    /// L'abscisse du coin du rectangle, gouttière comprise.
    pub(crate) x: u32,
    /// Son ordonnée.
    pub(crate) y: u32,
    /// Son côté en `u`, puissance de deux, gouttière comprise.
    pub(crate) width: u32,
    /// Son côté en `v`.
    pub(crate) height: u32,
}

/// Le rangement des surfaces d'une cellule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Atlas {
    /// Le côté de l'image, une puissance de deux.
    pub(crate) side: u32,
    /// Un emplacement par surface, dans l'ordre de la cellule.
    pub(crate) slots: Vec<Slot>,
}

/// La plus petite puissance de deux supérieure ou égale, au moins un.
fn round_up(value: u32) -> u32 {
    if value <= 1 {
        return 1;
    }
    1 << (u32::BITS - (value - 1).leading_zeros())
}

/// Range les surfaces d'une cellule et rend l'atlas.
///
/// **L'ordre de rangement entre dans l'image** — il décide des coordonnées que la
/// soumission ajoutera —, donc il est contractuel : les surfaces se posent par
/// classe de taille décroissante, et à taille égale par rang dans la cellule. Le
/// tri est stable, si bien que deux constructions rangent la même cellule de la
/// même façon.
///
/// Chaque rectangle se pose au **premier emplacement libre balayé en lignes, à
/// l'alignement de sa propre taille**. Un balayage et non un algorithme de
/// placement plus fin : le gaspillage d'un atlas de quelques dizaines de
/// rectangles est de l'ordre du quart, pour une ressource qui occupe quelques
/// dizaines de kilo-octets, et une heuristique plus serrée coûterait une seconde
/// liste de règles à figer pour toujours.
pub(crate) fn pack(cell: &Cell) -> Result<Atlas> {
    let mut sizes = reserved(cell.surfaces.len())?;
    for (rank, surface) in cell.surfaces.iter().enumerate() {
        let width = round_up(surface.luxels.width + GUTTER * 2);
        let height = round_up(surface.luxels.height + GUTTER * 2);
        sizes.push((rank, width, height));
    }

    // Décroissant par côté le plus long, puis par l'autre, puis par rang : les
    // grands rectangles se posent d'abord, ce qui laisse les interstices aux
    // petits plutôt que l'inverse.
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| {
        let (rank, w, h) = sizes[i];
        let (long, short) = if w >= h { (w, h) } else { (h, w) };
        (core::cmp::Reverse(long), core::cmp::Reverse(short), rank)
    });

    // Le côté de départ : celui qui contiendrait toutes les surfaces bout à bout
    // si elles se rangeaient parfaitement. Il double tant que le rangement échoue,
    // ce qui termine — le plafond est atteint au pire après quelques tours.
    let area: u64 = sizes
        .iter()
        .map(|&(_, w, h)| u64::from(w) * u64::from(h))
        .sum();
    let longest = sizes.iter().map(|&(_, w, h)| w.max(h)).max().unwrap_or(1);
    let mut side = round_up(longest);
    while u64::from(side) * u64::from(side) < area {
        side *= 2;
    }

    loop {
        if let Some(slots) = try_pack(&sizes, &order, side) {
            return Ok(Atlas { side, slots });
        }
        side *= 2;
        if side > MAX_ATLAS {
            // Refusé ici et non au chargement : c'est le rangement qui décide, et
            // une surface seule peut tenir sous son plafond sans que la cellule
            // entière y tienne.
            return Err(Error::InvalidFormat(Malformation::Mapping));
        }
    }
}

/// Tente de ranger toutes les surfaces dans un atlas de ce côté.
///
/// Rend `None` dès qu'un rectangle ne trouve pas sa place : l'appelant double le
/// côté et recommence, plutôt que de poser ce qui rentre et d'abandonner le reste.
fn try_pack(sizes: &[(usize, u32, u32)], order: &[usize], side: u32) -> Option<Vec<Slot>> {
    let mut placed: Vec<(usize, Slot)> = Vec::new();
    for &i in order {
        let (_, width, height) = sizes[i];
        if width > side || height > side {
            return None;
        }
        let slot = find(&placed, side, width, height)?;
        placed.push((i, slot));
    }

    let mut slots = alloc::vec![
        Slot {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        sizes.len()
    ];
    for (i, slot) in placed {
        slots[i] = slot;
    }
    Some(slots)
}

/// Le premier emplacement libre pour un rectangle de cette taille.
///
/// **Aligné sur sa propre taille**, ce qui est la clause dont dépend l'exactitude
/// du mipmap : un rectangle de 16 posé à une abscisse multiple de 16 voit chacune
/// de ses réductions rester à l'intérieur de lui-même.
fn find(placed: &[(usize, Slot)], side: u32, width: u32, height: u32) -> Option<Slot> {
    let mut y = 0;
    while y + height <= side {
        let mut x = 0;
        while x + width <= side {
            let candidate = Slot {
                x,
                y,
                width,
                height,
            };
            if !placed.iter().any(|(_, other)| overlaps(candidate, *other)) {
                return Some(candidate);
            }
            x += width;
        }
        y += height;
    }
    None
}

/// Vrai si deux emplacements se recouvrent.
fn overlaps(a: Slot, b: Slot) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

#[cfg(test)]
mod tests;
