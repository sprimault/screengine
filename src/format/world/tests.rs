// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests du décodeur de carte.
//!
//! Même doctrine que le maillage : les fichiers s'écrivent en octets ici, par
//! des constructeurs qui posent les champs un à un. C'est la seconde écriture
//! des dispositions, et c'est elle qui fait rougir un désaccord avec le
//! décodeur.

use alloc::vec;
use alloc::vec::Vec;

use super::*;
use crate::testing::Rng;

/// Les octets d'une suite de flottants.
pub(crate) fn floats(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Les octets d'une suite d'entiers.
pub(crate) fn words(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Une entrée de matériau : son identifiant, puis son nom.
pub(crate) fn material(id: u32, name: &str) -> Vec<u8> {
    let mut bytes = words(&[id]);
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

/// Un repère : origine, axe u, axe v.
///
/// Les axes valent une unité de longueur par défaut, ce qui est une puissance de
/// deux : le repère de lightmap l'exige, celui de texture s'en accommode.
pub(crate) fn frame(origin: [f32; 3], u: [f32; 3], v: [f32; 3]) -> Vec<u8> {
    floats(&[
        origin[0], origin[1], origin[2], u[0], u[1], u[2], v[0], v[1], v[2],
    ])
}

/// Le repère par défaut : origine nulle, axes unitaires sur X et Y.
pub(crate) fn unit_frame() -> Vec<u8> {
    frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
}

/// Une surface dont le repère de lightmap est donné, celui de texture restant
/// neutre.
///
/// Les deux repères se lisent par le même décodeur, mais un seul est contraint :
/// isoler le repère de lightmap est ce qui permet à chaque contrôle d'avoir son
/// propre cas au lieu d'un cas qui en refuserait plusieurs à la fois.
pub(crate) fn surface_with_lightmap(indices: &[u32], lightmap: &[u8]) -> Vec<u8> {
    let mut bytes = words(&[11, 0, 1, indices.len() as u32]);
    bytes.extend_from_slice(&words(indices));
    bytes.extend_from_slice(&unit_frame());
    bytes.extend_from_slice(lightmap);
    bytes
}

/// La carte d'un carré dont la surface porte le repère de lightmap donné.
pub(crate) fn map_with_lightmap(lightmap: &[u8]) -> Vec<u8> {
    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_with_lightmap(&[0, 1, 2, 3], lightmap)],
        &[],
    );
    file(&cell, &[], &[], &material(1, "mur"))
}

/// Une surface : son en-tête, ses indices, ses deux repères.
pub(crate) fn surface_bytes(id: u32, flags: u32, material: u32, indices: &[u32]) -> Vec<u8> {
    let mut bytes = words(&[id, flags, material, indices.len() as u32]);
    bytes.extend_from_slice(&words(indices));
    bytes.extend_from_slice(&unit_frame());
    bytes.extend_from_slice(&unit_frame());
    bytes
}

/// Un portail : son identifiant et ses indices.
pub(crate) fn portal_bytes(id: u32, indices: &[u32]) -> Vec<u8> {
    let mut bytes = words(&[id, indices.len() as u32]);
    bytes.extend_from_slice(&words(indices));
    bytes
}

/// Une cellule complète, longueur-préfixée.
pub(crate) fn cell_bytes(
    id: u32,
    flags: u32,
    points: &[[f32; 3]],
    surfaces: &[Vec<u8>],
    portals: &[Vec<u8>],
) -> Vec<u8> {
    let mut body = words(&[
        id,
        flags,
        points.len() as u32,
        surfaces.len() as u32,
        portals.len() as u32,
    ]);
    for point in points {
        body.extend_from_slice(&floats(point));
    }
    for surface in surfaces {
        body.extend_from_slice(surface);
    }
    for portal in portals {
        body.extend_from_slice(portal);
    }

    let mut bytes = words(&[body.len() as u32]);
    bytes.extend_from_slice(&body);
    bytes
}

/// Un fichier de carte bien formé, à partir de ses sections.
///
/// Les décalages sont posés ici à la main, comme pour le maillage : vingt octets
/// d'en-tête, douze par entrée de table, les sections par genre croissant.
pub(crate) fn file(cells: &[u8], ents: &[u8], lgts: &[u8], mats: &[u8]) -> Vec<u8> {
    let sections: Vec<([u8; 4], &[u8])> = [
        (*b"CELL", cells),
        (*b"ENTS", ents),
        (*b"LGTS", lgts),
        (*b"MATS", mats),
    ]
    .into_iter()
    .filter(|(_, body)| !body.is_empty())
    .collect();

    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"WRLD");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(sections.len() as u32).to_le_bytes());

    let mut offset = first;
    for (tag, body) in &sections {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        offset += body.len();
    }
    for (_, body) in &sections {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// Les quatre coins d'un carré du plan `z = 0`, en sens antihoraire vu de `+Z`.
pub(crate) const SQUARE: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [4.0, 0.0, 0.0],
    [4.0, 4.0, 0.0],
    [0.0, 4.0, 0.0],
];

/// Une carte d'une cellule, un carré, un matériau : celle que les tests
/// abîment.
pub(crate) fn valid() -> Vec<u8> {
    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[],
    );
    file(&cell, &[], &[], &material(1, "mur"))
}

