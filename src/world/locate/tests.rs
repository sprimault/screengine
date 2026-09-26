// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La localisation se teste sur des cartes écrites en octets, par les
//! constructeurs du décodeur.
//!
//! Le cas qui compte le plus est le **point en face d'une arête** : le comptage
//! porte sur des triangles, donc sur des arêtes internes que la découpe d'oreilles
//! a créées, et une arête comptée deux fois inverse la parité — la caméra se
//! retrouve dehors à une position parfaitement légitime.

use alloc::vec::Vec;

use super::*;
use crate::format::world::tests::{cell_bytes, file, material, portal_bytes};

/// Le sol d'un tronçon, dont le plan porte les deux axes du repère par défaut.
const FLOOR: [u32; 4] = [0, 4, 5, 1];
/// Son plafond.
const CEILING: [u32; 4] = [3, 2, 6, 7];
/// Son mur de gauche.
const LEFT: [u32; 4] = [0, 3, 7, 4];
/// Son mur de droite.
const RIGHT: [u32; 4] = [1, 5, 6, 2];

/// Les huit sommets d'une boîte de `from` à `to`, de quatre unités de côté.
fn box_points(from: f32, to: f32) -> [[f32; 3]; 8] {
    let mut points = [[0.0f32; 3]; 8];
    for (i, x) in [from, to].iter().enumerate() {
        for (j, (y, z)) in [(-2.0, -2.0), (2.0, -2.0), (2.0, 2.0), (-2.0, 2.0)]
            .iter()
            .enumerate()
        {
            points[i * 4 + j] = [*x, *y, *z];
        }
    }
    points
}

/// Les quatre faces latérales d'un tronçon, avec des repères dans leur plan.
fn faces(base: u32) -> Vec<Vec<u8>> {
    [
        (FLOOR, [0.0f32, 1.0, 0.0]),
        (CEILING, [0.0, 1.0, 0.0]),
        (LEFT, [0.0, 0.0, 1.0]),
        (RIGHT, [0.0, 0.0, 1.0]),
    ]
    .iter()
    .enumerate()
    .map(|(i, (indices, v))| {
        use crate::format::world::tests::{frame, words};
        let mut bytes = words(&[base + i as u32, 0, 1, 4]);
        bytes.extend_from_slice(&words(indices));
        let along = [1.0f32, 0.0, 0.0];
        bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], along, *v));
        bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], along, *v));
        bytes
    })
    .collect()
}

