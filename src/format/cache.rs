// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le cache de lightmaps : troisième genre du conteneur commun.
//!
//! **L'écriture et la lecture sont dans le même module, à quelques lignes l'une de
//! l'autre.** Séparées, deux dispositions finissent par diverger d'un champ, et le
//! symptôme est un cache que le moteur relit de travers sans rien signaler — le
//! décalage tombe dans un rectangle d'atlas, qui est valide quoi qu'il contienne.
//!
//! La disposition fait foi dans `docs/rust.md`, section « Le cache de lightmaps ».
//! Ce module l'applique et n'en décide rien.
//!
//! **Le moteur est le seul écrivain**, ce qui rend la canonicité gratuite : les
//! enregistrements sont triés par identifiant de cellule, uniques, et pavent leur
//! section. Deux caches du même état sont donc comparables octet pour octet, et
//! c'est ce qui permet à un test de comparer des blocs plutôt que des champs.
//!
//! Ce qui vient d'un bloc reste hostile, comme pour les deux autres genres : aucune
//! lecture n'indexe le bloc, et aucune capacité d'allocation ne vient d'un nombre
//! déclaré.

use alloc::vec::Vec;

use crate::buffer::reserved;
use crate::error::{Argument, Error, Malformation, Result};
use crate::format::world::Cell;
use crate::world::atlas::{Atlas, Slot};

use super::{Cursor, decode};

/// Le genre que porte l'en-tête d'un cache.
const KIND: [u8; 4] = *b"LMAP";

/// La version de disposition que ce module lit et écrit.
///
/// Propre au cache : un champ de cellule qui bouge ne fait pas migrer les
/// maillages, et réciproquement.
const VERSION: u32 = 1;

/// Le genre de la section des enregistrements.
const RECORDS: [u8; 4] = *b"CELL";

/// Celui de la section des luxels.
const LUXELS: [u8; 4] = *b"LUXL";

/// Longueur de l'en-tête, signature comprise.
const HEADER_LEN: usize = 20;

/// Longueur d'une entrée de la table de sections.
const ENTRY_LEN: usize = 12;

/// Ce qu'un enregistrement porte avant la liste de ses rectangles.
const RECORD_HEAD: usize = 4 + 8 + 4 + 4 + 4 + 4;

/// Ce qu'un rectangle occupe : son identifiant de surface, puis ses quatre bornes.
const SLOT_LEN: usize = 4 + 4 * 4;

/// Ce qu'une cellule cuite dépose dans le cache.
pub(crate) struct Record<'a> {
    /// La cellule, pour son identifiant et ceux de ses surfaces.
    pub(crate) cell: &'a Cell,
    /// Son empreinte au moment du calcul.
    pub(crate) fingerprint: u64,
    /// Le rangement qui a produit ses luxels.
    pub(crate) atlas: &'a Atlas,
    /// Le niveau zéro de son atlas, un mot par luxel.
    pub(crate) texels: &'a [u32],
}

/// Ce qu'une entrée relue rend, avant qu'on décide de la garder.
pub(crate) struct Entry<'a> {
    /// L'identifiant de la cellule qu'elle prétend porter.
    pub(crate) cell_id: u32,
    /// L'empreinte qui décide si elle vaut encore.
    pub(crate) fingerprint: u64,
    /// Le rangement relu.
    pub(crate) atlas: Atlas,
    /// Les identifiants de surface, dans l'ordre des rectangles.
    pub(crate) surfaces: Vec<u32>,
    /// Ses luxels, quatre octets chacun, tels que le constructeur de texture les
    /// attend.
    pub(crate) texels: &'a [u8],
}