/// L'erreur attendue, écrite court.
pub(crate) fn refused(malformation: Malformation) -> Error {
    Error::InvalidFormat(malformation)
}

/// Une carte bien formée rend ses cellules, ses triangles et ses matériaux.
#[test]
fn une_carte_bien_formee_rend_son_contenu() {
    let world = World::load(&valid()).expect("carte valide");

    assert_eq!(world.material_count(), 1);
    assert_eq!(world.material_name(0), Some("mur"));
    assert_eq!(world.material_name(1), None);
    assert_eq!(world.triangle_count(), 2, "le carré donne deux triangles");

    let cell = &world.cells()[0];
    assert_eq!(cell.id, 7);
    assert_eq!(cell.vertices.len(), 4);
    assert_eq!(cell.triangles.len(), 2);
    assert_eq!(cell.surfaces[0].triangle_count, 2);
    assert_eq!(cell.surfaces[0].first_triangle, 0);
}

/// Les coordonnées de texture se dérivent du repère, et pas du fichier.
///
/// Le carré fait quatre unités de côté et le repère une unité par texel : les
/// coins vont donc de zéro à quatre. Un décodeur qui lirait des coordonnées
/// écrites rendrait des zéros.
#[test]
fn les_coordonnees_se_derivent_du_repere() {
    let world = World::load(&valid()).expect("carte valide");
    let vertices = &world.cells()[0].vertices;

    assert_eq!((vertices[0].u, vertices[0].v), (0.0, 0.0));
    assert_eq!((vertices[1].u, vertices[1].v), (4.0, 0.0));
    assert_eq!((vertices[2].u, vertices[2].v), (4.0, 4.0));
    assert_eq!((vertices[3].u, vertices[3].v), (0.0, 4.0));
}

/// L'échelle du repère est la longueur de ses axes.
#[test]
fn la_longueur_des_axes_porte_l_echelle() {
    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[{
            let mut bytes = words(&[11, 0, 1, 4]);
            bytes.extend_from_slice(&words(&[0, 1, 2, 3]));
            // Huit texels par unité sur u, deux sur v : deux échelles dans le
            // même repère, pour qu'un axe recopié à la place de l'autre se voie.
            bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], [8.0, 0.0, 0.0], [0.0, 2.0, 0.0]));
            bytes.extend_from_slice(&unit_frame());
            bytes
        }],
        &[],
    );
    let world = World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide");
    let vertices = &world.cells()[0].vertices;

    assert_eq!((vertices[2].u, vertices[2].v), (32.0, 8.0));
}

/// Les coordonnées d'une surface éloignée sont repliées dans `[0, 2048)`.
///
/// Un plaquage fin sur une grande surface dépasse trivialement la borne de
/// l'ABI, et la soumission refuserait alors le lot entier sans dire quelle
/// surface. Le repli retire un multiple de 2048, le même pour tous les sommets :
/// l'image est identique au bit près.
#[test]
fn les_coordonnees_lointaines_sont_repliees() {
    let far: [[f32; 3]; 4] = [
        [10000.0, 0.0, 0.0],
        [10004.0, 0.0, 0.0],
        [10004.0, 4.0, 0.0],
        [10000.0, 4.0, 0.0],
    ];
    let cell = cell_bytes(7, 0, &far, &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])], &[]);
    let world = World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide");
    let vertices = &world.cells()[0].vertices;

    // 10000 = 4 × 2048 + 1808.
    assert_eq!(vertices[0].u, 1808.0);
    assert_eq!(vertices[1].u, 1812.0);
    assert!(
        vertices.iter().all(|v| (0.0..2048.0).contains(&v.u)),
        "toutes les abscisses sont dans la fenêtre"
    );
}

/// Le repli garde l'écart entre les sommets, qui est ce que le rendu lit.
///
/// Un repli calculé par sommet déchirerait la surface ; c'est le même multiple
/// pour tous, et c'est ce qui rend l'image identique au bit près.
#[test]
fn le_repli_garde_les_ecarts() {
    let near = World::load(&valid()).expect("carte valide");
    let far: [[f32; 3]; 4] = SQUARE.map(|p| [p[0] + 8192.0, p[1], p[2]]);
    let cell = cell_bytes(7, 0, &far, &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])], &[]);
    let loin = World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide");

    let ecart = |world: &World| {
        let v = &world.cells()[0].vertices;
        (v[1].u - v[0].u, v[2].v - v[1].v)
    };
    assert_eq!(ecart(&near), ecart(&loin));
}

/// Les coordonnées négatives se replient vers le haut, pas vers zéro.
///
/// L'arrondi du multiple se fait vers le bas : tronquer vers zéro laisserait une
/// abscisse négative, que la soumission refuserait.
#[test]
fn les_coordonnees_negatives_se_replient() {
    let behind: [[f32; 3]; 4] = SQUARE.map(|p| [p[0] - 100.0, p[1], p[2]]);
    let cell = cell_bytes(
        7,
        0,
        &behind,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[],
    );
    let world = World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide");

    assert!(
        world.cells()[0].vertices.iter().all(|v| v.u >= 0.0),
        "une abscisse est restée négative"
    );
}

