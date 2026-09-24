// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les codes de retour de l'ABI.
//!
//! Les plages sont réservées par étape, et c'est une règle arithmétique : la
//! catégorie d'un code est `(-code) / 100`. Une liaison qui rencontre un code
//! qu'elle ne connaît pas l'y ramène, au lieu de le traiter en erreur
//! générique. Voir `docs/abi.md`.

use screengine::{Argument, Error};

/// Success.
pub const SCG_OK: i32 = 0;

/// A pointer argument was null where the call requires one.
pub const SCG_ERR_NULL: i32 = -1;

/// An argument is outside what the engine accepts.
pub const SCG_ERR_INVALID_ARGUMENT: i32 = -2;

/// An allocation failed.
pub const SCG_ERR_OUT_OF_MEMORY: i32 = -3;

/// The call came out of sequence, such as rendering a tile before the frame began.
pub const SCG_ERR_INVALID_STATE: i32 = -4;

/// The engine panicked. The object is now faulted; see `SCG_ERR_FAULTED`.
///
/// A panic is an engine defect, never a reaction to invalid input. Read the
/// message with `scg_last_error` and report it.
pub const SCG_ERR_PANIC: i32 = -5;

/// A previous call on this object panicked and left it in an unspecified state.
///
/// Every call on a faulted object returns this code, except `scg_last_error`
/// and the destructor, which stay available so the host can read the cause and
/// release the object.
pub const SCG_ERR_FAULTED: i32 = -6;

/// Deprecated name of `SCG_ERR_FAULTED`, same value. Kept so that hosts written
/// against an earlier header still compile; it will not be removed before 1.0.
// Écrit en littéral et non par renvoi à `SCG_ERR_FAULTED` : les liaisons qui
// lisent les `#define` du header, JavaScript et PHP, n'évaluent pas un nom.
pub const SCG_ERR_POISONED: i32 = -6;

/// Traduit une erreur du noyau en code d'ABI.
///
/// Sans bras générique : c'est ce qui fait échouer la compilation le jour où le
/// noyau gagne une variante que personne n'a pensé à traduire.
pub(crate) fn code_of(error: Error) -> i32 {
    match error {
        Error::InvalidArgument(_) => SCG_ERR_INVALID_ARGUMENT,
        Error::OutOfMemory => SCG_ERR_OUT_OF_MEMORY,
        Error::InvalidState => SCG_ERR_INVALID_STATE,
        // Le contrat d'ABI promet ce code quand une tuile a paniqué, et c'est
        // exactement ce que le noyau signale ici : il n'a pas vu la panique,
        // seulement une tuile entrée sans ressortir.
        Error::Faulted => SCG_ERR_FAULTED,
    }
}

/// Le texte anglais que `scg_last_error` rendra pour une erreur du noyau.
///
/// Anglais parce qu'il franchit la frontière, et jamais localisé : un message
/// qui change avec l'environnement donne des journaux qu'on ne peut plus
/// rapprocher d'un poste à l'autre. Le code dit la catégorie, le message dit
/// quel argument : une liaison ne décide rien sur ce texte.
pub(crate) fn message_of(error: Error) -> &'static str {
    match error {
        Error::InvalidArgument(Argument::Resolution) => {
            "invalid resolution: each side must be between 1 and 2048, and the current resolution within the maximum"
        }
        Error::InvalidArgument(Argument::TileSize) => "invalid tile size: must be 32 or 64",
        Error::InvalidArgument(Argument::Stride) => {
            "invalid stride: must be at least the internal width"
        }
        Error::InvalidArgument(Argument::BufferLength) => {
            "pixel buffer too short: needs stride x height x 4 bytes"
        }
        Error::InvalidArgument(Argument::TileIndex) => {
            "invalid tile index: must be less than the tile count of the frame"
        }
        Error::InvalidArgument(Argument::Region) => "invalid region: must lie within the image",
        Error::InvalidArgument(Argument::ScratchLength) => {
            "scratch buffer too short: needs one word per pixel of the region"
        }
        Error::InvalidArgument(Argument::TriangleCapacity) => {
            "too many triangles submitted for the capacity reserved at creation"
        }
        Error::InvalidArgument(Argument::VertexIndex) => {
            "invalid triangle index: must be less than the vertex count of the batch"
        }
        Error::InvalidArgument(Argument::Projection) => {
            "invalid projection: the vertical field of view must be within ]0, pi[ radians, and the near plane positive and finite"
        }
        Error::InvalidArgument(Argument::VertexCoordinate) => {
            "vertex coordinate is not finite: NaN and infinities are rejected, and the whole batch with them"
        }
        Error::InvalidArgument(Argument::TextureCapacity) => {
            "too many distinct textures in one frame for the capacity reserved at creation"
        }
        Error::InvalidArgument(Argument::TextureCoordinate) => {
            "texture coordinate is not finite, or beyond 16384 texels"
        }
        Error::InvalidArgument(Argument::TextureSize) => {
            "invalid texture size: each side must be a power of two between 1 and 2048"
        }
        Error::InvalidArgument(Argument::Overbright) => {
            "invalid overbright shift: must be 0, 1 or 2"
        }
        Error::InvalidArgument(Argument::Fog) => {
            "invalid fog range: start must be finite and not negative, end finite and beyond start"
        }
        Error::InvalidArgument(Argument::LightCapacity) => "too many dynamic lights for one frame",
        Error::InvalidArgument(Argument::Light) => {
            "invalid light: position must be finite, and radius finite and positive"
        }
        Error::InvalidArgument(Argument::Grade) => {
            "invalid output curve: gamma must be within ]0, 8], each channel gain within [0, 4]"
        }
        Error::InvalidArgument(Argument::TextureLength) => {
            "pixel block of the wrong length: needs width x height x 4 bytes"
        }
        Error::OutOfMemory => "out of memory",
        // Un seul message pour toute la variante, et il ne nomme aucun cas :
        // le noyau ne distingue pas une tuile déjà rendue d'une soumission
        // pendant le rendu, et un texte qui parlerait de tuiles enverrait sur
        // une fausse piste l'hôte qui a simplement soumis trop tard.
        Error::InvalidState => {
            "call out of sequence: check the frame state and whether this tile was already rendered"
        }
        Error::Faulted => "a tile did not return from rendering; this frame is incomplete",
    }
}

#[cfg(test)]
mod tests;
