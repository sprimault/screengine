// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le décor de validation du balayage : deux cellules, sept cas.
//!
//! **Une carte à part plutôt que des cas ajoutés à `salles.world`.** Celle-ci a
//! pour métier de figer une image, et chaque cas de collision qu'on y ajouterait
//! traînerait derrière lui une mise à jour de référence — alors que les cas
//! voulus ici sont délibérément dégénérés : un passage exactement à la largeur
//! d'une boîte, un portail qui ne mène nulle part, une surface qui n'arrête rien.
//! Les deux points de gel restent ainsi découplés.
//!
//! Les sept cas, et ce que chacun éprouve que les autres n'éprouvent pas :
//!
//! - **les coins rentrants** de la salle en L — cinq angles droits vus de
//!   l'intérieur : leur arête partagée est éteinte au chargement, et une boîte
//!   qui glisse le long d'un mur ne doit pas y accrocher ;
//! - **le coin saillant**, l'angle de 270° que le L dessine en `(4, 4)` : son
//!   arête garde son prisme, sans quoi une boîte passerait au voisinage de
//!   l'angle ;
//! - **le portail apparié** entre les deux cellules, franchi d'un seul balayage :
//!   c'est lui qui met la traversée à l'épreuve, et l'égalité avec le chemin de
//!   force brute avec elle ;
//! - **le portail non apparié**, sur l'arête `(4, 8)`–`(0, 8)` : il est **solide**,
//!   c'est le mot du format, et c'est ce qui garde la cellule fermée ;
//! - **la surface non solide**, le plafond du couloir : le premier lecteur du
//!   drapeau, qui l'exclut du balayage et de rien d'autre ;
//! - **le passage exactement à la largeur d'une boîte** : le couloir fait une
//!   unité de large, donc exactement le côté de la boîte d'épreuve. C'est le cas
//!   qui rend la constante de dilatation **mesurable** au lieu d'arbitraire — une
//!   boîte qui n'y passe plus dit que la marge est trop grande ;
//! - **le passage plus étroit que la boîte**, qui est le même couloir balayé avec
//!   une boîte plus grande : il doit refuser franchement, jamais laisser passer.
//!
//! **Les deux portails s'apparient au bit près**, ce qui décide de la géométrie :
//! la salle porte deux sommets colinéaires sur son mur de droite, en `y = 2` et
//! `y = 3`, pour que l'arête entre eux soit exactement celle du couloir. Sans
//! eux, les deux portails n'auraient aucun sommet commun et le chargement en
//! ferait deux murs — la traversée ne franchirait rien, et la scène éprouverait
//! le contraire de ce qu'elle annonce.
//!
//! **Toutes les arêtes ont une longueur puissance de deux**, et ce n'est pas un
//! choix esthétique : le repère de lightmap d'un mur prend l'arête pour axe
//! horizontal, et le chargement exige que le carré de cet axe soit une puissance
//! de deux — faute de quoi la reconstruction d'un luxel demanderait une division.
//! Un décor qui ne se dessine pas en a besoin quand même : le format ne connaît
//! pas la différence, et une arête de 1,5 unité fait refuser la carte entière.

use crate::map_bytes::{FLOOR, WALLS, flagged, floats, surface, words};

/// Le sol des deux cellules.
pub const FLOOR_Z: f32 = 0.0;

/// Leur plafond.
pub const CEILING_Z: f32 = 4.0;

/// Le bit qui dit qu'une surface n'arrête aucun volume qui la balaie.
const NON_SOLID: u32 = 0b100;

/// L'empreinte de la salle en L.
///
/// Les sommets 2 et 3 sont colinéaires avec 1 et 4 : ils ne dessinent aucun coin
/// et n'existent que pour porter l'arête du portail. Un polygone a le droit
/// d'avoir des sommets alignés, et la triangulation par découpe d'oreilles les
/// traverse sans rien en faire.
pub const ROOM: [[f32; 2]; 8] = [
    [0.0, 0.0],
    [8.0, 0.0],
    [8.0, 2.0],
    [8.0, 3.0],
    [8.0, 4.0],
    [4.0, 4.0],
    [4.0, 8.0],
    [0.0, 8.0],
];

/// L'empreinte du couloir étroit, large d'une unité.
pub const CORRIDOR: [[f32; 2]; 4] = [[8.0, 2.0], [16.0, 2.0], [16.0, 3.0], [8.0, 3.0]];

/// Le rang de l'arête de la salle qui porte le portail vers le couloir.
const ROOM_PORTAL: usize = 2;

