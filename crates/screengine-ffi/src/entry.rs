// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! L'enveloppe que traverse chaque point d'entrée.
//!
//! Elle fixe l'environnement flottant, rattrape les paniques, traduit l'erreur
//! en code et retient le message. Elle s'écrit une fois : un point d'entrée
//! ajouté sans elle est le défaut le plus discret du projet, puisqu'il ne se
//! manifeste que le jour où quelque chose panique, chez quelqu'un d'autre.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

use screengine::{Context, Error};

use crate::context::ScgContext;
use crate::fpenv::FpEnv;
use crate::message;
use crate::status::{SCG_ERR_NULL, SCG_ERR_PANIC, SCG_ERR_POISONED, SCG_OK, code_of, message_of};

/// Une erreur qui porte son code d'ABI et son texte.
///
/// Elle réunit ce que rend le noyau et ce que la frontière refuse elle-même —
/// un pointeur nul n'a pas de sens pour le noyau, qui ne manipule jamais de
/// pointeur.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AbiError {
    code: i32,
    message: &'static str,
}

impl AbiError {
    /// Un argument obligatoire est nul.
    pub(crate) const NULL: Self = Self {
        code: SCG_ERR_NULL,
        message: "null pointer argument",
    };
}

impl From<Error> for AbiError {
    fn from(error: Error) -> Self {
        Self {
            code: code_of(error),
            message: message_of(error),
        }
    }
}

/// Ce qui empêche un appel d'aboutir.
enum Failure {
    /// Un refus, avec son code.
    Abi(AbiError),
    /// Une panique, et sa charge utile telle que la bibliothèque standard la
    /// rend.
    Panic(Box<dyn Any + Send>),
}

/// Le texte d'une charge utile de panique, sans rien allouer.
///
/// La bibliothèque standard produit un `&'static str` pour `panic!("…")` sans
/// argument et une `String` sinon ; un autre type vient d'un `panic_any`, que
/// le projet n'utilise pas.
fn panic_text(payload: &(dyn Any + Send)) -> &str {
    if let Some(text) = payload.downcast_ref::<&str>() {
        text
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text
    } else {
        "panic with a non-string payload"
    }
}

/// Exécute un appel sous l'environnement du moteur, paniques rattrapées.
///
/// La garde d'environnement flottant enveloppe `catch_unwind` et non l'inverse :
/// le dépliage s'arrête donc avant elle, sa destruction a toujours lieu sur un
/// retour normal, et le message de panique se formate encore sous
/// l'environnement par défaut.
fn guarded<F>(f: F) -> Result<(), Failure>
where
    F: FnOnce() -> Result<(), AbiError>,
{
    let _fpenv = FpEnv::enter();

    // `AssertUnwindSafe` ne se pose qu'ici : c'est le poison qui la rend
    // honnête, puisque l'état laissé par une panique n'est plus jamais observé.
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(Failure::Abi(error)),
        Err(payload) => Err(Failure::Panic(payload)),
    }
}

/// Enveloppe un appel qui porte sur un contexte.
///
/// # Safety
///
/// `ctx` est nul, ou un handle rendu par `scg_create` et pas encore détruit.
pub(crate) unsafe fn with_context<F>(ctx: *mut ScgContext, f: F) -> i32
where
    F: FnOnce(&mut Context) -> Result<(), AbiError>,
{
    // Vidé en entrant, pour qu'un thread recyclé ne rende jamais le message
    // d'une tâche précédente.
    message::clear_orphan();

    // SAFETY: précondition de la fonction — `ctx` est nul ou valide, et
    // l'appelant garantit qu'aucun autre thread ne s'en sert pendant l'appel.
    let Some(ctx) = (unsafe { ctx.as_mut() }) else {
        message::set_orphan(AbiError::NULL.message);
        return SCG_ERR_NULL;
    };

    ctx.message_mut().clear();

    if ctx.poisoned() {
        ctx.message_mut()
            .set("a previous call panicked; this object is poisoned");
        return SCG_ERR_POISONED;
    }

    match guarded(|| f(ctx.inner_mut())) {
        Ok(()) => SCG_OK,
        Err(Failure::Abi(error)) => {
            ctx.message_mut().set(error.message);
            error.code
        }
        Err(Failure::Panic(payload)) => {
            ctx.poison();
            ctx.message_mut().set(panic_text(&*payload));
            SCG_ERR_PANIC
        }
    }
}

/// Enveloppe un appel qui n'a pas de contexte auquel se rattacher.
///
/// Son message va dans l'emplacement par thread, que `scg_last_error(NULL)`
/// lit.
pub(crate) fn without_context<F>(f: F) -> i32
where
    F: FnOnce() -> Result<(), AbiError>,
{
    message::clear_orphan();

    match guarded(f) {
        Ok(()) => SCG_OK,
        Err(Failure::Abi(error)) => {
            message::set_orphan(error.message);
            error.code
        }
        Err(Failure::Panic(payload)) => {
            message::set_orphan(panic_text(&*payload));
            SCG_ERR_PANIC
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::SCG_ERR_INVALID_ARGUMENT;

    #[test]
    fn un_appel_qui_aboutit_rend_le_succes() {
        assert_eq!(without_context(|| Ok(())), SCG_OK);
    }

    #[test]
    fn une_erreur_du_noyau_devient_son_code() {
        let code = without_context(|| Err(Error::InvalidArgument.into()));
        assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    }

    #[test]
    fn une_panique_devient_un_code_et_ne_s_echappe_pas() {
        let code = without_context(|| panic!("défaut simulé du moteur"));
        assert_eq!(code, SCG_ERR_PANIC);
    }

    #[test]
    fn le_message_d_une_panique_est_retenu() {
        without_context(|| panic!("défaut simulé du moteur"));
        // SAFETY: le pointeur vise le stockage local du thread courant, et rien
        // n'écrit entre-temps.
        let text = unsafe { std::ffi::CStr::from_ptr(message::orphan_ptr()) };
        assert_eq!(
            text.to_str().expect("UTF-8 valide"),
            "défaut simulé du moteur"
        );
    }

    #[test]
    fn un_appel_qui_aboutit_efface_le_message_precedent() {
        without_context(|| Err(Error::OutOfMemory.into()));
        without_context(|| Ok(()));
        // SAFETY: même raisonnement.
        let text = unsafe { std::ffi::CStr::from_ptr(message::orphan_ptr()) };
        assert_eq!(text.to_bytes(), b"");
    }

    /// Le texte d'une charge utile se lit sans allouer, quelle que soit la forme
    /// que la bibliothèque standard lui donne.
    #[test]
    fn le_texte_d_une_charge_utile_se_lit_dans_les_deux_formes() {
        let statique: Box<dyn Any + Send> = Box::new("littéral");
        assert_eq!(panic_text(&*statique), "littéral");

        let formate: Box<dyn Any + Send> = Box::new(String::from("formaté"));
        assert_eq!(panic_text(&*formate), "formaté");

        let autre: Box<dyn Any + Send> = Box::new(7u8);
        assert_eq!(panic_text(&*autre), "panic with a non-string payload");
    }
}
