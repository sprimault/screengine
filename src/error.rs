// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

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
    /// Une valeur reçue est hors de ce que le moteur accepte : dimension nulle,
    /// résolution au-delà de [`MAX_RESOLUTION`], taille de tuile autre que 32
    /// ou 64, `stride` inférieur à la largeur.
    ///
    /// [`MAX_RESOLUTION`]: crate::MAX_RESOLUTION
    InvalidArgument,
    /// Un tampon du contexte n'a pas pu être alloué.
    OutOfMemory,
}

/// Le résultat d'un appel du noyau.
pub type Result<T> = core::result::Result<T, Error>;
