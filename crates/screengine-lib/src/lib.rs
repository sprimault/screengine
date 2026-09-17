// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La bibliothèque publiée de Screengine.
//!
//! Aucun code : les points d'entrée sont ceux de screengine-ffi, que la
//! bibliothèque dynamique exporte et que la statique embarque. Ce crate n'existe
//! que pour leur donner le nom `screengine`.

pub use screengine_ffi::*;