/// Des coordonnées dont le repli ne serait plus exact sont refusées.
///
/// Au-delà de 2²⁴, un `f32` ne porte plus tous les entiers : soustraire le
/// multiple de 2048 cesserait d'être exact, et le repli déchirerait la surface
/// au lieu de la déplacer. **Trouvé par le test de mutation**, qui a fait
/// déborder la soustraction du multiple avant que ce contrôle n'existe.
#[test]
fn des_coordonnees_irrepliables_sont_refusees() {
    for scale in [f32::MAX, -f32::MAX, 1e30, -1e30] {
        let mut body = words(&[11, 0, 1, 4]);
        body.extend_from_slice(&words(&[0, 1, 2, 3]));
        // Un axe démesuré : les coordonnées dérivées sortent de la fenêtre où
        // le repli est exact, alors que le fichier n'a que des valeurs finies.
        body.extend_from_slice(&frame([0.0, 0.0, 0.0], [scale, 0.0, 0.0], [0.0, 1.0, 0.0]));
        body.extend_from_slice(&unit_frame());

        let cell = cell_bytes(7, 0, &SQUARE, &[body], &[]);
        assert_eq!(
            World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
            refused(Malformation::Mapping),
            "échelle {scale}"
        );
    }
}

/// Deux portails qui partagent leurs sommets s'apparient, au bit près.
#[test]
fn deux_portails_qui_se_touchent_s_apparient() {
    let first = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[portal_bytes(21, &[0, 1, 2, 3])],
    );
    // La seconde cellule a les mêmes sommets, dans un autre ordre : la clé se
    // trie, donc l'appariement les reconnaît quand même.
    let mirrored = [SQUARE[3], SQUARE[2], SQUARE[1], SQUARE[0]];
    let second = cell_bytes(
        8,
        0,
        &mirrored,
        &[surface_bytes(12, 0, 1, &[0, 1, 2, 3])],
        &[portal_bytes(22, &[0, 1, 2, 3])],
    );

    let mut cells = first;
    cells.extend_from_slice(&second);
    let world = World::load(&file(&cells, &[], &[], &material(1, "mur"))).expect("carte valide");

    assert_eq!(world.cells()[0].portals[0].link, Some((1, 0)));
    assert_eq!(world.cells()[1].portals[0].link, Some((0, 0)));
}

/// Deux portails dont les sommets collisionnaient sous l'ancienne clé restent
/// des murs.
///
/// La clé d'un sommet a longtemps mêlé ses trois mots en un seul `u64`, par
/// rotations et `XOR`. Les deux sommets ci-dessous sont construits pour tomber
/// sur la même valeur : `y` diffère d'un bit de mantisse, et `z` du bit que la
/// rotation de vingt-et-un amenait au même rang. L'appariement les prenait donc
/// pour le même point et liait deux cellules qui ne se touchent pas — une
/// traversée vers une cellule non voisine, c'est-à-dire un trou ou une fuite que
/// rien ne signale.
#[test]
fn deux_portails_qui_collisionnaient_restent_des_murs() {
    // Même clé sous l'ancien condensé, deux points distincts : `4.0` dont le bit
    // de mantisse le plus bas est mis, et `0.0` dont le bit 21 l'est.
    let colliding = [
        0.0,
        f32::from_bits(4.0f32.to_bits() ^ 1),
        f32::from_bits(1 << 21),
    ];
    let triangle = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]];
    let shifted = [triangle[0], triangle[1], colliding];

    let first = cell_bytes(
        7,
        0,
        &triangle,
        &[surface_bytes(11, 0, 1, &[0, 1, 2])],
        &[portal_bytes(21, &[0, 1, 2])],
    );
    let second = cell_bytes(
        8,
        0,
        &shifted,
        &[surface_bytes(12, 0, 1, &[0, 1, 2])],
        &[portal_bytes(22, &[0, 1, 2])],
    );

    let mut cells = first;
    cells.extend_from_slice(&second);
    let world = World::load(&file(&cells, &[], &[], &material(1, "mur"))).expect("carte valide");

    assert_eq!(world.cells()[0].portals[0].link, None);
    assert_eq!(world.cells()[1].portals[0].link, None);
}

/// Deux portails d'une même cellule ne s'apparient pas.
///
/// Le lien ramènerait sur la cellule courante, et la traversée tournerait sur
/// place au lieu d'avancer. Rien dans le fichier ne l'interdisait : les deux
/// portails ont les mêmes sommets, donc la même clé, et l'appariement les liait
/// l'un à l'autre.
#[test]
fn deux_portails_de_la_meme_cellule_sont_refuses() {
    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[
            portal_bytes(21, &[0, 1, 2, 3]),
            portal_bytes(22, &[3, 2, 1, 0]),
        ],
    );
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Portal)
    );
}

/// Un portail seul est un mur, pas une erreur : une carte en cours d'édition en
/// a toujours.
#[test]
fn un_portail_seul_est_un_mur() {
    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[portal_bytes(21, &[0, 1, 2, 3])],
    );
    let world = World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide");

    assert_eq!(world.cells()[0].portals[0].link, None);
}

