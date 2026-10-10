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

/// Le plus grand côté qu'une icône de fenêtre peut avoir.
///
/// Aucun système n'en demande davantage : au-delà de 256, c'est le compositeur
/// qui réduit, et il le fait moins bien qu'un outil d'image. La borne existe
/// surtout pour que le refus tombe sur l'en-tête plutôt qu'après avoir alloué
/// ce qu'un PNG de 8192 réclamerait.
const MAX_ICON_SIZE: u32 = 256;

/// Une icône de fenêtre décodée, prête pour le système.
///
/// **Un type à nous plutôt que celui de `winit`**, qui n'apparaît dans aucune
/// signature de ce crate : son `Icon` n'est pas construisible hors d'une boucle
/// d'événements sur toutes les plateformes, et l'exposer ferait dépendre les
/// réglages d'un hôte d'une bibliothèque qui change d'API à chaque mineure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    /// Les texels, quatre octets par pixel, lignes jointives.
    pub(crate) rgba: Vec<u8>,
    /// La largeur en pixels.
    pub(crate) width: u32,
    /// La hauteur en pixels.
    pub(crate) height: u32,
}

/// Décode un PNG en icône de fenêtre.
///
/// **Les côtés ne sont pas tenus d'être des puissances de deux**, à la
/// différence d'une texture : une icône ne se replie pas par masque, et les
/// tailles que les systèmes attendent — 16, 24, 48 — n'en sont pas. Ce qui est
/// refusé est une dimension nulle ou au-delà de 256.
///
/// L'usage naturel est `include_bytes!` : l'icône voyage dans le binaire, et la
/// question du répertoire courant à l'exécution ne se pose pas.
pub fn load_png_icon(bytes: &[u8]) -> Result<Icon, Error> {
    let fits = |s: u32| s > 0 && s <= MAX_ICON_SIZE;
    let (rgba, width, height) = decode_rgba(bytes, |w, h| {
        if fits(w) && fits(h) {
            Ok(())
        } else {
            Err(screengine::Error::InvalidArgument(Argument::TextureSize).into())
        }
    })?;
    Ok(Icon {
        rgba,
        width,
        height,
    })
}

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
    decode(bytes, false)
}

/// La même chose, en **texture masquée** : un texel d'alpha nul ne s'écrit pas.
///
/// C'est le format d'un sprite, et il se déclare au chargement et non au
/// dessin, parce que c'est là que la chaîne de mipmaps se construit : décidé
/// plus tard, il faudrait en tenir deux, ou en tenir une fausse.
///
/// Une fonction de plus plutôt qu'un drapeau sur [`load_png`] : les appels
/// existants ne changent pas, et `load_png_masked` se lit à l'appel là où un
/// booléen se lirait dans la signature.
pub fn load_png_masked(bytes: &[u8]) -> Result<Texture, Error> {
    decode(bytes, true)
}

/// Le corps commun des deux, `masked` décidant du format.
fn decode(bytes: &[u8], masked: bool) -> Result<Texture, Error> {
    // Les deux côtés en puissance de deux, jusqu'au plafond du moteur : c'est
    // la contrainte du repli par masque, et elle est propre à une texture — une
    // icône n'y est pas tenue.
    let side = |s: u32| s.is_power_of_two() && s <= MAX_TEXTURE_SIZE;
    let (rgba, width, height) = decode_rgba(bytes, |w, h| {
        if side(w) && side(h) {
            Ok(())
        } else {
            Err(screengine::Error::InvalidArgument(Argument::TextureSize).into())
        }
    })?;

    Ok(if masked {
        Texture::load_masked(width, height, &rgba)?
    } else {
        Texture::load(width, height, &rgba)?
    })
}

/// Décode un PNG en octets RGBA, avec ses dimensions.
///
/// **`accepte` reçoit les dimensions avant toute allocation**, et c'est sa
/// raison d'être : un fichier de 8192 de côté demanderait deux cent cinquante
/// mégaoctets pour être refusé juste après. Le contrôle n'est pas le même selon
/// l'usage — une texture veut des puissances de deux, une icône une taille que
/// le système accepte —, donc il se passe en paramètre plutôt que de vivre ici.
pub(crate) fn decode_rgba(
    bytes: &[u8],
    accepte: impl Fn(u32, u32) -> Result<(), Error>,
) -> Result<(Vec<u8>, u32, u32), Error> {
    let mut decoder = Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(Transformations::normalize_to_color8() | Transformations::ALPHA);
    let mut reader = decoder.read_info()?;

    let (width, height) = reader.info().size();
    accepte(width, height)?;

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

    Ok((rgba, width, height))
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
