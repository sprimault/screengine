// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le fichier de maillage que la scène chargée rend.
//!
//! **La disposition est écrite ici à la main**, octet par octet, comme elle
//! l'est dans les tests du noyau et dans ceux de la frontière. Les trois
//! écritures sont voulues et ne se factorisent pas : elles vivent dans trois
//! crates, et un constructeur commun ferait éprouver le décodeur, la frontière
//! et le rendu avec les octets d'un seul écrivain — ce qui prouverait qu'ils
//! s'accordent, jamais que l'un d'eux est juste.
//!
//! Ce que le fichier porte est fixé par `docs/rust.md`, section « Formats de
//! fichier ».

/// Le demi-côté de la caisse, en unités de monde.
const HALF: f32 = 1.0;

/// Le côté de la texture des faces latérales, en texels.
///
/// Les coordonnées d'une face vont de zéro à cette valeur : le damier s'y
/// applique une fois par face, sans répétition, ce qui rend un décalage d'un
/// texel visible sur l'arête plutôt que noyé dans une redite.
pub const SIDE_TEXELS: f32 = 64.0;

/// L'identifiant du groupe des faces latérales, tel que l'éditeur l'aurait
/// attribué.
const SIDES_ID: u32 = 11;

/// L'identifiant du groupe des faces horizontales.
const CAPS_ID: u32 = 12;

/// La couleur des faces latérales, ignorée puisqu'elles portent une texture.
const SIDE_TINT: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];

/// La couleur des faces horizontales, qui décide de leur teinte : leur
/// emplacement ne reçoit aucune texture.
const CAP_TINT: [u8; 4] = [0x50, 0x80, 0xC8, 0xFF];

/// Une face : sa normale, et les deux axes dont le produit vectoriel la rend.
///
/// L'ordre `T`, `B` n'est pas décoratif : les quatre coins `−T−B`, `+T−B`,
/// `+T+B`, `−T+B` sont alors antihoraires vus de l'extérieur, ce qui est
/// l'enroulement que le moteur garde. Une paire écrite dans l'autre sens rend la
/// face invisible, et la caisse se lirait comme un défaut du décodeur.
struct Face {
    /// La normale sortante.
    normal: [f32; 3],
    /// Le premier axe du plan.
    t: [f32; 3],
    /// Le second axe, tel que `t × b == normal`.
    b: [f32; 3],
}

/// Les quatre faces latérales, dans l'ordre du fichier.
const SIDES: [Face; 4] = [
    Face {
        normal: [1.0, 0.0, 0.0],
        t: [0.0, 1.0, 0.0],
        b: [0.0, 0.0, 1.0],
    },
    Face {
        normal: [-1.0, 0.0, 0.0],
        t: [0.0, 0.0, 1.0],
        b: [0.0, 1.0, 0.0],
    },
    Face {
        normal: [0.0, 1.0, 0.0],
        t: [0.0, 0.0, 1.0],
        b: [1.0, 0.0, 0.0],
    },
    Face {
        normal: [0.0, -1.0, 0.0],
        t: [1.0, 0.0, 0.0],
        b: [0.0, 0.0, 1.0],
    },
];

/// Le dessus et le dessous.
const CAPS: [Face; 2] = [
    Face {
        normal: [0.0, 0.0, 1.0],
        t: [1.0, 0.0, 0.0],
        b: [0.0, 1.0, 0.0],
    },
    Face {
        normal: [0.0, 0.0, -1.0],
        t: [0.0, 1.0, 0.0],
        b: [1.0, 0.0, 0.0],
    },
];

/// Les octets d'un `f32`, petit-boutistes comme le format l'impose.
fn f32_bytes(value: f32, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Les quatre coins d'une face, et leurs coordonnées de texture.
fn corners(face: &Face) -> [([f32; 3], (f32, f32)); 4] {
    let point = |ts: f32, bs: f32| {
        let mut p = [0.0; 3];
        for (axis, coordinate) in p.iter_mut().enumerate() {
            *coordinate = (face.normal[axis] + ts * face.t[axis] + bs * face.b[axis]) * HALF;
        }
        p
    };
    [
        (point(-1.0, -1.0), (0.0, 0.0)),
        (point(1.0, -1.0), (SIDE_TEXELS, 0.0)),
        (point(1.0, 1.0), (SIDE_TEXELS, SIDE_TEXELS)),
        (point(-1.0, 1.0), (0.0, SIDE_TEXELS)),
    ]
}

/// Le fichier de la caisse : six faces, deux groupes, deux emplacements.
///
/// Les faces latérales forment le premier groupe et réclament l'emplacement
/// zéro ; le dessus et le dessous forment le second et réclament le premier
/// emplacement, auquel la scène ne lie aucune texture — c'est ainsi qu'un groupe
/// se dessine à la couleur de ses triangles.
pub fn crate_mesh() -> Vec<u8> {
    let faces: Vec<&Face> = SIDES.iter().chain(CAPS.iter()).collect();

    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for (index, face) in faces.iter().enumerate() {
        let first = (index * 4) as u32;
        for (position, (u, v)) in corners(face) {
            for value in position {
                f32_bytes(value, &mut vertices);
            }
            f32_bytes(u, &mut vertices);
            f32_bytes(v, &mut vertices);
        }

        let tint = if index < SIDES.len() {
            SIDE_TINT
        } else {
            CAP_TINT
        };
        for [a, b, c] in [[0u32, 1, 2], [0, 2, 3]] {
            for corner in [first + a, first + b, first + c] {
                triangles.extend_from_slice(&corner.to_le_bytes());
            }
            triangles.extend_from_slice(&tint);
        }
    }

    let sides_triangles = (SIDES.len() * 2) as u32;
    let caps_triangles = (CAPS.len() * 2) as u32;
    let mut groups = Vec::new();
    for (id, first, count, slot) in [
        (SIDES_ID, 0, sides_triangles, 0),
        (CAPS_ID, sides_triangles, caps_triangles, 1),
    ] {
        for value in [id, first, count, slot] {
            groups.extend_from_slice(&value.to_le_bytes());
        }
    }

    let mut names = Vec::new();
    for name in ["cote", "chapeau"] {
        names.extend_from_slice(&(name.len() as u16).to_le_bytes());
        names.extend_from_slice(name.as_bytes());
    }

    file(&groups, &names, &triangles, &vertices)
}

/// Assemble un fichier de maillage à partir de ses quatre sections.
///
/// En-tête de vingt octets — signature, genre, version, longueur totale, nombre
/// de sections —, puis douze octets par entrée de table, puis les sections par
/// genre croissant, pavant le fichier.
fn file(surf: &[u8], texn: &[u8], tris: &[u8], vtxs: &[u8]) -> Vec<u8> {
    let sections = [
        (*b"SURF", surf),
        (*b"TEXN", texn),
        (*b"TRIS", tris),
        (*b"VTXS", vtxs),
    ];
    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"MESH");
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
