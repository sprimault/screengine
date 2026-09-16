// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Les maths du noyau.
//!
//! Elles n'appellent jamais la libm : `sinf`, `cosf` et `sqrtf` ne rendent pas
//! les mêmes bits d'une implémentation à l'autre, et l'empreinte de conformance
//! s'en trouverait dépendante de la bibliothèque C de la cible. Trigonométrie et
//! racine inverse passeront par les tables de ce module.

pub mod fixed;
