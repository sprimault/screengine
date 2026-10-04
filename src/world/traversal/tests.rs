// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La traversée se teste sur des cartes écrites en octets, par les mêmes
//! constructeurs que le décodeur : ce sont eux qui portent les décalages du
//! format, et une seconde copie divergerait.

use alloc::vec::Vec;

use super::*;
use crate::format::world::tests::{cell_bytes, file, material, portal_bytes, surface_bytes};
use crate::math::{Quat, Vec3};
use crate::scene::Camera;

/// La largeur de l'image des tests.
const WIDTH: u32 = 640;
/// Sa hauteur.
const HEIGHT: u32 = 360;

/// Le cadrage des tests.
fn projection() -> Projection {
    Projection::new(WIDTH, HEIGHT, core::f32::consts::FRAC_PI_2, 0.1).expect("cadrage valide")
}

/// La vue d'une caméra posée en `x`, regardant le `+X` du monde.
fn at(x: f32) -> Affine3 {
    Camera {
        position: Vec3::new(x, 0.0, 0.0),
        orientation: Quat::IDENTITY,
        fov_y: core::f32::consts::FRAC_PI_2,
        near: 0.1,
    }
    .view()
}

/// La fenêtre de départ : l'image entière.
fn full() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: WIDTH,
        height: HEIGHT,
    }
}

/// Le sol d'un tronçon, en indices de ses huit sommets.
///
/// La surface est le sol et non un bout : son plan est `z` constant, et le repère
/// de lightmap par défaut y porte ses deux axes. Un bout de tronçon vit dans un
/// plan `x` constant, où l'axe `X` du repère serait la normale — ce que le
/// chargement refuse, à juste titre.
const FLOOR: [u32; 4] = [0, 1, 5, 4];

/// Les huit sommets d'un tronçon de couloir, de `from` à `to`.
///
/// Les quatre premiers ferment le bout proche, les quatre suivants le bout
/// lointain : c'est par eux que les portails se découpent.
fn section(from: f32, to: f32) -> [[f32; 3]; 8] {
    let mut points = [[0.0f32; 3]; 8];
    for (i, x) in [from, to].iter().enumerate() {
        for (j, (y, z)) in [(-2.0, -2.0), (2.0, -2.0), (2.0, 2.0), (-2.0, 2.0)]
            .iter()
            .enumerate()
        {
            points[i * 4 + j] = [*x, *y, *z];
        }
    }
    points
}

/// Les huit sommets d'une case de grille, en `[x0, x0+8] × [y0, y0+8]`.
///
/// Même disposition que [`section`] — bout proche puis bout lointain, et dans
/// chacun l'ordre `(y, z)` qui met le sol sur [`FLOOR`] —, à ceci près que les
/// deux côtés en `y` sont des paramètres : c'est ce qui permet à une case d'avoir
/// quatre voisines au lieu de deux.
fn tile_points(x0: f32, y0: f32) -> [[f32; 3]; 8] {
    let (x1, y1) = (x0 + 8.0, y0 + 8.0);
    let mut points = [[0.0f32; 3]; 8];
    for (i, x) in [x0, x1].iter().enumerate() {
        for (j, (y, z)) in [(y0, -2.0), (y1, -2.0), (y1, 2.0), (y0, 2.0)]
            .iter()
            .enumerate()
        {
            points[i * 4 + j] = [*x, *y, *z];
        }
    }
    points
}

