// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que l'empreinte doit voir, et ce qu'elle doit ignorer.
//!
//! **Les deux moitiés comptent autant.** Qu'elle change quand la cuisson changerait
//! évite qu'un cache valide rende une image que la version suivante ne rend plus —
//! l'erreur la plus coûteuse, parce qu'elle se découvre en regardant une capture et
//! non en lisant un message. Qu'elle ne change pas quand la cuisson ne changerait
//! pas est ce qui rend le cache utile dans un éditeur, où l'on réhabille et l'on
//! déplace sans arrêt ; une empreinte trop large ferait recuire un niveau entier
//! au premier changement de texture, et personne ne s'en servirait.

use alloc::vec::Vec;

use super::*;
use crate::format::world::tests::{cell_bytes, entity_bytes, file, frame, material, words};

/// Ce qu'une variante de la carte d'épreuve change.
///
/// Un seul champ bouge par test : une empreinte qui change dit alors *lequel*, ce
/// qu'une carte réécrite à la main ne dirait pas.
#[derive(Clone, Copy)]
struct Tweak {
    /// Le matériau des surfaces, qui n'entre pas dans le calcul.
    material: u32,
    /// L'échelle du repère de **texture**, qui n'y entre pas non plus.
    texture: f32,
    /// L'échelle du repère de **lightmap**, qui décide de la grille des luxels.
    lightmap: f32,
    /// Le rayon de la lampe.
    radius: f32,
    /// Le décalage vertical des sommets de la **seconde** cellule, la voisine.
    neighbour: f32,
    /// La position de l'entité que la carte porte.
    entity: f32,
}

impl Default for Tweak {
    fn default() -> Self {
        Self {
            material: 1,
            texture: 1.0,
            lightmap: 1.0,
            radius: 8.0,
            neighbour: 0.0,
            entity: 1.0,
        }
    }
}

/// Une carte de deux cellules mitoyennes, une lampe et une entité dans la première.
///
/// Les deux carrés se touchent en `x = 4`, où chacun porte un portail décrit avec
/// les mêmes littéraux : c'est ce que l'appariement exige, et c'est ce qui met la
/// voisine dans l'empreinte.
fn map(tweak: Tweak) -> Vec<u8> {
    let mut cells = square(1, 100, 0.0, 0.0, &tweak);
    cells.extend_from_slice(&square(2, 200, 4.0, tweak.neighbour, &tweak));

    let entities = entity_bytes(
        1,
        1,
        "depart",
        [tweak.entity, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        &[],
    );

    let mut lights = words(&[1]);
    for value in [2.0f32, 2.0, 2.0, tweak.radius] {
        lights.extend_from_slice(&value.to_le_bytes());
    }
    lights.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]);

    let mut materials = material(1, "mur");
    materials.extend_from_slice(&material(2, "sol"));

    file(&cells, &entities, &lights, &materials)
}

