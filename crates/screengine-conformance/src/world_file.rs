// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La carte que la scène chargée rend : un couloir de deux cellules.
//!
//! **La disposition est écrite ici à la main**, comme celle du maillage et pour
//! la même raison : trois écritures indépendantes dans trois crates qui ne se
//! voient pas, et c'est ce qui fait rougir un désaccord au lieu de prouver que
//! l'écrivain et le lecteur s'accordent.
//!
//! Ce que le fichier porte est fixé par `docs/rust.md`, section « Formats de
//! fichier ».

/// Le demi-côté du couloir, en unités de monde.
const HALF: f32 = 3.0;

/// La cote du sol, sous la caméra qui est à l'origine.
const FLOOR_Z: f32 = -1.5;

/// Celle du plafond.
const CEILING_Z: f32 = 2.5;

/// La longueur d'une cellule, le long de l'axe de visée.
const LENGTH: f32 = 14.0;

/// Où commence le couloir, derrière la caméra.
///
/// Derrière et non devant : la caméra doit être **dans** la géométrie, ce qui
/// fait travailler le plan proche et la bande de garde à chaque image — un
/// couloir qui commencerait au ras de l'objectif ne les éprouverait pas.
const START: f32 = -2.0;

/// Les texels par unité de monde du plaquage.
///
/// Huit, comme les sols des scènes texturées : une case du damier fait alors une
/// unité, et la fuite se lit case par case.
const DENSITY: f32 = 8.0;

/// Le côté du repère de lightmap, puissance de deux comme le chargement
/// l'exige.
const LUXEL: f32 = 1.0;

/// Les identifiants des deux matériaux.
const WALLS: u32 = 1;
/// Celui du sol et du plafond.
const FLOOR: u32 = 2;

/// Les octets d'une suite de flottants.
fn floats(values: &[f32], out: &mut Vec<u8>) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// Les octets d'une suite d'entiers.
fn words(values: &[u32], out: &mut Vec<u8>) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// Un repère de plaquage aligné sur deux axes du monde.
///
/// L'origine est nulle et les axes portent l'échelle : c'est le repère qui est
/// la source, et les coordonnées la dérivée.
fn frame(u: [f32; 3], v: [f32; 3], out: &mut Vec<u8>) {
    floats(&[0.0, 0.0, 0.0], out);
    floats(&u, out);
    floats(&v, out);
}

/// Une surface : son en-tête, ses indices, son repère de texture, celui de sa
/// lightmap.
fn surface(id: u32, material: u32, indices: &[u32], u: [f32; 3], v: [f32; 3], out: &mut Vec<u8>) {
    words(&[id, 0, material, indices.len() as u32], out);
    words(indices, out);
    frame(
        [u[0] * DENSITY, u[1] * DENSITY, u[2] * DENSITY],
        [v[0] * DENSITY, v[1] * DENSITY, v[2] * DENSITY],
        out,
    );
    frame(
        [u[0] * LUXEL, u[1] * LUXEL, u[2] * LUXEL],
        [v[0] * LUXEL, v[1] * LUXEL, v[2] * LUXEL],
        out,
    );
}

/// Une cellule du couloir : un tronçon fermé, ouvert à ses deux bouts par un
/// portail.
///
/// Les huit sommets sont ceux d'une boîte ; les quatre faces latérales sont des
/// surfaces, les deux bouts sont des portails. La cellule n'est donc pas close
/// au sens du volume — c'est ce que les portails ferment, et c'est le modèle du
/// projet.
fn cell(id: u32, first_id: u32, from: f32, to: f32, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    words(&[id, 0, 8, 4, 2], &mut body);

    // Quatre coins à chaque bout : bas-gauche, bas-droite, haut-droite,
    // haut-gauche, vus depuis l'origine.
    for x in [from, to] {
        for (y, z) in [
            (-HALF, FLOOR_Z),
            (HALF, FLOOR_Z),
            (HALF, CEILING_Z),
            (-HALF, CEILING_Z),
        ] {
            floats(&[x, y, z], &mut body);
        }
    }

    // **Les quatre faces regardent l'intérieur**, ce que l'ordre des indices
    // décide : une face retournée disparaît au découpage, et le couloir rend
    // alors une image noire — vu en écrivant cette scène, avant de la figer.
    let along = [1.0, 0.0, 0.0];
    surface(
        first_id,
        FLOOR,
        &[0, 4, 5, 1],
        along,
        [0.0, 1.0, 0.0],
        &mut body,
    );
    surface(
        first_id + 1,
        FLOOR,
        &[3, 2, 6, 7],
        along,
        [0.0, 1.0, 0.0],
        &mut body,
    );
    surface(
        first_id + 2,
        WALLS,
        &[0, 3, 7, 4],
        along,
        [0.0, 0.0, 1.0],
        &mut body,
    );
    surface(
        first_id + 3,
        WALLS,
        &[1, 5, 6, 2],
        along,
        [0.0, 0.0, 1.0],
        &mut body,
    );

    // Les deux bouts, convexes comme un portail doit l'être.
    words(&[first_id + 4, 4], &mut body);
    words(&[0, 3, 2, 1], &mut body);
    words(&[first_id + 5, 4], &mut body);
    words(&[4, 5, 6, 7], &mut body);

    words(&[body.len() as u32], out);
    out.extend_from_slice(&body);
}

/// Le fichier du couloir : deux cellules qui se rejoignent par un portail.
///
/// Deux et non une : c'est le seul moyen de rendre visible que la soumission
/// dessine **toutes** les cellules, ce que la traversée de l'étape suivante
/// remplacera. Leurs portails du milieu partagent leurs sommets au bit près et
/// s'apparient donc au chargement — ce que l'image ne montre pas, mais que le
/// décodeur vérifie.
pub fn bytes() -> Vec<u8> {
    let mut cells = Vec::new();
    cell(1, 10, START, START + LENGTH, &mut cells);
    cell(2, 20, START + LENGTH, START + LENGTH * 2.0, &mut cells);

    let mut materials = Vec::new();
    for (id, name) in [(WALLS, "mur"), (FLOOR, "sol")] {
        words(&[id], &mut materials);
        materials.extend_from_slice(&(name.len() as u16).to_le_bytes());
        materials.extend_from_slice(name.as_bytes());
    }

    file(&cells, &materials)
}

/// Assemble un fichier de carte à partir de ses deux sections.
fn file(cells: &[u8], materials: &[u8]) -> Vec<u8> {
    let sections = [(*b"CELL", cells), (*b"MATS", materials)];
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
