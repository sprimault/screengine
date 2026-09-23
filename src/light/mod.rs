// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce qui éclaire un pixel une fois sa couleur trouvée.
//!
//! La combinaison d'un texel et de son éclairage, le sur-éclairement qui
//! l'accompagne, et le brouillard par la distance. Le post-traitement de tuile
//! viendra ici.

pub mod fog;

/// Le décalage le plus fort qu'un contexte accepte.
///
/// Deux, soit un quadruplement au plus. Au-delà, la saturation emporte la
/// moitié haute de la dynamique et une surface bien éclairée devient un aplat
/// blanc : ce n'est plus un réglage, c'est un défaut.
pub const MAX_OVERBRIGHT: u32 = 2;

/// Bits de l'éclairage, et le décalage de la combinaison sans sur-éclairement.
const LIGHT_BITS: u32 = 8;

/// Combine un texel et son éclairage, canal par canal.
///
/// `light` porte trois canaux de huit bits **où 255 est le neutre**, et
/// `overbright` le décalage du contexte, entre zéro et [`MAX_OVERBRIGHT`].
///
/// La forme est `t·(l + 1) >> 8` et non `(t·l + 128) >> 8`. Cette dernière est
/// celle d'un mélange entre deux valeurs, et elle ne vaut que pour des poids
/// qui somment à 256 : appliquée à un facteur dont le maximum est 255, elle
/// rend le blanc à 254 sous pleine lumière — un assombrissement d'un
/// deux-cent-cinquante-sixième sur toute surface éclairée, donc sur la scène
/// entière. Le `+1` rend le texel intact à `l = 255` et noir à `l = 0`, sans
/// division ni table.
///
/// **Le sur-éclairement est ici et non dans le post-traitement.** Appliqué
/// après coup, un doublement ne rendrait que des valeurs paires et éclaircirait
/// aussi ce qu'aucune lumière ne touche. La saturation s'écrit, plutôt que
/// d'être laissée à une conversion qui enroulerait.
///
/// L'alpha traverse tel quel : il n'est pas une couleur, et le tampon de sortie
/// le force à l'opacité de toute façon.
pub fn modulate(texel: u32, light: u32, overbright: u32) -> u32 {
    debug_assert!(overbright <= MAX_OVERBRIGHT);
    let shift = LIGHT_BITS - overbright;
    let channel = |index: u32| {
        let t = (texel >> index) & 0xFF;
        let l = (light >> index) & 0xFF;
        ((t * (l + 1)) >> shift).min(0xFF) << index
    };
    channel(0) | channel(8) | channel(16) | (texel & 0xFF00_0000)
}

#[cfg(test)]
mod tests;