/// Deux tronçons séparés par un mur percé de `openings` ouvertures.
///
/// Chaque ouverture est un portail à elle seule, donc chacune porte la cellule
/// d'en face **par une fenêtre différente** : c'est ce que la documentation de
/// [`MAX_VISITS`] décrit, et le seul mécanisme qui multiplie les visites sans
/// multiplier les cellules — une grille, elle, n'en donne qu'une par cellule, ses
/// chemins de biais se vidant.
///
/// Les ouvertures sont alignées en `y`, chacune d'un seizième d'unité, et les deux
/// cellules décrivent chaque plan avec les mêmes coordonnées.
fn pierced(openings: u32) -> Vec<u8> {
    let span = 4.0 / openings as f32;
    let mut points = Vec::new();
    // Les huit sommets du pavé, comme ailleurs, puis quatre par ouverture.
    for point in tile_points(0.0, -2.0) {
        points.push(point);
    }
    for index in 0..openings {
        let y0 = -2.0 + index as f32 * span;
        let y1 = y0 + span * 0.5;
        for (y, z) in [(y0, -1.0), (y1, -1.0), (y1, 1.0), (y0, 1.0)] {
            points.push([8.0, y, z]);
        }
    }

    // Chaque portail porte son propre identifiant, les deux côtés d'une ouverture
    // compris : le format refuse un identifiant dupliqué, et c'est l'appariement
    // par sommets partagés qui les joint, jamais leur numéro.
    let near_portals: Vec<_> = (0..openings)
        .map(|index| {
            let base = 8 + index * 4;
            portal_bytes(100 + index, &[base, base + 1, base + 2, base + 3])
        })
        .collect();
    let far_portals: Vec<_> = (0..openings)
        .map(|index| {
            let base = 8 + index * 4;
            portal_bytes(
                100 + openings + index,
                &[base, base + 1, base + 2, base + 3],
            )
        })
        .collect();

    let mut far = Vec::new();
    for point in tile_points(8.0, -2.0) {
        far.push(point);
    }
    for index in 0..openings {
        let y0 = -2.0 + index as f32 * span;
        let y1 = y0 + span * 0.5;
        for (y, z) in [(y0, -1.0), (y1, -1.0), (y1, 1.0), (y0, 1.0)] {
            far.push([8.0, y, z]);
        }
    }

    let mut cells = cell_bytes(
        7,
        0,
        &points,
        &[surface_bytes(11, 0, 1, &FLOOR)],
        &near_portals,
    );
    cells.extend_from_slice(&cell_bytes(
        8,
        0,
        &far,
        &[surface_bytes(12, 0, 1, &FLOOR)],
        &far_portals,
    ));
    file(&cells, &[], &[], &material(1, "mur"))
}

/// Une carte d'un seul tronçon, sans portail.
fn one_cell() -> Vec<u8> {
    let cell = cell_bytes(
        7,
        0,
        &section(0.0, 8.0),
        &[surface_bytes(11, 0, 1, &FLOOR)],
        &[],
    );
    file(&cell, &[], &[], &material(1, "mur"))
}

/// Une carte de deux tronçons joints par un portail à `x = 8`.
///
/// Les deux cellules décrivent ce plan avec les mêmes octets, ce que
/// l'appariement exige : c'est l'éditeur qui en répond, et c'est une clause du
/// format.
fn two_cells() -> Vec<u8> {
    let first = cell_bytes(
        7,
        0,
        &section(0.0, 8.0),
        &[surface_bytes(11, 0, 1, &FLOOR)],
        &[portal_bytes(21, &[4, 5, 6, 7])],
    );
    let second = cell_bytes(
        8,
        0,
        &section(8.0, 16.0),
        &[surface_bytes(12, 0, 1, &FLOOR)],
        &[portal_bytes(22, &[0, 1, 2, 3])],
    );
    let mut cells = first;
    cells.extend_from_slice(&second);
    file(&cells, &[], &[], &material(1, "mur"))
}

/// Trois tronçons en anneau : chacun est joint aux deux autres.
///
/// C'est le cas que la borne de profondeur existe pour couvrir, et celui que le
/// scan du chemin courant doit couper sans rien perdre.
fn ring() -> Vec<u8> {
    let mut cells = Vec::new();
    for (id, from) in [(7u32, 0.0f32), (8, 8.0), (9, 16.0)] {
        let points = section(from, from + 8.0);
        cells.extend_from_slice(&cell_bytes(
            id,
            0,
            &points,
            &[surface_bytes(id * 10, 0, 1, &FLOOR)],
            &[
                portal_bytes(id * 10 + 1, &[4, 5, 6, 7]),
                portal_bytes(id * 10 + 2, &[0, 1, 2, 3]),
            ],
        ));
    }
    file(&cells, &[], &[], &material(1, "mur"))
}

/// Une liste de visites réservée **comme le contexte la réserve**.
///
/// Par [`crate::buffer::reserved`] et non par `Vec::with_capacity`, pour que ce
/// qu'éprouvent ces cas soit la condition du contexte et non une autre. Mesuré :
/// les deux rendent exactement [`MAX_VISITS`] sur ce poste, si bien qu'une borne
/// lue sur la capacité y passerait inaperçue — c'est pourquoi le cas qui la garde
/// réserve, lui, volontairement plus large.
fn visits() -> Vec<Visit> {
    crate::buffer::reserved(MAX_VISITS).expect("la réservation des visites tient")
}

