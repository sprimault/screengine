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

/// The call came out of sequence, such as ending a frame that never began.
pub const SCG_ERR_INVALID_STATE: i32 = -4;

/// The engine panicked. The object is now poisoned; see `SCG_ERR_POISONED`.
///
/// A panic is an engine defect, never a reaction to invalid input. Read the
/// message with `scg_last_error` and report it.
pub const SCG_ERR_PANIC: i32 = -5;

/// A previous call on this object panicked and left it in an unspecified state.
///
/// Every call on a poisoned object returns this code, except `scg_last_error`
/// and the destructor, which stay available so the host can read the cause and
/// release the object.
pub const SCG_ERR_POISONED: i32 = -6;

/// Traduit une erreur du noyau en code d'ABI.
///
/// Sans bras générique : c'est ce qui fait échouer la compilation le jour où le
/// noyau gagne une variante que personne n'a pensé à traduire.
pub(crate) fn code_of(error: Error) -> i32 {
    match error {
        Error::InvalidArgument(_) => SCG_ERR_INVALID_ARGUMENT,
        Error::OutOfMemory => SCG_ERR_OUT_OF_MEMORY,
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
        Error::OutOfMemory => "out of memory",
    }
}

#[cfg(test)]
mod tests;
