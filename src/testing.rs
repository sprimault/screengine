// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que les tests du noyau partagent.

use alloc::vec::Vec;

/// FNV-1a 64 bits, l'empreinte de la conformance, sur une suite d'octets.
pub(crate) fn fnv1a(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Le générateur des tests aléatoires : xorshift64*, écrit ici parce que le
/// noyau n'a aucune dépendance, et parce qu'un échec qui ne se rejoue pas
/// n'a pas été trouvé.
pub(crate) struct Rng(u64);

impl Rng {
    /// La graine ne peut pas être nulle : la suite y resterait bloquée.
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    /// L'entier suivant.
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Un `f32` dans [0, 1), par les 24 bits de poids fort : la conversion est
    /// exacte.
    pub(crate) fn unit_f32(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u32 << 24) as f32
    }

    /// Un entier entre `lo` et `hi`, bornes comprises.
    pub(crate) fn coord(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }
}

/// Un sommet de maillage, écrit en octets.
pub(crate) fn vertex_bytes(x: f32, y: f32, z: f32, u: f32, v: f32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [x, y, z, u, v] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Un triangle de maillage : trois indices puis quatre composantes de couleur.
pub(crate) fn triangle_bytes(i0: u32, i1: u32, i2: u32, color: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in [i0, i1, i2] {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes.extend_from_slice(&color);
    bytes
}

/// Un groupe de surface : identifiant, premier triangle, compte, emplacement.
pub(crate) fn group_bytes(id: u32, first: u32, count: u32, slot: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [id, first, count, slot] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Un nom d'emplacement : sa longueur en deux octets, puis ses octets.
pub(crate) fn name_bytes(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(text.len() as u16).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// Une section de trames : le compte, puis les poses données telles quelles.
pub(crate) fn frames_bytes(count: u32, poses: &[u8]) -> Vec<u8> {
    let mut bytes = count.to_le_bytes().to_vec();
    bytes.extend_from_slice(poses);
    bytes
}

/// Une pose de sommet : sa position, puis sa normale.
pub(crate) fn pose_bytes(position: [f32; 3], normal: [f32; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in position.into_iter().chain(normal) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Un fichier de maillage bien formé, à partir de ses cinq sections.
///
/// **La disposition est écrite ici une seconde fois**, à la main : en-tête de
/// vingt octets, douze par entrée de table, sections par genre croissant. C'est
/// ce qui fait rougir un désaccord entre l'écrivain et le lecteur, là où un
/// fichier produit par le décodeur rendrait le test tautologique. Une section
/// vide n'entre pas dans la table.
/// Un fichier de maillage à partir de sommets écrits « x, y, z, u, v ».
///
/// **Les tests décrivent un sommet comme un point habillé**, et c'est leur
/// intention ; le format, lui, sépare ce qui anime de ce qui n'anime pas. Le
/// découpage est fait ici, une fois, plutôt que dans chacun des essais — et la
/// disposition reste écrite une seconde fois, ce qui est tout ce que la
/// doctrine demande.
///
/// La normale vaut `+Z` partout : aucune passe ne la lit à cette étape, et une
/// normale nulle serait un vecteur sans direction, que le format refusera le
/// jour où il la vérifiera.
pub(crate) fn mesh_file(surf: &[u8], texn: &[u8], tris: &[u8], vtxs: &[u8]) -> Vec<u8> {
    let mut poses = Vec::new();
    let mut uvs = Vec::new();
    for sommet in vtxs.chunks_exact(20) {
        poses.extend_from_slice(&sommet[..12]);
        poses.extend_from_slice(&pose_normal());
        uvs.extend_from_slice(&sommet[12..]);
    }
    let frms = if vtxs.is_empty() {
        Vec::new()
    } else {
        frames_bytes(1, &poses)
    };
    mesh_sections(&frms, surf, texn, tris, &uvs)
}

/// La normale que les fichiers de test portent, en octets.
fn pose_normal() -> [u8; 12] {
    let mut bytes = [0u8; 12];
    bytes[8..].copy_from_slice(&1.0f32.to_le_bytes());
    bytes
}

/// Le même, section par section, pour les essais qui les composent eux-mêmes.
pub(crate) fn mesh_sections(
    frms: &[u8],
    surf: &[u8],
    texn: &[u8],
    tris: &[u8],
    vtxs: &[u8],
) -> Vec<u8> {
    let sections: Vec<([u8; 4], &[u8])> = [
        (*b"FRMS", frms),
        (*b"SURF", surf),
        (*b"TEXN", texn),
        (*b"TRIS", tris),
        (*b"VTXS", vtxs),
    ]
    .into_iter()
    .filter(|(_, body)| !body.is_empty())
    .collect();

    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"MESH");
    bytes.extend_from_slice(&2u32.to_le_bytes());
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