/// Une enfilade droite de `count` tronçons, chacun joint au suivant.
///
/// **Des tronçons courts et larges**, et c'est ce qui rend le cas atteignable :
/// un couloir de huit unités par tronçon place le soixante-cinquième à cinq
/// cents unités, où son portail se projette sur moins d'un pixel et où la
/// fenêtre se ferme bien avant la borne de profondeur. À un demi-unité, les
/// soixante-cinq tiennent dans trente-cinq unités, et un portail large de
/// quatre y couvre encore assez de pixels pour que la fenêtre reste ouverte.
///
/// Pas plus court non plus : le premier portail doit rester devant le plan
/// proche de la caméra, faute de quoi la traversée s'arrête au premier
/// tronçon pour une raison qui n'a rien à voir avec la borne.
pub(crate) fn corridor(count: u32) -> Vec<u8> {
    let mut cells = Vec::new();
    for index in 0..count {
        let id = index + 7;
        let points = section(index as f32 * 0.5, (index + 1) as f32 * 0.5);
        // Les extrémités n'ont qu'un portail : un portail qui n'apparie rien
        // est une incohérence que le chargement refuse.
        let mut portals = Vec::new();
        if index + 1 < count {
            portals.push(portal_bytes(id * 10 + 1, &[4, 5, 6, 7]));
        }
        if index > 0 {
            portals.push(portal_bytes(id * 10 + 2, &[0, 1, 2, 3]));
        }
        cells.extend_from_slice(&cell_bytes(
            id,
            0,
            &points,
            &[surface_bytes(id * 10, 0, 1, &FLOOR)],
            &portals,
        ));
    }
    file(&cells, &[], &[], &material(1, "mur"))
}

/// Une cellule seule se visite une fois, par la fenêtre reçue.
#[test]
fn une_cellule_seule_se_visite_entiere() {
    let world = World::load(&one_cell()).expect("carte valide");
    let mut out = visits();
    let truncated = traverse(&world, 0, full(), at(2.0), &projection(), &mut out);

    assert!(!truncated);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].cell, 0);
    assert_eq!(out[0].window, full());
}

/// Un portail mène à la cellule voisine, par une fenêtre plus petite.
#[test]
fn un_portail_mene_a_la_cellule_voisine() {
    let world = World::load(&two_cells()).expect("carte valide");
    let mut out = visits();
    let truncated = traverse(&world, 0, full(), at(2.0), &projection(), &mut out);

    assert!(!truncated);
    assert_eq!(out.len(), 2, "les deux cellules sont visibles");
    assert_eq!(out[0].cell, 0);
    assert_eq!(out[1].cell, 1);
    assert_eq!(out[0].window, full(), "la cellule de la caméra garde tout");
    assert!(
        out[1].window.width < WIDTH,
        "la voisine se voit par le portail, donc plus étroite"
    );
}

/// **Un portail que le plan proche atteint mène quand même à sa voisine.**
///
/// Remonté d'un intégrateur : en marchant dans un labyrinthe, une bande verticale
/// du champ de vision se vide de tout décor — les sprites y restent dessinés, donc
/// c'est la traversée qui perd une cellule, pas le remplissage. La condition est
/// l'approche du **plan d'un portail**, jamais le milieu d'une case, et la
/// traversée annonce pourtant avoir tout déplié.
///
/// Ce que sa mesure ajoute et qui désigne le plan proche : le phénomène suit
/// `near`. À 0,1 il survient vingt-neuf fois par parcours, à 0,01 neuf fois, à
/// 0,001 jamais — et descendre plus bas n'est pas une sortie, la profondeur se
/// rangeant en `near/w` et un mur approché se mettant à vibrer. Le réglage est
/// donc borné des deux côtés, ce qui en fait un défaut du moteur.
///
/// Le cas place l'œil à la distance où le plan du portail tombe **sur** le plan
/// proche. La voisine doit rester visible : on la voit alors par presque tout
/// l'écran, et c'est le moment où la perdre se voit le plus.
#[test]
fn un_portail_au_plan_proche_mene_quand_meme_a_la_voisine() {
    let world = World::load(&two_cells()).expect("carte valide");

    // Le portail est en `x = 8`, le plan proche à `0.1` de l'œil : la tangence
    // est donc en `7.9`. Les deux voisines encadrent le cas, qui ne tient qu'à
    // quelques millièmes d'unité.
    for x in [7.85f32, 7.9, 7.95] {
        let mut out = visits();
        let truncated = traverse(&world, 0, full(), at(x), &projection(), &mut out);

        assert_eq!(
            out.len(),
            2,
            "x={x} : la voisine manque, et la traversée dit {}",
            if truncated { "tronqué" } else { "complet" }
        );
    }
}

