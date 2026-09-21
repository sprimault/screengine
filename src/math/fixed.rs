// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

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

/// La marge qui sépare une profondeur de sommet des bornes du `u32`.
///
/// L'équation de plan arrondit ses gradients et se trompe de moins de 64 unités
/// en un pixel couvert. Tant que chaque sommet est au moins à 64 unités des
/// bornes, la profondeur interpolée ne sort jamais de [0, 2³²), et aucune
/// valeur interpolée ne tombe à 0, la profondeur du fond, qu'elle ne pourrait
/// pas battre.
pub const DEPTH_MARGIN: u32 = 64;

/// Passe une profondeur `near/w` en 0.32, bornée à [`DEPTH_MARGIN`] des bornes.
///
/// **C'est ici, et nulle part en aval, que la profondeur est bornée.** Le
/// parcours des pixels ne borne rien : un minimum ou un maximum sur 64 bits
/// n'existe pas en SIMD avant AVX-512, et le payer dans la boucle intérieure
/// pour un cas que cette fonction exclut coûterait partout. Un sommet qui
/// arrive au rasteriseur sans être passé par elle rompt l'invariant sans que
/// rien ne le signale.
///
/// Le clipping du plan proche garantit `w ≥ near`, donc `depth` dans ]0, 1] ;
/// le reste est refusé par comparaison écrite, avant toute conversion entière.
pub fn to_depth(depth: f32) -> u32 {
    let (low, high) = (DEPTH_MARGIN, u32::MAX - DEPTH_MARGIN);
    if depth.is_nan() || depth <= 0.0 {
        return low;
    }
    if depth >= 1.0 {
        return high;
    }
    // Sous 1, le plus grand `f32` vaut 1 − 2⁻²⁴ : le produit tient sous
    // 2³² − 255, et la troncature est un plancher sur des positifs.
    let scaled = (depth * 4_294_967_296.0) as u32;
    if scaled < low {
        low
    } else if scaled > high {
        high
    } else {
        scaled
    }
}

/// Bits fractionnaires d'une coordonnée de texture au sommet.
///
/// Douze, donc du 14.12 : la partie entière porte les 2¹⁴ texels que la
/// soumission admet, et il reste cinq bits de marge dans l'`i32`.
pub const TEXEL_BITS: u32 = 12;

/// Bits fractionnaires d'une coordonnée de texture par pixel, après division.
///
/// Seize, donc du 16.16. Il vit ici et non dans le rasteriseur parce que
/// l'échantillonnage le lit aussi : le tramage décale ces bits avant de les
/// tronquer, le bilinéaire en tire ses poids.
pub const UV_BITS: u32 = 16;

/// La plus grande coordonnée de texture qu'un sommet peut porter, en texels.
///
/// Ce n'est pas la taille d'une texture, mais l'amplitude du pavage : une
/// surface qui répète sa texture huit mille fois reste dans la borne. Elle vient
/// du produit que la récupération par pixel fait plus loin — coordonnée par
/// profondeur, deux grandeurs anticorrélées dont le produit reste un bit sous le
/// débordement d'un `i64`.
pub const MAX_TEXEL_COORD: f32 = 16_384.0;

/// La coordonnée de texture extrême en 14.12, vers laquelle `to_texel` borne.
const TEXEL_LIMIT: i32 = (MAX_TEXEL_COORD as i32) << TEXEL_BITS;

/// Passe une coordonnée de texture en 14.12, arrondie au plus proche.
///
/// Même demi-pas que [`to_subpixel`], et pour une raison qui se voit ici plus
/// qu'ailleurs : la conversion `as` tronque vers zéro, si bien que le sens de
/// l'arrondi s'inverserait de part et d'autre de l'origine de la texture — une
/// couture y apparaîtrait, sur une ligne que rien d'autre ne distingue.
///
/// Le bornage est écrit, jamais laissé à la saturation de `as` : un sommet
/// engendré par le découpage peut retomber quelques ulp sous le plan proche,
/// donc porter une profondeur à peine au-dessus de un, exactement comme
/// [`to_depth`] le borne déjà.
pub fn to_texel(value: f32) -> i32 {
    if value.is_nan() {
        return 0;
    }
    let scaled = value * (1 << TEXEL_BITS) as f32;
    let rounded = if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    };
    if rounded >= TEXEL_LIMIT as f32 {
        return TEXEL_LIMIT;
    }
    if rounded <= -(TEXEL_LIMIT as f32) {
        return -TEXEL_LIMIT;
    }
    rounded as i32
}

#[cfg(test)]
mod tests;
