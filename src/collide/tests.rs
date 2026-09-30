// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests du balayage, sur une salle cubique fermée.
//!
//! **Tout y porte sur des propriétés vraies pour tout un intervalle** — la boîte
//! ne pénètre pas, elle s'arrête avant le mur, la normale s'oppose au mouvement,
//! les deux chemins rendent les mêmes bits. Aucune fraction attendue n'y figure :
//! la constante de dilatation n'est pas figée, et des valeurs la feraient entrer
//! dans des tests qu'elle n'a pas à décider.

use alloc::vec::Vec;

use super::*;
use crate::format::world::tests::{cell_bytes, file, material, surface_in_plane};
use crate::testing::Rng;

/// Les huit coins d'un cube de huit unités, à l'origine.
const CUBE: [[f32; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [8.0, 0.0, 0.0],
    [8.0, 8.0, 0.0],
    [0.0, 8.0, 0.0],
    [0.0, 0.0, 8.0],
    [8.0, 0.0, 8.0],
    [8.0, 8.0, 8.0],
    [0.0, 8.0, 8.0],
];

/// Une salle cubique fermée, dont les six faces s'enroulent vers l'extérieur.
///
/// Le volume signé sort donc positif, et le chargement retourne les normales vers
/// l'intérieur : c'est exactement ce qu'une salle d'éditeur produit, et le
/// balayage n'aurait aucun sens sur une cellule dont les normales rentrent.
///
/// `flags` s'applique au **sol**, ce qui permet à un seul constructeur de servir
/// aussi le cas de la surface non solide.
fn room(flags: u32) -> World {
    let faces = [
        // Sol et plafond, repère dans le plan `XY`.
        surface_in_plane(11, flags, &[0, 3, 2, 1], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        surface_in_plane(12, 0, &[4, 5, 6, 7], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        // Les deux murs perpendiculaires à `Y`.
        surface_in_plane(13, 0, &[0, 1, 5, 4], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        surface_in_plane(14, 0, &[3, 7, 6, 2], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        // Les deux murs perpendiculaires à `X`.
        surface_in_plane(15, 0, &[0, 4, 7, 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        surface_in_plane(16, 0, &[1, 2, 6, 5], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let cell = cell_bytes(7, 0, &CUBE, &faces, &[]);
    World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide")
}

/// Une boîte d'un côté, commode pour les cas où sa forme n'importe pas.
fn cube_half() -> Vec3d {
    Vec3d::new(0.5, 0.5, 0.5)
}

/// Un balayage qui ne rencontre rien parcourt tout son déplacement.
#[test]
fn un_balayage_libre_parcourt_tout_son_deplacement() {
    let world = room(0);
    let hit = sweep(
        &world,
        7,
        cube_half(),
        Vec3d::new(4.0, 4.0, 4.0),
        Vec3d::new(4.5, 4.0, 4.0),
    )
    .expect("cellule connue");

    assert_eq!(hit.fraction, 1.0);
    assert_eq!(hit.surface, 0);
    assert!(!hit.start_solid);
    assert!(!hit.incomplete);
}

/// Une boîte lancée contre un mur s'arrête avant de le traverser.
///
/// La propriété, pas la valeur : le bord de la boîte reste du bon côté du mur,
/// quelle que soit la dilatation.
#[test]
fn une_boite_lancee_contre_un_mur_ne_le_traverse_pas() {
    let world = room(0);
    let half = cube_half();
    let from = Vec3d::new(4.0, 4.0, 4.0);
    let to = Vec3d::new(20.0, 4.0, 4.0);
    let hit = sweep(&world, 7, half, from, to).expect("cellule connue");

    assert!(hit.fraction < 1.0, "le mur arrête la boîte");
    assert_eq!(hit.surface, 16, "c'est le mur `x = 8`");
    let centre = from + (to - from) * f64::from(hit.fraction);
    assert!(
        centre.x + half.x <= 8.0,
        "la boîte reste en deçà du mur : {centre:?}"
    );
}

/// La normale rendue s'oppose au mouvement, et elle est unitaire.
///
/// C'est ce dont l'hôte a besoin pour écrire sa réponse — glissade ou rebond —,
/// et le moteur ne lui en donne pas d'autre.
#[test]
fn la_normale_rendue_s_oppose_au_mouvement() {
    let world = room(0);
    let from = Vec3d::new(4.0, 4.0, 4.0);
    let to = Vec3d::new(20.0, 4.0, 4.0);
    let hit = sweep(&world, 7, cube_half(), from, to).expect("cellule connue");

    let normal = Vec3d::from(hit.normal);
    assert!(normal.dot(to - from) < 0.0, "elle s'oppose au mouvement");
    let square = normal.dot(normal);
    assert!((square - 1.0) > -1e-6 && (square - 1.0) < 1e-6, "unitaire");
}

/// Le temps rendu, appliqué, laisse la boîte hors du solide.
///
/// **C'est la propriété que la boîte de sécurité existe pour tenir** : sans
/// dilatation, la boîte s'arrêterait exactement sur le mur et le balayage suivant
/// repartirait en départ solide. Elle vaut pour toute valeur de dilatation
/// strictement positive, donc elle survivra au réglage de la constante.
#[test]
fn le_temps_rendu_laisse_la_boite_hors_du_solide() {
    let world = room(0);
    let half = cube_half();
    let from = Vec3d::new(4.0, 4.0, 4.0);
    let to = Vec3d::new(20.0, 4.0, 4.0);
    let hit = sweep(&world, 7, half, from, to).expect("cellule connue");

    let landed = from + (to - from) * f64::from(hit.fraction);
    let again =
        sweep(&world, 7, half, landed, landed + Vec3d::new(0.1, 0.0, 0.0)).expect("cellule connue");
    assert!(
        !again.start_solid,
        "le point d'arrivée n'est pas dans le mur"
    );
}

/// Une boîte posée dans un mur rend un départ solide, sans dégager.
#[test]
fn une_boite_dans_un_mur_rend_un_depart_solide() {
    let world = room(0);
    let inside = Vec3d::new(8.0, 4.0, 4.0);
    let hit = sweep(
        &world,
        7,
        cube_half(),
        inside,
        inside + Vec3d::new(1.0, 0.0, 0.0),
    )
    .expect("cellule connue");

    assert!(hit.start_solid);
    assert_eq!(hit.fraction, 0.0);
    assert_ne!(hit.surface, 0, "la surface la moins pénétrée est nommée");
}

/// Un sol non solide ne retient rien, mais le reste de la salle si.
///
/// Le drapeau décrit la géométrie et non l'appelant : c'est le seul lecteur qui
/// l'écarte, et la parité qui localise un point continue de le compter.
#[test]
fn un_sol_non_solide_ne_retient_rien() {
    let world = room(0b100);
    let half = cube_half();
    let from = Vec3d::new(4.0, 4.0, 4.0);

    let through =
        sweep(&world, 7, half, from, Vec3d::new(4.0, 4.0, -20.0)).expect("cellule connue");
    assert_eq!(through.fraction, 1.0, "le sol laisse passer");

    let stopped = sweep(&world, 7, half, from, Vec3d::new(4.0, 4.0, 20.0)).expect("cellule connue");
    assert!(stopped.fraction < 1.0, "le plafond arrête toujours");
}

/// Une cellule de départ inconnue ne rend rien.
///
/// C'est à l'appelant d'en faire une ressource inconnue : le noyau ne connaît pas
/// les codes de l'ABI.
#[test]
fn une_cellule_inconnue_ne_rend_rien() {
    let world = room(0);
    let from = Vec3d::new(4.0, 4.0, 4.0);
    assert_eq!(sweep(&world, 99, cube_half(), from, from), None);
}

/// Un déplacement nul hors du solide ne rencontre rien.
#[test]
fn un_deplacement_nul_hors_du_solide_ne_rencontre_rien() {
    let world = room(0);
    let point = Vec3d::new(4.0, 4.0, 4.0);
    let hit = sweep(&world, 7, cube_half(), point, point).expect("cellule connue");

    assert_eq!(hit.fraction, 1.0);
    assert!(!hit.start_solid);
}

/// Une boîte d'étendue nulle balaie comme un rayon.
#[test]
fn une_boite_d_etendue_nulle_balaie_comme_un_rayon() {
    let world = room(0);
    let from = Vec3d::new(4.0, 4.0, 4.0);
    let hit =
        sweep(&world, 7, Vec3d::ZERO, from, Vec3d::new(20.0, 4.0, 4.0)).expect("cellule connue");

    assert!(hit.fraction < 1.0);
    assert_eq!(hit.surface, 16);
}

/// **Le premier oracle** : sur une carte d'une cellule, la traversée et le chemin
/// de force brute rendent exactement les mêmes bits.
///
/// Une carte d'une seule cellule ne met pas la traversée à l'épreuve — c'est le
/// décor de conformance qui le fera, avec ses portails. Ce que ce test attrape
/// dès maintenant, c'est une divergence entre les deux parcours sur la même
/// géométrie : ils doivent départager deux contacts au même instant de la même
/// façon, et l'ordre du fichier est le seul critère qui le leur donne.
#[test]
fn la_traversee_et_la_force_brute_rendent_les_memes_bits() {
    let world = room(0);
    let half = cube_half();
    let mut rng = Rng::new(0x51EE_D000_0000_0007);

    for round in 0..256 {
        let from = random_point(&mut rng);
        let to = random_point(&mut rng);
        let fast = sweep(&world, 7, half, from, to).expect("cellule connue");
        let slow = sweep_brute(&world, half, from, to);
        assert_eq!(
            fast, slow,
            "tour {round} : {from:?} vers {to:?}, graine 0x51EED00000000007"
        );
    }
}

/// **Le second oracle** : le prédicat de recouvrement ne trouve rien avant
/// l'instant rendu.
///
/// Il ne partage aucune algèbre avec le balayage — des axes séparateurs contre
/// une découpe d'intervalle, la triangulation contre le polygone —, et c'est ce
/// qui en fait un contrôle et non une redite. Sans lui, une formule de balayage
/// fausse rendrait la traversée et la force brute fausses de la même façon.
#[test]
fn rien_ne_recouvre_avant_l_instant_rendu() {
    let world = room(0);
    let half = cube_half();
    let mut rng = Rng::new(0x0BEC_7000_0000_0003);

    for round in 0..128 {
        let from = random_inside(&mut rng);
        let to = random_point(&mut rng);
        let hit = sweep(&world, 7, half, from, to).expect("cellule connue");
        if hit.start_solid {
            continue;
        }
        // Une grille d'instants strictement avant le contact : aucun ne doit
        // recouvrir quoi que ce soit, sans quoi la boîte serait passée à travers.
        for step in 0..8 {
            let t = f64::from(hit.fraction) * (f64::from(step) / 8.0);
            let centre = from + (to - from) * t;
            for cell in world.cells() {
                for surface in &cell.surfaces {
                    if !surface.is_solid() {
                        continue;
                    }
                    assert_eq!(
                        overlap::penetration(cell, surface, shrunk(half), centre),
                        None,
                        "tour {round}, pas {step} : recouvrement avant le contact"
                    );
                }
            }
        }
    }
}

/// La boîte du prédicat, légèrement plus petite que celle du balayage.
///
/// Le balayage dilate la sienne pour laisser une marge ; l'oracle doit donc
/// vérifier qu'il n'y a pas de recouvrement **de la boîte vraie**, sans quoi il
/// rougirait sur la marge elle-même — c'est-à-dire sur ce que la dilatation
/// existe pour créer.
fn shrunk(half: Vec3d) -> Vec3d {
    half * (1.0 - 2.0 * SKIN)
}

/// Un point tiré dans le cube et un peu autour.
///
/// Un peu autour, parce que les départs dans le solide et les mouvements qui
/// sortent sont des cas de la carte que l'oracle doit voir : les tirer tous à
/// l'intérieur ne ferait jamais travailler le départ solide.
fn random_point(rng: &mut Rng) -> Vec3d {
    let mut axis = || f64::from(rng.coord(-2, 10));
    Vec3d::new(axis(), axis(), axis())
}

/// Un point tiré strictement dans le cube.
///
/// Le second oracle en a besoin, là où le premier veut des points partout : une
/// boîte qui **entre** dans la salle depuis le dehors traverse ses murs par leur
/// face arrière, que le balayage ne retient pas — c'est la clause « franchi en
/// entrant » —, alors qu'un prédicat de recouvrement, lui, la voit. Les deux ont
/// raison, et les mélanger ferait rougir l'oracle sur un cas qui n'est pas le
/// sien.
fn random_inside(rng: &mut Rng) -> Vec3d {
    let mut axis = || f64::from(rng.coord(2, 6));
    Vec3d::new(axis(), axis(), axis())
}

/// Les sommets d'une surface tiennent dans le tableau de taille fixe.
///
/// Le plafond est celui de la triangulation, que le chargement fait déjà
/// respecter : ce test dit que les deux sont bien le même nombre, et il rougirait
/// si l'un des deux bougeait sans l'autre.
#[test]
fn les_sommets_d_une_surface_tiennent_sur_la_pile() {
    let world = room(0);
    let cell = &world.cells()[0];
    for surface in &cell.surfaces {
        let points: Vec<Vec3d> = corners(cell, surface).iter().copied().collect();
        assert_eq!(points.len(), surface.corners.len());
    }
}
