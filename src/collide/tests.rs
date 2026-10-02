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
