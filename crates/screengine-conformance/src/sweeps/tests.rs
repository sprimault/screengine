// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la scène de collision doit éprouver, et qu'une empreinte ne dit pas.
//!
//! **Une empreinte dit qu'un résultat a changé, jamais qu'il est juste.** Une
//! scène où tout traverserait tout, ou dont tous les départs seraient hors
//! cellule, se figerait aussi bien qu'une autre — c'est arrivé au premier jet,
//! où 1 760 balayages sur 2 400 ne touchaient rien. Ces tests gardent ce que le
//! rapport texte a servi à voir une fois.

use super::*;

/// La taille d'un enregistrement haché, en octets.
///
/// Ici plutôt qu'à côté de l'écriture, qui va champ par champ et n'a pas à la
/// connaître : c'est l'uniformité qui a de la valeur, pas le nombre, et elle se
/// perdrait au premier cas particulier réintroduit.
const RECORD: usize = 37;

/// Joue les balayages et range leurs résultats à côté de leur description.
fn played() -> Vec<(Sweep, Option<Hit>)> {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    all()
        .into_iter()
        .map(|sweep| {
            let cell = world.locate(sweep.from);
            let hit = if cell == 0 {
                None
            } else {
                world.sweep(cell, sweep.half, sweep.from, sweep.to)
            };
            (sweep, hit)
        })
        .collect()
}

/// La scène balaie surtout des départs qui sont dans une cellule.
///
/// Le seuil est bas exprès : ce n'est pas une mesure de qualité, c'est un garde
/// contre le premier jet, où le treillis débordait tellement que la scène
/// n'éprouvait presque rien.
#[test]
fn la_plupart_des_departs_sont_dans_une_cellule() {
    let played = played();
    let inside = played.iter().filter(|(_, hit)| hit.is_some()).count();
    assert!(
        inside * 2 > played.len(),
        "{inside} départs utiles sur {}",
        played.len()
    );
}

/// La scène rencontre les trois états qu'un balayage peut rendre.
///
/// Un décor qui n'en produirait qu'un figerait une empreinte sans avoir éprouvé
/// les deux autres — et le statut du départ solide est précisément celui qu'aucun
/// autre chemin du moteur ne rend.
#[test]
fn les_trois_etats_sont_rencontres() {
    let played = played();
    let libre = played
        .iter()
        .any(|(_, hit)| hit.is_some_and(|h| h.fraction == 1.0 && h.surface == 0));
    let contact = played
        .iter()
        .any(|(_, hit)| hit.is_some_and(|h| h.fraction > 0.0 && h.surface != 0));
    let solide = played
        .iter()
        .any(|(_, hit)| hit.is_some_and(|h| h.start_solid));

    assert!(libre, "aucun balayage libre");
    assert!(contact, "aucun contact franc");
    assert!(solide, "aucun départ dans le solide");
}

/// **La petite boîte circule dans le couloir, la grande n'y tient pas.**
///
/// C'est le couple qui rend la constante de dilatation mesurable : une marge trop
/// grande coincerait aussi la petite, une marge trop petite laisserait passer la
/// grande. Le test le garde, et il rougira le jour où la constante sera réglée de
/// travers — ce qu'aucune empreinte ne dirait, puisqu'elle changerait simplement
/// de valeur.
#[test]
fn la_petite_boite_passe_le_couloir_et_la_grande_non() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    let inside = Vec3::new(11.0, 2.5, 1.0);
    let cell = world.locate(inside);
    assert_eq!(cell, 8, "le point est dans le couloir");

    let small = Vec3::new(HALVES[0], HALVES[0], HALVES[0]);
    let large = Vec3::new(HALVES[1], HALVES[1], HALVES[1]);
    let along = Vec3::new(15.0, 2.5, 1.0);

    let small_hit = world
        .sweep(cell, small, inside, along)
        .expect("cellule connue");
    assert!(
        !small_hit.start_solid,
        "la petite boîte tient dans le couloir"
    );
    assert!(small_hit.fraction > 0.0, "et elle y avance");

    let large_hit = world
        .sweep(cell, large, inside, along)
        .expect("cellule connue");
    assert!(large_hit.start_solid, "la grande n'y tient pas");
}

