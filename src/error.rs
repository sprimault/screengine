// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce qui peut échouer dans le noyau.

/// Une erreur du noyau.
///
/// Sans chaîne de caractères : le message se formate dans `screengine-ffi`, qui
/// traduit aussi chaque variante en code d'ABI. L'énumération n'est
/// volontairement pas `non_exhaustive` — c'est ce qui fait échouer la
/// compilation de la couche FFI le jour où une variante y est ajoutée sans
/// traduction, au lieu de la laisser tomber dans un bras générique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Une valeur reçue est hors de ce que le moteur accepte, et [`Argument`]
    /// dit laquelle.
    ///
    /// Une variante qui porte l'argument plutôt qu'une variante par argument :
    /// la catégorie reste une, comme le code d'ABI qui la traduit, et c'est le
    /// message seul qui se précise.
    InvalidArgument(Argument),
    /// Un tampon du contexte n'a pas pu être alloué.
    OutOfMemory,
    /// Un appel hors séquence : une tuile déjà rendue dans cette image.
    InvalidState,
}

/// L'argument qu'une [`Error::InvalidArgument`] refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Argument {
    /// Une dimension nulle, un maximum au-delà de [`MAX_RESOLUTION`], ou une
    /// résolution courante au-delà du maximum reçu à la création.
    ///
    /// [`MAX_RESOLUTION`]: crate::MAX_RESOLUTION
    Resolution,
    /// Une taille de tuile autre que 32 ou 64.
    TileSize,
    /// Un `stride` inférieur à la largeur courante, ou si grand que la taille
    /// du tampon qu'il décrit ne tient pas dans un `usize`.
    Stride,
    /// Un tampon de pixels plus court que `stride × hauteur × 4` octets.
    ///
    /// Seul un appelant Rust le rencontre : la frontière C ne reçoit pas la
    /// longueur du tampon et en fait une précondition.
    BufferLength,
    /// Un index de tuile au-delà du nombre de tuiles de l'image.
    TileIndex,
    /// Une région qui déborde de l'image.
    Region,
    /// Un tampon de travail plus court que la région qu'il doit porter.
    ScratchLength,
    /// Plus de triangles soumis que la capacité réservée à la création.
    TriangleCapacity,
    /// Un indice de triangle au-delà du tableau de sommets du lot.
    VertexIndex,
    /// Un champ de vision hors de `]0, π[`, ou un plan proche nul, négatif ou
    /// démesuré.
    Projection,
    /// Un côté de texture nul, au-delà de [`MAX_TEXTURE_SIZE`], ou qui n'est
    /// pas une puissance de deux.
    ///
    /// [`MAX_TEXTURE_SIZE`]: crate::MAX_TEXTURE_SIZE
    TextureSize,
    /// Un bloc de pixels dont la longueur n'est pas exactement
    /// `largeur × hauteur × 4` octets.
    TextureLength,
}

/// Le résultat d'un appel du noyau.
pub type Result<T> = core::result::Result<T, Error>;
