// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le rangement se teste sur ses propriétés, pas sur une disposition attendue.
//!
//! **La propriété qui compte est l'alignement** : chaque rectangle posé à un
//! multiple de sa propre taille. C'est elle qui rend la chaîne de mipmaps de
//! l'atlas identique à celle qu'auraient donnée des textures séparées, donc elle
//! qui permet de ranger les lightmaps ensemble sans toucher à l'image. Une
//! disposition attendue, elle, se figerait sur un détail d'implémentation.

use alloc::vec::Vec;

use super::*;
use crate::format::World;
use crate::format::world::tests::{cell_bytes, file, frame, material, words};

/// Une carte d'une cellule dont les surfaces ont les étendues de lightmap
/// demandées.
///
/// Les surfaces sont des carrés du plan `z = 0`, dimensionnés par leur repère :
/// un axe de longueur `1 / n` donne `n` luxels sur une unité de monde. Les carrés
/// des longueurs restent des puissances de deux, ce que le chargement exige.
fn cell_with(steps: &[(f32, f32)]) -> World {
    let points = [
        [0.0f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let surfaces: Vec<Vec<u8>> = steps
        .iter()
        .enumerate()
        .map(|(i, (u, v))| {
            let mut bytes = words(&[i as u32 + 1, 0, 1, 4]);
            bytes.extend_from_slice(&words(&[0, 1, 2, 3]));
            bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
            bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], [*u, 0.0, 0.0], [0.0, *v, 0.0]));
            bytes
        })
        .collect();
    let cell = cell_bytes(7, 0, &points, &surfaces, &[]);
    World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide")
}

/// Chaque rectangle est posé à un multiple de sa propre taille.
///
/// **C'est la propriété dont dépend l'exactitude du mipmap**, et donc le droit de
/// ranger les lightmaps d'une cellule dans une seule image : à un multiple de sa
/// taille, aucune réduction 2×2 d'un rectangle ne mord sur son voisin, et la
/// chaîne de l'atlas est celle que des textures séparées auraient donnée.
///
/// **Ce test n'a pas pu être mis en échec**, et il faut le savoir avant de s'y
/// fier : deux mécanismes garantissent l'alignement, et il suffit d'un.
/// [`find`] n'essaie que des positions multiples de la taille du rectangle, et
/// les côtés étant des puissances de deux, un rectangle posé après un plus grand
/// tombe de toute façon sur un multiple du sien. Casser l'un ou l'autre — pas de
/// balayage d'un luxel, tri croissant — laisse le test vert. Il garde sa valeur
/// comme énoncé de la propriété, pas comme preuve qu'elle tient.
#[test]
fn chaque_rectangle_est_aligne_sur_sa_taille() {
    let world = cell_with(&[(1.0, 1.0), (0.25, 0.25), (0.5, 1.0), (0.125, 0.5)]);
    let atlas = pack(&world.cells()[0]).expect("rangement possible");

    for (rank, slot) in atlas.slots.iter().enumerate() {
        assert_eq!(
            slot.x % slot.width,
            0,
            "le rectangle {rank} n'est pas aligné en x : {slot:?}"
        );
        assert_eq!(
            slot.y % slot.height,
            0,
            "le rectangle {rank} n'est pas aligné en y : {slot:?}"
        );
    }
}

/// Les côtés d'un rectangle sont des puissances de deux.
#[test]
fn chaque_rectangle_a_des_cotes_puissance_de_deux() {
    let world = cell_with(&[(1.0, 1.0), (0.25, 0.5), (0.0625, 0.0625)]);
    let atlas = pack(&world.cells()[0]).expect("rangement possible");

    assert!(atlas.side.is_power_of_two(), "le côté de l'atlas");
    for slot in &atlas.slots {
        assert!(slot.width.is_power_of_two(), "{slot:?}");
        assert!(slot.height.is_power_of_two(), "{slot:?}");
    }
}

/// Deux rectangles ne se recouvrent jamais, et tous tiennent dans l'atlas.
#[test]
fn les_rectangles_ne_se_recouvrent_pas() {
    let world = cell_with(&[
        (1.0, 1.0),
        (0.5, 0.5),
        (0.25, 1.0),
        (0.125, 0.125),
        (0.5, 0.25),
    ]);
    let atlas = pack(&world.cells()[0]).expect("rangement possible");

    for slot in &atlas.slots {
        assert!(
            slot.x + slot.width <= atlas.side && slot.y + slot.height <= atlas.side,
            "{slot:?} déborde d'un atlas de {}",
            atlas.side
        );
    }
    for (i, a) in atlas.slots.iter().enumerate() {
        for b in &atlas.slots[i + 1..] {
            assert!(!overlaps(*a, *b), "{a:?} recouvre {b:?}");
        }
    }
}

/// Chaque rectangle porte l'étendue de sa surface plus sa gouttière.
///
/// La gouttière n'est pas un supplément facultatif : c'est ce que le bilinéaire du
/// rasteriseur va chercher au bord d'une surface, et sans elle il irait chercher
/// la surface voisine dans l'atlas.
#[test]
fn chaque_rectangle_porte_sa_gouttiere() {
    let steps = [(1.0f32, 1.0f32), (0.25, 0.5)];
    let world = cell_with(&steps);
    let cell = &world.cells()[0];
    let atlas = pack(cell).expect("rangement possible");

    for (surface, slot) in cell.surfaces.iter().zip(&atlas.slots) {
        assert!(
            slot.width >= surface.luxels.width + GUTTER * 2,
            "{slot:?} ne porte pas les {} luxels et leur gouttière",
            surface.luxels.width
        );
        assert!(slot.height >= surface.luxels.height + GUTTER * 2);
    }
}

/// Le rangement est reproductible : deux appels rendent le même atlas.
///
/// L'ordre entre dans l'image, puisqu'il décide des coordonnées que la soumission
/// ajoutera. Un rangement qui dépendrait d'un ordre d'itération non contractuel
/// ferait diverger deux constructions sur la même carte.
#[test]
fn le_rangement_est_reproductible() {
    let world = cell_with(&[(0.5, 1.0), (1.0, 0.5), (0.25, 0.25), (0.5, 0.5)]);
    let cell = &world.cells()[0];
    assert_eq!(pack(cell).unwrap(), pack(cell).unwrap());
}

/// Une cellule sans surface rend un atlas vide plutôt qu'une erreur.
#[test]
fn une_cellule_sans_surface_se_range() {
    let points = [
        [0.0f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let cell = cell_bytes(7, 0, &points, &[], &[]);
    let world = World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide");
    let atlas = pack(&world.cells()[0]).expect("rangement possible");
    assert!(atlas.slots.is_empty());
}

/// Un côté se monte à la puissance de deux supérieure, jamais en dessous.
#[test]
fn un_cote_se_monte_a_la_puissance_de_deux_superieure() {
    assert_eq!(round_up(0), 1);
    assert_eq!(round_up(1), 1);
    assert_eq!(round_up(2), 2);
    assert_eq!(round_up(3), 4);
    assert_eq!(round_up(17), 32);
    assert_eq!(round_up(1024), 1024);
}