/// Trois portails sur les mêmes sommets sont refusés.
#[test]
fn trois_portails_sur_la_meme_cle_sont_refuses() {
    let mut cells = Vec::new();
    for (cell_id, portal_id, surface_id) in [(7, 21, 11), (8, 22, 12), (9, 23, 13)] {
        cells.extend_from_slice(&cell_bytes(
            cell_id,
            0,
            &SQUARE,
            &[surface_bytes(surface_id, 0, 1, &[0, 1, 2, 3])],
            &[portal_bytes(portal_id, &[0, 1, 2, 3])],
        ));
    }
    assert_eq!(
        World::load(&file(&cells, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Portal)
    );
}

/// Un portail concave est refusé, là où une surface concave passe.
///
/// C'est la différence que le format assume : une cellule concave rend la
/// traversée conservatrice, un portail concave la rend fausse.
#[test]
fn un_portail_concave_est_refuse() {
    // Un cerf-volant rentrant : le troisième sommet est tiré vers l'intérieur.
    let dented: [[f32; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 4.0, 0.0],
    ];
    let cell = cell_bytes(
        7,
        0,
        &dented,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[portal_bytes(21, &[0, 1, 2, 3])],
    );
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Polygon)
    );

    // La même géométrie en surface seule est acceptée.
    let cell = cell_bytes(
        7,
        0,
        &dented,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[],
    );
    assert!(World::load(&file(&cell, &[], &[], &material(1, "mur"))).is_ok());
}

/// Un axe de lightmap dont la longueur n'est pas une puissance de deux est
/// refusé.
///
/// C'est cet alignement qui évite une marche d'éclairage à la jointure de deux
/// surfaces coplanaires, et il se vérifie plutôt qu'il ne se convient.
#[test]
fn un_repere_de_lightmap_mal_aligne_est_refuse() {
    let mut body = words(&[11, 0, 1, 4]);
    body.extend_from_slice(&words(&[0, 1, 2, 3]));
    body.extend_from_slice(&unit_frame());
    body.extend_from_slice(&frame([0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0, 1.0, 0.0]));

    let cell = cell_bytes(7, 0, &SQUARE, &[body], &[]);
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Mapping)
    );
}

/// Un repère de lightmap en diagonale est accepté.
///
/// **C'est ce qui rend un mur oblique éclairable**, et le contrôle l'interdisait :
/// `(1, 1, 0)` donne `|u|² = 2`, une puissance de deux d'exposant impair, et
/// exiger la parité écartait le seul repère qu'un plan à 45° admette — un axe y
/// s'écrit `(p, −p, 0)`, de carré `2p²`, et le second doit être orthogonal à lui
/// tout en restant dans le plan. Le pas de la grille n'est alors plus une puissance
/// de deux, et l'alignement de deux surfaces coplanaires redevient l'affaire de
/// l'éditeur : il ne touche ni la justesse ni le déterminisme, seulement une marche
/// d'éclairage à une jointure.
#[test]
fn un_repere_de_lightmap_en_diagonale_est_accepte() {
    let oblique = frame([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]);
    World::load(&map_with_lightmap(&oblique)).expect("un mur oblique s'éclaire");
}

/// Une origine de lightmap qui ne tombe pas sur un nœud de sa grille est
/// refusée.
///
/// Des axes au bon pas ne suffisent pas : deux grilles de pas égal mais
/// déphasées d'un demi-luxel ne coïncident pas davantage, et c'est la même marche
/// d'éclairage à la jointure.
#[test]
fn une_origine_de_lightmap_hors_grille_est_refusee() {
    let shifted = frame([0.5, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    assert_eq!(
        World::load(&map_with_lightmap(&shifted)).unwrap_err(),
        refused(Malformation::Mapping)
    );
}

/// Deux axes de lightmap qui ne sont pas orthogonaux sont refusés.
///
/// Pris confondus, le cas le plus net : sans orthogonalité, retrouver le point du
/// monde d'un luxel demande d'inverser une 2×2 quelconque, donc une division dont
/// l'arrondi deviendrait contractuel.
#[test]
fn des_axes_de_lightmap_non_orthogonaux_sont_refuses() {
    let collapsed = frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    assert_eq!(
        World::load(&map_with_lightmap(&collapsed)).unwrap_err(),
        refused(Malformation::Mapping)
    );
}

/// Un axe de lightmap qui sort du plan de la surface est refusé.
///
/// Le carré vit dans `z = 0` ; un axe porté par `Z` est sa normale, et la grille
/// qu'il engendre ne recouvre rien de ce qu'elle est censée éclairer.
#[test]
fn un_axe_de_lightmap_hors_du_plan_est_refuse() {
    let normal = frame([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    assert_eq!(
        World::load(&map_with_lightmap(&normal)).unwrap_err(),
        refused(Malformation::Mapping)
    );
}

/// Une surface dont l'étendue en luxels dépasse le plafond est refusée.
///
/// Un pas de `2⁻⁹` sur un carré de quatre unités demande 2048 luxels de côté. Le
/// refus tombe ici et non au calcul : l'hôte l'apprend en ouvrant la carte, une
/// fois, au lieu de le voir remonter d'une cuisson pour une seule cellule.
#[test]
fn une_etendue_de_lightmap_demesuree_est_refusee() {
    let fine = frame([0.0, 0.0, 0.0], [0.001_953_125, 0.0, 0.0], [0.0, 1.0, 0.0]);
    assert_eq!(
        World::load(&map_with_lightmap(&fine)).unwrap_err(),
        refused(Malformation::Mapping)
    );
}

/// Un identifiant nul est refusé dans chacune des trois familles.
#[test]
fn un_identifiant_nul_est_refuse() {
    let cases = [
        cell_bytes(
            0,
            0,
            &SQUARE,
            &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
            &[],
        ),
        cell_bytes(7, 0, &SQUARE, &[surface_bytes(0, 0, 1, &[0, 1, 2, 3])], &[]),
        cell_bytes(
            7,
            0,
            &SQUARE,
            &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
            &[portal_bytes(0, &[0, 1, 2, 3])],
        ),
    ];
    for (i, cell) in cases.iter().enumerate() {
        assert_eq!(
            World::load(&file(cell, &[], &[], &material(1, "mur"))).unwrap_err(),
            refused(Malformation::Identifier),
            "famille {i}"
        );
    }

    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[],
    );
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(0, "mur"))).unwrap_err(),
        refused(Malformation::Identifier),
        "matériau"
    );
}

