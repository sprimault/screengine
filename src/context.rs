// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le contexte de rendu et sa configuration.

mod frame;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use crate::buffer::reserved;
use crate::error::{Argument, Error, Result};
use crate::math::projection::ClipVertex;
use crate::math::{Projection, Vec3};
use crate::raster::{Bins, Grid, MAX_CLIP_TRIANGLES, Point, Prepared, Vertex, clip, prepare};

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

/// La couleur du premier triangle de la scène de démonstration.
///
/// Franchement distincte du fond : c'est la première chose qu'un hôte affiche,
/// et un aplat sombre laisserait douter entre « ça rend » et « ça ne rend pas ».
const DEMO_COLOR: u32 = 0xFF30_A0E0;

/// La couleur du second, qui partage une arête avec le premier.
///
/// Proche de la première mais distincte : c'est ce qui fait qu'un trou ou un
/// recouvrement le long de l'arête commune se voit à l'œil dans un hôte, sans
/// attendre qu'une empreinte le dise.
const DEMO_COLOR_SHARED: u32 = 0xFF30_E0A0;

/// Le champ de vision vertical de la scène de démonstration, en radians.
const DEMO_FOV_Y: f32 = core::f32::consts::FRAC_PI_3;

/// Le plan proche de la scène de démonstration.
const DEMO_NEAR: f32 = 0.1;

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
    /// Le découpage de l'image en cours, fixé au début.
    grid: Grid,
    /// La projection, qui dépend de la résolution et se recalculera avec elle.
    projection: Projection,
    /// [`RECORDING`], [`RENDERING`] ou [`CLOSING`].
    state: AtomicU8,
    /// Les tuiles en cours de rendu, que la fin attend à zéro.
    in_flight: AtomicU32,
}

/// Le contexte accepte la scène : aucune image n'est commencée.
const RECORDING: u8 = 0;

/// L'image est répartie, et ses tuiles se rendent.
const RENDERING: u8 = 1;