/// Le plafond non solide laisse passer, le sol arrête.
///
/// Le drapeau n'a qu'un lecteur dans tout le moteur, et c'est ce test qui dit
/// qu'il le lit : sans lui, un balayage vers le haut et un balayage vers le bas
/// se ressembleraient dans l'empreinte sans que personne ne sache lequel devait
/// traverser.
#[test]
fn le_plafond_non_solide_laisse_passer_et_le_sol_arrete() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    let inside = Vec3::new(11.0, 2.5, 1.0);
    let cell = world.locate(inside);
    let half = Vec3::new(0.45, 0.45, 0.45);

    let up = world
        .sweep(cell, half, inside, Vec3::new(11.0, 2.5, 11.0))
        .expect("cellule connue");
    assert_eq!(up.fraction, 1.0, "le plafond du couloir est non solide");
    assert_eq!(up.surface, 0);

    let down = world
        .sweep(cell, half, inside, Vec3::new(11.0, 2.5, -9.0))
        .expect("cellule connue");
    assert!(down.fraction < 1.0, "le sol arrête");
    assert_ne!(down.surface, 0);
}

/// Un balayage qui franchit le portail rend ce que la force brute rend.
///
/// Le théorème de l'étape, sur le seul trajet du décor qui traverse une
/// frontière. Les autres balayages le vérifient aussi, mais celui-ci le nomme :
/// si la traversée se refermait trop tôt, c'est ici que cela se verrait.
#[test]
fn le_trajet_qui_franchit_le_portail_suit_la_force_brute() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    let from = Vec3::new(6.0, 2.5, 1.0);
    let to = Vec3::new(15.0, 2.5, 1.0);
    let half = Vec3::new(0.45, 0.45, 0.45);
    let cell = world.locate(from);
    assert_eq!(cell, 7, "le départ est dans la salle");

    let fast = world.sweep(cell, half, from, to).expect("cellule connue");
    let slow = world.sweep_brute(half, from, to);
    assert!(same(&fast, &slow), "traversée {fast:?}, brute {slow:?}");
}

/// Tous les enregistrements hachés font la même taille.
///
/// **C'est l'uniformité qui a de la valeur ici**, pas la valeur du nombre : un
/// hôte qui lit une liste de balayages et hache le résultat de chacun n'a alors
/// aucun cas particulier à porter, et le départ hors cellule ne se distingue plus
/// des autres que par son statut. Le test rougit au premier cas particulier
/// réintroduit, qui serait sinon invisible jusqu'à ce qu'un hôte diverge.
#[test]
fn chaque_enregistrement_fait_la_meme_taille() {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    let sweeps = all();

    let mut bytes = Vec::new();
    for sweep in &sweeps {
        let cell = world.locate(sweep.from);
        let found = if cell == 0 {
            None
        } else {
            world.sweep(cell, sweep.half, sweep.from, sweep.to)
        };
        let (hit, status) = publish(found, sweep.to);
        absorb(&hit, status, &mut bytes);
    }

    assert_eq!(bytes.len(), sweeps.len() * RECORD);
}

/// Un départ hors de toute cellule rend le déplacement libre que la frontière
/// rend.
///
/// La conformance ne fabrique pas ici une marque à elle : elle reproduit ce que
/// `scg_world_sweep` écrit quand `from_cell` vaut zéro — une fraction de 1 et le
/// point demandé. Un hôte hache donc la même chose sans rien savoir du cas.
#[test]
fn un_depart_hors_cellule_rend_un_deplacement_libre() {
    let to = Vec3::new(3.0, 4.0, 5.0);
    let (hit, status) = publish(None, to);

    assert_eq!(status, STATUS_NO_CELL);
    assert_eq!(hit.fraction, 1.0);
    assert_eq!(hit.point, to);
    assert_eq!(hit.surface, 0);
    assert_eq!(hit.cell, 0);
}

/// Un départ dans le solide masque une région tronquée.
///
/// La règle de priorité de l'ABI, celle que l'empreinte fige : des deux drapeaux
/// du noyau, la frontière garde le plus actionnable. Écrit ici parce que le décor
/// ne produit pas le cas — il a deux cellules, rien n'y approche la borne —, et
/// qu'une règle qu'aucune donnée n'éprouve se perd au premier remaniement.
#[test]
fn le_depart_solide_masque_la_troncature() {
    let mut hit = free(Vec3::ZERO);
    hit.start_solid = true;
    hit.incomplete = true;

    assert_eq!(publish(Some(hit), Vec3::ZERO).1, STATUS_START_SOLID);

    hit.start_solid = false;
    assert_eq!(publish(Some(hit), Vec3::ZERO).1, STATUS_INCOMPLETE);
}