/// La longueur qu'un bloc occuperait, sans rien sérialiser.
///
/// **Analytique, et c'est une clause de l'ABI** : l'hôte mesure, alloue, puis
/// remplit, et la mesure ne doit pas coûter une sérialisation jetée. `None` quand
/// le total déborde un `u32`, que l'en-tête ne saurait pas porter.
pub(crate) fn measure(records: &[Record<'_>]) -> Option<usize> {
    let mut total = HEADER_LEN + 2 * ENTRY_LEN;
    for record in records {
        total = total.checked_add(4 + RECORD_HEAD + record.atlas.slots.len() * SLOT_LEN)?;
        total = total.checked_add(record.texels.len().checked_mul(4)?)?;
    }
    (total <= u32::MAX as usize).then_some(total)
}

/// Écrit le bloc dans le tampon de l'hôte.
///
/// `out` doit faire exactement la longueur que [`measure`] annonce : un tampon plus
/// court est refusé avant la première écriture, et un tampon plus long ferait mentir
/// la longueur que l'en-tête porte.
pub(crate) fn write(records: &[Record<'_>], out: &mut [u8]) -> Result<()> {
    let total = measure(records).ok_or(Error::InvalidArgument(Argument::BufferLength))?;
    if out.len() != total {
        return Err(Error::InvalidArgument(Argument::BufferLength));
    }

    let body: usize = records
        .iter()
        .map(|record| 4 + RECORD_HEAD + record.atlas.slots.len() * SLOT_LEN)
        .sum();
    let first = HEADER_LEN + 2 * ENTRY_LEN;

    let mut out = Writer::new(out);
    out.tag(super::SIGNATURE);
    out.tag(KIND);
    out.u32(VERSION);
    out.u32(total as u32);
    out.u32(2);

    out.tag(RECORDS);
    out.u32(first as u32);
    out.u32(body as u32);
    out.tag(LUXELS);
    out.u32((first + body) as u32);
    out.u32((total - first - body) as u32);

    let mut luxels = 0u32;
    for record in records {
        let length = record.texels.len() as u32 * 4;
        out.u32((RECORD_HEAD + record.atlas.slots.len() * SLOT_LEN) as u32);
        out.u32(record.cell.id());
        out.u32(record.fingerprint as u32);
        out.u32((record.fingerprint >> 32) as u32);
        out.u32(record.atlas.side);
        out.u32(luxels);
        out.u32(length);
        out.u32(record.atlas.slots.len() as u32);
        for (surface, slot) in record.cell.surfaces.iter().zip(&record.atlas.slots) {
            out.u32(surface.id());
            out.u32(slot.x);
            out.u32(slot.y);
            out.u32(slot.width);
            out.u32(slot.height);
        }
        luxels += length;
    }

    for record in records {
        for texel in record.texels {
            out.u32(*texel);
        }
    }
    Ok(())
}

/// Relit un bloc et rend ses entrées, dans l'ordre où il les porte.
///
/// Refuse ce qui n'est pas une disposition valide ; **ne juge aucune empreinte**,
/// qui est l'affaire de l'appelant : une entrée périmée n'est pas un bloc malformé,
/// et confondre les deux ferait échouer la reprise d'un cache normal d'éditeur.
pub(crate) fn read(bytes: &[u8]) -> Result<Vec<Entry<'_>>> {
    let [records, luxels] = decode(bytes, KIND, VERSION, [RECORDS, LUXELS])?;

    let mut out: Vec<Entry<'_>> = reserved(0)?;
    let mut cursor = Cursor::new(records);
    let mut previous: Option<u32> = None;
    while cursor.remaining() != 0 {
        let length = cursor.u32()? as usize;
        let mut record = Cursor::new(cursor.take(length)?);

        let cell_id = record.u32()?;
        // **Triés et uniques**, ce que le moteur garantit en écrivant : une entrée
        // hors d'ordre vient donc d'ailleurs, et rien ne dit ce que porte le reste.
        if previous.is_some_and(|last| cell_id <= last) {
            return Err(Error::InvalidFormat(Malformation::SectionOrder));
        }
        previous = Some(cell_id);

        let low = record.u32()? as u64;
        let high = record.u32()? as u64;
        let side = record.u32()?;
        let offset = record.u32()? as usize;
        let span = record.u32()? as usize;
        let count = record.u32()? as usize;

        // **Tout produit porte sur des nombres que le bloc a écrits**, donc aucun
        // ne se fait sans contrôle : un côté de quatre milliards déborde un `usize`
        // avant d'avoir été comparé à quoi que ce soit.
        let listed = count
            .checked_mul(SLOT_LEN)
            .ok_or(Error::InvalidFormat(Malformation::Truncated))?;
        if record.remaining() != listed {
            return Err(Error::InvalidFormat(Malformation::Truncated));
        }
        // La longueur annoncée décide de ce qu'on lit dans l'autre section, donc
        // elle se confronte au côté plutôt que d'être crue : un `side` et un `span`
        // qui ne se répondent pas laisseraient lire un rectangle en dehors.
        let expected = (side as usize)
            .checked_mul(side as usize)
            .and_then(|area| area.checked_mul(4))
            .ok_or(Error::InvalidFormat(Malformation::SectionBounds))?;
        if expected != span {
            return Err(Error::InvalidFormat(Malformation::SectionBounds));
        }
        let end = offset
            .checked_add(span)
            .ok_or(Error::InvalidFormat(Malformation::SectionBounds))?;
        let texels = luxels
            .get(offset..end)
            .ok_or(Error::InvalidFormat(Malformation::SectionBounds))?;

        let mut surfaces = reserved(count)?;
        let mut slots = reserved(count)?;
        for _ in 0..count {
            surfaces.push(record.u32()?);
            slots.push(Slot {
                x: record.u32()?,
                y: record.u32()?,
                width: record.u32()?,
                height: record.u32()?,
            });
        }

        out.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        out.push(Entry {
            cell_id,
            fingerprint: low | (high << 32),
            atlas: Atlas { side, slots },
            surfaces,
            texels,
        });
    }
    Ok(out)
}

/// Un curseur d'écriture, qui n'écrit jamais plus que sa tranche.
///
/// Sans contrôle par écriture : la tranche a été mesurée par [`measure`] et
/// comparée, et un dépassement serait donc un défaut de ce module, que
/// l'indexation fait remonter en panique de débogage plutôt qu'en octets faux.
struct Writer<'a> {
    bytes: &'a mut [u8],
    offset: usize,
}

impl<'a> Writer<'a> {
    /// Un curseur au début de la tranche.
    fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    /// Écrit quatre octets bruts.
    fn tag(&mut self, value: [u8; 4]) {
        self.bytes[self.offset..self.offset + 4].copy_from_slice(&value);
        self.offset += 4;
    }

    /// Écrit un entier de trente-deux bits, petit-boutiste.
    fn u32(&mut self, value: u32) {
        self.tag(value.to_le_bytes());
    }
}

#[cfg(test)]
mod tests;
