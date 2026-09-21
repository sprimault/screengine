// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! L'enveloppe que traverse chaque point d'entrée.
//!
//! Elle fixe l'environnement flottant, rattrape les paniques, traduit l'erreur
//! en code et retient le message. Elle s'écrit une fois : un point d'entrée
//! ajouté sans elle est le défaut le plus discret du projet, puisqu'il ne se
//! manifeste que le jour où quelque chose panique, chez quelqu'un d'autre.

use std::any::Any;
use std::cell::UnsafeCell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use screengine::{Context, Error};

use crate::context::ScgContext;
use crate::fpenv::FpEnv;
use crate::message;
use crate::status::{
    SCG_ERR_FAULTED, SCG_ERR_INVALID_ARGUMENT, SCG_ERR_NULL, SCG_ERR_PANIC, SCG_OK, code_of,
    message_of,
};

/// Une erreur qui porte son code d'ABI et son texte.
///
/// Elle réunit ce que rend le noyau et ce que la frontière refuse elle-même —
/// un pointeur nul n'a pas de sens pour le noyau, qui ne manipule jamais de
/// pointeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Un champ réservé d'une structure n'est pas nul.
    ///
    /// Refusé ici et non par le noyau : les champs réservés n'existent que parce
    /// que l'ABI est figée, et le noyau n'a pas à savoir qu'elle l'est.
    pub(crate) const RESERVED: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "reserved fields must be zero",
    };

    /// Une coordonnée de sommet n'est pas un nombre fini.
    ///
    /// Le noyau ferait disparaître le triangle sans erreur, ce qui est le bon
    /// comportement pour une donnée qu'il a lui-même transformée. Reçue telle
    /// quelle d'un hôte, elle est une erreur d'appel, et le dire vaut mieux que
    /// laisser un mur manquer dans l'image.
    pub(crate) const VERTEX_NOT_FINITE: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "vertex coordinates must be finite numbers",
    };

    /// La position ou l'orientation de la caméra n'est pas finie.
    pub(crate) const CAMERA_NOT_FINITE: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "camera position and orientation must be finite numbers",
    };

    /// La matrice du modèle n'est pas une transformation affine.
    ///
    /// Refusée ici parce que le noyau ne voit qu'une 3×4, où la question ne se
    /// pose plus : c'est la frontière qui reçoit une 4×4 et doit vérifier que
    /// sa dernière ligne n'y cache pas une perspective.
    pub(crate) const MATRIX: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "model matrix must be finite, with last row exactly 0, 0, 0, 1",
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

/// Installe, une fois, le crochet qui garde le texte d'une panique sur wasm.
///
/// Sans dépliage sur une chaîne stable, une panique y est un trap :
/// `catch_unwind` ne rattrape rien, et le texte serait perdu. Le crochet
/// s'exécute avant l'arrêt et l'écrit dans l'emplacement sans contexte, dont
/// l'adresse ne change pas pendant la vie de l'instance — faute de threads, le
/// stockage local y est un statique. L'hôte le lit dans la mémoire linéaire
/// après avoir attrapé l'erreur, sans rappeler un module dont la pile est dans
/// un état inconnu.
///
/// Une fois et non à chaque appel : `set_hook` alloue, et une allocation par
/// image est ce que l'invariant interdit.
#[cfg(target_arch = "wasm32")]
fn install_panic_hook() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            message::set_orphan(panic_text(info.payload()));
        }));
    });
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
    #[cfg(target_arch = "wasm32")]
    install_panic_hook();

    let _fpenv = FpEnv::enter();

    // `AssertUnwindSafe` ne se pose qu'ici : c'est l'état défaillant qui la rend
    // honnête, puisque l'état laissé par une panique n'est plus jamais observé.
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(Failure::Abi(error)),
        Err(payload) => Err(Failure::Panic(payload)),
    }
}

/// L'accès au contexte du noyau que reçoit un appel exclusif.
///
/// Le partage est toujours permis ; l'exclusivité ne l'est que hors du rendu
/// des tuiles, et c'est l'état atomique du noyau qui le dit avant qu'aucune
/// référence exclusive n'existe. Une tuile qui tournerait pendant ce temps
/// serait un appel hors du contrat de l'ABI.
pub(crate) struct Core<'a> {
    cell: &'a UnsafeCell<Context>,
}

