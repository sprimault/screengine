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
use crate::format::world::{Cell, Surface};
use crate::texture::Texture;

use crate::format::cache::{self, Entry, Record};

use super::atlas::{Atlas, pack};
use super::bake::bake;
use super::digest::fingerprint;

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
    /// L'empreinte de la cellule au moment du calcul.
    ///
    /// Gardée plutôt que recalculée à l'écriture : elle dit ce que cette lightmap
    /// *a* vu, et une carte que l'édition à chaud d'une étape ultérieure modifiera
    /// n'en dirait plus rien. C'est aussi ce qui permettra à `state` de rendre
    /// « périmée » sans recuire.
    fingerprint: u64,
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
            fingerprint: fingerprint(world, index)?,
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

    /// La longueur qu'occuperait le cache, sans rien sérialiser.
    ///
    /// **L'hôte mesure, alloue, puis remplit** : c'est le patron de l'ABI, et la
    /// mesure ne doit pas coûter une sérialisation jetée. Zéro quand aucune cellule
    /// n'est cuite — un bloc vide reste un bloc valide, que relire ne rend rien.
    pub fn save_len(&self, world: &World) -> Result<usize> {
        let records = self.records(world)?;
        cache::measure(&records).ok_or(Error::OutOfMemory)
    }

    /// Écrit le cache dans le tampon de l'hôte.
    ///
    /// `out` doit faire exactement la longueur que [`Lightmaps::save_len`] annonce,
    /// mesurée sur le même état : un tampon d'une autre longueur est refusé sans
    /// rien écrire.
    pub fn save(&self, world: &World, out: &mut [u8]) -> Result<()> {
        cache::write(&self.records(world)?, out)
    }

    /// Ce que l'état courant déposerait, dans l'ordre des cellules de la carte.
    ///
    /// L'ordre du fichier est déjà celui des identifiants croissants — le
    /// chargement le vérifie —, donc les enregistrements sortent triés sans tri.
    fn records<'a>(&'a self, world: &'a World) -> Result<Vec<Record<'a>>> {
        let cells = world.cells();
        let mut out = reserved(self.cells.len())?;
        for (index, entry) in self.cells.iter().enumerate() {
            let Some(lit) = entry else {
                continue;
            };
            out.push(Record {
                cell: &cells[index],
                fingerprint: lit.fingerprint,
                atlas: &lit.atlas,
                texels: lit.texture.level_texels(0),
            });
        }
        Ok(out)
    }

    /// Reprend un cache, et rend le nombre d'entrées acceptées.
    ///
    /// **Une entrée écartée n'est pas une erreur** : ni celle dont l'empreinte ne
    /// concorde plus, ni celle qui désigne une cellule que la carte ne porte pas.
    /// Un cache partiellement périmé est le cas normal d'un éditeur, et refuser le
    /// bloc entier ferait tout recuire pour un mur déplacé. Seul un bloc malformé
    /// remonte une erreur.
    ///
    /// Ce que porte le bloc ne décide de rien : chaque entrée est confrontée à la
    /// carte chargée. Une empreinte se vole, elle ne prouve rien — c'est un FNV, pas
    /// une signature —, donc le rangement relu est vérifié pour lui-même.
    pub fn restore(&mut self, world: &World, bytes: &[u8]) -> Result<u32> {
        let cells = world.cells();
        let mut accepted = 0;
        for entry in cache::read(bytes)? {
            let Some(index) = world.cell_of(entry.cell_id) else {
                continue;
            };
            if entry.fingerprint != fingerprint(world, index)? {
                continue;
            }
            if !fits(&entry, &cells[index as usize]) {
                continue;
            }
            let texture = Texture::load(entry.atlas.side, entry.atlas.side, entry.texels)?;
            self.cells[index as usize] = Some(Lit {
                texture: Arc::new(texture),
                atlas: entry.atlas,
                fingerprint: entry.fingerprint,
            });
            accepted += 1;
        }
        Ok(accepted)
    }

    /// L'atlas d'une cellule, par son rang, ou `None` si rien n'est calculé.
    pub(crate) fn of(&self, index: u32) -> Option<(&Arc<Texture>, &Atlas)> {
        self.cells
            .get(index as usize)?
            .as_ref()
            .map(|lit| (&lit.texture, &lit.atlas))
    }
}

/// Le rangement relu décrit-il bien cette cellule ?
///
/// **L'empreinte ne suffit pas à s'en assurer.** C'est un FNV, pas une signature :
/// un bloc forgé peut la porter juste et décrire n'importe quel rangement. Or la
/// soumission lit `slots[rang de la surface]` sans borne — un rectangle de moins
/// ferait paniquer une image —, et un rectangle qui déborde de l'atlas ferait lire
/// les luxels d'une autre surface. Les deux se vérifient ici, une fois, plutôt qu'à
/// chaque image.
fn fits(entry: &Entry<'_>, cell: &Cell) -> bool {
    if entry.surfaces.len() != cell.surfaces.len() {
        return false;
    }
    if entry
        .surfaces
        .iter()
        .copied()
        .ne(cell.surfaces.iter().map(Surface::id))
    {
        return false;
    }
    entry.atlas.slots.iter().all(|slot| {
        slot.width <= entry.atlas.side
            && slot.height <= entry.atlas.side
            && slot.x <= entry.atlas.side - slot.width
            && slot.y <= entry.atlas.side - slot.height
    })
}

#[cfg(test)]
mod tests;
