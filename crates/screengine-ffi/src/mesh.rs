// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le handle de maillage.
//!
//! **Un maillage n'appartient à aucun contexte**, comme une texture : il se
//! charge et se détruit sans en avoir un sous la main, ce qu'impose la collision
//! sans rendu d'une étape ultérieure. Ses erreurs vont donc dans l'emplacement
//! par thread, que `scg_last_error(NULL)` lit.
//!
//! Il ne porte pas de compteur de références, et c'est la différence avec la
//! texture : le contexte garde une référence forte sur une texture jusqu'à la
//! fin de l'image, alors que plus rien ne lit un maillage après le retour de la
//! soumission, qui transforme et découpe immédiatement.

use screengine::Mesh;

/// An opaque handle to a loaded mesh.
///
/// Created by `scg_mesh_load`, released by `scg_mesh_destroy`. It belongs to no
/// context: the same mesh may be submitted to several, from several threads.
pub struct ScgMesh {
    /// La ressource du noyau, immuable une fois chargée.
    pub(crate) inner: Mesh,
}
