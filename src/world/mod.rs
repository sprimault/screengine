// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le monde : cellules, portails, traversée.
//!
//! La carte elle-même — son décodage, ses dispositions binaires — vit dans
//! [`crate::format`] : ce module-ci ne lit aucun octet, il exploite ce que le
//! chargement a dérivé.
//!
//! Il naît avec la fenêtre de traversée, qui est la pièce dont tout le reste
//! dépend : la pile de (cellule, fenêtre) la réduit de portail en portail, et
//! une fenêtre trop étroite laisse un trou définitif dans l'image.

pub(crate) mod atlas;
pub(crate) mod locate;
pub(crate) mod traversal;
pub(crate) mod window;