/// Les visites sortent dans l'ordre des index de cellules.
///
/// C'est l'ordre du fichier, celui qui départage deux surfaces coplanaires : en
/// ordre de traversée il dépendrait de la position de la caméra, et deux images
/// d'une même scène se contrediraient.
#[test]
fn les_visites_sortent_dans_l_ordre_du_fichier() {
    let world = World::load(&ring()).expect("carte valide");
    let mut out = visits();
    traverse(&world, 1, full(), at(10.0), &projection(), &mut out);

    let cells: Vec<u32> = out.iter().map(|visit| visit.cell).collect();
    let mut sorted = cells.clone();
    sorted.sort_unstable();
    assert_eq!(cells, sorted, "les visites ne sont pas triées");
    assert!(
        cells.windows(2).all(|w| w[0] != w[1]),
        "une cellule en double"
    );
}

/// Un anneau de cellules termine, et ne visite chaque cellule qu'une fois.
///
/// Sans le scan du chemin courant, la traversée repasserait indéfiniment par les
/// mêmes cellules jusqu'à la borne de profondeur — et la borne servirait à
/// masquer un défaut au lieu de couvrir un décor.
#[test]
fn un_anneau_de_cellules_termine() {
    let world = World::load(&ring()).expect("carte valide");
    let mut out = visits();
    let truncated = traverse(&world, 0, full(), at(2.0), &projection(), &mut out);

    assert!(!truncated, "l'anneau ne doit pas tronquer");
    assert!(out.len() <= 3, "au plus trois cellules, pas de répétition");
}

/// Une cellule de départ hors borne ne visite rien.
#[test]
fn une_cellule_de_depart_inconnue_ne_visite_rien() {
    let world = World::load(&one_cell()).expect("carte valide");
    let mut out = visits();
    assert!(!traverse(
        &world,
        99,
        full(),
        at(2.0),
        &projection(),
        &mut out
    ));
    assert!(out.is_empty());
}

/// Une fenêtre vide au départ ne visite rien.
///
/// Pas un cas d'appel tordu : une image dont la fenêtre est vide est celle
/// qu'un hôte rend quand sa région ne couvre rien, et la traversée doit y
/// rendre une liste vide plutôt que la cellule de départ — que le remplissage
/// dessinerait alors hors de toute fenêtre.
#[test]
fn une_fenetre_vide_au_depart_ne_visite_rien() {
    let world = World::load(&two_cells()).expect("carte valide");
    let mut out = visits();
    let empty = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: HEIGHT,
    };
    assert!(!traverse(
        &world,
        0,
        empty,
        at(2.0),
        &projection(),
        &mut out
    ));
    assert!(out.is_empty());
}

/// La traversée tronque à [`MAX_VISITS`], et pas une visite plus tôt.
///
/// **Le décor atteint la borne pour de vrai**, là où ce cas l'éprouvait jusqu'ici
/// sur une liste de capacité un : la borne se lisant alors sur la capacité, le test
/// mesurait un artefact, et c'est ce qui a laissé la troncature dépendre de
/// l'allocateur. Le compte des visites vaut une pour la cellule de départ plus une
/// par ouverture — chaque ouverture porte la cellule d'en face par une fenêtre à
/// elle —, donc un mur assez percé y mène sans carte de quatre mille cellules.
/// Une grille n'y menait pas : ses chemins de biais se vident, et elle ne donne
/// qu'une visite par cellule.
///
/// **Les deux côtés de la borne se mesurent, et c'est ce qui la rend opposable** :
/// à la borne exacte la traversée est complète, une visite au-delà elle tronque.
/// Un cas qui ne verrait que la troncature passerait aussi sur une borne trop
/// basse.
#[test]
fn la_traversee_tronque_a_sa_borne_et_pas_avant() {
    let exact = u32::try_from(MAX_VISITS).expect("la borne tient dans un u32") - 1;

    let world = World::load(&pierced(exact)).expect("carte valide");
    let mut out = visits();
    assert!(
        !traverse(&world, 0, full(), at(2.0), &projection(), &mut out),
        "{MAX_VISITS} visites entrent sans troncature, la borne étant atteinte et non franchie"
    );

    let world = World::load(&pierced(exact + 1)).expect("carte valide");
    let mut out = visits();
    assert!(
        traverse(&world, 0, full(), at(2.0), &projection(), &mut out),
        "la visite au-delà de la borne tronque l'image"
    );
}

/// Deux fenêtres pour une même cellule se fusionnent en leur enveloppe.
///
/// L'enveloppe sur-dessine, et c'est admissible pour une fenêtre de bornage et
/// nulle part ailleurs : trop large rend la même image, trop étroite troue.
#[test]
fn deux_fenetres_d_une_cellule_fusionnent() {
    let mut out = alloc::vec![
        Visit {
            cell: 3,
            first_triangle: 0,
            window: Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
        },
        Visit {
            cell: 3,
            first_triangle: 0,
            window: Rect {
                x: 20,
                y: 5,
                width: 10,
                height: 10,
            },
        },
    ];
    merge(&mut out);

    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].window,
        Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 15,
        }
    );
}

