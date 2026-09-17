// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Une image en cours de rendu, tuile par tuile.
//!
//! Entre le début et la fin, des tuiles distinctes peuvent se rendre depuis des
//! threads distincts : la [`Frame`] ne se lit qu'en partage, et chaque tuile
//! n'écrit que dans sa pile et dans son rectangle du tampon de l'hôte.

use core::sync::atomic::Ordering;

use crate::context::{BYTES_PER_PIXEL, CLEAR_COLOR, Context};
use crate::error::{Argument, Error, Result};
use crate::raster::{Grid, Rect, Target, fill};

/// Le plus grand côté de tuile, qui dimensionne le tampon de travail posé sur
/// la pile.
const MAX_TILE: usize = 64;

/// Où une image écrit ses pixels : le tampon de l'hôte, ou une partie de
/// celui-ci.
///
/// Un trait plutôt qu'une tranche parce que deux tuiles de la même ligne
/// partagent des lignes du tampon : pour les rendre depuis deux threads, la
/// frontière C fournit à chacune ses propres morceaux de ligne, là où un appelant
/// Rust passe le tampon entier ou une bande.
pub trait Output {
    /// Refuse une sortie qui ne peut pas recevoir `rect`, avant que la moindre
    /// tuile ne soit prise.
    fn check(&self, rect: Rect) -> Result<()>;

    /// Les `width × 4` octets de la ligne `y` qui commencent à la colonne `x`,
    /// en coordonnées de l'image, ou `None` s'ils ne sont pas dans la sortie.
    fn span(&mut self, x: u32, y: u32, width: u32) -> Option<&mut [u8]>;
}

/// Un tampon d'hôte rangé par lignes, ou une bande de lignes consécutives.
#[derive(Debug)]
pub struct Rows<'a> {
    pixels: &'a mut [u8],
    stride: u32,
    first_row: u32,
}

impl<'a> Rows<'a> {
    /// Le tampon entier, `stride` pixels par ligne.
    pub fn new(pixels: &'a mut [u8], stride: u32) -> Self {
        Self::band(pixels, stride, 0)
    }

    /// Une bande du tampon dont la première ligne est la ligne `first_row` de
    /// l'image.
    ///
    /// C'est ce qui permet à un appelant Rust de rendre sur plusieurs threads
    /// sans `unsafe` : des bandes disjointes s'obtiennent par `chunks_mut`, et
    /// chaque thread rend les tuiles de la sienne.
    pub fn band(pixels: &'a mut [u8], stride: u32, first_row: u32) -> Self {
        Self {
            pixels,
            stride,
            first_row,
        }
    }

    /// Le décalage en octets du début de la ligne `y` de l'image.
    fn row_start(&self, y: u32) -> Option<usize> {
        (y.checked_sub(self.first_row)? as usize)
            .checked_mul(self.stride as usize)?
            .checked_mul(BYTES_PER_PIXEL)
    }
}

impl Output for Rows<'_> {
    fn check(&self, rect: Rect) -> Result<()> {
        if rect.x + rect.width > self.stride {
            return Err(Error::InvalidArgument(Argument::Stride));
        }
        if rect.y < self.first_row {
            return Err(Error::InvalidArgument(Argument::BufferLength));
        }
        // Seul le `stride` peut faire déborder le produit : la région est déjà
        // bornée par l'image.
        let end = self
            .row_start(rect.y + rect.height)
            .ok_or(Error::InvalidArgument(Argument::Stride))?;
        if self.pixels.len() < end {
            return Err(Error::InvalidArgument(Argument::BufferLength));
        }
        Ok(())
    }

    fn span(&mut self, x: u32, y: u32, width: u32) -> Option<&mut [u8]> {
        let start = self.row_start(y)? + x as usize * BYTES_PER_PIXEL;
        self.pixels
            .get_mut(start..start + width as usize * BYTES_PER_PIXEL)
    }
}

/// Le tampon de couleur d'une région, vu comme un puits de remplissage.
struct Scratch<'a> {
    pixels: &'a mut [u32],
    rect: Rect,
}

impl Target for Scratch<'_> {
    fn put(&mut self, x: i32, y: i32, color: u32) {
        let row = (y - self.rect.y as i32) as usize;
        let column = (x - self.rect.x as i32) as usize;
        self.pixels[row * self.rect.width as usize + column] = color;
    }
}

/// Une image entre son début et sa fin.
///
/// `Sync` : des tuiles d'index distincts se rendent depuis des threads
/// distincts. Elle emprunte le contexte, donc rien d'autre ne le touche tant
/// qu'elle vit.
#[derive(Debug)]
pub struct Frame<'a> {
    context: &'a Context,
    grid: Grid,
}

