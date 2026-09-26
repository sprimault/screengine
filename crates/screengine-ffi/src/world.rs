// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le handle de carte.
//!
//! Mêmes règles que le maillage, et c'est voulu : deux ressources chargées
//! depuis un bloc n'ont aucune raison de se manipuler autrement, et une liaison
//! écrite pour l'une se relit pour l'autre. Elle n'appartient à aucun contexte,
//! ce qu'impose la collision sans rendu d'une étape ultérieure — un serveur de
//! jeu charge une carte sans jamais allouer de tampon d'image.

use std::sync::Arc;

use screengine::{Lightmaps, World};

/// An opaque handle to a loaded map.
///
/// Created by `scg_world_load`, released by `scg_world_destroy`. It belongs to
/// no context: the same map may be submitted to several, from several threads.
pub struct ScgWorld {
    /// La ressource du noyau, immuable une fois chargée.
    ///
    /// **Partagée par compteur de références**, ce que l'hôte ne voit pas : c'est
    /// ce qui permet à un handle de lightmaps de garder vivante la carte dont il
    /// vient, et donc à l'hôte de détruire les deux dans l'ordre qu'il veut.
    pub(crate) inner: Arc<World>,
}

/// An opaque handle to a map's computed lightmaps.
///
/// Created by `scg_lighting_create`, released by `scg_lighting_destroy`, filled
/// one cell at a time by `scg_lighting_build`. It keeps the map it was created
/// from alive, so the host may destroy the two in either order.
///
/// **It belongs to no context**, like every resource: computing lightmaps is a
/// named call that allocates, and nothing allows it between the start and the end
/// of a frame.
pub struct ScgLighting {
    /// Les lightmaps calculées, côté noyau.
    pub(crate) inner: Lightmaps,
    /// La carte dont elles viennent, gardée vivante.
    ///
    /// **Repasser le `ScgWorld` à chaque appel a été écarté** : cela ferait de
    /// « la même carte » une précondition que rien ne vérifie, et un hôte qui se
    /// tromperait de carte obtiendrait des rectangles pris sur une autre
    /// géométrie, sans erreur.
    pub(crate) world: Arc<World>,
}
