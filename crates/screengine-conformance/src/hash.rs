// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! L'empreinte d'une image.
//!
//! FNV-1a sur 64 bits : une dizaine de lignes dans n'importe quel langage, et
//! native en PHP (`hash('fnv1a64', …)`). Chaque hôte la recalcule de son côté,
//! et c'est ce qui rend les empreintes comparables d'un langage à l'autre. Ce
//! n'est pas un hachage cryptographique et il n'a pas à l'être : il détecte une
//! régression, pas une falsification.
//!
//! Ce qui est haché, dans cet ordre :
//!
//! 1. la largeur puis la hauteur, en `u32` petit-boutiste — sans quoi une image
//!    de 640×360 et une de 360×640 aux mêmes octets se confondraient ;
//! 2. les pixels de la zone utile, ligne par ligne, `largeur × 4` octets R, G,
//!    B, A, alpha compris.
//!
//! Le `stride` n'y entre pas : ce qui dépasse la largeur appartient à l'hôte, et
//! l'image est la même quel que soit l'espacement des lignes.

/// L'état initial de FNV-1a 64 bits.
const OFFSET: u64 = 0xCBF2_9CE4_8422_2325;

/// Le multiplicateur de FNV-1a 64 bits.
const PRIME: u64 = 0x0000_0100_0000_01B3;

/// Un hachage FNV-1a 64 bits en cours.
struct Fnv(u64);

impl Fnv {
    /// Ajoute des octets.
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(PRIME);
        }
    }
}

/// L'empreinte de l'image `width × height` écrite dans `pixels` avec `stride`
/// pixels par ligne.
pub(crate) fn image(pixels: &[u8], width: u32, height: u32, stride: u32) -> u64 {
    let mut fnv = Fnv(OFFSET);
    fnv.write(&width.to_le_bytes());
    fnv.write(&height.to_le_bytes());

    let row = width as usize * 4;
    let step = stride as usize * 4;
    for y in 0..height as usize {
        fnv.write(&pixels[y * step..][..row]);
    }
    fnv.0
}

/// La forme canonique : seize chiffres hexadécimaux minuscules, poids fort
/// d'abord. C'est celle que rend `hash('fnv1a64')` en PHP.
pub(crate) fn format(hash: u64) -> String {
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests;