/// Deux cellules du même identifiant sont refusées.
#[test]
fn deux_cellules_du_meme_identifiant_sont_refusees() {
    let one = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[],
    );
    let two = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(12, 0, 1, &[0, 1, 2, 3])],
        &[],
    );
    let mut cells = one;
    cells.extend_from_slice(&two);
    assert_eq!(
        World::load(&file(&cells, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Identifier)
    );
}

/// Les identifiants de surface sont uniques dans toute la carte, pas seulement
/// dans leur cellule.
///
/// Un espace par famille, et non par cellule : sans quoi l'étape 8 ne pourrait
/// pas désigner une surface sans nommer aussi sa cellule.
#[test]
fn les_identifiants_de_surface_sont_uniques_dans_la_carte() {
    let one = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[],
    );
    let two = cell_bytes(
        8,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[],
    );
    let mut cells = one;
    cells.extend_from_slice(&two);
    assert_eq!(
        World::load(&file(&cells, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Identifier)
    );
}

/// Un bit de drapeau non défini est refusé, sur la cellule comme sur la surface.
#[test]
fn un_drapeau_non_defini_est_refuse() {
    let cell = cell_bytes(
        7,
        1,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[],
    );
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Flags),
        "la cellule n'a aucun bit défini"
    );

    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 8, 1, &[0, 1, 2, 3])],
        &[],
    );
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Flags),
        "la surface en a trois"
    );

    // Les trois bits définis passent.
    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 7, 1, &[0, 1, 2, 3])],
        &[],
    );
    assert!(World::load(&file(&cell, &[], &[], &material(1, "mur"))).is_ok());
}

/// Un matériau qu'aucune entrée ne déclare est refusé.
#[test]
fn un_materiau_inconnu_est_refuse() {
    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 9, &[0, 1, 2, 3])],
        &[],
    );
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Index)
    );
}

/// Un indice de sommet hors de la cellule est refusé.
#[test]
fn un_indice_de_sommet_hors_borne_est_refuse() {
    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 4])],
        &[],
    );
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Index)
    );
}

/// Une surface de plus de soixante-quatre sommets est refusée.
#[test]
fn une_surface_trop_grande_est_refusee() {
    let points: Vec<[f32; 3]> = (0..MAX_POLYGON + 1).map(|i| [i as f32, 0.0, 0.0]).collect();
    let indices: Vec<u32> = (0..points.len() as u32).collect();
    let cell = cell_bytes(7, 0, &points, &[surface_bytes(11, 0, 1, &indices)], &[]);
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Polygon)
    );
}

/// Un compte démesuré est refusé sans déborder son produit.
#[test]
fn un_compte_demesure_est_refuse() {
    let mut body = words(&[7, 0, u32::MAX, 0, 0]);
    body.extend_from_slice(&floats(&[0.0, 0.0, 0.0]));
    let mut cell = words(&[body.len() as u32]);
    cell.extend_from_slice(&body);

    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Truncated)
    );
}

/// Des octets laissés au bout d'un enregistrement de cellule sont refusés.
///
/// La longueur borne l'enregistrement : ce qu'elle couvre et que rien ne lit est
/// le signe d'un compte faux, pas d'une extension.
#[test]
fn des_octets_laisses_dans_une_cellule_sont_refuses() {
    let mut body = words(&[7, 0, 4, 1, 0]);
    for point in SQUARE {
        body.extend_from_slice(&floats(&point));
    }
    body.extend_from_slice(&surface_bytes(11, 0, 1, &[0, 1, 2, 3]));
    body.extend_from_slice(&[0, 0, 0, 0]);

    let mut cell = words(&[body.len() as u32]);
    cell.extend_from_slice(&body);
    assert_eq!(
        World::load(&file(&cell, &[], &[], &material(1, "mur"))).unwrap_err(),
        refused(Malformation::Count)
    );
}

/// Une carte sans cellule est légitime : un décor vide se charge.
#[test]
fn une_carte_vide_est_legitime() {
    let world = World::load(&file(&[], &[], &[], &[])).expect("carte vide");
    assert_eq!(world.triangle_count(), 0);
    assert_eq!(world.material_count(), 0);
}

