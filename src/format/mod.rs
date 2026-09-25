// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le socle de décodage des formats de fichier.
//!
//! Le maillage et la carte partagent une signature, un en-tête de vingt octets,
//! une table de sections et ce décodeur ; chacun garde son propre numéro de
//! version, pour qu'un champ de cellule qui bouge ne fasse pas migrer les
//! maillages. Les dispositions font foi dans `docs/rust.md`, section « Formats
//! de fichier » : ce module les applique et n'en décide rien.
//!
//! **Que `(ptr, len)` couvre bien `len` octets lisibles est une précondition de
//! la frontière ; tout ce qui est à l'intérieur de ces octets est hostile, sans
//! exception.** Un bloc malformé rend une erreur et ne panique pas, même en
//! débogage : aucune lecture n'indexe le bloc, et aucune capacité d'allocation
//! ne vient d'un nombre déclaré.

mod cursor;
mod mesh;

pub(crate) use cursor::Cursor;
pub use mesh::Mesh;

use crate::error::{Error, Malformation, Result};

/// Les quatre octets que porte en tête tout fichier du projet.
///
/// `0x1a` est le marqueur de fin de fichier des systèmes de l'époque : il ne
/// coûte rien et attrape un fichier transféré en mode texte, dont les fins de
/// ligne auraient été converties.
const SIGNATURE: [u8; 4] = *b"SCG\x1a";

/// Longueur d'une entrée de la table de sections, en octets.
const ENTRY_LEN: usize = 12;

/// Les sections d'un bloc, rangées dans l'ordre de `tags`.
///
/// `kind` est le genre que l'appelant attend, `version` la seule version de
/// format qu'il lit, et `tags` les genres de sections de ce format, croissants.
/// Une section absente rend une tranche vide : une section de longueur nulle est
/// légitime, un maillage sans triangle l'étant aussi.
///
/// **L'ordre des contrôles est contractuel sur un point** : la version passe
/// immédiatement après la signature et le genre, avant tout contrôle de
/// structure. Sinon un fichier écrit par une version plus récente remonte
/// « format invalide », et l'intégrateur cherche une corruption qui n'existe
/// pas.
pub(crate) fn decode<'a, const N: usize>(
    bytes: &'a [u8],
    kind: [u8; 4],
    version: u32,
    tags: [[u8; 4]; N],
) -> Result<[&'a [u8]; N]> {
    debug_assert!(
        tags.windows(2).all(|pair| pair[0] < pair[1]),
        "les genres de sections d'un format sont croissants et distincts"
    );

    let mut cursor = Cursor::new(bytes);
    if cursor.tag()? != SIGNATURE {
        return Err(Error::InvalidFormat(Malformation::Signature));
    }
    if cursor.tag()? != kind {
        return Err(Error::InvalidFormat(Malformation::Kind));
    }
    if cursor.u32()? != version {
        return Err(Error::UnsupportedFormatVersion);
    }
    if cursor.u32()? as usize != bytes.len() {
        return Err(Error::InvalidFormat(Malformation::Length));
    }

    // La table se prélève avant les sections, ce qui place le curseur
    // exactement au premier octet que la première section doit occuper : le
    // pavage se vérifie ensuite en comparant chaque décalage annoncé à la
    // position courante, sans jamais l'additionner.
    let count = cursor.u32()? as usize;
    let table = count
        .checked_mul(ENTRY_LEN)
        .ok_or(Error::InvalidFormat(Malformation::Truncated))?;
    let mut entries = Cursor::new(cursor.take(table)?);

    let mut sections: [&'a [u8]; N] = [&[]; N];
    let mut previous = None;
    for _ in 0..count {
        let tag = entries.tag()?;
        let offset = entries.u32()? as usize;
        let length = entries.u32()? as usize;

        let rank = tags
            .iter()
            .position(|known| *known == tag)
            .ok_or(Error::InvalidFormat(Malformation::SectionKind))?;
        if previous.is_some_and(|last| rank <= last) {
            return Err(Error::InvalidFormat(Malformation::SectionOrder));
        }
        if offset != cursor.offset() {
            return Err(Error::InvalidFormat(Malformation::SectionBounds));
        }
        sections[rank] = cursor.take(length)?;
        previous = Some(rank);
    }
    if cursor.remaining() != 0 {
        return Err(Error::InvalidFormat(Malformation::SectionBounds));
    }

    Ok(sections)
}

#[cfg(test)]
mod tests;
