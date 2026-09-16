// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Les codes de retour de l'ABI.
//!
//! Les plages sont réservées par étape, et c'est une règle arithmétique : la
//! catégorie d'un code est `(-code) / 100`. Une liaison qui rencontre un code
//! qu'elle ne connaît pas l'y ramène, au lieu de le traiter en erreur
//! générique. Voir `docs/abi.md`.

use screengine::Error;

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
        Error::InvalidArgument => SCG_ERR_INVALID_ARGUMENT,
        Error::OutOfMemory => SCG_ERR_OUT_OF_MEMORY,
    }
}

/// Le texte anglais que `scg_last_error` rendra pour une erreur du noyau.
///
/// Anglais parce qu'il franchit la frontière, et jamais localisé : un message
/// qui change avec l'environnement donne des journaux qu'on ne peut plus
/// rapprocher d'un poste à l'autre.
pub(crate) fn message_of(error: Error) -> &'static str {
    match error {
        Error::InvalidArgument => "invalid argument",
        Error::OutOfMemory => "out of memory",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La catégorie d'un code est `(-code) / 100`, et les codes généraux sont
    /// tous dans la catégorie 0.
    #[test]
    fn les_codes_generaux_sont_dans_la_premiere_plage() {
        for code in [
            SCG_ERR_NULL,
            SCG_ERR_INVALID_ARGUMENT,
            SCG_ERR_OUT_OF_MEMORY,
            SCG_ERR_INVALID_STATE,
            SCG_ERR_PANIC,
            SCG_ERR_POISONED,
        ] {
            assert!(code < 0, "un code d'erreur est négatif");
            assert_eq!((-code) / 100, 0, "code {code} hors de la plage générale");
        }
    }

    /// Deux erreurs distinctes ne partagent jamais un code : un code publié ne
    /// change pas de sens, et deux sens pour un code reviendrait au même.
    #[test]
    fn chaque_erreur_du_noyau_a_son_code() {
        let errors = [Error::InvalidArgument, Error::OutOfMemory];
        for (i, a) in errors.iter().enumerate() {
            for b in &errors[i + 1..] {
                assert_ne!(code_of(*a), code_of(*b));
            }
        }
    }
}