/// Un maillage là où une carte est attendue est refusé par le genre.
#[test]
fn un_maillage_n_est_pas_une_carte() {
    let mut bytes = valid();
    bytes[4..8].copy_from_slice(b"MESH");
    assert_eq!(
        World::load(&bytes).unwrap_err(),
        refused(Malformation::Kind)
    );
}

/// Une carte tronquée à n'importe laquelle de ses longueurs est refusée, sans
/// une seule panique.
#[test]
fn toute_troncature_est_refusee() {
    let bytes = valid();
    for len in 0..bytes.len() {
        match World::load(&bytes[..len]) {
            Ok(_) => panic!("tronqué à {len} octets, et accepté"),
            Err(error) => assert!(
                matches!(
                    error,
                    Error::InvalidFormat(_) | Error::UnsupportedFormatVersion
                ),
                "à {len} octets : {error:?}"
            ),
        }
    }
}

/// Une mutation quelconque rend un succès ou une erreur de données, jamais
/// autre chose et jamais une panique.
#[test]
fn une_mutation_quelconque_ne_panique_pas() {
    const SEED: u64 = 0x5752_4c44_2026_0925;
    let mut rng = Rng::new(SEED);
    let original = valid();
    for round in 0..8192 {
        let mut bytes = original.clone();
        for _ in 0..2 {
            let at = (rng.next() % bytes.len() as u64) as usize;
            bytes[at] ^= (rng.next() % 255 + 1) as u8;
        }
        if let Err(error) = World::load(&bytes) {
            assert!(
                matches!(
                    error,
                    Error::InvalidFormat(_) | Error::UnsupportedFormatVersion
                ),
                "graine {SEED:#x}, tour {round} : {error:?}"
            );
        }
    }
}

/// Une carte de plusieurs cellules compte ses triangles sur l'ensemble.
#[test]
fn les_triangles_se_comptent_sur_toute_la_carte() {
    let mut cells = Vec::new();
    for (cell_id, surface_id) in [(7u32, 11u32), (8, 12), (9, 13)] {
        cells.extend_from_slice(&cell_bytes(
            cell_id,
            0,
            &SQUARE,
            &[surface_bytes(surface_id, 0, 1, &[0, 1, 2, 3])],
            &[],
        ));
    }
    let world = World::load(&file(&cells, &[], &[], &material(1, "mur"))).expect("carte valide");

    assert_eq!(world.cells().len(), 3);
    assert_eq!(world.triangle_count(), 6);
}

/// Les triangles d'une surface indexent ses propres sommets.
///
/// Deux surfaces d'une même cellule ont chacune les siens, parce que leurs
/// repères leur donnent des coordonnées différentes sur un sommet partagé.
#[test]
fn chaque_surface_a_ses_propres_sommets() {
    let cell = cell_bytes(
        7,
        0,
        &SQUARE,
        &[
            surface_bytes(11, 0, 1, &[0, 1, 2, 3]),
            surface_bytes(12, 0, 1, &[0, 1, 2]),
        ],
        &[],
    );
    let world = World::load(&file(&cell, &[], &[], &material(1, "mur"))).expect("carte valide");
    let cell = &world.cells()[0];

    assert_eq!(cell.vertices.len(), 7, "quatre sommets, puis trois");
    assert_eq!(cell.surfaces[1].first_triangle, 2);
    assert_eq!(
        cell.triangles[2],
        [4, 5, 6],
        "la seconde surface indexe les siens"
    );
}

/// Un nom de matériau qui n'est pas de l'UTF-8 valide est refusé.
#[test]
fn un_nom_de_materiau_invalide_est_refuse() {
    let mut mats = words(&[1]);
    mats.extend_from_slice(&2u16.to_le_bytes());
    mats.extend_from_slice(&[0xff, 0xfe]);
    assert_eq!(
        World::load(&file(&[], &[], &[], &mats)).unwrap_err(),
        refused(Malformation::NonUtf8)
    );
}

/// Une lumière statique : identifiant, position, rayon, couleur.
fn light_bytes(id: u32, position: [f32; 3], radius: f32, color: [u8; 3]) -> Vec<u8> {
    let mut bytes = words(&[id]);
    bytes.extend_from_slice(&floats(&[position[0], position[1], position[2], radius]));
    bytes.extend_from_slice(&color);
    bytes.push(0);
    bytes
}

/// Une entité, longueur-préfixée.
fn entity_bytes(
    id: u32,
    cell: u32,
    class: &str,
    position: [f32; 3],
    orientation: [f32; 4],
    data: &[u8],
) -> Vec<u8> {
    let mut body = words(&[id, cell]);
    body.extend_from_slice(&(class.len() as u16).to_le_bytes());
    body.extend_from_slice(class.as_bytes());
    body.extend_from_slice(&floats(&position));
    body.extend_from_slice(&floats(&orientation));
    body.extend_from_slice(&words(&[data.len() as u32]));
    body.extend_from_slice(data);

    let mut bytes = words(&[body.len() as u32]);
    bytes.extend_from_slice(&body);
    bytes
}

/// La cellule des cartes peuplées, celle où les entités se trouvent.
fn one_cell() -> Vec<u8> {
    cell_bytes(
        7,
        0,
        &SQUARE,
        &[surface_bytes(11, 0, 1, &[0, 1, 2, 3])],
        &[],
    )
}

