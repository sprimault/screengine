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

use crate::entry::AbiError;

/// An opaque handle to a loaded map.
///
/// Created by `scg_world_load`, released by `scg_world_destroy`. It belongs to
/// no context: the same map may be submitted to several, from several threads.
///
/// **Every function that only reads a loaded map is callable from any thread**,
/// on the same handle, simultaneously and without limit — the map is immutable
/// once loaded, and such a call takes no resource and holds no state. That is
/// the criterion, written once: it covers the sweeps, the ray queries, locating
/// and tracking a cell, and every accessor keyed on an identifier. The
/// per-function notes do not repeat it, and a function added later inherits it
/// by being of that kind rather than by being named here.
///
/// What is **not** of that kind, and says so on its own declaration: anything
/// that takes a context, and `scg_lighting_build`, which fills a cache.
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
/// named call that allocates, and so is restoring a cache — neither is allowed
/// between the start and the end of a frame. Belonging to no context is also why
/// that is a precondition and not a refusal: there is no frame here to check
/// against.
pub struct ScgLighting {
    /// Les lightmaps calculées, côté noyau.
    pub(crate) inner: Lightmaps,
    /// La carte dont elles viennent, gardée vivante.
    ///
    /// **Repasser le `ScgWorld` à chaque appel a été écarté** : cela ferait de
    /// « la même carte » une précondition que rien ne vérifie, et un hôte qui se
    /// tromperait de carte obtiendrait des rectangles pris sur une autre
    /// géométrie, sans erreur.
    ///
    /// **Les quatre appels de cuisson tiennent cette clause en lisant ce
    /// champ** ; les deux soumissions éclairées, elles, reçoivent une carte de
    /// l'hôte, et c'est [`paired`] qui les confronte.
    pub(crate) world: Arc<World>,
}

/// Le porteur vient-il de cette carte, et que passer au noyau ?
///
/// **Un seul endroit pour les deux soumissions éclairées**, qui prennent toutes
/// deux un `world` et un `lighting` indépendants : la confrontation et la
/// conversion vers ce que le noyau attend s'y écrivent ensemble, de sorte qu'un
/// site ne puisse pas faire l'une sans l'autre. C'était le défaut — les deux
/// convertissaient sans confronter, et une surface de plus dans la cellule
/// soumise faisait indexer l'atlas cuit hors de ses emplacements.
///
/// L'égalité porte sur le **compteur de références**, donc sur l'identité de la
/// carte et non sur sa forme. C'est plus fort que ce que le noyau peut faire, et
/// c'est la seule chose que ce handle achète : il tient la carte vivante, donc il
/// peut la reconnaître.
///
/// Un porteur nul n'est pas une erreur — un décor se soumet sans lightmaps.
pub(crate) fn paired<'a>(
    lighting: Option<&'a ScgLighting>,
    world: &ScgWorld,
) -> Result<Option<&'a Lightmaps>, AbiError> {
    match lighting {
        None => Ok(None),
        Some(lighting) if Arc::ptr_eq(&lighting.world, &world.inner) => Ok(Some(&lighting.inner)),
        Some(_) => Err(AbiError::LIGHTING_WORLD),
    }
}
