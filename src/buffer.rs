// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les réservations des appels nommés.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, Result};

/// Un vecteur vide qui peut recevoir `capacity` éléments sans réallouer, ou
/// [`Error::OutOfMemory`].
///
/// `try_reserve_exact` plutôt que `Vec::with_capacity` : une allocation qui
/// échoue en paniquant ne laisserait rien à traduire en code de retour, et la
/// variante `OutOfMemory` n'aurait jamais de cas.
pub(crate) fn reserved<T>(capacity: usize) -> Result<Vec<T>> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(capacity)
        .map_err(|_| Error::OutOfMemory)?;
    Ok(buffer)
}

/// Une chaîne qui possède `text`, ou [`Error::OutOfMemory`].
///
/// Même raison que [`reserved`] : `String::from` paniquerait sur un échec
/// d'allocation, et une panique du noyau ne laisserait rien à traduire en code
/// de retour.
pub(crate) fn owned(text: &str) -> Result<String> {
    let mut string = String::new();
    string
        .try_reserve_exact(text.len())
        .map_err(|_| Error::OutOfMemory)?;
    string.push_str(text);
    Ok(string)
}
