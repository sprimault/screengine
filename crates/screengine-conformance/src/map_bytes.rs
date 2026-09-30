// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que les générateurs de cartes écrivent tous de la même façon.
//!
//! Extrait le jour où une troisième carte est apparue, et pas avant : les deux
//! premières écrivaient ces primitives chacune de son côté, à l'expression près
//! les mêmes. Ce qui est ici sort donc **inchangé au bit près**, et c'est ce qui
//! permet aux deux fichiers versionnés — `hosts/couloir.world` et
//! `hosts/salles.world` — de rester identiques à l'octet.
//!
//! **Les densités de plaquage y sont, et ce n'est pas de la plomberie.** Elles
//! suivent celles des hôtes — 256 texels par unité de monde aux murs, 128 au
//! sol —, et une carte qui en choisirait d'autres rendrait un damier dont les
//! cases n'auraient plus la taille que le décor annonce. Une troisième carte qui
//! les recopierait au jugé finirait par en dériver.

/// Le rang du matériau des murs dans la table.
pub const WALLS: u32 = 1;

/// Le rang du matériau du sol et du plafond.
pub const FLOOR: u32 = 2;

/// Texels par unité de monde sur un mur.
pub const WALL_DENSITY: f32 = 256.0;

/// Texels par unité de monde au sol et au plafond.
pub const FLOOR_DENSITY: f32 = 128.0;

/// Luxels par unité de monde, pour tous les repères de lightmap.
///
/// Un luxel par unité : c'est le pas qui garde les étendues sous le plafond de
/// 256 luxels par côté sur des cellules de cette taille, et il est le même
/// partout pour que deux surfaces coplanaires adjacentes alignent leurs grilles.
pub const LUXEL: f32 = 1.0;

/// Écrit des flottants en petit-boutiste.
pub fn floats(values: &[f32], out: &mut Vec<u8>) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// Écrit des entiers en petit-boutiste.
pub fn words(values: &[u32], out: &mut Vec<u8>) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// Écrit un repère : origine à l'origine du monde, puis ses deux axes.
///
/// L'origine est nulle parce que le format ne l'exige pas dans le plan de la
/// surface — un éditeur pose un repère par matériau et le partage —, et parce
/// qu'un nœud de grille à l'origine du monde aligne toutes les surfaces sans rien
/// avoir à calculer.
pub fn frame(u: [f32; 3], v: [f32; 3], out: &mut Vec<u8>) {
    floats(&[0.0, 0.0, 0.0], out);
    floats(&u, out);
    floats(&v, out);
}

/// Écrit une surface : son en-tête, ses indices, puis ses deux repères.
///
/// Le repère de texture prend la densité de son matériau, celui de lightmap
/// prend [`LUXEL`]. Les deux sont indépendants, et c'est leur raison d'être : une
/// surface répète sa texture plusieurs fois et étire sa lightmap une seule fois
/// sur toute son étendue.
pub fn surface(
    id: u32,
    material: u32,
    indices: &[u32],
    u: [f32; 3],
    v: [f32; 3],
    out: &mut Vec<u8>,
) {
    flagged(id, 0, material, indices, u, v, out);
}

/// La même, avec les drapeaux de la surface.
///
/// Séparée parce qu'un seul décor en pose : celui de la collision, qui a besoin
/// d'une surface **non solide** pour donner son premier lecteur au drapeau. Les
/// deux autres cartes n'écrivent que des zéros, et leur appel n'a pas à porter un
/// argument qu'elles ne remplissent jamais.
pub fn flagged(
    id: u32,
    flags: u32,
    material: u32,
    indices: &[u32],
    u: [f32; 3],
    v: [f32; 3],
    out: &mut Vec<u8>,
) {
    let density = if material == WALLS {
        WALL_DENSITY
    } else {
        FLOOR_DENSITY
    };
    words(&[id, flags, material, indices.len() as u32], out);
    words(indices, out);
    frame(
        [u[0] * density, u[1] * density, u[2] * density],
        [v[0] * density, v[1] * density, v[2] * density],
        out,
    );
    frame(
        [u[0] * LUXEL, u[1] * LUXEL, u[2] * LUXEL],
        [v[0] * LUXEL, v[1] * LUXEL, v[2] * LUXEL],
        out,
    );
}

// **Ce qui n'est pas ici, et pourquoi.** Chaque générateur garde son assemblage
// de fichier : l'un porte une section de lumières que l'autre n'a pas, et leurs
// deux fonctions n'écrivent donc pas la même table de sections. Les fondre
// demanderait d'arbitrer une forme commune, c'est-à-dire de risquer un octet sur
// deux fichiers versionnés — pour une vingtaine de lignes. Ce module ne porte
// que ce qui est **prouvé identique** des deux côtés.
