// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le décor de collision se charge, et porte les neuf cas qu'il annonce.
//!
//! **Ce que ces tests attrapent ne se verrait nulle part ailleurs.** Une carte
//! qui ne se dessine pas n'a pas d'image pour trahir une erreur de géométrie :
//! deux portails qui ne s'apparient pas, une empreinte à l'envers ou un drapeau
//! oublié passeraient inaperçus, et la scène de conformance figerait une
//! empreinte qui éprouve autre chose que ce qu'elle annonce.

use screengine::{Vec3, World};
use screengine_conformance::collision_file;

/// Un point du monde, écrit en coordonnées locales de l'empreinte.
///
/// **Le décor est posé loin de l'origine du monde**, ce qui est une propriété de
/// la scène et non un placement — voir `collision_file`. L'abscisse suit donc
/// cette origine, et les coordonnées de ce fichier restent celles qu'on lit sur
/// les empreintes.
fn at(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(collision_file::ORIGIN_X + x, y, z)
}

/// La carte se charge, et elle porte ses cinq cellules.
#[test]
fn le_decor_se_charge() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    assert_eq!(world.cell_count(), 5);
}

/// **La cage d'escalier se charge, et elle porte ses vingt-neuf surfaces.**
///
/// Le compte est le sujet : douze marches donnent vingt-sept arêtes de profil,
/// donc vingt-sept faces latérales plus les deux flancs. Une marche perdue ou un
/// point de profil en trop se verrait ici, et nulle part ailleurs — un décor à une
/// surface près se charge, se balaie et rend une empreinte sans rien signaler.
///
/// Que la cellule soit **close** est vérifié par ailleurs, sur tout le décor :
/// c'est ce qui dit qu'un profil extrudé dans le plan vertical ferme son volume
/// aussi bien qu'une empreinte extrudée à l'horizontale.
#[test]
fn la_cage_porte_ses_vingt_neuf_surfaces() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");

    let mut surfaces = 0;
    for id in 500..600 {
        if world.surface_material(id).is_some() {
            surfaces += 1;
        }
    }
    assert_eq!(surfaces, 29, "douze marches, deux flancs");
}

/// **La rampe se charge, et c'est déjà une information** : son sol et ses flancs
/// ont des repères de lightmap obliques, que le contrôle du chargement accepte
/// parce que leurs axes ont pour carré une puissance de deux. Une pente autre que
/// 45° ferait refuser le fichier entier.
///
/// Et une boîte qui y tombe est arrêtée par le sol, où qu'elle tombe sur la
/// largeur. C'est le prédicat du noyau rejoué sur le décor versionné que les cinq
/// hôtes chargent : le test d'appartenance d'une face se faisait au centre de la
/// boîte, ce qui décalait le domaine d'une demi-extension dès que la normale
/// n'était pas axiale.
#[test]
fn une_boite_qui_tombe_sur_la_rampe_est_arretee() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    // Des demi-étendues, pas un point : `at` y mettrait l'origine du décor.
    let half = Vec3::new(0.45, 0.45, 0.45);

    for pas in 1..8u32 {
        let y = f32::from(pas as u16);
        let x = 28.0;
        let floor = collision_file::FLOOR_Z + y * collision_file::RAMP_RISE;
        let from = at(x, y, floor + 1.5);
        let to = at(x, y, floor - 1.5);

        let cell = world.locate(from);
        assert_eq!(cell, 10, "y={y} : le départ est dans la rampe");
        let hit = world.sweep(cell, half, from, to).expect("cellule connue");

        assert!(hit.fraction < 1.0, "y={y} : la rampe n'arrête pas la chute");
        assert!(
            !hit.start_solid,
            "y={y} : le départ devait être dégagé, pas solide"
        );
    }
}

/// **La cellule en U est à l'écart et sans portail**, et c'est sa raison d'être.
///
/// Le chemin de force brute l'examine quand la traversée ne la visite pas :
/// c'est la seule part du décor où les deux ne regardent pas la même géométrie,
/// donc la seule où un faux contact les fait diverger au lieu de les tromper
/// ensemble. Un portail, ou un voisinage, lui retirerait cela.
#[test]
fn la_cellule_en_u_est_a_l_ecart_et_sans_portail() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");

    let branch = at(1.0, 21.0, 2.0);
    let inside = world.locate(branch);
    assert_eq!(inside, 9, "le point est dans la branche gauche du U");

    // Vers l'extérieur par un mur : aucune cellule au-delà, le U ne mène nulle
    // part. Et le creux du U n'est dans aucune cellule — c'est ce qui en fait un
    // U et non un carré.
    assert_eq!(world.track(inside, branch, at(1.0, 30.0, 2.0)), 0);
    assert_eq!(world.locate(at(4.0, 22.0, 2.0)), 0, "le creux est dehors");
    assert_eq!(world.locate(at(4.0, 18.0, 2.0)), 9, "la base en est");
}

/// **Un départ derrière le plan d'une face de sa propre cellule reste libre.**
///
/// Le cas que la cellule en U existe pour porter, joué sur le décor versionné que
/// les cinq hôtes chargent et non sur une carte de test : le départ est à cinq
/// unités derrière le plan de la face intérieure de la branche droite, et sa
/// projection tombe en plein milieu d'elle. Le volume dilaté d'une face étant une
/// dalle et non un demi-espace, il n'y a là aucun contact.
#[test]
fn un_depart_derriere_une_face_du_u_ne_rencontre_rien() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    // Des demi-étendues, pas un point : `at` y mettrait l'origine du décor.
    let half = Vec3::new(0.45, 0.45, 0.45);
    let from = at(1.0, 21.0, 2.0);
    let to = at(0.75, 21.0, 2.0);

    let hit = world.sweep(9, half, from, to).expect("cellule connue");
    assert_eq!(hit.fraction, 1.0, "le pas est libre");
    assert_eq!(hit.surface, 0, "aucune surface n'est touchée");

    let brute = world.sweep_brute(half, from, to);
    assert_eq!(brute.fraction, hit.fraction, "les deux chemins s'accordent");
    assert_eq!(brute.surface, hit.surface);
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