/// Une carte d'une cellule, une lumière et une entité.
fn peopled() -> Vec<u8> {
    file(
        &one_cell(),
        &entity_bytes(
            31,
            7,
            "depart",
            [1.0, 2.0, 3.0],
            [0.0, 0.0, 0.0, 2.0],
            &[0xDE, 0xAD],
        ),
        &light_bytes(41, [4.0, 5.0, 6.0], 8.0, [0xF0, 0x80, 0x40]),
        &material(1, "mur"),
    )
}

/// Une carte peuplée rend ses lumières et ses entités.
#[test]
fn une_carte_rend_ses_lumieres_et_ses_entites() {
    let world = World::load(&peopled()).expect("carte valide");

    assert_eq!(world.light_count(), 1);
    let light = world.light(0).expect("la lumière existe");
    assert_eq!(light.position, Vec3::new(4.0, 5.0, 6.0));
    assert_eq!(light.radius, 8.0);
    assert_eq!(light.color, Color::new(0xF0, 0x80, 0x40, 0xFF));
    assert!(world.light(1).is_none());

    assert_eq!(world.entity_count(), 1);
    assert_eq!(world.entity_ids(0), Some((31, 7)));
    assert_eq!(world.entity_class(0), Some("depart"));
    assert_eq!(world.entity_data(0), Some(&[0xDE, 0xAD][..]));
    assert!(world.entity_ids(1).is_none());
}

/// L'orientation d'une entité est normalisée au chargement.
///
/// Le fichier porte `(0, 0, 0, 2)`, qui n'est pas unitaire : laissée telle
/// quelle, elle ferait tourner l'objet autrement selon l'échelle que l'éditeur a
/// écrite.
#[test]
fn l_orientation_d_une_entite_est_normalisee() {
    let world = World::load(&peopled()).expect("carte valide");
    let (position, orientation) = world.entity_pose(0).expect("la pose existe");

    assert_eq!(position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(orientation.w, 1.0);
    assert_eq!(
        (orientation.x, orientation.y, orientation.z),
        (0.0, 0.0, 0.0)
    );
}

/// Une orientation nulle est refusée plutôt que redressée en silence.
#[test]
fn une_orientation_nulle_est_refusee() {
    let bytes = file(
        &one_cell(),
        &entity_bytes(31, 7, "depart", [0.0; 3], [0.0; 4], &[]),
        &[],
        &material(1, "mur"),
    );
    assert_eq!(
        World::load(&bytes).unwrap_err(),
        refused(Malformation::Pose)
    );
}

/// Une entité qui désigne une cellule inexistante est refusée.
///
/// Par identifiant et jamais par index : c'est ce qui permet à un éditeur de
/// supprimer une cellule du milieu sans renuméroter le reste.
#[test]
fn une_entite_sans_cellule_est_refusee() {
    for cell in [0, 8] {
        let bytes = file(
            &one_cell(),
            &entity_bytes(31, cell, "depart", [0.0; 3], [0.0, 0.0, 0.0, 1.0], &[]),
            &[],
            &material(1, "mur"),
        );
        assert_eq!(
            World::load(&bytes).unwrap_err(),
            refused(Malformation::Index),
            "cellule {cell}"
        );
    }
}

/// Un rayon de lumière nul, négatif ou non fini est refusé.
#[test]
fn un_rayon_de_lumiere_invalide_est_refuse() {
    for radius in [0.0, -1.0] {
        let bytes = file(
            &one_cell(),
            &[],
            &light_bytes(41, [0.0; 3], radius, [0xFF; 3]),
            &material(1, "mur"),
        );
        assert_eq!(
            World::load(&bytes).unwrap_err(),
            refused(Malformation::Light),
            "rayon {radius}"
        );
    }

    // Un rayon non fini est refusé plus tôt, par le curseur : aucun flottant du
    // fichier n'a de non-fini légitime.
    let bytes = file(
        &one_cell(),
        &[],
        &light_bytes(41, [0.0; 3], f32::INFINITY, [0xFF; 3]),
        &material(1, "mur"),
    );
    assert_eq!(
        World::load(&bytes).unwrap_err(),
        refused(Malformation::NonFinite)
    );
}

/// L'octet réservé d'une lumière est nul, comme dans la structure de l'ABI.
#[test]
fn l_octet_reserve_d_une_lumiere_est_nul() {
    let mut light = light_bytes(41, [0.0; 3], 1.0, [0xFF; 3]);
    *light.last_mut().expect("l'octet réservé") = 1;
    let bytes = file(&one_cell(), &[], &light, &material(1, "mur"));
    assert_eq!(
        World::load(&bytes).unwrap_err(),
        refused(Malformation::Flags)
    );
}

/// Deux lumières ou deux entités du même identifiant sont refusées.
#[test]
fn les_identifiants_de_lumiere_et_d_entite_sont_uniques() {
    let mut lights = light_bytes(41, [0.0; 3], 1.0, [0xFF; 3]);
    lights.extend_from_slice(&light_bytes(41, [1.0; 3], 2.0, [0xFF; 3]));
    let bytes = file(&one_cell(), &[], &lights, &material(1, "mur"));
    assert_eq!(
        World::load(&bytes).unwrap_err(),
        refused(Malformation::Identifier),
        "deux lumières"
    );

    let mut entities = entity_bytes(31, 7, "a", [0.0; 3], [0.0, 0.0, 0.0, 1.0], &[]);
    entities.extend_from_slice(&entity_bytes(
        31,
        7,
        "b",
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        &[],
    ));
    let bytes = file(&one_cell(), &entities, &[], &material(1, "mur"));
    assert_eq!(
        World::load(&bytes).unwrap_err(),
        refused(Malformation::Identifier),
        "deux entités"
    );
}

/// Une classe qui n'est pas de l'UTF-8 valide est refusée.
#[test]
fn une_classe_qui_n_est_pas_de_l_utf8_est_refusee() {
    let mut body = words(&[31, 7]);
    body.extend_from_slice(&2u16.to_le_bytes());
    body.extend_from_slice(&[0xff, 0xfe]);
    body.extend_from_slice(&floats(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]));
    body.extend_from_slice(&words(&[0]));

    let mut entity = words(&[body.len() as u32]);
    entity.extend_from_slice(&body);
    let bytes = file(&one_cell(), &entity, &[], &material(1, "mur"));
    assert_eq!(
        World::load(&bytes).unwrap_err(),
        refused(Malformation::NonUtf8)
    );
}

