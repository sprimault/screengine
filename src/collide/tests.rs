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
use crate::format::world::tests::{cell_bytes, file, material, portal_bytes, surface_in_plane};
use crate::testing::Rng;

/// Le balayage de ces épreuves : contre les **surfaces solides**, celles qui
/// arrêtent un volume.
///
/// Enveloppé une fois plutôt que nommé à chaque appel : tout ce fichier porte
/// sur la collision, et c'est elle qui définit le filtre. Ce que l'autre filtre
/// change est éprouvé à part, là où il est le sujet.
fn sweep(world: &World, from_cell: u32, half: Vec3d, from: Vec3d, to: Vec3d) -> Option<Hit> {
    super::sweep(world, from_cell, half, from, to, Surfaces::Solid)
}

/// L'oracle de force brute, même filtre et pour la même raison.
fn sweep_brute(world: &World, half: Vec3d, from: Vec3d, to: Vec3d) -> Hit {
    super::brute::sweep_brute(world, half, from, to, Surfaces::Solid)
}

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
    let cell = cell_bytes(7, 0, &CUBE, &cube_faces(11, flags), &[]);
    World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide")
}

/// Une boîte d'un côté, commode pour les cas où sa forme n'importe pas.
fn cube_half() -> Vec3d {
    Vec3d::new(0.5, 0.5, 0.5)
}

/// Les six faces d'une salle cubique, à partir du rang de son premier sommet.
///
/// Extrait de [`room`] quand une seconde salle a eu besoin des mêmes faces sur
/// d'autres sommets : la liste des enroulements est ce qu'on recopie de travers,
/// et une face retournée ne se voit sur aucune image, cette carte n'en rendant
/// aucune.
fn cube_faces(first_id: u32, floor_flags: u32) -> Vec<Vec<u8>> {
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    alloc::vec![
        // Sol et plafond, repère dans le plan `XY`.
        surface_in_plane(first_id, floor_flags, &[0, 3, 2, 1], x, y),
        surface_in_plane(first_id + 1, 0, &[4, 5, 6, 7], x, y),
        // Les deux murs perpendiculaires à `Y`.
        surface_in_plane(first_id + 2, 0, &[0, 1, 5, 4], x, z),
        surface_in_plane(first_id + 3, 0, &[3, 7, 6, 2], x, z),
        // Les deux murs perpendiculaires à `X`.
        surface_in_plane(first_id + 4, 0, &[0, 4, 7, 3], y, z),
        surface_in_plane(first_id + 5, 0, &[1, 2, 6, 5], y, z),
    ]
}

/// Les huit coins du cube, décalés le long de `Y`.
fn shifted_cube(dy: f32) -> [[f32; 3]; 8] {
    let mut points = CUBE;
    for point in &mut points {
        point[1] += dy;
    }
    points
}

