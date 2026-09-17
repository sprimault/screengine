// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les réservations des appels nommés.

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