/// Une cellule carrée de quatre unités, posée à `low` sur l'axe des `x`.
///
/// Son arête `x = low + 4` est un portail quand `low` vaut zéro, son arête `x = low`
/// quand il vaut quatre : les deux se rejoignent donc en `x = 4`. `lift` élève ses
/// sommets, ce qui change sa géométrie sans toucher à celle de l'autre.
fn square(id: u32, first_id: u32, low: f32, lift: f32, tweak: &Tweak) -> Vec<u8> {
    let footprint = [
        [low, 0.0f32],
        [low + 4.0, 0.0],
        [low + 4.0, 4.0],
        [low, 4.0],
    ];
    let opening = if low == 0.0 { 1 } else { 3 };

    let mut points = Vec::new();
    for z in [lift, lift + 4.0] {
        for corner in &footprint {
            points.push([corner[0], corner[1], z]);
        }
    }

    let mut surfaces: Vec<Vec<u8>> = Vec::new();
    let mut push = |id: u32, indices: &[u32], u: [f32; 3], v: [f32; 3]| {
        let scaled = |axis: [f32; 3], by: f32| [axis[0] * by, axis[1] * by, axis[2] * by];
        let mut bytes = words(&[id, 0, tweak.material, indices.len() as u32]);
        bytes.extend_from_slice(&words(indices));
        bytes.extend_from_slice(&frame(
            [0.0, 0.0, 0.0],
            scaled(u, tweak.texture),
            scaled(v, tweak.texture),
        ));
        bytes.extend_from_slice(&frame(
            [0.0, 0.0, 0.0],
            scaled(u, tweak.lightmap),
            scaled(v, tweak.lightmap),
        ));
        surfaces.push(bytes);
    };

    push(first_id, &[0, 1, 2, 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    push(
        first_id + 1,
        &[7, 6, 5, 4],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    );
    let mut next = first_id + 2;
    for i in 0..4 {
        if i == opening {
            continue;
        }
        let j = (i + 1) % 4;
        let (a, b) = (footprint[i], footprint[j]);
        push(
            next,
            &[i as u32, (i + 4) as u32, (j + 4) as u32, j as u32],
            [b[0] - a[0], b[1] - a[1], 0.0],
            [0.0, 0.0, 1.0],
        );
        next += 1;
    }

    let j = (opening + 1) % 4;
    let mut portal = words(&[next, 4]);
    portal.extend_from_slice(&words(&[
        opening as u32,
        j as u32,
        (j + 4) as u32,
        (opening + 4) as u32,
    ]));

    cell_bytes(id, 0, &points, &surfaces, &[portal])
}

/// L'empreinte de la première cellule d'une variante.
fn of(tweak: Tweak) -> u64 {
    let world = World::load(&map(tweak)).expect("carte valide");
    fingerprint(&world, 0).expect("empreinte calculable")
}

/// La même carte rend deux fois la même empreinte.
#[test]
fn deux_calculs_rendent_la_meme_empreinte() {
    assert_eq!(of(Tweak::default()), of(Tweak::default()));
}

/// Deux cellules d'une même carte n'ont pas la même empreinte.
///
/// Sans quoi le cache rendrait à l'une ce qui a été cuit pour l'autre, et
/// l'écarterait sans erreur — donc sans que rien ne le dise.
#[test]
fn deux_cellules_ont_des_empreintes_distinctes() {
    let world = World::load(&map(Tweak::default())).expect("carte valide");
    assert_ne!(
        fingerprint(&world, 0).expect("empreinte"),
        fingerprint(&world, 1).expect("empreinte")
    );
}

/// Changer le repère de lightmap change l'empreinte.
#[test]
fn un_repere_de_lightmap_change_l_empreinte() {
    assert_ne!(
        of(Tweak::default()),
        of(Tweak {
            lightmap: 2.0,
            ..Tweak::default()
        })
    );
}

/// Changer une lumière change l'empreinte.
#[test]
fn une_lumiere_change_l_empreinte() {
    assert_ne!(
        of(Tweak::default()),
        of(Tweak {
            radius: 9.0,
            ..Tweak::default()
        })
    );
}

/// **Modifier une cellule périme aussi l'empreinte de sa voisine.**
///
/// C'est la clause qu'un hôte lit de travers : il recalcule la cellule qu'il vient
/// d'éditer et s'étonne que la salle d'à côté passe à « périmée ». La lumière qui
/// franchissait la porte était calculée là-bas, et le mur qui l'occulte aussi.
#[test]
fn modifier_une_voisine_change_l_empreinte() {
    assert_ne!(
        of(Tweak::default()),
        of(Tweak {
            neighbour: 1.0,
            ..Tweak::default()
        })
    );
}

/// **Réhabiller un décor ne périme aucune lightmap.**
///
/// Ni le matériau ni le repère de texture n'entrent dans le calcul : les inclure
/// ferait recuire un niveau entier au premier changement de texture, et un éditeur
/// cesserait de se servir du cache.
#[test]
fn rehabiller_ne_change_pas_l_empreinte() {
    let dressed = Tweak {
        material: 2,
        texture: 4.0,
        ..Tweak::default()
    };
    // Que les deux cartes diffèrent vraiment : sans ce contrôle, un test
    // d'invariance passe aussi quand la variante n'a rien changé du tout.
    assert_ne!(map(Tweak::default()), map(dressed));
    assert_eq!(of(Tweak::default()), of(dressed));
}

/// **Déplacer une entité ne périme aucune lightmap.**
///
/// Une entité n'éclaire pas et n'occulte pas — le moteur n'en connaît que la pose
/// et des octets opaques.
#[test]
fn deplacer_une_entite_ne_change_pas_l_empreinte() {
    let moved = Tweak {
        entity: 3.0,
        ..Tweak::default()
    };
    assert_ne!(map(Tweak::default()), map(moved));
    assert_eq!(of(Tweak::default()), of(moved));
}