impl<'a> Frame<'a> {
    /// Ouvre le rendu d'une image déjà répartie.
    pub(super) fn new(context: &'a Context, grid: Grid) -> Self {
        Self { context, grid }
    }

    /// Le nombre de tuiles de l'image, numérotées ligne par ligne.
    pub fn tile_count(&self) -> u32 {
        self.grid.count()
    }

    /// Rend la tuile `index` dans `out`.
    ///
    /// Une tuile se rend une fois par image : un index déjà pris, y compris
    /// par un appel simultané sur un autre thread, rend
    /// [`Error::InvalidState`]. La sortie est vérifiée avant la prise, pour
    /// qu'un refus ne consomme pas la tuile.
    pub fn tile<O: Output>(&self, index: u32, out: &mut O) -> Result<()> {
        if index >= self.grid.count() {
            return Err(Error::InvalidArgument(Argument::TileIndex));
        }
        let rect = self.grid.rect(index);
        out.check(rect)?;
        if self.context.taken[index as usize].swap(true, Ordering::SeqCst) {
            return Err(Error::InvalidState);
        }
        self.render_tile(index, rect, out)
    }

    /// Rend les tuiles que personne n'a prises, puis clôt l'image.
    ///
    /// Appelée seule, elle rend l'image entière : c'est ce qui garde valide un
    /// hôte qui n'appelle jamais [`Frame::tile`].
    pub fn end<O: Output>(self, out: &mut O) -> Result<()> {
        out.check(self.grid.image())?;
        for index in 0..self.grid.count() {
            if !self.context.taken[index as usize].swap(true, Ordering::SeqCst) {
                self.render_tile(index, self.grid.rect(index), out)?;
            }
        }
        Ok(())
    }

    /// Rend une région quelconque de l'image, sans passer par la répartition.
    ///
    /// Le chemin de référence des tuiles : tous les triangles, dans l'ordre de
    /// soumission, dans un tampon de travail fourni par l'appelant et aussi
    /// grand que la région. Une tuile qui en diffère a perdu un triangle à la
    /// répartition, ou dépend de son découpage. Ne prend aucune tuile.
    pub fn region<O: Output>(&self, rect: Rect, scratch: &mut [u32], out: &mut O) -> Result<()> {
        let image = self.grid.image();
        let inside = rect
            .x
            .checked_add(rect.width)
            .zip(rect.y.checked_add(rect.height))
            .is_some_and(|(right, bottom)| right <= image.width && bottom <= image.height);
        if !inside {
            return Err(Error::InvalidArgument(Argument::Region));
        }
        let pixels = rect.width as usize * rect.height as usize;
        let scratch = scratch
            .get_mut(..pixels)
            .ok_or(Error::InvalidArgument(Argument::ScratchLength))?;
        out.check(rect)?;
        self.draw(rect, scratch, 0..self.context.triangles.len() as u32, out)
    }

    /// Rend une tuile dans un tampon de travail posé sur la pile.
    fn render_tile<O: Output>(&self, index: u32, rect: Rect, out: &mut O) -> Result<()> {
        let mut scratch = [0u32; MAX_TILE * MAX_TILE];
        let pixels = rect.width as usize * rect.height as usize;
        self.draw(
            rect,
            &mut scratch[..pixels],
            self.context.bins.tile(index),
            out,
        )
    }

    /// Dessine `triangles` dans `scratch`, puis recopie la région dans `out`.
    ///
    /// `to_le_bytes` plutôt qu'une réinterprétation du tampon : l'ordre R, G, B,
    /// A en mémoire est celui de l'ABI, et le déduire de l'ordre natif de la
    /// cible marcherait partout aujourd'hui pour de mauvaises raisons.
    fn draw<O: Output>(
        &self,
        rect: Rect,
        scratch: &mut [u32],
        triangles: impl Iterator<Item = u32>,
        out: &mut O,
    ) -> Result<()> {
        scratch.fill(CLEAR_COLOR);
        let mut target = Scratch {
            pixels: scratch,
            rect,
        };
        for index in triangles {
            fill(&mut target, rect, &self.context.triangles[index as usize]);
        }

        let width = rect.width as usize;
        for (row, source) in target.pixels.chunks_exact(width.max(1)).enumerate() {
            let span = out
                .span(rect.x, rect.y + row as u32, rect.width)
                .ok_or(Error::InvalidArgument(Argument::BufferLength))?;
            for (pixel, slot) in source.iter().zip(span.chunks_exact_mut(BYTES_PER_PIXEL)) {
                slot.copy_from_slice(&pixel.to_le_bytes());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
