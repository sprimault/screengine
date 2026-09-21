// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le décodage PNG, du côté de l'hôte.
//!
//! Le moteur n'ouvre aucun fichier et ne connaît aucun format d'image : il
//! reçoit un bloc de texels RGBA et le copie. Décoder appartient donc à l'étage
//! d'accueil, comme cela appartient à un hôte écrit contre l'ABI C — qui le
//! fera avec la bibliothèque de sa plateforme.

use std::io::Cursor;

use png::{ColorType, Decoder, Transformations};
use screengine::{Argument, MAX_TEXTURE_SIZE, Texture};

use crate::Error;

/// Décode un PNG et en fait une texture du moteur, mipmaps compris.
///
/// Tout ce que le format porte est ramené à quatre octets par texel : palette
/// étendue, niveaux de gris répétés sur les trois canaux, `tRNS` devenu un
/// canal alpha, échantillons de seize bits ramenés à huit. Une image sans
/// transparence reçoit un alpha opaque.
///
/// **Les deux côtés doivent être des puissances de deux**, jusqu'à 2048 : c'est
/// la contrainte du repli par masque du moteur, et elle ne s'assouplit pas ici.
///
/// L'usage naturel est `include_bytes!`, qui range la texture dans le binaire
/// et supprime la question du répertoire courant à l'exécution ; rien n'empêche
/// de passer ce qu'on vient de lire sur le disque ou de recevoir du réseau.
pub fn load_png(bytes: &[u8]) -> Result<Texture, Error> {
    let mut decoder = Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(Transformations::normalize_to_color8() | Transformations::ALPHA);
    let mut reader = decoder.read_info()?;

    // Les dimensions se contrôlent sur l'en-tête, avant d'allouer quoi que ce
    // soit : un fichier de 8192 de côté demanderait deux cent cinquante
    // mégaoctets pour être refusé juste après par `Texture::load`.
    let (width, height) = reader.info().size();
    let side = |s: u32| s.is_power_of_two() && s <= MAX_TEXTURE_SIZE;
    if !side(width) || !side(height) {
        return Err(screengine::Error::InvalidArgument(Argument::TextureSize).into());
    }

    // Borné par ce qui précède, donc jamais nul en pratique ; un zéro se fait
    // rejeter par `next_frame`, qui refuse un tampon trop court, plutôt que par
    // une panique.
    let mut raw = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut raw)?;
    raw.truncate(info.buffer_size());

    let rgba = match info.color_type {
        ColorType::Rgba => raw,
        ColorType::Rgb => expand(&raw, 3, |p| [p[0], p[1], p[2], 0xFF]),
        ColorType::GrayscaleAlpha => expand(&raw, 2, |p| [p[0], p[0], p[0], p[1]]),
        // `Indexed` ne sort pas des transformations réglées plus haut, qui
        // étendent toute palette. Il partage le bras d'un octet par pixel
        // plutôt que d'ouvrir un chemin qui panique.
        ColorType::Grayscale | ColorType::Indexed => expand(&raw, 1, |p| [p[0], p[0], p[0], 0xFF]),
    };

    Ok(Texture::load(width, height, &rgba)?)
}

/// Recompose un bloc RGBA depuis des pixels de `samples` octets.
fn expand(raw: &[u8], samples: usize, pixel: impl Fn(&[u8]) -> [u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() / samples * 4);
    for chunk in raw.chunks_exact(samples) {
        out.extend_from_slice(&pixel(chunk));
    }
    out
}

#[cfg(test)]
mod tests;
