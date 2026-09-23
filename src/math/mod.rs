// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les maths du noyau.
//!
//! Elles n'appellent jamais la libm : `sinf`, `cosf` et `sqrtf` ne rendent pas
//! les mêmes bits d'une implémentation à l'autre, et l'empreinte de conformance
//! s'en trouverait dépendante de la bibliothèque C de la cible. Trigonométrie et
//! racine inverse passent par les tables de ce module, et `clippy.toml` refuse
//! les méthodes flottantes qui y mèneraient.

mod affine;
mod angle;
// Rien ne l'appelle encore : c'est la table de post-traitement qui le
// consommera, et le lot qui l'écrit retire cette autorisation.
#[cfg_attr(not(test), allow(dead_code))]
pub mod exp2;
pub mod fixed;
pub mod projection;
mod quat;
mod rsqrt;
mod vector;

pub use affine::Affine3;
pub use angle::Angle;
pub use fixed::MAX_TEXEL_COORD;
pub use projection::Projection;
pub use quat::Quat;
pub use vector::Vec3;

#[cfg(test)]
mod tests;
