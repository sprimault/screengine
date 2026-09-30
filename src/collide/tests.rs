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
