// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La lecture bornée d'un bloc de données.

use crate::error::{Error, Malformation, Result};

/// Un curseur borné sur le bloc reçu de l'hôte.
///
/// Toutes les lectures du décodeur passent par lui, et aucune n'indexe le bloc :
/// une panique sur un fichier malformé devient alors impossible par
/// construction, au lieu de dépendre de l'attention du relecteur sur chaque
/// borne.
///
/// Aucun alignement n'est exigé du bloc, qui peut venir d'une lecture, d'une
/// projection à un décalage quelconque ou d'une entrée d'archive : chaque champ
/// se lit octet par octet, et jamais par réinterprétation d'une tranche en
/// tranche de mots.
pub(crate) struct Cursor<'a> {
    /// Le bloc, dont la longueur est la seule borne.
    bytes: &'a [u8],
    /// Ce qui a déjà été lu, jamais au-delà de la longueur du bloc.
    offset: usize,
}

impl<'a> Cursor<'a> {
    /// Un curseur au début d'un bloc.
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    /// Ce qui reste à lire, en octets.
    ///
    /// Une soustraction, et c'est ce qui fait tenir tout le reste sans
    /// arithmétique vérifiée : `offset` ne dépasse jamais la longueur du bloc,
    /// alors qu'un `offset + n` comparé à cette longueur déborderait sur une
    /// cible 32 bits dès que `n` vient du fichier.
    pub(crate) const fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    /// Où en est la lecture dans le bloc.
    pub(crate) const fn offset(&self) -> usize {
        self.offset
    }

    /// Les `len` prochains octets.
    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.remaining() < len {
            return Err(Error::InvalidFormat(Malformation::Truncated));
        }
        let end = self.offset + len;
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    /// Les `N` prochains octets, en tableau.
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut bytes = [0; N];
        bytes.copy_from_slice(self.take(N)?);
        Ok(bytes)
    }

    /// Un octet.
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    /// Deux octets, petit-boutiste.
    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// Quatre octets, petit-boutiste.
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// Un flottant, refusé s'il n'est pas fini.
    ///
    /// Le contrôle est ici et non chez l'appelant parce qu'aucun flottant de ces
    /// formats n'a de non-fini légitime : posé à la lecture, l'invariant tient
    /// pour le champ qu'on ajoutera sans y penser. `from_bits` et rien d'autre
    /// avant le contrôle — une arithmétique sur un `NaN` signalant normaliserait
    /// sa charge utile, différemment selon la cible.
    pub(crate) fn f32(&mut self) -> Result<f32> {
        let value = f32::from_bits(self.u32()?);
        if !value.is_finite() {
            return Err(Error::InvalidFormat(Malformation::NonFinite));
        }
        Ok(value)
    }

    /// Une étiquette de quatre octets : signature, genre de fichier, genre de
    /// section.
    pub(crate) fn tag(&mut self) -> Result<[u8; 4]> {
        self.array()
    }
}

#[cfg(test)]
mod tests;
