// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le handle de texture.
//!
//! **Une texture n'appartient à aucun contexte.** Elle se charge et se détruit
//! sans en avoir un sous la main, ce qu'impose la collision sans rendu d'une
//! étape ultérieure et ce dont profite déjà un hôte qui prépare ses ressources
//! avant d'ouvrir sa fenêtre. Ses erreurs vont donc dans l'emplacement par
//! thread, que `scg_last_error(NULL)` lit.
//!
//! Immuable une fois chargée, elle se partage en lecture entre contextes et
//! entre threads : le compteur de références est atomique, et le moteur en
//! garde une jusqu'à la fin de l'image qui l'emploie.

use std::sync::Arc;

use screengine::Texture;

/// An opaque handle to a loaded texture.
///
/// Created by `scg_texture_load`, released by `scg_texture_destroy`. It belongs
/// to no context: the same texture may be submitted to several, from several
/// threads, and the engine keeps it alive for as long as a frame references it.
pub struct ScgTexture {
    /// La ressource du noyau, partagée avec les contextes qui la dessinent.
    pub(crate) inner: Arc<Texture>,
}