/// Celui de l'arête qui porte un portail **sans vis-à-vis**.
const ROOM_DEAD_PORTAL: usize = 6;

/// Celui de l'arête du couloir qui rejoint la salle.
const CORRIDOR_PORTAL: usize = 3;

/// Les octets du décor.
pub fn bytes() -> Vec<u8> {
    let mut cells = Vec::new();
    prism(
        7,
        100,
        &ROOM,
        &[ROOM_PORTAL, ROOM_DEAD_PORTAL],
        false,
        &mut cells,
    );
    prism(8, 200, &CORRIDOR, &[CORRIDOR_PORTAL], true, &mut cells);

    let mut materials = Vec::new();
    for (id, name) in [(1u32, "mur"), (2, "sol")] {
        words(&[id], &mut materials);
        materials.extend_from_slice(&(name.len() as u16).to_le_bytes());
        materials.extend_from_slice(name.as_bytes());
    }

    file(&cells, &materials)
}

/// Une cellule prismatique : une empreinte au sol, deux hauteurs.
///
/// `portals` désigne les arêtes qui sont des ouvertures plutôt que des murs, et
/// `open_ceiling` pose le drapeau « non solide » sur le plafond.
fn prism(
    id: u32,
    first_id: u32,
    footprint: &[[f32; 2]],
    portals: &[usize],
    open_ceiling: bool,
    out: &mut Vec<u8>,
) {
    let n = footprint.len();
    // **Toutes les empreintes tournent dans le même sens.** Une empreinte à
    // l'envers rend les normales sortantes du mauvais côté, et le balayage
    // n'arrêterait plus rien de l'intérieur — c'est l'erreur qui ne se voit pas
    // sur une carte qui ne se dessine pas.
    let twice_area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (footprint[i], footprint[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    assert!(
        twice_area > 0.0,
        "l'empreinte tourne à l'envers : aire signée {twice_area}"
    );

    let mut body = Vec::new();
    let walls = n - portals.len();
    words(
        &[
            id,
            0,
            (n * 2) as u32,
            (walls + 2) as u32,
            portals.len() as u32,
        ],
        &mut body,
    );

    for z in [FLOOR_Z, CEILING_Z] {
        for point in footprint {
            floats(&[point[0], point[1], z], &mut body);
        }
    }

    // Le sol suit l'empreinte, le plafond la parcourt à l'envers : les deux
    // normales sortent alors de la cellule, et le chargement les retourne
    // ensemble vers l'intérieur.
    let bottom: Vec<u32> = (0..n as u32).collect();
    surface(
        first_id,
        FLOOR,
        &bottom,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        &mut body,
    );
    let top: Vec<u32> = (0..n as u32).rev().map(|i| i + n as u32).collect();
    flagged(
        first_id + 1,
        if open_ceiling { NON_SOLID } else { 0 },
        FLOOR,
        &top,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        &mut body,
    );

    let mut next_id = first_id + 2;
    for i in 0..n {
        if portals.contains(&i) {
            continue;
        }
        let j = (i + 1) % n;
        let (a, b) = (footprint[i], footprint[j]);
        let along = [b[0] - a[0], b[1] - a[1], 0.0];
        surface(
            next_id,
            WALLS,
            &[i as u32, (i + n) as u32, (j + n) as u32, j as u32],
            along,
            [0.0, 0.0, 1.0],
            &mut body,
        );
        next_id += 1;
    }

    for i in portals {
        let j = (i + 1) % n;
        words(&[next_id, 4], &mut body);
        words(
            &[*i as u32, j as u32, (j + n) as u32, (i + n) as u32],
            &mut body,
        );
        next_id += 1;
    }

    words(&[body.len() as u32], out);
    out.extend_from_slice(&body);
}

/// Le conteneur : en-tête, table de deux sections, puis les sections.
fn file(cells: &[u8], materials: &[u8]) -> Vec<u8> {
    let header = 20 + 2 * 12;
    let total = header + cells.len() + materials.len();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"WRLD");
    words(&[1, total as u32, 2], &mut bytes);

    // Par genre croissant, ce que le décodeur vérifie avant de lire un champ.
    bytes.extend_from_slice(b"CELL");
    words(&[header as u32, cells.len() as u32], &mut bytes);
    bytes.extend_from_slice(b"MATS");
    words(
        &[(header + cells.len()) as u32, materials.len() as u32],
        &mut bytes,
    );

    bytes.extend_from_slice(cells);
    bytes.extend_from_slice(materials);
    bytes
}
