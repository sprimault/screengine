// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le contexte de rendu et sa configuration.

mod frame;

use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;

use crate::buffer::reserved;
use crate::error::{Argument, Error, Result};
use crate::math::fixed::to_subpixel;
use crate::raster::{Bins, Grid, Point, Prepared, prepare};

pub use frame::{Frame, Output, Rows};

/// La plus grande résolution interne qu'un contexte accepte, en pixels de côté.
///
/// Ce n'est pas une limite de confort. Les formats en virgule fixe de
/// `docs/rust.md` calculent leurs pires cas sur cette borne : au-delà, une
/// fonction de bord déborderait avant que quoi que ce soit d'autre ne le
/// signale.
pub const MAX_RESOLUTION: u32 = 2048;

/// Les deux tailles de tuile admises, en pixels de côté.
///
/// Une tuile de 32 ou 64 tient en L1 avec sa profondeur. Au-delà, l'intérêt
/// principal des tuiles — le cache — disparaît, et le tampon de travail posé
/// sur la pile de chaque appel grossirait avec elle.
pub const TILE_SIZES: [u32; 2] = [32, 64];

/// Quatre octets par pixel, R, G, B puis A en mémoire.
pub const BYTES_PER_PIXEL: usize = 4;

/// Les triangles qu'une image peut recevoir.
///
/// Ce que la frontière C annonce comme capacité par défaut, tant que la
/// configuration ne permet pas de la choisir.
pub const TRIANGLE_CAPACITY: usize = 16_384;

/// Le noir opaque dont chaque image part.
const CLEAR_COLOR: u32 = 0xFF00_0000;

/// La couleur du triangle de l'étape 0.
///
/// Franchement distincte du fond : c'est la première chose qu'un hôte affiche,
/// et un aplat sombre laisserait douter entre « ça rend » et « ça ne rend pas ».
const DEMO_COLOR: u32 = 0xFF30_A0E0;

/// Ce que reçoit la création d'un contexte.
///
/// La résolution maximale dimensionne tout ce que l'image consomme dès la
/// création. Changer de résolution sous ce maximum n'alloue donc rien, ce qui
/// est la seule façon de tenir « zéro allocation par image » quand l'hôte
/// ajuste sa résolution en cours de partie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Largeur maximale, en pixels. Entre 1 et [`MAX_RESOLUTION`].
    pub max_width: u32,
    /// Hauteur maximale, en pixels. Entre 1 et [`MAX_RESOLUTION`].
    pub max_height: u32,
    /// Largeur initiale, en pixels. Au plus `max_width`.
    pub width: u32,
    /// Hauteur initiale, en pixels. Au plus `max_height`.
    pub height: u32,
    /// Côté d'une tuile : 32 ou 64.
    pub tile_size: u32,
}

impl Config {
    /// Refuse une configuration que le moteur ne peut pas honorer.
    fn validate(&self) -> Result<()> {
        let bounded = |v: u32, max: u32| v >= 1 && v <= max;

        if !bounded(self.max_width, MAX_RESOLUTION)
            || !bounded(self.max_height, MAX_RESOLUTION)
            || !bounded(self.width, self.max_width)
            || !bounded(self.height, self.max_height)
        {
            return Err(Error::InvalidArgument(Argument::Resolution));
        }
        if !TILE_SIZES.contains(&self.tile_size) {
            return Err(Error::InvalidArgument(Argument::TileSize));
        }
        Ok(())
    }
}

/// Un contexte de rendu.
///
/// Il ne porte aucun tampon à l'échelle de l'image : couleur et profondeur
/// vivent sur la pile de l'appel qui rend une tuile. Ce qu'il réserve à la
/// création — triangles préparés, répartition par tuile, drapeaux de tuile —
/// l'est pour la capacité et la résolution maximales, et plus jamais réalloué.
#[derive(Debug)]
pub struct Context {
    config: Config,
    width: u32,
    height: u32,
    /// Les triangles de l'image en cours, dans l'ordre de soumission.
    triangles: Vec<Prepared>,
    bins: Bins,
    /// Vrai pour chaque tuile déjà prise dans l'image en cours.
    ///
    /// Atomique parce que des tuiles distinctes se rendent depuis des threads
    /// distincts, et que c'est lui qui dit à la fin ce qui reste à rendre.
    taken: Vec<AtomicBool>,
}

