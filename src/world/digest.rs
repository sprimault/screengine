// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! L'empreinte qui dit si une entrée de cache vaut encore.
//!
//! **Ce qu'elle couvre est exactement ce que la cuisson consomme**, ni plus ni
//! moins, et les deux bornes coûtent. Trop peu, et un cache valide rend une image
//! que la version suivante ne rend plus — l'erreur la plus coûteuse, parce qu'elle
//! se découvre en regardant une capture et non en lisant un message. Trop, et
//! réhabiller un décor périme un niveau entier de lightmaps, ce qui rend le cache
//! inutile là où il sert le plus, dans un éditeur.
//!
//! La liste des champs et leur ordre font foi dans `docs/rust.md`, section « Le
//! cache de lightmaps ». Ce module les applique et n'en décide rien.

use alloc::vec::Vec;

use crate::buffer::reserved;
use crate::error::Result;
use crate::format::World;
use crate::format::world::{Cell, Surface};
use crate::math::Vec3;

use super::bake::{REVISION, retained};

/// Le hacheur : FNV-1a 64 bits, sur des octets petit-boutistes.
///
/// **Aucune opération flottante n'entre dans le calcul** — `to_bits` donne les
/// octets, et deux cibles hachent donc la même chose. Une comparaison flottante
/// suffirait à faire diverger deux empreintes sur `-0.0` et `0.0`, qui sont égaux
/// et n'ont pas les mêmes bits ; ici c'est l'inverse qui est voulu, puisque la
/// cuisson les distingue.
struct Digest(u64);

impl Digest {
    /// Le hacheur, à son état initial.
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    /// Absorbe un octet.
    fn byte(&mut self, value: u8) {
        self.0 = (self.0 ^ value as u64).wrapping_mul(0x0100_0000_01b3);
    }

    /// Absorbe un entier de trente-deux bits.
    fn u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    /// Absorbe un flottant par ses bits.
    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    /// Absorbe un point.
    fn point(&mut self, value: Vec3) {
        self.f32(value.x);
        self.f32(value.y);
        self.f32(value.z);
    }

    /// L'empreinte.
    fn finish(self) -> u64 {
        self.0
    }
}

/// L'empreinte de la lightmap d'une cellule, par son rang.
///
/// Elle est **prouvée suffisante** par la portée de l'ensemble d'occulteurs : rien
/// hors de la cellule et de ses voisines à un portail ne contribue à sa cuisson,
/// parce que le bord de cette région est refermé. Un changement au-delà ne peut
/// donc pas changer ses luxels, et c'est ce qui autorise à ne pas hacher la carte
/// entière — ce qui ferait périmer un niveau au premier réglage d'une torche à
/// l'autre bout.
pub(crate) fn fingerprint(world: &World, index: u32) -> Result<u64> {
    let cells = world.cells();
    let cell = &cells[index as usize];

    let mut digest = Digest::new();
    // La révision d'abord, parce qu'elle périme tout le reste : un cache écrit par
    // une version dont le calcul a changé n'a pas à être comparé champ par champ.
    digest.u32(REVISION);
    absorb_cell(&mut digest, cells, cell);

    // **Les voisines par identifiant croissant, et non dans l'ordre des portails.**
    // Deux cartes qui décrivent la même géométrie en rangeant autrement les
    // portails d'une cellule donneraient sinon deux empreintes, et le cache d'un
    // éditeur périmerait à la première réécriture de son fichier.
    let mut neighbours = neighbours_of(world, cell)?;
    neighbours.sort_unstable();
    for id in neighbours {
        let Some(rank) = world.cell_of(id) else {
            continue;
        };
        absorb_cell(&mut digest, cells, &cells[rank as usize]);
    }

    // Les lumières que la cuisson retiendrait, dans l'ordre de leur section : la
    // même fonction que le calcul, pour que les deux ne puissent pas diverger.
    for light in retained(world, cell)? {
        digest.point(light.position);
        digest.f32(light.radius);
        for channel in [light.color.r, light.color.g, light.color.b, light.color.a] {
            digest.byte(channel);
        }
    }

    Ok(digest.finish())
}

/// Les identifiants des cellules qu'un portail apparié rejoint.
fn neighbours_of(world: &World, cell: &Cell) -> Result<Vec<u32>> {
    let cells = world.cells();
    let mut out = reserved(cell.portals.len())?;
    for portal in &cell.portals {
        if let Some((next, _)) = portal.link {
            out.push(cells[next as usize].id());
        }
    }
    Ok(out)
}

/// Absorbe tout ce qu'une cellule apporte au calcul.
///
/// **Le matériau et le repère de texture n'y sont pas**, et c'est la clause qui
/// rend le cache utilisable : réhabiller un décor ne périme aucune lightmap. Les
/// triangles non plus — ils sont dérivés des coins par la découpe d'oreilles, qui
/// est déterministe, donc les hacher reviendrait à hacher deux fois la même chose.
fn absorb_cell(digest: &mut Digest, cells: &[Cell], cell: &Cell) {
    digest.u32(cell.id());
    digest.u32(cell.flags());
    for vertex in &cell.vertices {
        digest.point(vertex.position);
    }
    for surface in &cell.surfaces {
        absorb_surface(digest, surface);
    }
    for portal in &cell.portals {
        digest.u32(portal.id());
        for point in &portal.points {
            digest.point(*point);
        }
        // **L'identifiant de la cellule liée, jamais son rang.** Le rang est une
        // valeur dérivée du chargement : supprimer une salle ailleurs dans le
        // fichier décale tous ceux qui la suivent, et périmerait des entrées que
        // rien n'a touchées. Zéro pour un mur, qu'aucune cellule ne porte.
        digest.u32(match portal.link {
            Some((next, _)) => cells[next as usize].id(),
            None => 0,
        });
    }
}

/// Absorbe ce qu'une surface apporte : ce qui décide de ses luxels.
fn absorb_surface(digest: &mut Digest, surface: &Surface) {
    digest.u32(surface.id());
    digest.u32(surface.flags());
    for index in &surface.corners {
        digest.u32(*index);
    }
    digest.point(surface.lightmap.origin);
    digest.point(surface.lightmap.u);
    digest.point(surface.lightmap.v);
}

#[cfg(test)]
mod tests;
