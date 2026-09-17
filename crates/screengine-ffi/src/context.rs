// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le handle de contexte et la structure qui le crée.
//!
//! `ScgContextConfig` est la première structure publiée, donc le premier cas de
//! la règle d'extension : figée, complétée par des champs réservés, et faite de
//! champs dont aucun ne change de largeur selon la cible.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use screengine::{Config, Context};

use crate::entry::AbiError;
use crate::message::Message;

/// Configuration passed to `scg_create`.
///
/// Zero the whole structure before filling it in. The reserved fields must be
/// zero: that is what lets a later version give one a meaning without breaking
/// bindings already written against this header.
///
/// Every field is a `uint32_t`, so the offsets are the same on every target —
/// including 32-bit ones, where a `size_t` field would not be.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgContextConfig {
    /// Widest internal resolution this context will ever render, in pixels.
    /// From 1 to 2048. Every per-frame buffer is sized for it at creation.
    pub max_width: u32,
    /// Tallest internal resolution this context will ever render, in pixels.
    /// From 1 to 2048.
    pub max_height: u32,
    /// Initial internal width in pixels, at most `max_width`.
    pub width: u32,
    /// Initial internal height in pixels, at most `max_height`.
    pub height: u32,
    /// Tile side in pixels: 32 or 64. Any other value is rejected.
    pub tile_size: u32,
    /// Reserved. Must be zero.
    pub reserved0: u32,
    /// Reserved. Must be zero.
    pub reserved1: u32,
    /// Reserved. Must be zero.
    pub reserved2: u32,
}

impl ScgContextConfig {
    /// Convertit vers la configuration du noyau, ou refuse un champ réservé non
    /// nul.
    pub(crate) fn to_core(self) -> Result<Config, AbiError> {
        if self.reserved0 | self.reserved1 | self.reserved2 != 0 {
            return Err(AbiError::RESERVED);
        }
        Ok(Config {
            max_width: self.max_width,
            max_height: self.max_height,
            width: self.width,
            height: self.height,
            tile_size: self.tile_size,
        })
    }
}

/// An opaque rendering context.
///
/// Created by `scg_create`, released by `scg_destroy`. Use it from one thread
/// at a time, with a single exception: between `scg_frame_begin` and
/// `scg_frame_end`, distinct tiles may be rendered by `scg_frame_tile` from
/// distinct threads. Two contexts are independent and may each serve their own
/// thread.
pub struct ScgContext {
    /// Le contexte du noyau, partagé par les tuiles et pris en exclusivité hors
    /// du rendu. C'est l'état de rendu du noyau, atomique, qui départage.
    core: UnsafeCell<Context>,
    /// [`HEALTHY`], [`WRITING`] ou [`FAULTED`].
    fault: AtomicU8,
    /// Vrai quand le texte d'une panique de tuile attend d'être rendu par le
    /// message du contexte.
    pending: AtomicBool,
    /// Le texte de la première panique survenue dans une tuile.
    panic: UnsafeCell<Message>,
    /// Le message de `scg_last_error(ctx)`, écrit par les seuls appels
    /// exclusifs.
    message: UnsafeCell<Message>,
}

/// Aucun appel n'a paniqué.
const HEALTHY: u8 = 0;

/// Une tuile a paniqué et écrit son texte.
const WRITING: u8 = 1;

/// Un appel a paniqué : le contexte ne sert plus.
const FAULTED: u8 = 2;

impl ScgContext {
    /// Enveloppe un contexte du noyau.
    pub(crate) fn new(core: Context) -> Self {
        Self {
            core: UnsafeCell::new(core),
            fault: AtomicU8::new(HEALTHY),
            pending: AtomicBool::new(false),
            panic: UnsafeCell::new(Message::new()),
            message: UnsafeCell::new(Message::new()),
        }
    }

    /// Le contexte du noyau, en partage.
    pub(crate) fn core(&self) -> &UnsafeCell<Context> {
        &self.core
    }

    /// Vrai si un appel précédent a paniqué.
    pub(crate) fn faulted(&self) -> bool {
        self.fault.load(Ordering::SeqCst) != HEALTHY
    }

    /// Marque le contexte comme défaillant, depuis un appel exclusif.
    ///
    /// Sans retour en arrière : une panique signale un défaut du moteur, et
    /// aucune réinitialisation n'est fiable depuis un état inconnu.
    pub(crate) fn mark_faulted(&self) {
        self.fault.store(FAULTED, Ordering::SeqCst);
    }

    /// Marque le contexte comme défaillant depuis une tuile, et garde le texte
    /// de la première panique pour la fin d'image.
    ///
    /// Plusieurs tuiles peuvent paniquer à la fois : seule celle qui passe le
    /// contexte de sain à l'écriture touche au tampon, et elle ne signale le
    /// texte qu'une fois écrit.
    pub(crate) fn fault_from_tile(&self, text: &str) {
        if self
            .fault
            .compare_exchange(HEALTHY, WRITING, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        // SAFETY: le passage de sain à l'écriture ne réussit qu'une fois dans la
        // vie du contexte, et le tampon n'est lu qu'après `pending`, posé plus
        // bas : ce thread est le seul à y accéder.
        unsafe { (*self.panic.get()).set(text) };
        self.pending.store(true, Ordering::SeqCst);
        self.fault.store(FAULTED, Ordering::SeqCst);
    }

    /// Le message de ce contexte, pour un appel exclusif.
    ///
    /// # Safety
    ///
    /// Aucune autre référence au message ne vit : l'appelant est un appel
    /// exclusif sur le contexte, que les tuiles ne touchent pas.
    // Le lint vise une `&mut` tirée d'une `&self` sans cellule ; ici, elle
    // passe par l'`UnsafeCell`, et la précondition tient l'exclusivité.
    #[allow(clippy::mut_from_ref)]
    pub(crate) unsafe fn message_mut(&self) -> &mut Message {
        // SAFETY: précondition de la fonction.
        unsafe { &mut *self.message.get() }
    }

    /// Le message de ce contexte, en lecture.
    pub(crate) fn message(&self) -> &Message {
        // SAFETY: le message n'est écrit que par les appels exclusifs, et un
        // lecteur concurrent d'un appel exclusif est hors du contrat de l'ABI.
        unsafe { &*self.message.get() }
    }

    /// Recopie dans le message le texte d'une panique de tuile en attente, et
    /// rend vrai s'il y en avait un.
    ///
    /// # Safety
    ///
    /// Comme [`ScgContext::message_mut`].
    pub(crate) unsafe fn take_tile_panic(&self) -> bool {
        if !self.pending.swap(false, Ordering::SeqCst) {
            return false;
        }
        // SAFETY: `pending` n'est posé qu'une fois le texte écrit, et plus rien
        // ne l'écrit ensuite ; le message relève de la précondition.
        unsafe { (*self.message.get()).copy_from(&*self.panic.get()) };
        true
    }
}

#[cfg(test)]
mod tests;