impl Core<'_> {
    /// Le contexte en partage, tel que les tuiles le voient.
    pub(crate) fn shared(&self) -> &Context {
        // SAFETY: aucune référence exclusive ne vit tant que `self` est
        // emprunté en partage : `exclusive` exige `&mut self`.
        unsafe { &*self.cell.get() }
    }

    /// Le contexte en exclusivité, ou [`Error::InvalidState`] pendant le rendu
    /// des tuiles.
    pub(crate) fn exclusive(&mut self) -> Result<&mut Context, AbiError> {
        if self.shared().is_rendering() {
            return Err(Error::InvalidState.into());
        }
        // SAFETY: hors du rendu, le contrat de l'ABI interdit tout autre appel
        // sur le contexte, et `&mut self` empêche une référence partagée issue
        // de ce même accès de survivre.
        Ok(unsafe { &mut *self.cell.get() })
    }
}

/// Enveloppe un appel qui porte sur un contexte, hors rendu d'une tuile.
///
/// Son message va dans le contexte. Il ne s'exécute pas en même temps qu'un
/// autre appel sur le même contexte ; seule la fin d'image peut croiser des
/// tuiles, qu'elle refuse d'attendre.
///
/// # Safety
///
/// `ctx` est nul, ou un handle rendu par `scg_create` et pas encore détruit.
pub(crate) unsafe fn with_context<F>(ctx: *mut ScgContext, f: F) -> i32
where
    F: FnOnce(Core<'_>) -> Result<(), AbiError>,
{
    // Vidé en entrant, pour qu'un thread recyclé ne rende jamais le message
    // d'une tâche précédente.
    message::clear_orphan();

    // SAFETY: précondition de la fonction — `ctx` est nul ou valide.
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        message::set_orphan(AbiError::NULL.message);
        return SCG_ERR_NULL;
    };

    // SAFETY: un appel exclusif est seul à écrire le message du contexte.
    unsafe { ctx.message_mut() }.clear();

    if ctx.faulted() {
        // SAFETY: même raisonnement.
        if !unsafe { ctx.take_tile_panic() } {
            // SAFETY: idem.
            unsafe { ctx.message_mut() }.set("a previous call panicked; this object is faulted");
        }
        return SCG_ERR_FAULTED;
    }

    let core = Core { cell: ctx.core() };
    match guarded(|| f(core)) {
        Ok(()) => SCG_OK,
        Err(Failure::Abi(error)) => {
            // SAFETY: un appel exclusif est seul à écrire le message.
            unsafe { ctx.message_mut() }.set(error.message);
            error.code
        }
        Err(Failure::Panic(payload)) => {
            ctx.mark_faulted();
            // SAFETY: idem.
            unsafe { ctx.message_mut() }.set(panic_text(&*payload));
            SCG_ERR_PANIC
        }
    }
}

/// Enveloppe le rendu d'une tuile, appelable depuis plusieurs threads à la
/// fois sur le même contexte.
///
/// Rien ne s'y écrit dans le contexte hors de ses atomiques : le message va
/// dans l'emplacement par thread, et une panique ne laisse son texte que par
/// [`ScgContext::fault_from_tile`].
///
/// # Safety
///
/// `ctx` est nul, ou un handle rendu par `scg_create` et pas encore détruit.
pub(crate) unsafe fn with_tile<F>(ctx: *mut ScgContext, f: F) -> i32
where
    F: FnOnce(&Context) -> Result<(), AbiError>,
{
    message::clear_orphan();

    // SAFETY: précondition de la fonction — `ctx` est nul ou valide.
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        message::set_orphan(AbiError::NULL.message);
        return SCG_ERR_NULL;
    };

    if ctx.faulted() {
        message::set_orphan("a previous call panicked; this object is faulted");
        return SCG_ERR_FAULTED;
    }

    let core = Core { cell: ctx.core() };
    match guarded(|| f(core.shared())) {
        Ok(()) => SCG_OK,
        Err(Failure::Abi(error)) => {
            message::set_orphan(error.message);
            error.code
        }
        Err(Failure::Panic(payload)) => {
            let text = panic_text(&*payload);
            ctx.fault_from_tile(text);
            message::set_orphan(text);
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
mod tests;