/// L'enveloppe d'un rectangle vide est l'autre.
///
/// Sans cette clause, une fenêtre vide tirerait l'enveloppe vers l'origine et
/// couvrirait tout le coin haut gauche de l'image.
#[test]
fn l_enveloppe_ignore_un_rectangle_vide() {
    let some = Rect {
        x: 5,
        y: 5,
        width: 10,
        height: 10,
    };
    assert_eq!(cover(Rect::EMPTY, some), some);
    assert_eq!(cover(some, Rect::EMPTY), some);
}

/// La troncature tombe à [`MAX_VISITS`], quelle que soit la capacité réservée.
///
/// **La liste est réservée par `try_reserve_exact`, qui ne promet pas la capacité
/// demandée mais au moins elle** : l'allocateur a le droit d'en rendre davantage.
/// Une borne lue sur `capacity()` ferait donc dépendre la troncature — donc le
/// drapeau d'image incomplète, donc l'image — de l'allocateur de la cible, et deux
/// plateformes rendraient deux images de la même scène.
///
/// La capacité est ici volontairement plus large que la borne, ce qu'aucun décor
/// du dépôt ne produit : c'est la seule façon d'éprouver la propriété sans
/// dépendre de ce que l'allocateur a bien voulu donner le jour du test.
#[test]
fn la_borne_de_visites_ne_suit_pas_la_capacite() {
    let window = Rect {
        x: 0,
        y: 0,
        width: WIDTH,
        height: HEIGHT,
    };
    let mut visits = Vec::with_capacity(MAX_VISITS + 8);

    for rank in 0..MAX_VISITS {
        assert!(
            push_visit(&mut visits, 0, window),
            "la visite {rank} entre, la liste n'étant pas pleine"
        );
    }

    assert!(
        !push_visit(&mut visits, 0, window),
        "la borne est MAX_VISITS, jamais la capacité — la liste en porte {} pour une capacité de {}",
        visits.len(),
        visits.capacity()
    );
    assert_eq!(visits.len(), MAX_VISITS);
}

/// Une carte dont les portails ne s'apparient pas ne sort jamais de sa cellule.
#[test]
fn un_portail_non_apparie_est_un_mur() {
    let cell = cell_bytes(
        7,
        0,
        &section(0.0, 8.0),
        &[surface_bytes(11, 0, 1, &FLOOR)],
        &[portal_bytes(21, &[4, 5, 6, 7])],
    );
    let world = World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide");
    let mut out = visits();
    traverse(&world, 0, full(), at(2.0), &projection(), &mut out);
    assert_eq!(out.len(), 1);
}

/// Au-delà de la borne de profondeur, la traversée s'arrête et le dit.
///
/// **La branche n'était atteinte par aucun test** : aucune carte d'épreuve
/// n'enchaînait plus de soixante-quatre cellules, si bien que la seule borne
/// que l'ABI publie à l'hôte — et le statut qu'elle lui rend — n'était vérifiée
/// nulle part.
///
/// Ce que la borne coupe est une cellule **plus loin** qu'elle : la
/// soixante-cinquième est enregistrée, seuls ses portails ne sont pas dépliés.
#[test]
fn la_borne_de_profondeur_tronque_et_le_dit() {
    let world = World::load(&corridor(MAX_DEPTH as u32 + 8)).expect("carte valide");
    let mut out = visits();
    let truncated = traverse(&world, 0, full(), at(0.25), &projection(), &mut out);

    assert!(truncated, "la borne n'a pas été atteinte");
    assert_eq!(
        out.len(),
        MAX_DEPTH + 1,
        "la cellule de la borne est dessinée, ses portails seuls sont coupés"
    );
}

/// En deçà de la borne, le même couloir se traverse entier et sans troncature.
///
/// La garde de la précédente : sans elle, un couloir dont la fenêtre se
/// refermerait au dixième tronçon rendrait `truncated` faux pour une tout
/// autre raison, et le cas passerait pour couvert.
#[test]
fn un_couloir_plus_court_que_la_borne_se_traverse_entier() {
    let count = MAX_DEPTH as u32 - 4;
    let world = World::load(&corridor(count)).expect("carte valide");
    let mut out = visits();
    let truncated = traverse(&world, 0, full(), at(0.25), &projection(), &mut out);

    assert!(
        !truncated,
        "troncature sur un couloir plus court que la borne"
    );
    assert_eq!(
        out.len(),
        count as usize,
        "toutes les cellules sont visibles"
    );
}
