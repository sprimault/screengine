// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le handle de carte.
//!
//! Mêmes règles que le maillage, et c'est voulu : deux ressources chargées
//! depuis un bloc n'ont aucune raison de se manipuler autrement, et une liaison
//! écrite pour l'une se relit pour l'autre. Elle n'appartient à aucun contexte,
//! ce qu'impose la collision sans rendu d'une étape ultérieure — un serveur de
//! jeu charge une carte sans jamais allouer de tampon d'image.

use screengine::World;

/// An opaque handle to a loaded map.
///
/// Created by `scg_world_load`, released by `scg_world_destroy`. It belongs to
/// no context: the same map may be submitted to several, from several threads.
pub struct ScgWorld {
    /// La ressource du noyau, immuable une fois chargée.
    pub(crate) inner: World,
}

/// An opaque handle to a map's computed lightmaps.
///
/// **Nothing creates one yet**, so the only value `scg_submit_world_visible`
/// accepts for it is `NULL`; anything else is rejected. The parameter exists from
/// the first version on purpose: a published signature never changes, and adding
/// it later would mean a second submission function, for good.
pub struct ScgLighting {
    /// Réservé au lot qui calcule les lightmaps.
    ///
    /// Un champ privé plutôt qu'une structure vide : `cbindgen` en rend un type
    /// incomplet, que l'hôte ne peut donc ni construire ni déréférencer, et c'est
    /// ce que « handle opaque » veut dire.
    #[allow(dead_code)]
    pub(crate) reserved: u8,
}