/// Une classe vide et des données vides sont légitimes.
#[test]
fn une_entite_sans_classe_ni_donnees_est_legitime() {
    let bytes = file(
        &one_cell(),
        &entity_bytes(31, 7, "", [0.0; 3], [0.0, 0.0, 0.0, 1.0], &[]),
        &[],
        &material(1, "mur"),
    );
    let world = World::load(&bytes).expect("carte valide");
    assert_eq!(world.entity_class(0), Some(""));
    assert_eq!(world.entity_data(0), Some(&[][..]));
}

/// Des octets laissés au bout d'un enregistrement d'entité sont refusés.
#[test]
fn des_octets_laisses_dans_une_entite_sont_refuses() {
    let mut entity = entity_bytes(31, 7, "a", [0.0; 3], [0.0, 0.0, 0.0, 1.0], &[]);
    let len = u32::from_le_bytes(entity[..4].try_into().expect("la longueur"));
    entity[..4].copy_from_slice(&(len + 4).to_le_bytes());
    entity.extend_from_slice(&[0, 0, 0, 0]);

    let bytes = file(&one_cell(), &entity, &[], &material(1, "mur"));
    assert_eq!(
        World::load(&bytes).unwrap_err(),
        refused(Malformation::Count)
    );
}

/// Une longueur de données démesurée est refusée sans allouer dessus.
#[test]
fn une_longueur_de_donnees_demesuree_est_refusee() {
    let mut body = words(&[31, 7]);
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(b"a");
    body.extend_from_slice(&floats(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]));
    body.extend_from_slice(&words(&[u32::MAX]));

    let mut entity = words(&[body.len() as u32]);
    entity.extend_from_slice(&body);
    let bytes = file(&one_cell(), &entity, &[], &material(1, "mur"));
    assert_eq!(
        World::load(&bytes).unwrap_err(),
        refused(Malformation::Truncated)
    );
}

/// Une carte peuplée résiste à la troncature et aux mutations.
///
/// Le même filet que pour la géométrie, sur le fichier qui porte les quatre
/// sections : c'est le seul qui les éprouve ensemble.
#[test]
fn une_carte_peuplee_resiste_aux_mutations() {
    const SEED: u64 = 0x4c47_5453_2026_0925;
    let original = peopled();
    for len in 0..original.len() {
        match World::load(&original[..len]) {
            Ok(_) => panic!("tronqué à {len} octets, et accepté"),
            Err(error) => assert!(
                matches!(
                    error,
                    Error::InvalidFormat(_) | Error::UnsupportedFormatVersion
                ),
                "à {len} octets : {error:?}"
            ),
        }
    }

    let mut rng = Rng::new(SEED);
    for round in 0..8192 {
        let mut bytes = original.clone();
        for _ in 0..2 {
            let at = (rng.next() % bytes.len() as u64) as usize;
            bytes[at] ^= (rng.next() % 255 + 1) as u8;
        }
        if let Err(error) = World::load(&bytes) {
            assert!(
                matches!(
                    error,
                    Error::InvalidFormat(_) | Error::UnsupportedFormatVersion
                ),
                "graine {SEED:#x}, tour {round} : {error:?}"
            );
        }
    }
}

/// Une carte qui porte ses quatre sections se charge entière.
///
/// Le seul test qui les éprouve ensemble : les autres n'en peuplent qu'une à la
/// fois, et une section qui n'aurait pas sa place dans la table ne se verrait
/// nulle part ailleurs.
#[test]
fn les_sections_d_entites_et_de_lumieres_sont_admises() {
    let world = World::load(&peopled()).expect("carte valide");
    assert_eq!(world.triangle_count(), 2);
    assert_eq!(world.light_count(), 1);
    assert_eq!(world.entity_count(), 1);
}

/// Une section de genre inconnu refuse toujours le fichier.
#[test]
fn le_conteneur_garde_ses_refus() {
    let bytes = vec![0u8; 8];
    let mut file = file(&bytes, &[], &[], &[]);
    file[20..24].copy_from_slice(b"XXXX");
    assert_eq!(
        World::load(&file).unwrap_err(),
        refused(Malformation::SectionKind)
    );
}