impl Context {
    /// Crée un contexte, ou refuse la configuration.
    ///
    /// C'est l'un des appels nommés où l'allocation est permise : tout ce que
    /// l'image consomme se réserve ici, pour la résolution maximale.
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;

        let tiles = Grid::new(config.max_width, config.max_height, config.tile_size).count();
        let mut taken = reserved(tiles as usize)?;
        taken.resize_with(tiles as usize, AtomicBool::default);

        Ok(Self {
            config,
            width: config.width,
            height: config.height,
            triangles: reserved(TRIANGLE_CAPACITY)?,
            bins: Bins::new(tiles as usize, TRIANGLE_CAPACITY)?,
            taken,
        })
    }

    /// La configuration reçue à la création.
    pub fn config(&self) -> Config {
        self.config
    }

    /// La résolution interne courante, en pixels.
    pub fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Commence une image : scelle la scène, la répartit par tuile, et rend de
    /// quoi rendre les tuiles.
    ///
    /// La répartition est la seule phase qui écrit dans un état partagé, et
    /// elle se termine ici, avant toute tuile. La [`Frame`] emprunte le
    /// contexte : aucun autre appel n'est possible tant qu'elle vit.
    pub fn frame_begin(&mut self) -> Result<Frame<'_>> {
        self.triangles.clear();
        self.draw_demo_triangle()?;
        Ok(self.seal())
    }

    /// Répartit les triangles soumis et ouvre le rendu des tuiles.
    fn seal(&mut self) -> Frame<'_> {
        let grid = Grid::new(self.width, self.height, self.config.tile_size);
        self.bins.build(&grid, &self.triangles);
        for flag in &mut self.taken[..grid.count() as usize] {
            *flag.get_mut() = false;
        }

        Frame::new(self, grid)
    }

    /// Rend une image entière dans le tampon de l'hôte, tuile par tuile.
    ///
    /// `stride` est en pixels et vaut au moins la largeur courante. Le tampon
    /// fait au moins `stride × hauteur` pixels de quatre octets ; la frontière C
    /// ne reçoit pas sa longueur et en fait une précondition, alors qu'un
    /// appelant Rust la porte avec la tranche — c'est le seul contrôle des deux
    /// qui distingue les deux chemins.
    pub fn frame_end(&mut self, pixels: &mut [u8], stride: u32) -> Result<()> {
        self.frame_begin()?.end(&mut Rows::new(pixels, stride))
    }

    /// Ajoute un triangle à l'image en cours.
    fn submit(&mut self, v: [Point; 3], color: u32) -> Result<()> {
        let Some(triangle) = prepare(v, color) else {
            return Ok(());
        };
        if self.triangles.len() >= TRIANGLE_CAPACITY {
            return Err(Error::InvalidArgument(Argument::TriangleCapacity));
        }
        self.triangles.push(triangle);
        Ok(())
    }

    /// Soumet le triangle en dur de l'étape 0.
    ///
    /// Il n'y a pas encore de scène à soumettre : ce triangle existe pour que
    /// les hôtes aient quelque chose à afficher, et il est rempli par les
    /// fonctions de bord et la règle top-left définitives — c'est le premier
    /// remplissage, et il est déjà celui de tout le moteur.
    fn draw_demo_triangle(&mut self) -> Result<()> {
        let (w, h) = (self.width as f32, self.height as f32);

        // Sens horaire à l'écran, Y vers le bas : sommet en haut, puis
        // bas-droite, puis bas-gauche.
        let vertices = [
            Point {
                x: to_subpixel(w * 0.5),
                y: to_subpixel(h * 0.12),
            },
            Point {
                x: to_subpixel(w * 0.88),
                y: to_subpixel(h * 0.86),
            },
            Point {
                x: to_subpixel(w * 0.12),
                y: to_subpixel(h * 0.86),
            },
        ];

        self.submit(vertices, DEMO_COLOR)
    }
}

#[cfg(test)]
mod tests;
