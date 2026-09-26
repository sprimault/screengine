// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le porteur se teste sur son cycle : rien, puis calculé, et le refus d'une
//! cellule qui n'existe pas.

use super::*;
use crate::format::world::tests::{cell_bytes, file, frame, material, words};

/// Une carte d'une cellule et d'une lumière.
fn one_cell() -> World {
    one_cell_with(7, 2.0)
}

/// La même, dont l'identifiant de cellule et l'abscisse de la lampe se choisissent.
///
/// Les deux servent aux tests de reprise, où il faut une carte **proche mais pas
/// identique** : déplacer la lampe périme l'empreinte, renuméroter la cellule fait
/// que l'entrée ne désigne plus rien. Une carte franchement différente ne
/// distinguerait pas les deux cas.
fn one_cell_with(id: u32, light_x: f32) -> World {
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
    for value in [light_x, 2.0, 2.0, 8.0] {
        lights.extend_from_slice(&value.to_le_bytes());
    }
    lights.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]);

    let cell = cell_bytes(id, 0, &points, &[surface], &[]);
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

/// Le cache d'un porteur neuf est un bloc valide, et vide.
///
/// Un hôte qui sauve avant d'avoir cuit ne doit pas obtenir un cas particulier :
/// il écrit un bloc, le relit, et reprend zéro entrée.
#[test]
fn un_porteur_neuf_sauve_un_bloc_vide() {
    let world = one_cell();
    let lighting = Lightmaps::new(&world).expect("porteur");

    let len = lighting.save_len(&world).expect("longueur");
    let mut bytes = alloc::vec![0u8; len];
    lighting.save(&world, &mut bytes).expect("écriture");

    let mut other = Lightmaps::new(&world).expect("porteur");
    assert_eq!(other.restore(&world, &bytes).expect("reprise"), 0);
    assert_eq!(other.state(&world, 7).unwrap(), Lightmap::Absent);
}

/// Un cache repris rend exactement les luxels qui ont été cuits.
///
/// **C'est la raison d'être du lot** : reprendre doit valoir calculer, sinon une
/// partie du niveau se rendrait autrement selon qu'elle vient du cache ou de la
/// cuisson, et la conformance ne vaudrait plus que pour l'un des deux chemins.
#[test]
fn un_cache_repris_rend_les_memes_luxels() {
    let world = one_cell();
    let mut lighting = Lightmaps::new(&world).expect("porteur");
    lighting.build(&world, 7).expect("cuisson possible");

    let len = lighting.save_len(&world).expect("longueur");
    let mut bytes = alloc::vec![0u8; len];
    lighting.save(&world, &mut bytes).expect("écriture");

    let mut other = Lightmaps::new(&world).expect("porteur");
    assert_eq!(other.restore(&world, &bytes).expect("reprise"), 1);
    assert_eq!(other.state(&world, 7).unwrap(), Lightmap::Ready);

    let (cooked, packing) = lighting.of(0).expect("cuite");
    let (taken, restored) = other.of(0).expect("reprise");
    assert_eq!(packing, restored);
    assert_eq!(cooked.level_texels(0), taken.level_texels(0));
}

/// Sauver puis reprendre puis resauver rend le même bloc, octet pour octet.
///
/// Le moteur est le seul écrivain, donc la canonicité est gratuite — et c'est elle
/// qui permet à un hôte de comparer deux caches sans les décoder.
#[test]
fn deux_ecritures_du_meme_etat_rendent_les_memes_octets() {
    let world = one_cell();
    let mut lighting = Lightmaps::new(&world).expect("porteur");
    lighting.build(&world, 7).expect("cuisson possible");

    let mut first = alloc::vec![0u8; lighting.save_len(&world).expect("longueur")];
    lighting.save(&world, &mut first).expect("écriture");

    let mut other = Lightmaps::new(&world).expect("porteur");
    other.restore(&world, &first).expect("reprise");
    let mut second = alloc::vec![0u8; other.save_len(&world).expect("longueur")];
    other.save(&world, &mut second).expect("écriture");

    assert_eq!(first, second);
}

/// **Une entrée dont l'empreinte ne concorde plus est écartée, sans erreur.**
///
/// Le cas normal d'un éditeur : un mur a bougé depuis la cuisson. Refuser le bloc
/// entier ferait tout recuire pour ce mur-là, et garder l'entrée rendrait un
/// éclairage périmé — la seule des deux options que le projet refuse par principe.
#[test]
fn une_entree_perimee_est_ecartee_sans_erreur() {
    let world = one_cell();
    let mut lighting = Lightmaps::new(&world).expect("porteur");
    lighting.build(&world, 7).expect("cuisson possible");
    let mut bytes = alloc::vec![0u8; lighting.save_len(&world).expect("longueur")];
    lighting.save(&world, &mut bytes).expect("écriture");

    let moved = moved_light();
    let mut other = Lightmaps::new(&moved).expect("porteur");
    assert_eq!(other.restore(&moved, &bytes).expect("reprise"), 0);
    assert_eq!(other.state(&moved, 7).unwrap(), Lightmap::Absent);
}

/// **Une entrée qui désigne une cellule absente est écartée de même.**
///
/// Un hôte qui a supprimé une salle et gardé son ancien cache est un hôte normal.
#[test]
fn une_cellule_absente_est_ecartee_sans_erreur() {
    let world = one_cell();
    let mut lighting = Lightmaps::new(&world).expect("porteur");
    lighting.build(&world, 7).expect("cuisson possible");
    let mut bytes = alloc::vec![0u8; lighting.save_len(&world).expect("longueur")];
    lighting.save(&world, &mut bytes).expect("écriture");

    // La même carte, dont la cellule porte un autre identifiant : l'entrée ne
    // désigne donc plus rien.
    let renumbered = renumbered();
    let mut other = Lightmaps::new(&renumbered).expect("porteur");
    assert_eq!(other.restore(&renumbered, &bytes).expect("reprise"), 0);
}

/// Un bloc malformé est une erreur, contrairement à une entrée périmée.
#[test]
fn un_bloc_malforme_est_une_erreur() {
    let world = one_cell();
    let mut lighting = Lightmaps::new(&world).expect("porteur");
    assert!(matches!(
        lighting.restore(&world, b"pas un bloc"),
        Err(Error::InvalidFormat(_))
    ));
}

/// La même carte, la lampe déplacée d'une unité.
fn moved_light() -> World {
    one_cell_with(7, 3.0)
}

/// La même carte, la cellule renumérotée.
fn renumbered() -> World {
    one_cell_with(9, 2.0)
}