/// Une surface qui ferme un bout de tronçon, dans un plan `x` constant.
///
/// **Un bout se ferme par une surface ou par un portail, jamais par rien.** Une
/// cellule laissée ouverte n'a plus de dedans : le comptage de traversées y rend
/// un résultat qui dépend de la direction du rayon, et c'est ce que les premières
/// versions de ces cartes faisaient — trois tests l'ont dit.
fn cap(id: u32, indices: [u32; 4]) -> Vec<u8> {
    use crate::format::world::tests::{frame, words};
    let mut bytes = words(&[id, 0, 1, 4]);
    bytes.extend_from_slice(&words(&indices));
    // Le plan est `x` constant : les axes du repère vivent donc dans `YZ`.
    let f = frame([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    bytes.extend_from_slice(&f);
    bytes.extend_from_slice(&f);
    bytes
}

/// Une boîte fermée par ses six faces, sans aucun portail.
fn closed_box() -> World {
    let mut surfaces = faces(11);
    surfaces.push(cap(21, [0, 1, 2, 3]));
    surfaces.push(cap(22, [4, 7, 6, 5]));
    let cell = cell_bytes(7, 0, &box_points(0.0, 8.0), &surfaces, &[]);
    World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide")
}

/// Deux tronçons joints par un portail à `x = 8`.
fn two_cells() -> World {
    // Chaque tronçon est clos : quatre faces latérales, un bout en surface, et
    // l'autre bout en portail — c'est lui qui ferme le volume de ce côté.
    let mut first_faces = faces(11);
    first_faces.push(cap(19, [0, 1, 2, 3]));
    let mut second_faces = faces(21);
    second_faces.push(cap(29, [4, 7, 6, 5]));

    let first = cell_bytes(
        7,
        0,
        &box_points(0.0, 8.0),
        &first_faces,
        &[portal_bytes(31, &[4, 5, 6, 7])],
    );
    let second = cell_bytes(
        8,
        0,
        &box_points(8.0, 16.0),
        &second_faces,
        &[portal_bytes(32, &[0, 1, 2, 3])],
    );
    let mut cells = first;
    cells.extend_from_slice(&second);
    World::load(&file(&cells, &[], &[], &material(1, "mur"))).expect("carte valide")
}

/// Un point au centre d'une boîte close est dans sa cellule.
#[test]
fn un_point_au_centre_est_dans_sa_cellule() {
    let world = closed_box();
    assert_eq!(locate(&world, Vec3::new(4.0, 0.0, 0.0)), Some(0));
}

/// Un point au-delà d'une face n'est dans aucune cellule.
#[test]
fn un_point_dehors_n_est_dans_aucune_cellule() {
    let world = closed_box();
    assert_eq!(locate(&world, Vec3::new(4.0, 9.0, 0.0)), None);
    assert_eq!(locate(&world, Vec3::new(-1.0, 0.0, 0.0)), None);
    assert_eq!(locate(&world, Vec3::new(4.0, 0.0, 9.0)), None);
}

/// Un point posé exactement en face d'une arête reste dans sa cellule.
///
/// **C'est le test qui compte.** Le comptage porte sur les triangles, donc sur les
/// arêtes que la découpe d'oreilles a créées à l'intérieur des faces : une arête
/// comptée deux fois, ou pas du tout, inverse la parité et met le point dehors.
/// Les positions ci-dessous alignent le rayon sur le sommet commun des deux
/// triangles de chaque face.
#[test]
fn un_point_en_face_d_une_arete_reste_dedans() {
    let world = closed_box();
    for (y, z) in [
        (0.0f32, 0.0f32),
        (-2.0, 0.0),
        (2.0, 0.0),
        (0.0, -2.0),
        (0.0, 2.0),
    ] {
        // Le point reste dans la boîte : seules les coordonnées transverses
        // changent, et elles visent les sommets et les arêtes.
        let point = Vec3::new(4.0, y * 0.5, z * 0.5);
        assert_eq!(
            locate(&world, point),
            Some(0),
            "le rayon en ({y}, {z}) a perdu sa cellule"
        );
    }
}

/// Deux cellules jointes se distinguent par la position.
#[test]
fn chaque_troncon_a_sa_cellule() {
    let world = two_cells();
    assert_eq!(locate(&world, Vec3::new(4.0, 0.0, 0.0)), Some(0));
    assert_eq!(locate(&world, Vec3::new(12.0, 0.0, 0.0)), Some(1));
}

/// Un déplacement qui reste dans sa cellule la garde.
#[test]
fn un_pas_dans_la_cellule_la_garde() {
    let world = two_cells();
    let from = Vec3::new(4.0, 0.0, 0.0);
    let to = Vec3::new(6.0, 0.0, 0.0);
    assert_eq!(track(&world, 0, from, to), Some(0));
}

/// Un déplacement qui franchit un portail transporte la cellule.
#[test]
fn un_pas_a_travers_un_portail_transporte_la_cellule() {
    let world = two_cells();
    let from = Vec3::new(6.0, 0.0, 0.0);
    let to = Vec3::new(10.0, 0.0, 0.0);
    assert_eq!(track(&world, 0, from, to), Some(1));
}

/// Un déplacement qui sort par un mur laisse la caméra dehors.
///
/// Une clause, pas un défaut : un hôte peut pousser sa caméra dans un interstice
/// d'une carte en cours d'édition, et c'est à lui de la relocaliser.
#[test]
fn un_pas_a_travers_un_mur_sort_de_toute_cellule() {
    let world = two_cells();
    let from = Vec3::new(4.0, 0.0, 0.0);
    let to = Vec3::new(4.0, 9.0, 0.0);
    assert_eq!(track(&world, 0, from, to), None);
}

/// Un pas assez long pour franchir deux cellules aboutit dans la bonne.
///
/// Sans la boucle, le suivi rendrait la première voisine alors que le point
/// d'arrivée est plus loin, et la cellule rendue ne contiendrait pas la caméra.
#[test]
fn un_pas_long_traverse_plusieurs_cellules() {
    let world = two_cells();
    let from = Vec3::new(1.0, 0.0, 0.0);
    let to = Vec3::new(15.0, 0.0, 0.0);
    assert_eq!(track(&world, 0, from, to), Some(1));
}

/// Une cellule de départ hors borne ne suit rien.
#[test]
fn une_cellule_de_depart_inconnue_ne_suit_rien() {
    let world = two_cells();
    let point = Vec3::new(4.0, 0.0, 0.0);
    assert_eq!(track(&world, 99, point, point), None);
}