/// La fin d'image a pris la main : plus aucune tuile ne commence.
const CLOSING: u8 = 2;

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
            grid: Grid::new(config.width, config.height, config.tile_size),
            projection: Projection::new(config.width, config.height, DEMO_FOV_Y, DEMO_NEAR)?,
            state: AtomicU8::new(RECORDING),
            in_flight: AtomicU32::new(0),
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

    /// Commence une image : scelle la scène, la répartit par tuile, et rend le
    /// nombre de tuiles.
    ///
    /// La répartition est la seule phase qui écrit dans un état partagé, et
    /// elle se termine ici, avant toute tuile. Une image déjà commencée rend
    /// [`Error::InvalidState`] : sa répartition est peut-être lue par une tuile
    /// sur un autre thread.
    ///
    /// C'est la forme qu'emploie la frontière C, qui garde l'image ouverte d'un
    /// appel à l'autre. Un appelant Rust lui préfère [`Context::frame_begin`],
    /// dont la [`Frame`] rend la séquence vérifiable à la compilation.
    pub fn begin(&mut self) -> Result<u32> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.triangles.clear();
        self.draw_demo_scene()?;
        self.seal();
        Ok(self.grid.count())
    }

    /// Commence une image et rend de quoi en rendre les tuiles.
    ///
    /// La [`Frame`] emprunte le contexte : aucun autre appel n'est possible tant
    /// qu'elle vit, et sa destruction referme l'image si personne ne l'a fait.
    pub fn frame_begin(&mut self) -> Result<Frame<'_>> {
        self.begin()?;
        Ok(Frame::new(self))
    }

    /// Vrai entre le début et la fin d'une image.
    pub fn is_rendering(&self) -> bool {
        self.state.load(Ordering::SeqCst) != RECORDING
    }

    /// Répartit les triangles soumis et ouvre le rendu des tuiles.
    fn seal(&mut self) {
        self.grid = Grid::new(self.width, self.height, self.config.tile_size);
        self.bins.build(&self.grid, &self.triangles);
        for flag in &mut self.taken[..self.grid.count() as usize] {
            *flag.get_mut() = false;
        }
        *self.state.get_mut() = RENDERING;
    }

    /// Rend une image entière dans le tampon de l'hôte, tuile par tuile.
    ///
    /// Sans début, elle le fait elle-même ; après un début, elle rend les tuiles
    /// que personne n'a prises.
    ///
    /// `stride` est en pixels et vaut au moins la largeur courante. Le tampon
    /// fait au moins `stride × hauteur` pixels de quatre octets ; la frontière C
    /// ne reçoit pas sa longueur et en fait une précondition, alors qu'un
    /// appelant Rust la porte avec la tranche — c'est le seul contrôle des deux
    /// qui distingue les deux chemins.
    pub fn frame_end(&mut self, pixels: &mut [u8], stride: u32) -> Result<()> {
        if !self.is_rendering() {
            self.begin()?;
        }
        self.end(&mut Rows::new(pixels, stride))
    }

    /// Ajoute un triangle à l'image en cours.
    fn submit(&mut self, v: [Vertex; 3], color: u32) -> Result<()> {
        let Some(triangle) = prepare(v, color) else {
            return Ok(());
        };
        if self.triangles.len() >= TRIANGLE_CAPACITY {
            return Err(Error::InvalidArgument(Argument::TriangleCapacity));
        }
        self.triangles.push(triangle);
        Ok(())
    }

    /// Soumet un triangle donné en espace de vue : découpe, projection,
    /// préparation.
    ///
    /// **La découpe a lieu ici et non au début d'image**, parce que c'est la
    /// seule place qui rende vraie la clause de capacité : un triangle découpé
    /// en produit jusqu'à six, et le refus doit tomber sur l'appel qui déborde
    /// plutôt que sur une image entière déjà soumise. La capacité compte donc
    /// des triangles préparés, pas soumis.
    ///
    /// Un sommet qu'on ne peut pas projeter fait disparaître le triangle sans
    /// erreur : c'est une donnée, pas un défaut du moteur.
    fn submit_view(&mut self, view: [Vec3; 3], color: u32) -> Result<()> {
        let mut homogeneous = [ClipVertex {
            x: 0.0,
            y: 0.0,
            w: 0.0,
        }; 3];
        for (slot, point) in homogeneous.iter_mut().zip(view) {
            match self.projection.to_clip(point) {
                Some(vertex) => *slot = vertex,
                None => return Ok(()),
            }
        }

        let polygon = clip(homogeneous, self.projection.frustum());
        // La borne est démontrée par la géométrie du découpage ; c'est ici
        // qu'elle décide du nombre de places qu'un triangle soumis consomme
        // dans la capacité.
        debug_assert!(polygon.triangle_count() <= MAX_CLIP_TRIANGLES);
        for i in 0..polygon.triangle_count() {
            let vertices = polygon.triangle(i).map(|c| {
                let projected = self.projection.to_vertex(c);
                Vertex {
                    position: Point {
                        x: projected.x,
                        y: projected.y,
                    },
                    z: projected.z,
                }
            });
            self.submit(vertices, color)?;
        }
        Ok(())
    }

    /// Soumet la scène de démonstration : deux triangles qui partagent une
    /// arête, vus de biais.
    ///
    /// Il n'y a pas encore de scène à soumettre par l'hôte, mais celle-ci
    /// traverse désormais toute la chaîne — projection, découpe, virgule fixe —
    /// au lieu d'être écrite en coordonnées d'écran. Deux triangles et non un :
    /// c'est leur arête commune qui éprouve la propriété la plus coûteuse du
    /// moteur, et deux couleurs distinctes la rendent visible dans les hôtes,
    /// là où un aplat unique la cacherait.
    ///
    /// Le quadrilatère fuit vers la droite, donc chaque pixel a sa propre
    /// profondeur, et il déborde à gauche pour que le parcours traite des
    /// triangles plus larges que l'image.
    fn draw_demo_scene(&mut self) -> Result<()> {
        // Espace de vue : X à droite, Y vers le bas, Z vers l'avant. La caméra
        // arrive avec la soumission par l'ABI ; ici, elle est l'identité.
        let a = Vec3::new(-2.5, -1.6, 2.0);
        let b = Vec3::new(2.5, -1.6, 3.5);
        let c = Vec3::new(2.5, 1.6, 3.5);
        let d = Vec3::new(-2.5, 1.6, 2.0);

        // Antihoraire dans les données, donc horaire à l'écran une fois Y
        // retourné : l'arête commune `a → c` est parcourue dans un sens par le
        // premier triangle et dans l'autre par le second, ce qui est exactement
        // le cas que la règle top-left doit trancher.
        self.submit_view([a, c, b], DEMO_COLOR)?;
        self.submit_view([a, d, c], DEMO_COLOR_SHARED)
    }
}

#[cfg(test)]
mod tests;
