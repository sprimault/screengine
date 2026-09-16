// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Le contexte de rendu et sa configuration.

use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::math::fixed::to_subpixel;
use crate::raster::{Clip, Point, Target, fill_triangle};

/// Le tampon de couleur, vu comme un puits de remplissage.
struct ColorTarget<'a> {
    pixels: &'a mut [u32],
    width: i32,
}

impl Target for ColorTarget<'_> {
    fn put(&mut self, x: i32, y: i32, color: u32) {
        self.pixels[(y * self.width + x) as usize] = color;
    }
}

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
/// principal des tuiles — le cache — disparaît.
pub const TILE_SIZES: [u32; 2] = [32, 64];

/// Quatre octets par pixel, R, G, B puis A en mémoire.
pub const BYTES_PER_PIXEL: usize = 4;

/// Le noir opaque dont chaque image part.
const CLEAR_COLOR: u32 = 0xFF00_0000;

/// La couleur du triangle de l'étape 0.
///
/// Franchement distincte du fond : c'est la première chose qu'un hôte affiche,
/// et un aplat sombre laisserait douter entre « ça rend » et « ça ne rend pas ».
const DEMO_COLOR: u32 = 0xFF30_A0E0;

/// Ce que reçoit la création d'un contexte.
///
/// La résolution maximale dimensionne tous les tampons propres à l'image dès la
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

        if !bounded(self.max_width, MAX_RESOLUTION) || !bounded(self.max_height, MAX_RESOLUTION) {
            return Err(Error::InvalidArgument);
        }
        if !bounded(self.width, self.max_width) || !bounded(self.height, self.max_height) {
            return Err(Error::InvalidArgument);
        }
        if !TILE_SIZES.contains(&self.tile_size) {
            return Err(Error::InvalidArgument);
        }
        Ok(())
    }
}

/// Alloue un tampon de `len` mots nuls, ou rend [`Error::OutOfMemory`].
///
/// `try_reserve_exact` plutôt que la macro `vec!` : une allocation qui échoue en
/// paniquant ne laisserait rien à traduire en code de retour, et la variante
/// `OutOfMemory` n'aurait jamais de cas. Le `resize` qui suit ne réalloue pas.
fn filled(len: usize) -> Result<Vec<u32>> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(len)
        .map_err(|_| Error::OutOfMemory)?;
    buffer.resize(len, 0);
    Ok(buffer)
}

/// Un contexte de rendu.
///
/// Ses tampons sont dimensionnés pour la résolution maximale dès la création, et
/// ne sont plus jamais réalloués : c'est ce qui rend « zéro allocation par
/// image » vrai même quand l'hôte change de résolution en cours de partie.
#[derive(Debug)]
pub struct Context {
    config: Config,
    width: u32,
    height: u32,
    /// Couleur, un pixel par `u32`, `r | g << 8 | b << 16 | a << 24`.
    ///
    /// L'ordre des octets en mémoire ne dépend pas de la cible : la recopie
    /// écrit `to_le_bytes`, ce qui donne R, G, B, A partout et ne suppose rien
    /// de l'ordre natif.
    color: Vec<u32>,
    /// Profondeur, `near/w` en 0.32 : plus grand est plus proche.
    depth: Vec<u32>,
}

impl Context {
    /// Crée un contexte, ou refuse la configuration.
    ///
    /// C'est l'un des appels nommés où l'allocation est permise : tout ce que
    /// l'image consomme se réserve ici, pour la résolution maximale.
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;

        // `validate` a borné les deux dimensions à `MAX_RESOLUTION`, donc le
        // produit tient largement dans un `usize`, y compris sur 32 bits.
        let capacity = config.max_width as usize * config.max_height as usize;

        Ok(Self {
            config,
            width: config.width,
            height: config.height,
            color: filled(capacity)?,
            depth: filled(capacity)?,
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

    /// Le nombre de pixels de l'image courante.
    ///
    /// Les tampons sont rangés serré sur la largeur courante, pas sur la
    /// largeur maximale : la localité reste la même à toute résolution, et le
    /// calcul d'index ne dépend jamais du budget reçu à la création. Ce qui
    /// dépasse est de la capacité inutilisée, pas un trou entre les lignes.
    fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Remet la couleur au noir opaque et la profondeur au plus loin.
    ///
    /// Par `fill` sur la tranche utile, jamais par réaffectation : le tampon
    /// garde son allocation d'un bout à l'autre de la vie du contexte.
    fn clear(&mut self) {
        let count = self.pixel_count();
        self.color[..count].fill(CLEAR_COLOR);
        self.depth[..count].fill(0);
    }

    /// Recopie l'image vers le tampon de l'hôte, ligne par ligne.
    ///
    /// `to_le_bytes` plutôt qu'une réinterprétation du tampon : l'ordre R, G, B,
    /// A en mémoire est celui de l'ABI, et le déduire de l'ordre natif de la
    /// cible marcherait partout aujourd'hui pour de mauvaises raisons.
    fn blit(&self, pixels: &mut [u8], stride: u32) {
        let row_pixels = self.width as usize;
        let host_row = stride as usize * BYTES_PER_PIXEL;

        for y in 0..self.height as usize {
            let source = &self.color[y * row_pixels..][..row_pixels];
            let target = &mut pixels[y * host_row..][..row_pixels * BYTES_PER_PIXEL];

            for (pixel, slot) in source.iter().zip(target.chunks_exact_mut(BYTES_PER_PIXEL)) {
                slot.copy_from_slice(&pixel.to_le_bytes());
            }
        }
    }

    /// Termine l'image et écrit le résultat dans le tampon de l'hôte.
    ///
    /// `stride` est en pixels et vaut au moins la largeur courante. Le tampon
    /// fait au moins `stride × hauteur` pixels de quatre octets ; la frontière C
    /// ne reçoit pas sa longueur et en fait une précondition, alors qu'un
    /// appelant Rust la porte avec la tranche — c'est le seul contrôle des deux
    /// qui distingue les deux chemins.
    pub fn frame_end(&mut self, pixels: &mut [u8], stride: u32) -> Result<()> {
        if stride < self.width {
            return Err(Error::InvalidArgument);
        }

        let needed = (stride as usize)
            .checked_mul(self.height as usize)
            .and_then(|p| p.checked_mul(BYTES_PER_PIXEL))
            .ok_or(Error::InvalidArgument)?;
        if pixels.len() < needed {
            return Err(Error::InvalidArgument);
        }

        self.clear();
        self.draw_demo_triangle();
        self.blit(pixels, stride);
        Ok(())
    }

    /// Dessine le triangle en dur de l'étape 0.
    ///
    /// Il n'y a pas encore de scène à soumettre : ce triangle existe pour que
    /// les hôtes aient quelque chose à afficher, et il est rempli par les
    /// fonctions de bord et la règle top-left définitives — c'est le premier
    /// remplissage, et il est déjà celui de tout le moteur.
    fn draw_demo_triangle(&mut self) {
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

        let clip = Clip {
            width: self.width as i32,
            height: self.height as i32,
        };
        let mut target = ColorTarget {
            pixels: &mut self.color,
            width: self.width as i32,
        };

        fill_triangle(&mut target, clip, vertices, DEMO_COLOR);
    }
}

#[cfg(test)]
mod tests;
