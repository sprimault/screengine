// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Le handle de contexte et la structure qui le crée.
//!
//! `ScgContextConfig` est la première structure publiée, donc le premier cas de
//! la règle d'extension : figée, complétée par des champs réservés, et faite de
//! champs dont aucun ne change de largeur selon la cible.

use screengine::{Config, Context, Error};

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
    /// Convertit vers la configuration du noyau, ou refuse.
    ///
    /// Les champs réservés se contrôlent ici et pas dans le noyau : ils
    /// n'existent que parce que l'ABI est figée, et le noyau n'a pas à savoir
    /// qu'elle l'est.
    pub(crate) fn to_core(self) -> Result<Config, Error> {
        if self.reserved0 | self.reserved1 | self.reserved2 != 0 {
            return Err(Error::InvalidArgument);
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
/// at a time; two contexts are independent and may each serve their own.
pub struct ScgContext {
    inner: Context,
    poisoned: bool,
    message: Message,
}

impl ScgContext {
    /// Enveloppe un contexte du noyau.
    pub(crate) fn new(inner: Context) -> Self {
        Self {
            inner,
            poisoned: false,
            message: Message::new(),
        }
    }

    /// Le contexte du noyau, pour un appel qui va s'exécuter.
    pub(crate) fn inner_mut(&mut self) -> &mut Context {
        &mut self.inner
    }

    /// Vrai si un appel précédent a paniqué.
    pub(crate) fn poisoned(&self) -> bool {
        self.poisoned
    }

    /// Marque le contexte comme empoisonné.
    ///
    /// Sans retour en arrière : une panique signale un défaut du moteur, et
    /// aucune réinitialisation n'est fiable depuis un état inconnu.
    pub(crate) fn poison(&mut self) {
        self.poisoned = true;
    }

    /// Le message de ce contexte.
    pub(crate) fn message_mut(&mut self) -> &mut Message {
        &mut self.message
    }

    /// Le message de ce contexte, en lecture.
    pub(crate) fn message(&self) -> &Message {
        &self.message
    }
}

#[cfg(test)]
mod tests;