/// Deux salles cubiques disjointes, sans portail entre elles.
///
/// **Les deux sont convexes, et c'est tout l'intérêt.** Le cas de la cellule en U
/// demande une cellule non convexe parce que la face fautive y est celle du
/// départ ; celui-ci n'en demande aucune, parce que la face fautive appartient à
/// **l'autre** cellule. C'est la forme sous laquelle un intégrateur rencontre le
/// défaut, et c'est aussi la seule où les deux chemins **divergent** : la force
/// brute examine la seconde salle, la traversée ne l'atteint par aucun portail.
fn two_rooms() -> World {
    let mut cells = cell_bytes(7, 0, &CUBE, &cube_faces(11, 0), &[]);
    let far = shifted_cube(16.0);
    cells.extend_from_slice(&cell_bytes(8, 0, &far, &cube_faces(21, 0), &[]));
    World::load(&file(&cells, &[], &[], &material(1, "mur"))).expect("carte valide")
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

/// **Le rayon voit ce que le balayage ignore**, et c'est tout ce qui les
/// sépare.
///
/// Une sélection d'éditeur doit attraper une grille, une vitre, un volume de
/// déclenchement — des surfaces que la carte marque « non solides » et que la
/// collision traverse par construction. Sans ce filtre, l'éditeur ne pourrait
/// désigner que ce qui arrête un personnage.
#[test]
fn le_rayon_voit_les_surfaces_non_solides() {
    let world = room(0b100);
    let from = Vec3d::new(4.0, 4.0, 4.0);
    let to = Vec3d::new(4.0, 4.0, -20.0);

    let solide = world
        .pick(7, to_f32(from), to_f32(to), Surfaces::Solid)
        .expect("cellule connue");
    assert_eq!(
        solide.fraction, 1.0,
        "le sol non solide ne doit pas arrêter"
    );
    assert_eq!(solide.surface, 0, "et rien n'est nommé");

    let toutes = world
        .pick(7, to_f32(from), to_f32(to), Surfaces::All)
        .expect("cellule connue");
    assert!(toutes.fraction < 1.0, "la sélection touche le sol");
    assert_eq!(toutes.surface, 11, "et le nomme par son identifiant stable");
}

/// **Le rayon n'est pas dilaté**, là où le balayage l'est.
///
/// La marge de sécurité est relative à la plus grande demi-étendue, donc nulle
/// pour une boîte d'étendue nulle : un rayon touche ce qu'il croise, jamais ce
/// qu'il frôle. Une sélection dilatée désignerait une surface voisine de celle
/// que l'utilisateur vise, ce qui est le défaut qu'un éditeur pardonne le moins.
#[test]
fn le_rayon_ne_porte_aucune_marge() {
    let world = room(0);
    // Droit vers le mur en `x = 8`, depuis le centre : le contact tombe à
    // quatre unités sur les huit du trajet.
    let from = Vec3d::new(4.0, 4.0, 4.0);
    let to = Vec3d::new(12.0, 4.0, 4.0);

    let rayon = world
        .pick(7, to_f32(from), to_f32(to), Surfaces::Solid)
        .expect("cellule connue");
    assert_eq!(
        rayon.fraction, 0.5,
        "le rayon s'arrête exactement sur le plan"
    );

    // La même trajectoire avec une boîte s'arrête **avant**, de la dilatation
    // plus la demi-étendue : c'est ce qui rend la comparaison parlante.
    let boite = world
        .sweep(7, to_f32(cube_half()), to_f32(from), to_f32(to))
        .expect("cellule connue");
    assert!(
        boite.fraction < rayon.fraction,
        "la boîte s'arrête avant le rayon : {} contre {}",
        boite.fraction,
        rayon.fraction
    );
}

/// **Le théorème du lot** : le rayon qui traverse rend ce que la force brute
/// rend.
///
/// Le même que pour le balayage, et il porte sur les deux filtres : c'est la
/// traversée par portails qui est éprouvée, et elle ne doit pas dépendre de ce
/// que la requête retient.
#[test]
fn le_rayon_par_portails_egale_la_force_brute() {
    for flags in [0, 0b100] {
        let world = room(flags);
        let mut rng = Rng::new(0x5241_594F_4E00_0001);

        for surfaces in [Surfaces::Solid, Surfaces::All] {
            for round in 0..256 {
                let from = to_f32(random_point(&mut rng));
                let to = to_f32(random_point(&mut rng));
                let fast = world.pick(7, from, to, surfaces).expect("cellule connue");
                let slow = world.pick_brute(from, to, surfaces);
                assert_eq!(
                    fast, slow,
                    "tour {round}, drapeaux {flags:#b}, filtre {surfaces:?}"
                );
            }
        }
    }
}

/// Une cellule de départ inconnue rend `None`, comme pour le balayage.
#[test]
fn le_rayon_refuse_une_cellule_inconnue() {
    let world = room(0);
    let at = to_f32(Vec3d::new(4.0, 4.0, 4.0));
    assert_eq!(world.pick(99, at, at, Surfaces::All), None);
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

/// **La fraction rendue est toujours dans `[0, 1]`, quelle que soit l'entrée.**
///
/// Une propriété de la **sortie** du balayage, et non d'un chemin de calcul :
/// elle vaut pour tous ceux qui existent et pour ceux qu'on n'a pas écrits. Elle
/// se teste sans construire de scène particulière, et c'est ce qui la distingue
/// d'un correctif local.
///
/// Ce qu'elle protège est le défaut le plus discret de sa famille : le contrat
/// annonce `[0, 1]`, donc aucun hôte n'a de raison de tester une valeur hors
/// bornes. Elle traverserait tous ses garde-fous, et un déplacement à rebours se
/// manifesterait comme un défaut de son code de glissade, très loin de sa cause.
#[test]
fn la_fraction_reste_dans_ses_bornes() {
    let world = room(0);
    let mut rng = Rng::new(0x000B_07E5_0000_0011);

    for round in 0..512 {
        // Des entrées volontairement extrêmes : très loin, très près, nulles,
        // et des boîtes de toutes tailles jusqu'à plus grandes que la salle.
        let scale = f64::from(rng.coord(0, 40));
        let from = random_point(&mut rng);
        let to = Vec3d::new(
            from.x + f64::from(rng.coord(-20, 20)) * scale,
            from.y + f64::from(rng.coord(-20, 20)) * scale,
            from.z + f64::from(rng.coord(-20, 20)) * scale,
        );
        let side = f64::from(rng.coord(0, 12)) * 0.5;
        let half = Vec3d::new(side, side, side);

        for cell in [7, 0] {
            let Some(hit) = sweep(&world, cell.max(7), half, from, to) else {
                continue;
            };
            assert!(
                (0.0..=1.0).contains(&hit.fraction),
                "tour {round} : fraction {} hors bornes, graine 0xB07E500000000011",
                hit.fraction
            );
        }
    }
}

/// Une cellule cubique dont le portail tourne dans le sens qu'on lui donne.
///
/// `flip` écrit le portail de `x = x1` à l'envers de ses murs. Le format
/// l'autorise : il ne fixe que l'enroulement **inverse entre les deux portails
/// d'une paire**, jamais celui d'un portail par rapport aux surfaces de sa propre
/// cellule — et le générateur du décor de conformance écrit déjà les siens ainsi.
///
/// `origin` l'éloigne de l'origine du monde, ce qui est l'autre moitié du cas.
fn cell_with_portal(origin: f32, flip: bool) -> World {
    let x0 = origin;
    let x1 = x0 + 4.0;
    let points: [[f32; 3]; 8] = [
        [x0, 0.0, 0.0],
        [x1, 0.0, 0.0],
        [x1, 4.0, 0.0],
        [x0, 4.0, 0.0],
        [x0, 0.0, 4.0],
        [x1, 0.0, 4.0],
        [x1, 4.0, 4.0],
        [x0, 4.0, 4.0],
    ];
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    let surfaces = [
        surface_in_plane(11, 0, &[0, 3, 2, 1], x, y),
        surface_in_plane(12, 0, &[4, 5, 6, 7], x, y),
        surface_in_plane(13, 0, &[0, 1, 5, 4], x, z),
        surface_in_plane(14, 0, &[3, 7, 6, 2], x, z),
        surface_in_plane(15, 0, &[0, 4, 7, 3], y, z),
    ];
    let portal = if flip {
        portal_bytes(31, &[5, 6, 2, 1])
    } else {
        portal_bytes(31, &[1, 2, 6, 5])
    };
    let cells = cell_bytes(7, 0, &points, &surfaces, &[portal]);
    World::load(&file(&cells, &[], &[], &material(1, "mur"))).expect("carte valide")
}

/// **Ni l'enroulement d'un portail ni la distance à l'origine ne décident de ce
/// que la cellule arrête.**
///
/// Remonté d'un intégrateur : sur un labyrinthe de 16 cases de 4 unités, 161
/// cellules sur 499 n'arrêtaient **aucun** de leurs murs — ni sol, ni plafond, ni
/// parois —, 338 les arrêtaient tous, et aucune n'était mélangée. Un partage par
/// cellule et jamais par surface désigne le signe du volume, que [`Cell::inward`]
/// applique à toutes ses faces : faux, il retourne la cellule entière, le test
/// « franchi en entrant » rejette tout, et le départ dans le solide nomme son sol
/// avec une normale dirigée vers le bas. Les 161 cellules sourdes étaient les plus
/// éloignées de l'origine, et un décor plus petit n'aurait rien montré.
///
/// Ce que le test met sous tension : ce signe se calcule par une somme de
/// `point · normale` sur les surfaces **et** les portails, qui n'est invariante
/// par translation que si les normales se compensent exactement — donc que si les
/// deux familles tournent dans le même sens, ce que le format n'impose pas. Les
/// quatre distances tiennent la seconde moitié : à sens cohérent le signe est bon
/// partout, et c'est le déséquilibre qui croît avec l'éloignement.
///
/// [`Cell::inward`]: crate::format::world::Cell::inward
#[test]
fn ni_l_enroulement_ni_la_distance_ne_decident_de_ce_qui_arrete() {
    let half = Vec3d::new(0.5, 0.5, 0.5);
    for flip in [false, true] {
        for origin in [0.0f32, 64.0, 512.0, 4096.0] {
            let world = cell_with_portal(origin, flip);
            let from = Vec3d::new(f64::from(origin) + 2.0, 2.0, 2.0);
            // Vers le mur `y = 4`, qui est une surface et non un portail.
            let to = Vec3d::new(from.x, 12.0, 2.0);
            let hit = sweep(&world, 7, half, from, to).expect("cellule connue");

            assert!(
                hit.fraction < 1.0,
                "portail inversé {flip}, origine {origin} : le mur n'arrête plus, \
                 fraction {}",
                hit.fraction
            );
            assert!(
                hit.normal.y < 0.0,
                "portail inversé {flip}, origine {origin} : normale {:?}, \
                 elle devrait s'opposer au mouvement",
                hit.normal
            );
        }
    }
}

/// **Un portail qui ne partage aucune arête ne fait pas échouer le chargement.**
///
/// C'est le repli de la dérivation d'enroulement, et sans ce cas il resterait du
/// code que rien n'exécute. La précondition — un portail partage au moins une
/// arête avec une surface de sa cellule — tient par construction d'un décor plein,
/// un portail remplaçant un mur et reprenant ses sommets. Une carte d'éditeur
/// intermédiaire n'a pourtant pas à être refusée pour cela : le chargement passe,
/// le portail compte dans son sens écrit, et rien ne panique.
///
/// Le portail est ici une **diagonale** du cube, dont aucune arête n'appartient à
/// une face.
#[test]
fn un_portail_sans_arete_partagee_se_charge_quand_meme() {
    let cells = cell_bytes(
        7,
        0,
        &CUBE,
        &cube_faces(11, 0),
        &[portal_bytes(31, &[0, 2, 6, 4])],
    );
    let world = World::load(&file(&cells, &[], &[], &material(1, "mur")))
        .expect("une carte d'éditeur intermédiaire reste chargeable");

    // Et la cellule reste interrogeable : c'est tout ce qu'on lui demande.
    let hit = sweep(
        &world,
        7,
        cube_half(),
        Vec3d::new(4.0, 4.0, 4.0),
        Vec3d::new(4.0, 20.0, 4.0),
    )
    .expect("cellule connue");
    assert!(hit.fraction <= 1.0, "fraction {}", hit.fraction);
}

/// **La marge annoncée dimensionne vraiment une sonde.**
///
/// C'est tout ce qu'un hôte lui demande, et il faut l'énoncer en ces termes
/// plutôt qu'en égalité : la marge vit en `f64` dans le balayage et sort en `f32`
/// par la frontière, donc la valeur annoncée est l'arrondi de celle appliquée. Une
/// égalité stricte tiendrait à un bit et casserait au premier réglage de la
/// constante — ce que les tests de ce module s'interdisent précisément.
///
/// Ce qui est éprouvé : le jeu que le balayage laisse est strictement positif, et
/// il tient sous **deux fois** la marge annoncée. Une sonde de cette longueur
/// atteint donc toujours le sol sur lequel le mobile vient d'être posé.
#[test]
fn la_marge_annoncee_dimensionne_une_sonde() {
    let world = room(0);
    for half in [
        Vec3d::new(0.5, 0.5, 0.5),
        Vec3d::new(0.3, 0.3, 0.9),
        // Plate : le facteur porte sur la plus grande demi-étendue, donc même
        // une épaisseur nulle reçoit une marge — sans quoi rien ne séparerait
        // jamais une lame du sol qu'elle touche.
        Vec3d::new(2.0, 2.0, 0.0),
    ] {
        let margin = f64::from(super::sweep_skin(to_f32(half)));
        assert!(margin > 0.0, "une marge nulle pour {half:?}");

        // Posé sur le sol `z = 0` de la salle, par le balayage lui-même.
        let from = Vec3d::new(4.0, 4.0, 4.0);
        let to = Vec3d::new(4.0, 4.0, -4.0);
        let hit = sweep(&world, 7, half, from, to).expect("cellule connue");
        let landed = from + (to - from) * f64::from(hit.fraction);
        let gap = landed.z - half.z;

        assert!(gap > 0.0, "{half:?} : la boîte touche le sol, jeu {gap}");
        assert!(
            gap <= 2.0 * margin,
            "{half:?} : jeu {gap} au-delà de deux marges de {margin}"
        );
    }
}

/// Une boîte sans étendue n'a pas de marge, et un `NaN` non plus.
///
/// Le rayon n'est pas dilaté — c'est une clause publiée —, donc sa marge est
/// nulle ; et `NaN` est écarté nommément, faute de quoi il traverserait la
/// comparaison de bornes pour ressortir du produit.
#[test]
fn une_boite_sans_etendue_n_a_pas_de_marge() {
    assert_eq!(super::sweep_skin(Vec3::ZERO), 0.0);
    assert_eq!(super::sweep_skin(Vec3::new(f32::NAN, 0.0, 0.0)), 0.0);
    assert_eq!(super::sweep_skin(Vec3::new(-1.0, -1.0, -1.0)), 0.0);
}

/// **Suivre la normale d'un départ solide finit par en sortir.**
///
/// Le moteur signale un départ dans le solide et ne dégage pas — il n'existe
/// aucun vecteur de dégagement défini contre un jeu de surfaces non convexes.
/// Tout ce qu'il doit à l'hôte, c'est que la normale qu'il rend **mène dehors** :
/// c'est elle, et elle seule, qui rend la politique écrivable.
///
/// Ce qu'il éprouve est la question « suis-je dedans, et par où sortir », posée par
/// un déplacement nul — donc la normale seule, sans aucun contact plus loin pour
/// la départager. C'est ce dont un hôte a besoin pour écrire son dégagement, et
/// ce qui manquait à l'exemple du dépôt : rendre le pas libre sur un départ
/// solide enfonce davantage, le pas suivant repart solide, et l'engrenage ne se
/// défait jamais.
#[test]
fn suivre_la_normale_d_un_depart_solide_en_sort() {
    let world = room(0);
    let half = cube_half();
    // Dans le mur `x = 8`, assez pour que la boîte le traverse.
    let mut at = Vec3d::new(8.0, 4.0, 4.0);

    let mut escaped = false;
    for step in 0..64 {
        let hit = sweep(&world, 7, half, at, at).expect("cellule connue");
        if !hit.start_solid {
            escaped = true;
            break;
        }
        assert_ne!(
            hit.surface, 0,
            "pas {step} : un départ solide nomme la surface qui pénètre"
        );
        let normal = Vec3d::from(hit.normal);
        assert!(
            normal.dot(normal) > 0.0,
            "pas {step} : une normale nulle ne mène nulle part"
        );
        at = at + normal * 0.0625;
    }
    assert!(escaped, "le dégagement n'a pas abouti depuis {at:?}");

    // **Et il sort du bon côté**, ce qui est tout le propos : sans cette ligne le
    // test passe aussi avec une normale retournée, puisqu'on cesse de recouvrir
    // une surface en s'en éloignant dans un sens comme dans l'autre. Vérifié en
    // niant la normale rendue, qui le laissait vert.
    assert_eq!(
        world.locate(to_f32(at)),
        7,
        "le dégagement a quitté la salle au lieu d'y rentrer, en {at:?}"
    );
}

/// **Aucune composante du résultat ne porte un zéro négatif ni un `NaN`.**
///
/// `docs/rust.md` annonçait cette clause « tenue par un test qui inspecte les
/// bits », et aucun test ne le faisait : celui des bornes de la fraction compare
/// des valeurs, et `-0,0 == 0,0` est vrai. C'est donc une clause qui tenait par
/// chance, et le premier chemin qui niait un vecteur l'aurait rompue en silence.
///
/// Ce qu'elle protège : une empreinte hache des motifs de bits, donc deux
/// résultats mathématiquement égaux dont l'un porte un zéro négatif rendent deux
/// empreintes. Une cible qui en produirait là où une autre n'en produit pas ferait
/// diverger la conformance sans qu'aucun calcul soit faux — le pire cas à
/// instruire, puisque les deux résultats se lisent identiques.
///
/// **La chaîne plutôt que la salle**, et c'est ce qui donne sa portée au test :
/// ses deux bouts sont des portails non appariés, donc des murs dont la normale
/// est **retournée** quand le mouvement l'exige. C'est exactement l'opération d'où
/// un zéro négatif sort.
#[test]
fn le_resultat_ne_porte_ni_zero_negatif_ni_nan() {
    let world = chain(3);
    let mut rng = Rng::new(0x00D0_0E57_0000_0003);

    for round in 0..512 {
        let from = Vec3d::new(
            f64::from(rng.coord(-2, 14)),
            f64::from(rng.coord(-2, 6)),
            f64::from(rng.coord(-2, 6)),
        );
        let to = Vec3d::new(
            from.x + f64::from(rng.coord(-12, 12)),
            from.y + f64::from(rng.coord(-12, 12)),
            from.z + f64::from(rng.coord(-12, 12)),
        );
        let side = f64::from(rng.coord(0, 6)) * 0.25;
        let half = Vec3d::new(side, side, side);

        for cell in [1, 2, 3] {
            let Some(hit) = sweep(&world, cell, half, from, to) else {
                continue;
            };
            for (name, value) in [
                ("fraction", hit.fraction),
                ("normal.x", hit.normal.x),
                ("normal.y", hit.normal.y),
                ("normal.z", hit.normal.z),
                ("point.x", hit.point.x),
                ("point.y", hit.point.y),
                ("point.z", hit.point.z),
            ] {
                assert!(
                    !value.is_nan(),
                    "tour {round}, cellule {cell} : {name} est NaN, graine 0xD00E570000000003"
                );
                assert!(
                    value != 0.0 || value.to_bits() == 0,
                    "tour {round}, cellule {cell} : {name} est un zéro négatif, \
                     graine 0xD00E570000000003"
                );
            }
        }
    }
}

/// Une enfilade de `count` cubes de quatre unités, alignés sur `X`.
///
/// **Toutes les coordonnées sont des multiples de quatre**, et c'est ce qui fait
/// que le test mesure ce qu'il prétend : deux cellules voisines écrivent alors
/// leur portail commun avec **les mêmes bits**, donc il s'apparie. Sans cela il
/// deviendrait un mur, le balayage s'arrêterait bien avant la borne, et le test
/// passerait au vert pour la mauvaise raison.
fn chain(count: u32) -> World {
    let mut cells = Vec::new();
    for index in 0..count {
        let x0 = (index * 4) as f32;
        let x1 = x0 + 4.0;
        let points: [[f32; 3]; 8] = [
            [x0, 0.0, 0.0],
            [x1, 0.0, 0.0],
            [x1, 4.0, 0.0],
            [x0, 4.0, 0.0],
            [x0, 0.0, 4.0],
            [x1, 0.0, 4.0],
            [x1, 4.0, 4.0],
            [x0, 4.0, 4.0],
        ];
        // Sol, plafond, et les deux murs perpendiculaires à Y ; les deux faces
        // perpendiculaires à X sont des portails, sauf aux deux bouts de la
        // chaîne, où ils restent non appariés — donc solides.
        let first = 100 + index * 10;
        let surfaces = [
            surface_in_plane(
                first + 1,
                0,
                &[0, 3, 2, 1],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            ),
            surface_in_plane(
                first + 2,
                0,
                &[4, 5, 6, 7],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            ),
            surface_in_plane(
                first + 3,
                0,
                &[0, 1, 5, 4],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
            ),
            surface_in_plane(
                first + 4,
                0,
                &[3, 7, 6, 2],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
            ),
        ];
        let portals = [
            portal_bytes(first + 5, &[0, 4, 7, 3]),
            portal_bytes(first + 6, &[1, 2, 6, 5]),
        ];
        cells.extend_from_slice(&cell_bytes(index + 1, 0, &points, &surfaces, &portals));
    }
    World::load(&file(&cells, &[], &[], &material(1, "mur"))).expect("chaîne valide")
}

/// Une chaîne juste sous la borne se parcourt entière, sans statut.
///
/// **Le contrôle négatif, et il est indispensable** : sans lui, un statut rendu
/// toujours — ou rendu trop tôt — passerait inaperçu, et le test voisin serait
/// vert pour rien. C'est le témoin du compteur d'allocations, appliqué à un
/// statut.
#[test]
fn une_chaine_sous_la_borne_se_parcourt_entiere() {
    let count = SWEEP_CELLS as u32 - 1;
    let world = chain(count);
    let half = Vec3d::new(0.5, 0.5, 0.5);
    let from = Vec3d::new(2.0, 2.0, 2.0);
    let to = Vec3d::new(f64::from(count * 4) - 2.0, 2.0, 2.0);

    let hit = sweep(&world, 1, half, from, to).expect("cellule connue");
    assert!(!hit.incomplete, "la région entière a été examinée");
    assert_eq!(hit.fraction, 1.0, "et le déplacement est libre");
}

/// Une chaîne au-delà de la borne tronque le déplacement, et le dit.
///
/// **Le drapeau ne suffit pas.** Posé sur une fraction de 1, il signalerait une
/// limite tout en rendant un résultat qui dit le contraire : un hôte qui lit le
/// statut comme un avertissement plutôt que comme un refus ferait traverser le
/// mur. La fraction doit donc être **strictement inférieure**, et le point
/// d'arrêt **atteignable** — sans quoi la troncature serait pire qu'un refus
/// net.
#[test]
fn une_chaine_au_dela_de_la_borne_tronque_le_deplacement() {
    let count = SWEEP_CELLS as u32 + 8;
    let world = chain(count);
    let half = Vec3d::new(0.5, 0.5, 0.5);
    let from = Vec3d::new(2.0, 2.0, 2.0);
    let to = Vec3d::new(f64::from(count * 4) - 2.0, 2.0, 2.0);

    let hit = sweep(&world, 1, half, from, to).expect("cellule connue");
    assert!(hit.incomplete, "la région examinée s'arrête avant la fin");
    assert!(
        hit.fraction < 1.0,
        "le déplacement est tronqué, fraction {}",
        hit.fraction
    );
    assert_eq!(hit.surface, 0, "aucune surface n'a été touchée");

    // **Le point d'arrêt est atteignable** : un balayage repris de là ne part pas
    // du solide, ce qui est tout ce que l'hôte demande pour y poser son mobile.
    let landed = from + (to - from) * f64::from(hit.fraction);
    let cell = world.locate(to_f32(landed));
    assert_ne!(cell, 0, "le point d'arrêt est dans une cellule");
    let again = sweep(&world, cell, half, landed, landed).expect("cellule connue");
    assert!(!again.start_solid, "et il n'est pas dans un mur");
}

/// **Un portail non apparié arrête le balayage**, comme le ferait un mur.
///
/// C'est le mot du format — « un portail non apparié est un mur » — et c'est ce
/// qui garde la cellule fermée : passable, il ferait tomber un mobile hors du
/// monde sur une carte en cours d'édition. Aucun contrôle ne l'éprouvait, et
/// [`chain`] en porte deux, un à chaque bout.
#[test]
fn un_portail_non_apparie_arrete_le_balayage() {
    let world = chain(1);
    let half = Vec3d::new(0.5, 0.5, 0.5);
    let from = Vec3d::new(2.0, 2.0, 2.0);
    // Droit vers le bout de la chaîne, dont le portail n'a pas de voisin.
    let to = Vec3d::new(10.0, 2.0, 2.0);

    let hit = sweep(&world, 1, half, from, to).expect("cellule connue");

    assert!(
        hit.fraction < 1.0,
        "le portail non apparié retient la boîte, fraction {}",
        hit.fraction
    );
    let centre = from + (to - from) * f64::from(hit.fraction);
    assert!(
        centre.x + half.x <= 4.0,
        "et elle reste dans la cellule : {centre:?}"
    );
}

/// Une chaîne d'une seule cellule ne rend jamais une fraction négative.
///
/// Le cas dégénéré : la première cellule est déjà la dernière examinable, et le
/// recul mordrait sur l'origine. **Zéro est alors la réponse honnête** — l'hôte
/// apprend qu'il n'avance pas, ce qui est vrai — là où une valeur négative serait
/// un déplacement à rebours qu'il n'a pas demandé, et que le contrat lui donne
/// toutes les raisons de ne pas tester.
#[test]
fn une_troncature_ne_recule_jamais_avant_le_depart() {
    let world = chain(1);
    let half = Vec3d::new(0.5, 0.5, 0.5);
    let from = Vec3d::new(2.0, 2.0, 2.0);

    for reach in [0.1, 1.0, 10.0] {
        let to = Vec3d::new(from.x + reach, 2.0, 2.0);
        let hit = sweep(&world, 1, half, from, to).expect("cellule connue");
        assert!(
            hit.fraction >= 0.0,
            "portée {reach} : fraction {}",
            hit.fraction
        );
    }
}

/// **Un balayage repris là où le précédent s'est arrêté ne traverse pas.**
///
/// La propriété que tous les autres tests manquaient, parce qu'ils jouent chaque
/// balayage isolément : ce qu'un hôte fait, lui, c'est enchaîner, image après
/// image, en repartant chaque fois du point d'arrêt. C'est exactement là que le
/// moteur doit rester utilisable.
///
/// Ce qu'elle a attrapé : entre la vraie boîte et la boîte dilatée, une position
/// d'arrêt tombe dans une bande où le plan de contact donne un instant d'impact
/// **négatif**, écarté comme « pas de contact », tandis que le départ dans le
/// solide, mesuré sur la vraie boîte, reste faux. Le balayage suivant rendait
/// donc un déplacement libre, et la boîte entrait dans le mur — puis le
/// traversait, un pas après l'autre.
/// **Par l'API publique et en `f32`**, et non par la fonction interne : ce qui
/// déclenche le défaut est l'arrondi de la fraction, que le contrat rend en
/// simple précision et dont l'hôte tire sa position. En `f64`, le point d'arrêt
/// reste du bon côté et le cas ne se produit jamais — un test écrit là aurait
/// prouvé quelque chose que personne n'observe.
#[test]
fn un_balayage_repris_au_point_d_arret_ne_traverse_pas() {
    let world = room(0);
    let half = Vec3::new(0.3, 0.3, 0.9);
    let mut at = Vec3::new(4.0, 4.0, 4.0);

    // Des pas courts, comme ceux d'une image : c'est la répétition qui compte,
    // pas la longueur.
    for step in 0..40 {
        let to = Vec3::new(at.x, at.y - 0.2, at.z);
        let hit = world.sweep(7, half, at, to).expect("cellule connue");
        at = at + (to - at) * hit.fraction;

        assert!(
            at.y - half.y >= 0.0,
            "pas {step} : la boîte est entrée dans le mur, bord à {}",
            at.y - half.y
        );
    }
}

/// Les huit coins du cube, décalés le long de `X`.
fn cube_shifted_x(dx: f32) -> [[f32; 3]; 8] {
    let mut points = CUBE;
    for point in &mut points {
        point[0] += dx;
    }
    points
}

/// Deux salles cubiques accolées, reliées par un portail apparié.
///
/// **La seconde porte une vraie surface au fond**, et c'est ce qui en fait le
/// décor du cas : il y faut un contact de **face** dans une cellule que seule la
/// traversée atteint. [`chain`] ne l'offre pas — ses deux bouts sont des portails
/// non appariés, que le balayage ne regarde pas.
///
/// Les deux portails portent les mêmes positions au bit près, les coordonnées
/// étant des multiples de huit, et des enroulements inverses : ils s'apparient.
fn linked_rooms() -> World {
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    // Cinq faces pleines par salle, la sixième étant le portail : le `+X` de la
    // première, le `−X` de la seconde.
    let near = alloc::vec![
        surface_in_plane(11, 0, &[0, 3, 2, 1], x, y),
        surface_in_plane(12, 0, &[4, 5, 6, 7], x, y),
        surface_in_plane(13, 0, &[0, 1, 5, 4], x, z),
        surface_in_plane(14, 0, &[3, 7, 6, 2], x, z),
        surface_in_plane(15, 0, &[0, 4, 7, 3], y, z),
    ];
    let far = alloc::vec![
        surface_in_plane(21, 0, &[0, 3, 2, 1], x, y),
        surface_in_plane(22, 0, &[4, 5, 6, 7], x, y),
        surface_in_plane(23, 0, &[0, 1, 5, 4], x, z),
        surface_in_plane(24, 0, &[3, 7, 6, 2], x, z),
        surface_in_plane(25, 0, &[1, 2, 6, 5], y, z),
    ];
    let mut cells = cell_bytes(7, 0, &CUBE, &near, &[portal_bytes(31, &[1, 2, 6, 5])]);
    cells.extend_from_slice(&cell_bytes(
        8,
        0,
        &cube_shifted_x(8.0),
        &far,
        &[portal_bytes(32, &[0, 4, 7, 3])],
    ));
    World::load(&file(&cells, &[], &[], &material(1, "mur"))).expect("carte valide")
}

/// **Un départ dans le solide n'est pas écrasé par un contact plus loin.**
///
/// Le contrat annonce une fraction nulle pour un départ solide, et la normale de
/// la surface la moins pénétrée. Les deux ne tenaient que tant que le mouvement
/// restait dans la cellule de départ : la marque posait la fraction du
/// **résultat** sans toucher aux trois valeurs de départage, si bien qu'un
/// contact trouvé dans une cellule atteinte par portail passait pour meilleur et
/// remplaçait normale, point, surface et cellule — en laissant la marque. Un hôte
/// qui lit la normale pour se dégager recevait la normale d'un mur situé plus
/// loin, qui ne dégage rien, et la surface nommée n'était pas celle qui le
/// retient.
///
/// Remonté d'un intégrateur, sur le palier d'une cage d'escalier : la boîte posée
/// à la cote du sol, donc légitimement en départ solide, puis un pas horizontal
/// assez long pour sortir de la cellule. Le cas n'apparaît qu'à cette condition,
/// ce qui explique qu'aucune épreuve d'une seule cellule ne l'ait vu.
#[test]
fn un_depart_solide_resiste_a_un_contact_plus_loin() {
    let world = linked_rooms();
    // Posée pile à la cote du sol : la boîte le traverse, le départ est solide.
    let half = Vec3d::new(0.3, 0.3, 0.9);
    let from = Vec3d::new(2.0, 2.0, 0.0);
    // Au-delà du portail de `x = 8`, jusqu'au mur du fond de la seconde salle.
    let to = Vec3d::new(20.0, 2.0, 0.0);

    let hit = sweep(&world, 7, half, from, to).expect("cellule connue");

    assert!(hit.start_solid, "le sol de la première salle est traversé");
    assert_eq!(hit.fraction, 0.0, "le contrat annonce une fraction nulle");
    assert!(
        hit.normal.z > 0.9,
        "la normale est celle du sol qui pénètre, non d'un mur plus loin : {:?}",
        hit.normal
    );
    assert_eq!(hit.surface, 11, "et c'est le sol qui est nommé");
    assert_eq!(hit.cell, 7, "dans la cellule du départ");
}

/// Les seize coins d'une cellule en U, extrudée de `z = 0` à `z = 8`.
///
/// Les huit premiers dessinent le contour au sol, en sens antihoraire vu de
/// dessus ; les huit suivants les répètent au plafond. La région est la base
/// `y ≤ 2` et les deux branches `x ≤ 2` et `x ≥ 6`.
const U_ROOM: [[f32; 3]; 16] = [
    [0.0, 0.0, 0.0],
    [8.0, 0.0, 0.0],
    [8.0, 8.0, 0.0],
    [6.0, 8.0, 0.0],
    [6.0, 2.0, 0.0],
    [2.0, 2.0, 0.0],
    [2.0, 8.0, 0.0],
    [0.0, 8.0, 0.0],
    [0.0, 0.0, 8.0],
    [8.0, 0.0, 8.0],
    [8.0, 8.0, 8.0],
    [6.0, 8.0, 8.0],
    [6.0, 2.0, 8.0],
    [2.0, 2.0, 8.0],
    [2.0, 8.0, 8.0],
    [0.0, 8.0, 8.0],
];

/// Une cellule en U fermée, dont les faces s'enroulent vers l'extérieur.
///
/// **Ce que la salle cubique ne peut pas porter : un point intérieur situé
/// derrière le plan d'une de ses propres faces.** Dans un convexe, aucun point
/// intérieur ne l'est, et une salle en L non plus — la région derrière le plan
/// d'une de ses faces est précisément son quart manquant. Il faut un U : un
/// point de la branche gauche est derrière le plan de la face intérieure de la
/// branche droite, et sa projection tombe **dans** cette face.
fn u_room() -> World {
    let wall_x = ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let wall_y = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut faces = Vec::new();
    faces.push(surface_in_plane(
        11,
        0,
        &[0, 7, 6, 5, 4, 3, 2, 1],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ));
    faces.push(surface_in_plane(
        12,
        0,
        &[8, 9, 10, 11, 12, 13, 14, 15],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ));
    // Un mur par arête du contour, dans l'ordre : chacun porte les deux sommets
    // du sol puis les deux du plafond, ce qui l'enroule vers l'extérieur.
    for i in 0..8u32 {
        let j = (i + 1) % 8;
        // L'arête est-elle perpendiculaire à X ? Alors son repère prend Y.
        let frame = if U_ROOM[i as usize][0] == U_ROOM[j as usize][0] {
            wall_x
        } else {
            wall_y
        };
        faces.push(surface_in_plane(
            13 + i,
            0,
            &[i, j, j + 8, i + 8],
            frame.0,
            frame.1,
        ));
    }
    let cell = cell_bytes(7, 0, &U_ROOM, &faces, &[]);
    World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide")
}

/// **Un pas qui s'éloigne d'une face vue de derrière ne rencontre rien.**
///
/// Le contact immédiat qu'un instant d'impact négatif décrit vaut dans la dalle
/// dilatée, `|d| ≤ s`, et nulle part ailleurs. Sans cette borne, le départ en
/// `(1, 5, 4)` — dans la branche gauche du U, donc cinq unités derrière le plan
/// de la face intérieure de la branche droite — rendait un contact à la fraction
/// zéro contre cette face, que le segment ne croise jamais.
///
/// **Les deux chemins étaient faux de la même façon**, et leur égalité restait
/// verte : c'est la limite que le module de force brute annonce, et c'est
/// pourquoi ce cas est un test du noyau et non une comparaison d'oracle.
#[test]
fn un_pas_qui_s_eloigne_d_une_face_vue_de_derriere_ne_rencontre_rien() {
    let world = u_room();
    let half = cube_half();
    let from = Vec3d::new(1.0, 5.0, 4.0);
    let to = Vec3d::new(0.75, 5.0, 4.0);

    let fast = sweep(&world, 7, half, from, to).expect("cellule connue");
    assert_eq!(fast.fraction, 1.0, "le pas est libre");
    assert_eq!(fast.surface, 0, "aucune surface n'est touchée");
    assert!(!fast.start_solid);

    let slow = sweep_brute(&world, half, from, to);
    assert_eq!(slow.fraction, fast.fraction);
    assert_eq!(slow.surface, fast.surface);
}

/// **Un départ derrière la face d'une autre cellule ne rencontre rien**, et
/// aucune cellule n'a besoin d'être non convexe pour cela.
///
/// C'est la forme sous laquelle le défaut s'est présenté chez un intégrateur, et
/// elle se distingue du cas de la cellule en U sur les deux points qui comptent :
/// la face fautive appartient à une **autre** cellule, donc deux salles convexes
/// suffisent ; et la traversée ne l'atteignant par aucun portail, les deux chemins
/// **divergeaient** — un pas libre contre une fraction nulle — au lieu d'être faux
/// ensemble.
#[test]
fn un_depart_derriere_la_face_d_une_autre_cellule_ne_rencontre_rien() {
    let world = two_rooms();
    let half = cube_half();
    let from = Vec3d::new(4.0, 4.0, 4.0);
    let to = Vec3d::new(4.0, 3.0, 4.0);

    let fast = sweep(&world, 7, half, from, to).expect("cellule connue");
    assert_eq!(fast.fraction, 1.0, "le pas est libre");
    assert_eq!(fast.surface, 0, "aucune surface n'est touchée");

    let slow = sweep_brute(&world, half, from, to);
    assert_eq!(slow.fraction, fast.fraction, "la salle au loin est muette");
    assert_eq!(slow.surface, fast.surface);
}

/// **Une boîte lancée contre une face vue de derrière ne l'accroche pas non
/// plus**, et c'est l'autre moitié du même cas.
///
/// Le pas précédent s'éloigne du plan ; celui-ci part de derrière et traverse la
/// branche, donc il atteint pour de bon la face opposée. Ce qu'il vérifie est
/// que la borne de la dalle n'a pas changé le contact légitime en absence de
/// contact : la boîte s'arrête, et elle s'arrête sur la face qu'elle rencontre.
#[test]
fn une_boite_qui_traverse_un_u_s_arrete_sur_la_face_qu_elle_rencontre() {
    let world = u_room();
    let half = cube_half();
    let from = Vec3d::new(1.0, 5.0, 4.0);
    let to = Vec3d::new(1.0, 12.0, 4.0);

    let hit = sweep(&world, 7, half, from, to).expect("cellule connue");
    assert!(hit.fraction < 1.0, "le fond de la branche arrête la boîte");
    assert_eq!(hit.surface, 19, "c'est le mur `y = 8` de la branche gauche");
    let centre = from + (to - from) * f64::from(hit.fraction);
    assert!(
        centre.y + half.y <= 8.0,
        "la boîte reste en deçà du mur : {centre:?}"
    );

    let slow = sweep_brute(&world, half, from, to);
    assert_eq!(slow.fraction, hit.fraction);
    assert_eq!(slow.surface, hit.surface);
}

/// Une salle dont le sol est une **rampe à 45°**, montant le long de `Y`.
///
/// **45° est la seule pente que le chargement accepte**, et c'est ce qui rend ce
/// décor possible : l'axe de pente `(0, 1, 1)` a pour carré 2, une puissance de
/// deux, donc le contrôle du repère de lightmap le laisse passer. Une pente 1:2 ou
/// 1:3 ferait refuser le fichier entier, ce qui est une autre affaire que celle-ci.
///
/// Le sol va de `z = 0` en `y = 0` à `z = 8` en `y = 8` ; le plafond le suit huit
/// unités plus haut, si bien que la salle garde partout la même hauteur.
fn ramp() -> World {
    let points: [[f32; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [8.0, 0.0, 0.0],
        [8.0, 8.0, 8.0],
        [0.0, 8.0, 8.0],
        [0.0, 0.0, 8.0],
        [8.0, 0.0, 8.0],
        [8.0, 8.0, 16.0],
        [0.0, 8.0, 16.0],
    ];
    let slope = [0.0, 1.0, 1.0];
    let faces = alloc::vec![
        // La rampe, et le plafond qui la suit : leur axe de pente est `(0, 1, 1)`.
        surface_in_plane(11, 0, &[0, 3, 2, 1], [1.0, 0.0, 0.0], slope),
        surface_in_plane(12, 0, &[4, 5, 6, 7], [1.0, 0.0, 0.0], slope),
        // Les deux bouts, perpendiculaires à `Y`, et les deux flancs.
        surface_in_plane(13, 0, &[0, 1, 5, 4], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        surface_in_plane(14, 0, &[3, 7, 6, 2], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        surface_in_plane(15, 0, &[0, 4, 7, 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        surface_in_plane(16, 0, &[1, 2, 6, 5], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let cell = cell_bytes(7, 0, &points, &faces, &[]);
    World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide")
}

/// **Une boîte qui tombe sur une rampe est arrêtée, où qu'elle tombe dessus.**
///
/// C'est un **prédicat** et non une comparaison de chemins, et il ne pouvait pas
/// être autre chose : la formule fautive vivait dans `shape::face`, que le
/// balayage par portails et celui de force brute empruntent tous les deux. Les
/// deux rendaient donc la même absence de contact, et leur égalité restait verte —
/// le cas que le module de force brute nomme en tête.
///
/// Ce qu'il attrape : le test d'appartenance au polygone se faisait au **centre de
/// la boîte** au lieu du point de la facette, si bien que le domaine de la face se
/// décalait d'une demi-extension dès que la normale n'était pas axiale. Mesuré
/// avant correction, la chute en `y = 7` traversait la rampe sans rien toucher,
/// quand celles de `y = 1` à `y = 6` s'arrêtaient à `0.2498`.
#[test]
fn une_boite_qui_tombe_sur_une_rampe_est_arretee_partout() {
    let world = ramp();
    let half = cube_half();

    for pas in 1..8u32 {
        let y = f64::from(pas);
        // Le sol est à `z = y` : deux unités au-dessus, quatre de descente, donc
        // la chute croise le plan quelle que soit l'abscisse.
        let from = Vec3d::new(4.0, y, y + 2.0);
        let to = Vec3d::new(4.0, y, y - 2.0);
        let hit = sweep(&world, 7, half, from, to).expect("cellule connue");

        assert!(
            hit.fraction < 1.0,
            "y={y} : la rampe n'a pas arrêté la chute"
        );
        assert_eq!(hit.surface, 11, "y={y} : c'est la rampe qui arrête");
        assert!(!hit.start_solid, "y={y} : le départ était dégagé");

        // La propriété, pas la valeur : le bas de la boîte reste au-dessus de la
        // rampe, dont la cote est `y` à cette abscisse.
        let centre = from + (to - from) * f64::from(hit.fraction);
        assert!(
            centre.z - half.z >= y,
            "y={y} : la boîte est entrée dans la rampe, bas à {}",
            centre.z - half.z
        );
    }
}

/// **Le même prédicat par le chemin de force brute**, qui doit s'accorder.
///
/// Il n'attrape pas le défaut — les deux chemins partagent la formule —, mais il
/// garde l'égalité vraie sur une géométrie oblique, qu'aucun autre test n'avait.
#[test]
fn la_rampe_rend_les_memes_bits_par_les_deux_chemins() {
    let world = ramp();
    let half = cube_half();

    for pas in 1..8u32 {
        let y = f64::from(pas);
        let from = Vec3d::new(4.0, y, y + 2.0);
        let to = Vec3d::new(4.0, y, y - 2.0);
        let fast = sweep(&world, 7, half, from, to).expect("cellule connue");
        let slow = sweep_brute(&world, half, from, to);

        assert_eq!(slow.fraction, fast.fraction, "y={y}");
        assert_eq!(slow.surface, fast.surface, "y={y}");
        assert_eq!(slow.normal, fast.normal, "y={y}");
    }
}

/// **Une boîte posée au sol franchit la jointure de deux cellules.**
///
/// Le cas que l'intégrateur a rencontré à chaque pas de son décor, et qui ne se
/// voit que dans la **bande de peau** : la vraie boîte n'y touche rien, la boîte
/// dilatée y touche déjà, et c'est l'état qu'un balayage précédent laisse derrière
/// lui par construction. Posée plus haut, la boîte passait ; posée dessus, elle
/// partait légitimement dans le solide. Entre les deux, elle s'arrêtait net.
///
/// Ce qu'il attrape : le sol de chaque cellule voyait l'arête du seuil comme
/// exposée, faute de regarder la cellule d'en face, et le prisme de cette arête
/// barrait le passage — sur un sol horizontal, qui ne peut pas arrêter un
/// mouvement horizontal. Mesuré avant correction à `fraction = 0.374878` contre
/// la surface du sol de départ, soit la course du bord avant jusqu'au plan du
/// portail.
#[test]
fn une_boite_posee_au_sol_franchit_la_jointure() {
    let world = chain(3);
    let half = Vec3d::new(0.5, 0.5, 0.5);

    for part in [0.25f64, 0.5, 0.9] {
        let z = half.z + SKIN * half.z * part;
        let from = Vec3d::new(2.0, 2.0, z);
        let to = Vec3d::new(6.0, 2.0, z);
        let hit = sweep(&world, 1, half, from, to).expect("cellule connue");

        assert_eq!(hit.fraction, 1.0, "part={part} : le seuil a bloqué le pas");
        assert_eq!(hit.surface, 0, "part={part} : aucune surface n'est touchée");
        assert!(!hit.start_solid, "part={part} : le départ était dégagé");
    }
}

/// **Et le mur du bout arrête toujours**, dans la même bande.
///
/// Le contrôle négatif du précédent : éteindre une arête de trop ferait passer la
/// boîte au travers du bout de l'enfilade, dont le portail n'est pas apparié et
/// qui est donc un mur. Sans lui, un correctif trop large passerait au vert.
#[test]
fn le_bout_de_l_enfilade_arrete_une_boite_posee_au_sol() {
    let world = chain(3);
    let half = Vec3d::new(0.5, 0.5, 0.5);
    let z = half.z + SKIN * half.z * 0.5;
    let from = Vec3d::new(2.0, 2.0, z);
    let to = Vec3d::new(20.0, 2.0, z);

    let hit = sweep(&world, 1, half, from, to).expect("cellule connue");
    assert!(hit.fraction < 1.0, "le mur du bout arrête la boîte");
    let centre = from + (to - from) * f64::from(hit.fraction);
    assert!(
        centre.x + half.x <= 12.0,
        "la boîte reste dans l'enfilade : {centre:?}"
    );
}

/// **Longer un mur ne doit pas buter sur son arête terminale.**
///
/// Remonté d'un intégrateur : un mobile ne peut pas longer une paroi. À la
/// distance de contact que le balayage rend lui-même, le pas suivant, parallèle à
/// la face, est arrêté net par l'arête qui termine le panneau — avec une normale
/// perpendiculaire à celle du mur, et la même surface nommée que pour le contact
/// frontal.
///
/// **Le cas tient dans une seule cellule**, et c'est ce qui dit où la cause n'est
/// pas : le classement des arêtes au travers d'un portail, qui existe depuis la
/// jointure des sols, n'a rien à voir ici. L'arête du coin saillant `(6, 2)` est
/// exposée à juste titre — deux murs perpendiculaires s'y rejoignent — et son
/// prisme est légitime. Ce qui ne l'est pas, c'est qu'une boîte **tangente** à ce
/// prisme soit tenue pour dedans.
///
/// La position de départ vient du balayage lui-même, et non d'un nombre écrit
/// ici : c'est la pose exacte que l'hôte applique à l'image précédente, donc le
/// seul départ qui reproduise ce qu'il observe.
///
/// **Les deux chemins sont faux ensemble** — une seule cellule, le brut examine
/// les mêmes surfaces —, donc l'égalité d'oracle reste verte et ne garde rien :
/// c'est un test du noyau, comme le pas qui s'éloigne d'une face vue de derrière.
#[test]
fn longer_un_mur_ne_bute_pas_sur_son_arete_terminale() {
    let world = u_room();
    let half = Vec3d::new(0.3, 0.3, 0.9);

    // Le mur `y = 2` du bloc central, abordé de face : la boîte s'arrête à la
    // distance de contact, qui est la frontière de la boîte dilatée.
    let approach_from = Vec3d::new(4.0, 0.5, 4.0);
    let approach_to = Vec3d::new(4.0, 3.0, 4.0);
    let contact = sweep(&world, 7, half, approach_from, approach_to).expect("cellule connue");
    assert!(contact.fraction < 1.0, "le mur arrête l'approche");
    assert_eq!(contact.surface, 17, "c'est le mur du bloc central");

    let posed = approach_from + (approach_to - approach_from) * f64::from(contact.fraction);

    // Le pas suivant longe ce mur et dépasse le coin `(6, 2)`. Au-delà, la
    // branche droite du U est ouverte jusqu'à `y = 8` : rien n'arrête.
    let from = posed;
    let to = Vec3d::new(7.0, posed.y, posed.z);
    let hit = sweep(&world, 7, half, from, to).expect("cellule connue");

    assert_eq!(
        hit.fraction, 1.0,
        "le pas qui longe est libre, mais il est arrêté par {} à {} avec la normale {:?}",
        hit.surface, hit.fraction, hit.normal
    );
}

/// **Le comportement à la limite, éprouvé par une géométrie faite pour
/// l'atteindre** — et c'est ici qu'il doit vivre, parce qu'aucun décor réel ne s'en
/// approche : les scènes de conformance tiennent sept fois au-dessus du seuil, et
/// l'intégrateur quatre-vingts fois. La borne se franchit donc par la **petitesse**
/// plutôt que par l'éloignement, les deux étant le même quotient.
///
/// Les deux chemins le portent, et l'oracle le dit : une propriété posée sur un
/// seul d'entre eux les ferait diverger partout où elle s'applique.
#[test]
fn une_boite_minuscule_n_a_plus_de_jeu() {
    let world = room(0);
    let from = Vec3d::new(4.0, 4.0, 4.0);
    let to = Vec3d::new(6.0, 4.0, 4.0);

    // Six unités de l'origine laissent un seuil de 7,3 × 10⁻⁴ : la boîte est en
    // deçà, donc son jeu est noyé dans le pas du `f32` de sa propre position.
    let tiny = Vec3d::new(0.0001, 0.0001, 0.0001);
    let hit = sweep(&world, 7, tiny, from, to).expect("cellule connue");
    assert!(hit.no_gap, "une boîte de 10⁻⁴ à six unités n'a plus de jeu");
    assert!(
        sweep_brute(&world, tiny, from, to).no_gap,
        "et le chemin brut le dit aussi, sans quoi l'oracle divergerait"
    );

    // La même course avec une boîte ordinaire : le jeu tient largement.
    let hit = sweep(&world, 7, cube_half(), from, to).expect("cellule connue");
    assert!(!hit.no_gap, "une boîte d'une demi-unité garde son jeu");
}

/// Un rayon ne lève jamais le drapeau, et ce n'est pas un seuil qu'il passerait.
///
/// Sa dilatation est nulle par construction : il n'a aucun jeu, donc aucun à
/// perdre. Le signaler ferait du cas normal une anomalie permanente, sur chaque
/// interrogation d'éditeur.
#[test]
fn un_rayon_n_a_pas_de_jeu_a_perdre() {
    let world = room(0);
    let from = Vec3d::new(4.0, 4.0, 7.0);
    let to = Vec3d::new(4.0, 4.0, -1.0);

    let hit =
        super::sweep(&world, 7, Vec3d::ZERO, from, to, Surfaces::All).expect("cellule connue");
    assert!(hit.surface != 0, "le rayon touche bien le sol");
    assert!(!hit.no_gap, "et il ne signale aucun jeu perdu");
}

/// **La portée annoncée et le drapeau sont la même frontière**, et c'est le seul
/// test qui le garde.
///
/// Les deux expressions sont inverses l'une de l'autre et vivent à dix lignes
/// d'écart ; rien d'autre n'empêcherait qu'elles cessent de se répondre, et l'écart
/// serait pour l'hôte le pire possible — la fonction lui dirait sûr ce que le
/// drapeau lui dit perdu. Exact au bit près : les deux facteurs sont des puissances
/// de deux, donc la coordonnée rendue est précisément celle où la bascule a lieu.
#[test]
fn la_portee_annoncee_borne_le_drapeau() {
    for extent in [0.05f32, 0.3, 0.9, 1.0, 7.5] {
        let reach = sweep_reach(Vec3::new(extent, extent, extent));
        let half = Vec3d::new(f64::from(extent), f64::from(extent), f64::from(extent));

        let at = Vec3d::new(f64::from(reach), 0.0, 0.0);
        assert!(
            no_gap(half, at, at),
            "{extent} : à {reach}, le jeu est déjà perdu — c'est le seuil, pas la dernière valeur sûre"
        );

        let inside = Vec3d::new(f64::from(reach) * 0.5, 0.0, 0.0);
        assert!(
            !no_gap(half, inside, inside),
            "{extent} : à la moitié de {reach}, le jeu tient"
        );
    }
}

/// Un rayon n'a pas de portée, puisqu'il n'a pas de jeu : zéro, comme sa marge.
#[test]
fn un_rayon_n_a_pas_de_portee() {
    assert_eq!(sweep_reach(Vec3::ZERO), 0.0);
}
