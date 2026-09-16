// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Le passage des flottants à la virgule fixe, et lui seul.
//!
//! C'est ici que s'arrêtent les flottants : tout ce qui suit la projection est
//! entier. Une seule fonction fait la conversion, pour qu'il n'y ait qu'un
//! arrondi à rendre déterministe entre les cibles et les chemins SIMD.

/// Bits fractionnaires d'une coordonnée écran.
pub const SUBPIXEL_BITS: u32 = 4;

/// Un pixel, exprimé en sous-pixels.
pub const SUBPIXEL_SCALE: i32 = 1 << SUBPIXEL_BITS;

/// Décalage du centre d'un pixel par rapport à son coin.
///
/// Le pixel est échantillonné en son centre : le pixel entier `x` correspond à
/// la coordonnée `x * SUBPIXEL_SCALE + PIXEL_CENTER`.
pub const PIXEL_CENTER: i32 = SUBPIXEL_SCALE / 2;

/// Passe une coordonnée écran en virgule fixe, arrondie au plus proche.
///
/// Le demi-pas s'écarte de zéro. La conversion `as` seule ne suffirait pas :
/// elle tronque vers zéro, donc elle rapproche systématiquement chaque sommet de
/// l'origine de l'image, d'un seizième de pixel au pire au lieu d'un
/// trente-deuxième. La géométrie s'en trouve contractée, un objet symétrique
/// cesse de l'être, et un panoramique fait sauter les sommets inégalement de
/// part et d'autre.
///
/// L'ajout du demi-pas est exact : le produit reste très en deçà de la limite où
/// un `f32` cesse de représenter les demi-entiers, donc il n'y a pas de double
/// arrondi. Le sommet est déjà clippé dans la bande de garde quand il arrive
/// ici ; la saturation de `as` ne sert pas de garde-fou, elle masquerait un
/// défaut de clipping.
pub fn to_subpixel(v: f32) -> i32 {
    let scaled = v * SUBPIXEL_SCALE as f32;
    (if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    }) as i32
}

#[cfg(test)]
mod tests;
