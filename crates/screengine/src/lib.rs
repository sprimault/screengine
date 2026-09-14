// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Noyau de Screengine.
//!
//! Il reçoit une scène et un tampon, et remplit le tampon. Il n'ouvre ni
//! fichier, ni fenêtre, ne lit aucune horloge et ne crée aucun thread : tout
//! cela appartient à l'hôte. C'est ce qui permet au même code de servir sur
//! bureau, sur wasm et sur téléphone sans chemin de compilation parallèle.

#![no_std]
// Les chemins SIMD l'autoriseront localement, et ce seront les seuls.
#![deny(unsafe_code)]

extern crate alloc;
