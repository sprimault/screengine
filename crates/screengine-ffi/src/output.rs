// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le tampon de l'hôte, vu par morceaux de ligne.
//!
//! Deux tuiles de la même ligne de tuiles partagent des lignes du tampon. Une
//! tranche par tuile sur ces lignes ferait coexister deux références exclusives
//! sur la même mémoire, même si chacune n'écrit que sa part : c'est un
//! comportement indéfini en Rust, quel que soit le compilateur C en face. Le
//! noyau ne demande donc que les octets de la tuile, ligne par ligne, et ce sont
//! eux seuls qui deviennent une tranche.

use std::slice;

use screengine::{Argument, BYTES_PER_PIXEL, Error, Output, Rect, Result};

/// Le tampon de l'hôte, `stride` pixels par ligne, dont la longueur est une
/// précondition.
pub(crate) struct HostRows {
    base: *mut u8,
    stride: u32,
}

impl HostRows {
    /// Le tampon qui commence à `base`.
    ///
    /// # Safety
    ///
    /// `base` est non nul et vise au moins `stride × hauteur × 4` octets
    /// accessibles en écriture, que personne d'autre n'écrit dans les
    /// rectangles que le moteur rendra.
    pub(crate) unsafe fn new(base: *mut u8, stride: u32) -> Self {
        Self { base, stride }
    }

    /// Le décalage en octets du pixel `(x, y)`, ou `None` s'il ne tient pas
    /// dans l'espace d'adressage.
    fn offset(&self, x: u32, y: u32) -> Option<usize> {
        let pixels = (y as usize)
            .checked_mul(self.stride as usize)?
            .checked_add(x as usize)?;
        let bytes = pixels.checked_mul(BYTES_PER_PIXEL)?;
        // `add` et `from_raw_parts_mut` exigent de rester sous `isize::MAX`.
        (bytes <= isize::MAX as usize).then_some(bytes)
    }
}

impl Output for HostRows {
    fn check(&self, rect: Rect) -> Result<()> {
        if rect.x + rect.width > self.stride {
            return Err(Error::InvalidArgument(Argument::Stride));
        }
        self.offset(0, rect.y + rect.height)
            .map(|_| ())
            .ok_or(Error::InvalidArgument(Argument::Stride))
    }

    fn span(&mut self, x: u32, y: u32, width: u32) -> Option<&mut [u8]> {
        let start = self.offset(x, y)?;
        // SAFETY: le noyau ne demande que des morceaux d'un rectangle qu'il a
        // fait passer par `check`, donc dans `stride × hauteur × 4` octets que
        // la précondition de `new` garantit ; deux appels simultanés ne
        // demandent jamais le même octet, les tuiles étant disjointes.
        Some(unsafe {
            slice::from_raw_parts_mut(self.base.add(start), width as usize * BYTES_PER_PIXEL)
        })
    }
}
