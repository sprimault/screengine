// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le porteur se teste sur son cycle : rien, puis calculé, et le refus d'une
//! cellule qui n'existe pas.

use super::*;
use crate::format::world::tests::{cell_bytes, file, frame, material, words};

/// Une carte d'une cellule et d'une lumière.
fn one_cell() -> World {
    let points = [
        [0.0f32, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 4.0, 0.0],
        [0.0, 4.0, 0.0],
    ];
    let mut surface = words(&[11, 0, 1, 4]);
    surface.extend_from_slice(&words(&[0, 1, 2, 3]));
    surface.extend_from_slice(&frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    surface.extend_from_slice(&frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));

    let mut lights = words(&[1]);
    for value in [2.0f32, 2.0, 2.0, 8.0] {
        lights.extend_from_slice(&value.to_le_bytes());
    }
    lights.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]);

    let cell = cell_bytes(7, 0, &points, &[surface], &[]);
    World::load(&file(&cell, &[], &lights, &material(1, "mur"))).expect("carte valide")
}

/// Une cellule commence sans lightmap, et en a une après son calcul.
#[test]
fn une_cellule_passe_d_absente_a_prete() {
    let world = one_cell();
    let mut lighting = Lightmaps::new(&world).expect("porteur");
    assert_eq!(lighting.state(&world, 7).unwrap(), Lightmap::Absent);

    lighting.build(&world, 7).expect("cuisson possible");
    assert_eq!(lighting.state(&world, 7).unwrap(), Lightmap::Ready);
    assert!(
        lighting.of(0).is_some(),
        "l'atlas est accessible par son rang"
    );
}

/// Une cellule que la carte ne porte pas est refusée, au calcul comme à la
/// lecture.
#[test]
fn une_cellule_inconnue_est_refusee() {
    let world = one_cell();
    let mut lighting = Lightmaps::new(&world).expect("porteur");
    assert_eq!(lighting.build(&world, 99), Err(Error::UnknownResource));
    assert_eq!(lighting.state(&world, 99), Err(Error::UnknownResource));
}

/// Recalculer une cellule remplace sa lightmap.
///
/// Une cellule modifiée recalcule les siennes, jamais celles du niveau : c'est ce
/// que l'édition à chaud d'une étape ultérieure demandera, et le porteur doit le
/// permettre sans se vider.
#[test]
fn recalculer_remplace_la_lightmap() {
    let world = one_cell();
    let mut lighting = Lightmaps::new(&world).expect("porteur");
    lighting.build(&world, 7).expect("cuisson possible");
    lighting.build(&world, 7).expect("cuisson possible");
    assert_eq!(lighting.state(&world, 7).unwrap(), Lightmap::Ready);
}

/// Un porteur neuf n'a rien calculé, et n'alloue aucun luxel.
///
/// La création est un appel nommé qui alloue une entrée par cellule, et rien de
/// plus : c'est ce qui permet à un hôte de savoir quand il paie la cuisson.
#[test]
fn un_porteur_neuf_ne_calcule_rien() {
    let world = one_cell();
    let lighting = Lightmaps::new(&world).expect("porteur");
    assert!(lighting.of(0).is_none());
}
