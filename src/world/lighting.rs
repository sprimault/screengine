// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les lightmaps calculées d'une carte, et leur état.
//!
//! **Un objet à part, et non un champ de la carte.** Trois raisons, dans l'ordre :
//! l'hôte doit pouvoir sortir le cache et le rendre, et deux objets mettent
//! « source » et « cache dérivable » dans le type ; la carte est documentée
//! immuable et partageable en lecture entre contextes et entre threads, ce qu'un
//! appel qui la muterait lui retirerait après coup ; et la collision d'une étape
//! ultérieure charge une carte sans allouer un seul luxel.
//!
//! Il n'appartient à aucun contexte : sa création et ses calculs sont des appels
//! nommés, qui allouent, et que rien n'autorise entre le début et la fin d'une
//! image.

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::buffer::reserved;
use crate::error::{Error, Result};
use crate::format::World;
use crate::texture::Texture;

use super::atlas::{Atlas, pack};
use super::bake::bake;

/// Ce qu'une cellule a comme lightmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lightmap {
    /// Rien n'a encore été calculé pour elle.
    Absent,
    /// Sa lightmap est prête.
    Ready,
    /// Elle en a une, mais la cellule a changé depuis.
    ///
    /// Jamais rendue tant que rien ne modifie une carte chargée ; la variante
    /// existe parce que l'édition à chaud d'une étape ultérieure la produira, et
    /// qu'un état publié ne change pas de sens.
    Stale,
}

/// Ce qu'une cellule porte une fois cuite.
struct Lit {
    /// L'atlas, prêt à être échantillonné, mipmaps compris.
    texture: Arc<Texture>,
    /// Le rangement qui a produit ses texels : la soumission y lit le rectangle
    /// de chaque surface pour décaler ses coordonnées.
    atlas: Atlas,
}

/// Les lightmaps calculées d'une carte.
///
/// **`Lightmaps` et non `Lighting`** : ce dernier nom est pris par les plans
/// d'éclairage d'un triangle préparé, dans le rasteriseur, et deux notions
/// voisines sous un même nom finissent par se confondre dans une signature.
pub struct Lightmaps {
    /// Une entrée par cellule, dans l'ordre de la carte.
    cells: Vec<Option<Lit>>,
}

impl Lightmaps {
    /// Prépare le porteur des lightmaps d'une carte, sans rien calculer.
    ///
    /// L'allocation a lieu ici, dans un appel nommé, et jamais au premier calcul :
    /// c'est ce qui permet à un hôte de savoir quand il paie.
    pub fn new(world: &World) -> Result<Self> {
        let mut cells = reserved(world.cell_count() as usize)?;
        for _ in 0..world.cell_count() {
            cells.push(None);
        }
        Ok(Self { cells })
    }

    /// Calcule les lightmaps d'une cellule, désignée par son identifiant.
    ///
    /// **Par identifiant et non par rang** : le cache en stocke, et l'édition d'une
    /// étape ultérieure nommera la cellule qu'elle vient de modifier. Deux façons
    /// de désigner une cellule, dont l'une périme à la première suppression au
    /// milieu, c'est ce que la règle des identifiants stables interdit.
    ///
    /// Une cellule modifiée recalcule les siennes, jamais celles du niveau — mais
    /// **ses voisines immédiates deviennent fausses** : la lumière qui passait par
    /// la porte était calculée là-bas, et c'est à l'hôte de les rappeler.
    pub fn build(&mut self, world: &World, cell_id: u32) -> Result<()> {
        let index = world.cell_of(cell_id).ok_or(Error::UnknownResource)?;
        let atlas = pack(&world.cells()[index as usize])?;
        let baked = bake(world, index, atlas)?;
        let texture = Texture::load(baked.side, baked.side, &baked.texels)?;
        self.cells[index as usize] = Some(Lit {
            texture: Arc::new(texture),
            atlas: baked.atlas,
        });
        Ok(())
    }

    /// L'état de la lightmap d'une cellule.
    pub fn state(&self, world: &World, cell_id: u32) -> Result<Lightmap> {
        let index = world.cell_of(cell_id).ok_or(Error::UnknownResource)?;
        Ok(match self.cells[index as usize] {
            Some(_) => Lightmap::Ready,
            None => Lightmap::Absent,
        })
    }

    /// L'atlas d'une cellule, par son rang, ou `None` si rien n'est calculé.
    pub(crate) fn of(&self, index: u32) -> Option<(&Arc<Texture>, &Atlas)> {
        self.cells
            .get(index as usize)?
            .as_ref()
            .map(|lit| (&lit.texture, &lit.atlas))
    }
}

#[cfg(test)]
mod tests;
