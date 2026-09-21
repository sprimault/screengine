// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

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

mod buffer;
mod context;
mod error;
mod math;
mod raster;
mod scene;
#[cfg(test)]
mod testing;
mod texture;

pub use context::{
    BYTES_PER_PIXEL, Config, Context, Frame, MAX_RESOLUTION, Output, Rows, TILE_SIZES,
    TRIANGLE_CAPACITY,
};
pub use error::{Argument, Error, Result};
pub use math::{Affine3, Angle, MAX_TEXEL_COORD, Quat, Vec3};
pub use raster::Rect;
pub use scene::{Camera, Color, Triangle, VertexUv};
pub use texture::{MAX_TEXTURE_SIZE, Texture};
