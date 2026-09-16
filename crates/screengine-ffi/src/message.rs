// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Les tampons que `scg_last_error` rend à l'hôte.
//!
//! Deux emplacements : celui que porte chaque contexte, et un emplacement par
//! thread pour les appels qui n'ont pas de contexte auquel se rattacher.

use core::cell::UnsafeCell;
use core::ffi::c_char;

/// Taille d'un tampon de message, terminateur nul compris.
///
/// Fixe, parce que le chemin d'erreur ne doit rien allouer : un message trop
/// long est tronqué, jamais rejeté. Une panique y écrit la charge utile
/// formatée par la bibliothèque standard, qui tient largement en dessous.
pub(crate) const CAPACITY: usize = 256;

/// Un message de longueur bornée, terminé par un octet nul.
#[derive(Debug)]
pub(crate) struct Message {
    bytes: [u8; CAPACITY],
}

impl Message {
    /// Un message vide.
    pub(crate) const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
        }
    }

    /// Remplace le message.
    ///
    /// Tronque sur une frontière de caractère : couper au milieu d'une séquence
    /// UTF-8 rendrait une chaîne qu'une liaison ne peut pas décoder, et le
    /// texte d'une panique n'est pas garanti ASCII.
    pub(crate) fn set(&mut self, text: &str) {
        let mut end = text.len().min(CAPACITY - 1);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.bytes[..end].copy_from_slice(&text.as_bytes()[..end]);
        self.bytes[end] = 0;
    }

    /// Vide le message.
    pub(crate) fn clear(&mut self) {
        self.bytes[0] = 0;
    }

    /// Le pointeur rendu à l'hôte, valide jusqu'à la prochaine écriture.
    pub(crate) fn as_ptr(&self) -> *const c_char {
        self.bytes.as_ptr().cast()
    }
}

thread_local! {
    /// Le message des appels qui n'ont pas de contexte : création, allocation.
    ///
    /// Un tableau nu plutôt qu'un `String` ou un `RefCell`, et c'est le point
    /// important : un thread-local à `Drop` coûte une clé pthread — Android en
    /// plafonne le nombre par processus — et surtout son accès échoue pendant
    /// la destruction du stockage local du thread. Ce serait une panique dans
    /// `scg_last_error`, la seule fonction sans code de retour pour la porter.
    /// Sans destructeur, ce cas n'existe pas.
    static ORPHAN: UnsafeCell<Message> = const { UnsafeCell::new(Message::new()) };
}

/// Écrit le message sans contexte du thread courant.
pub(crate) fn set_orphan(text: &str) {
    ORPHAN.with(|slot| {
        // SAFETY: `slot` appartient au thread courant et personne d'autre ne
        // l'atteint. La référence exclusive ne survit pas à l'appel à `set`, et
        // la seule autre voie d'accès, `orphan_ptr`, ne rend qu'un pointeur
        // brut dont la durée de validité est documentée dans le header.
        let message = unsafe { &mut *slot.get() };
        message.set(text);
    });
}

/// Vide le message sans contexte du thread courant.
pub(crate) fn clear_orphan() {
    ORPHAN.with(|slot| {
        // SAFETY: même raisonnement que `set_orphan`.
        let message = unsafe { &mut *slot.get() };
        message.clear();
    });
}

/// Le pointeur vers le message sans contexte du thread courant.
///
/// Valide jusqu'au prochain appel sur ce thread, comme le dit le header. Le
/// pointeur sort de la fermeture : il vise le stockage local du thread, qui vit
/// aussi longtemps que lui.
pub(crate) fn orphan_ptr() -> *const c_char {
    ORPHAN.with(|slot| {
        // SAFETY: lecture seule d'un emplacement propre au thread courant,
        // sans référence exclusive vivante ailleurs.
        let message = unsafe { &*slot.get() };
        message.as_ptr()
    })
}

#[cfg(test)]
mod tests;
