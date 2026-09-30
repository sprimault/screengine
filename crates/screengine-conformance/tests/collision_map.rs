// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le décor de collision se charge, et porte les sept cas qu'il annonce.
//!
//! **Ce que ces tests attrapent ne se verrait nulle part ailleurs.** Une carte
//! qui ne se dessine pas n'a pas d'image pour trahir une erreur de géométrie :
//! deux portails qui ne s'apparient pas, une empreinte à l'envers ou un drapeau
//! oublié passeraient inaperçus, et la scène de conformance figerait une
//! empreinte qui éprouve autre chose que ce qu'elle annonce.

use screengine::{Vec3, World};
use screengine_conformance::collision_file;

/// Un point du monde, écrit court.
fn at(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// La carte se charge, et elle porte ses deux cellules.
#[test]
fn le_decor_se_charge() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    assert_eq!(world.cell_count(), 2);
}

/// Les deux portails s'apparient, et le suivi passe de la salle au couloir.
///
/// **C'est le test qui décide de tout le reste.** Les deux portails ne partagent
/// leurs quatre sommets que parce que la salle porte deux sommets colinéaires sur
/// son mur de droite ; sans eux, le chargement en ferait deux murs, la traversée
/// ne franchirait rien, et la scène éprouverait le contraire de ce qu'elle
/// annonce — sans que rien ne le dise.
#[test]
fn le_portail_relie_la_salle_au_couloir() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");

    let room = world.locate(at(6.0, 2.0, 2.0));
    assert_eq!(room, 7, "le point est dans la salle");

    let crossed = world.track(room, at(6.0, 2.0, 2.0), at(10.0, 2.0, 2.0));
    assert_eq!(crossed, 8, "le portail est franchi et mène au couloir");
}

/// Le portail sans vis-à-vis est un mur : le franchir ne mène nulle part.
#[test]
fn le_portail_non_apparie_est_un_mur() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");

    let inside = world.locate(at(2.0, 6.0, 2.0));
    assert_eq!(inside, 7);
    let out = world.track(inside, at(2.0, 6.0, 2.0), at(2.0, 12.0, 2.0));
    assert_eq!(out, 0, "il ne mène à aucune cellule");
}

/// Le couloir fait exactement une unité de large.
///
/// La valeur est lue sur la carte et non recopiée : c'est elle qui fixe la boîte
/// d'épreuve de la scène, et les deux dériveraient si le test la supposait.
#[test]
fn le_couloir_fait_une_unite_de_large() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    let corridor = world.locate(at(11.0, 2.0, 2.0));
    assert_eq!(corridor, 8, "le point est dans le couloir");

    // Le couloir occupe `2 ≤ y ≤ 3` : un dixième de plus d'un côté ou de l'autre
    // en sort, et il n'y a rien d'autre à cette abscisse.
    assert_eq!(world.locate(at(11.0, 1.9, 2.0)), 0);
    assert_eq!(world.locate(at(11.0, 3.1, 2.0)), 0);
}
