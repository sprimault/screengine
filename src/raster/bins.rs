// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le découpage de l'image en tuiles, et la répartition des triangles.
//!
//! La répartition se fait une fois par image, avant toute tuile, et c'est la
//! seule phase qui écrit dans un état partagé : ensuite, chaque tuile ne fait
//! que la lire.

use alloc::vec::Vec;

use crate::buffer::reserved;
use crate::error::Result;

use super::{Prepared, Rect};

/// Au-delà de ce nombre de tuiles touchées, un triangle n'est plus référencé
/// dans chacune : il rejoint la liste des grands triangles, que chaque tuile
/// parcourt.
///
/// Une surface de niveau couvre souvent des centaines de tuiles. Référencée
/// partout, la mémoire de répartition ne se bornerait plus qu'à la résolution
/// maximale multipliée par la capacité de triangles — plus d'un gigaoctet à
/// 2048 de côté. Bornée ici, elle vaut seize index par triangle.
pub const LARGE_TILES: u32 = 16;

/// Le découpage de l'image courante en tuiles.
///
/// Les tuiles se numérotent ligne par ligne, de gauche à droite puis de haut en
/// bas ; celles de la dernière colonne et de la dernière ligne sont partielles
/// quand la résolution n'est pas un multiple du côté.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    width: u32,
    height: u32,
    tile: u32,
    columns: u32,
    rows: u32,
}

impl Grid {
    /// Le découpage d'une image de `width × height` pixels en tuiles de `tile`.
    pub fn new(width: u32, height: u32, tile: u32) -> Self {
        Self {
            width,
            height,
            tile,
            columns: width.div_ceil(tile),
            rows: height.div_ceil(tile),
        }
    }

    /// Le nombre de tuiles.
    pub fn count(&self) -> u32 {
        self.columns * self.rows
    }

    /// L'image entière.
    pub fn image(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        }
    }

    /// Le rectangle d'une tuile, `index` étant inférieur à [`Grid::count`].
    pub fn rect(&self, index: u32) -> Rect {
        let x = index % self.columns * self.tile;
        let y = index / self.columns * self.tile;
        Rect {
            x,
            y,
            width: self.tile.min(self.width - x),
            height: self.tile.min(self.height - y),
        }
    }

    /// Les tuiles que la boîte d'un triangle touche, `(colonne, ligne)` de la
    /// première puis de la dernière, ou `None` s'il est hors de l'image.
    fn span(&self, triangle: &Prepared) -> Option<(u32, u32, u32, u32)> {
        let (x0, y0, x1, y1) = triangle.bounds();
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width as i32 - 1);
        let y1 = y1.min(self.height as i32 - 1);
        if x0 > x1 || y0 > y1 {
            return None;
        }
        Some((
            x0 as u32 / self.tile,
            y0 as u32 / self.tile,
            x1 as u32 / self.tile,
            y1 as u32 / self.tile,
        ))
    }
}

/// Les triangles de chaque tuile, par index de soumission.
///
/// Rangés en tableau compact : les références de toutes les tuiles bout à
/// bout, et pour chaque tuile le décalage où commencent les siennes. Deux
/// passes, un comptage puis un remplissage, et aucune liste chaînée : la
/// mémoire se borne à la création, et une tuile lit ses références d'un bloc.
#[derive(Debug)]
pub struct Bins {
    /// `offsets[t]..offsets[t + 1]` sont les références de la tuile `t`.
    offsets: Vec<u32>,
    /// Le prochain emplacement libre de chaque tuile, pendant le remplissage.
    cursor: Vec<u32>,
    refs: Vec<u32>,
    large: Vec<u32>,
}

impl Bins {
    /// Réserve de quoi répartir `triangles` triangles sur `tiles` tuiles.
    pub fn new(tiles: usize, triangles: usize) -> Result<Self> {
        Ok(Self {
            offsets: reserved(tiles + 1)?,
            cursor: reserved(tiles)?,
            refs: reserved(triangles * LARGE_TILES as usize)?,
            large: reserved(triangles)?,
        })
    }

    /// Répartit `triangles` sur les tuiles de `grid`.
    ///
    /// Sans allocation tant que `grid` et `triangles` restent dans ce qui a été
    /// réservé : un petit triangle occupe au plus [`LARGE_TILES`] références,
    /// un grand une seule place dans sa liste.
    ///
    /// Le rejet est conservateur, par la boîte du triangle : une référence de
    /// trop ne change rien à l'image, une tuile oubliée la trouerait dans une
    /// seule configuration.
    pub fn build(&mut self, grid: &Grid, triangles: &[Prepared]) {
        let count = grid.count() as usize;
        self.offsets.clear();
        self.offsets.resize(count + 1, 0);
        self.large.clear();

        let small = |span: (u32, u32, u32, u32)| {
            let (tx0, ty0, tx1, ty1) = span;
            (tx1 - tx0 + 1) * (ty1 - ty0 + 1) <= LARGE_TILES
        };

        for (index, triangle) in triangles.iter().enumerate() {
            let Some(span) = grid.span(triangle) else {
                continue;
            };
            if !small(span) {
                self.large.push(index as u32);
                continue;
            }
            let (tx0, ty0, tx1, ty1) = span;
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    self.offsets[(ty * grid.columns + tx) as usize + 1] += 1;
                }
            }
        }

        for t in 0..count {
            self.offsets[t + 1] += self.offsets[t];
        }

        self.refs.clear();
        self.refs.resize(self.offsets[count] as usize, 0);
        self.cursor.clear();
        self.cursor.extend_from_slice(&self.offsets[..count]);

        // Même parcours, dans le même ordre : les références de chaque tuile
        // sortent triées par index de soumission, sans tri.
        for (index, triangle) in triangles.iter().enumerate() {
            let Some(span) = grid.span(triangle).filter(|span| small(*span)) else {
                continue;
            };
            let (tx0, ty0, tx1, ty1) = span;
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    let slot = &mut self.cursor[(ty * grid.columns + tx) as usize];
                    self.refs[*slot as usize] = index as u32;
                    *slot += 1;
                }
            }
        }
    }

    /// Les triangles à dessiner dans la tuile `index`, dans l'ordre de
    /// soumission.
    pub fn tile(&self, index: u32) -> Merge<'_> {
        let (start, end) = (
            self.offsets[index as usize] as usize,
            self.offsets[index as usize + 1] as usize,
        );
        Merge {
            small: &self.refs[start..end],
            large: &self.large,
        }
    }
}

/// Les références propres à une tuile et la liste des grands triangles,
/// fusionnées par index croissant.
///
/// L'ordre n'est pas un détail : à profondeur égale, le test strict garde le
/// premier triangle soumis, et l'image d'une tuile doit être celle de l'image
/// entière.
#[derive(Debug, Clone)]
pub struct Merge<'a> {
    small: &'a [u32],
    large: &'a [u32],
}

impl Iterator for Merge<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let from_small = match (self.small.first(), self.large.first()) {
            (Some(s), Some(l)) => s < l,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => return None,
        };
        let list = if from_small {
            &mut self.small
        } else {
            &mut self.large
        };
        let (first, rest) = list.split_first()?;
        *list = rest;
        Some(*first)
    }
}

#[cfg(test)]
mod tests;
