// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le contexte de rendu et sa configuration.

mod frame;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use crate::buffer::reserved;
use crate::error::{Argument, Error, Result};
use crate::math::fixed::MAX_TEXEL_COORD;
use crate::math::projection::ClipVertex;
use crate::math::{Affine3, Projection, Vec3};
use crate::raster::{Bins, Grid, MAX_CLIP_TRIANGLES, Point, Prepared, Vertex, clip, prepare};
use crate::scene::{Camera, Color, Triangle, VertexUv};

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
/// La capacité de triangles par défaut, quand la configuration passe zéro.
pub const TRIANGLE_CAPACITY: usize = 16_384;

/// Le noir opaque dont chaque image part.
const CLEAR_COLOR: u32 = 0xFF00_0000;

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
    /// Triangles qu'une image peut recevoir, ou `0` pour
    /// [`TRIANGLE_CAPACITY`].
    ///
    /// La valeur compte des triangles **préparés** : un triangle découpé par
    /// le plan proche en produit jusqu'à six, et c'est l'appel qui déborde qui
    /// est refusé, jamais une image entière déjà soumise.
    pub max_triangles: u32,
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

    /// La capacité de triangles effective, `0` valant le défaut.
    fn capacity(&self) -> usize {
        if self.max_triangles == 0 {
            TRIANGLE_CAPACITY
        } else {
            self.max_triangles as usize
        }
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
    /// La caméra, qu'une image conserve d'un bout à l'autre.
    camera: Camera,
    /// Le monde vers l'espace de vue, recalculé avec la caméra.
    ///
    /// Gardée plutôt que recomposée à chaque soumission : la composer est une
    /// inversion, et la refaire par lot ferait dépendre l'image du découpage
    /// des lots — même résultat, mais plus rien ne le garantirait.
    view: Affine3,
    /// La projection, qui dépend de la résolution et de la caméra.
    projection: Projection,
    /// [`RECORDING`], [`RENDERING`] ou [`CLOSING`].
    state: AtomicU8,
    /// Les tuiles en cours de rendu, que la fin attend à zéro.
    in_flight: AtomicU32,
    /// Vrai quand la liste de dessin appartient à l'image déjà close.
    ///
    /// La fin d'image la périme mais ne peut pas la vider : elle ne tient
    /// qu'un `&self`, des tuiles pouvant encore la lire. Le prochain appel
    /// exclusif — une soumission, un début — la vide avant d'y toucher.
    stale: AtomicBool,
}

/// Un sommet porté en espace de vue, ses coordonnées de texture intactes.
///
/// La transformation ne touche que la position ; `u` et `v` traversent sans
/// être modifiés, et c'est ce qui rend leur bornage vérifiable une fois pour
/// toutes à la soumission.
#[derive(Debug, Clone, Copy)]
struct ClipSource {
    /// La position en espace de vue.
    view: Vec3,
    /// L'abscisse de texture, en texels.
    u: f32,
    /// L'ordonnée de texture, en texels.
    v: f32,
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

        let camera = Camera::DEFAULT;
        let capacity = config.capacity();
        Ok(Self {
            config,
            width: config.width,
            height: config.height,
            triangles: reserved(capacity)?,
            bins: Bins::new(tiles as usize, capacity)?,
            taken,
            grid: Grid::new(config.width, config.height, config.tile_size),
            camera,
            view: camera.view(),
            projection: Projection::new(config.width, config.height, camera.fov_y, camera.near)?,
            state: AtomicU8::new(RECORDING),
            in_flight: AtomicU32::new(0),
            stale: AtomicBool::new(false),
        })
    }

    /// La caméra courante.
    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Change la caméra, ou refuse son champ de vision et son plan proche.
    ///
    /// Refusée pendant le rendu, comme toute écriture dans l'état du contexte.
    /// La caméra vaut pour l'image entière : la déplacer entre deux soumissions
    /// du même lot rendrait une image que rien ne décrit.
    ///
    /// Le quaternion n'est pas exigé unitaire, il est normalisé ici ; en
    /// revanche `fov_y` est refusé **sur les radians**, avant toute conversion
    /// en angle binaire, qui replierait un champ de vision de trois demi-tours
    /// en un demi-tour parfaitement acceptable.
    pub fn set_camera(&mut self, camera: Camera) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.projection = Projection::new(self.width, self.height, camera.fov_y, camera.near)?;
        self.view = camera.view();
        self.camera = camera;
        Ok(())
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
    ///
    /// L'image est faite de ce qui a été soumis depuis la fin de la précédente.
    /// Rien de soumis donne une image de fond, sans erreur : c'est une scène
    /// vide, pas un appel fautif.
    pub fn begin(&mut self) -> Result<u32> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.drop_closed_frame();
        self.seal();
        Ok(self.grid.count())
    }

    /// Vide la liste de dessin si elle appartient à une image déjà close.
    ///
    /// Le vidage se fait ici et non à la fin de l'image parce que la fin ne
    /// tient qu'un `&self`. C'est aussi ce qui permet à un hôte de soumettre
    /// dès le retour de la fin sans que sa scène soit jetée au début suivant.
    fn drop_closed_frame(&mut self) {
        if *self.stale.get_mut() {
            self.triangles.clear();
            *self.stale.get_mut() = false;
        }
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

    /// Soumet un lot de triangles, chacun transformé par `model` puis par la
    /// caméra.
    ///
    /// `model` porte l'objet vers le monde, et le noyau compose la vue :
    /// une matrice modèle-vue reçue toute faite obligerait chaque hôte à
    /// inverser la pose de la caméra lui-même, donc à normaliser un quaternion
    /// par sa propre bibliothèque mathématique, et deux liaisons ne rendraient
    /// plus la même image.
    ///
    /// **Un lot est accepté ou refusé en entier.** Un lot à demi soumis
    /// laisserait dans l'image un mur dont il manque la moitié, sans que l'hôte
    /// sache où la coupure est tombée.
    ///
    /// Un triangle dont un sommet ne se projette pas — coordonnée démesurée ou
    /// non finie — disparaît sans erreur : c'est une donnée, pas un défaut du
    /// moteur. Un indice hors du tableau de sommets, lui, est une erreur de
    /// l'appelant.
    pub fn submit(
        &mut self,
        model: Affine3,
        vertices: &[Vec3],
        triangles: &[Triangle],
    ) -> Result<()> {
        self.submit_each(model, triangles.len(), |i| {
            let triangle = triangles[i];
            let mut corners = [Vec3::ZERO; 3];
            for (corner, &index) in corners.iter_mut().zip(&triangle.indices) {
                *corner = *vertices
                    .get(index as usize)
                    .ok_or(Error::InvalidArgument(Argument::VertexIndex))?;
            }
            Ok((corners, triangle.color))
        })
    }

    /// Soumet un lot dont chaque triangle se lit par une fonction d'accès.
    ///
    /// La forme générale, dont [`Context::submit`] n'est que la façade sur deux
    /// tranches. Elle existe pour la frontière C, qui reçoit des tableaux de
    /// structures `#[repr(C)]` qui lui appartiennent : sans elle, il lui
    /// faudrait soit les copier — une allocation par image —, soit
    /// réinterpréter ses tranches, ce qui imposerait au noyau une disposition
    /// mémoire qu'il n'a pas choisie.
    ///
    /// `read` est appelée une fois par triangle, dans l'ordre, et son erreur
    /// refuse le lot entier.
    pub fn submit_each<F>(&mut self, model: Affine3, count: usize, read: F) -> Result<()>
    where
        F: Fn(usize) -> Result<([Vec3; 3], Color)>,
    {
        self.submit_each_uv(model, count, |i| {
            let (corners, color) = read(i)?;
            Ok((corners.map(VertexUv::untextured), color))
        })
    }

    /// Soumet un lot de triangles dont les sommets portent leurs coordonnées de
    /// texture.
    ///
    /// Même contrat que [`Context::submit`] pour le reste : `model` porte
    /// l'objet vers le monde, et le lot est accepté ou refusé en entier.
    pub fn submit_uv(
        &mut self,
        model: Affine3,
        vertices: &[VertexUv],
        triangles: &[Triangle],
    ) -> Result<()> {
        self.submit_each_uv(model, triangles.len(), |i| {
            let triangle = triangles[i];
            let mut corners = [VertexUv::untextured(Vec3::ZERO); 3];
            for (corner, &index) in corners.iter_mut().zip(&triangle.indices) {
                *corner = *vertices
                    .get(index as usize)
                    .ok_or(Error::InvalidArgument(Argument::VertexIndex))?;
            }
            Ok((corners, triangle.color))
        })
    }

    /// La forme générale de [`Context::submit_each`], dont les sommets portent
    /// leurs coordonnées de texture.
    pub fn submit_each_uv<F>(&mut self, model: Affine3, count: usize, read: F) -> Result<()>
    where
        F: Fn(usize) -> Result<([VertexUv; 3], Color)>,
    {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.drop_closed_frame();
        let mark = self.triangles.len();
        let result = self.submit_batch(model, count, read);
        if result.is_err() {
            self.triangles.truncate(mark);
        }
        result
    }

    /// Le corps de [`Context::submit_each_uv`], qui peut laisser le lot à
    /// moitié posé.
    fn submit_batch<F>(&mut self, model: Affine3, count: usize, read: F) -> Result<()>
    where
        F: Fn(usize) -> Result<([VertexUv; 3], Color)>,
    {
        let transform = self.view.product(model);
        for i in 0..count {
            let (corners, color) = read(i)?;
            // Avant la transformation : c'est la valeur écrite par l'hôte qu'on
            // refuse, pas ce que la caméra en fait. Un sommet fini que la
            // matrice porte à l'infini reste une condition de vue, et son
            // triangle disparaît plus bas sans erreur.
            if corners.iter().any(|c| {
                !c.position.x.is_finite() || !c.position.y.is_finite() || !c.position.z.is_finite()
            }) {
                return Err(Error::InvalidArgument(Argument::VertexCoordinate));
            }
            // Les coordonnées de texture, elles, ne dépendent d'aucune
            // transformation : la borne porte sur la valeur exacte que l'hôte a
            // écrite, et le découpage n'en produira que des combinaisons
            // convexes. Revalider en aval serait de la défensive sur une valeur
            // qui ne peut plus sortir que par un défaut du moteur.
            let off = |c: f32| c.is_nan() || c.abs() > MAX_TEXEL_COORD;
            if corners.iter().any(|c| off(c.u) || off(c.v)) {
                return Err(Error::InvalidArgument(Argument::TextureCoordinate));
            }
            self.submit_view(
                corners.map(|c| ClipSource {
                    view: transform.transform_point(c.position),
                    u: c.u,
                    v: c.v,
                }),
                color,
            )?;
        }
        Ok(())
    }

    /// Ajoute un triangle déjà projeté à l'image en cours.
    ///
    /// La couleur y est déjà l'entier du rasteriseur : [`Color`] appartient à
    /// la scène, et se convertit au plus tôt.
    fn push(&mut self, v: [Vertex; 3], color: u32) -> Result<()> {
        let Some(triangle) = prepare(v, color) else {
            return Ok(());
        };
        if self.triangles.len() >= self.config.capacity() {
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
    fn submit_view(&mut self, view: [ClipSource; 3], color: Color) -> Result<()> {
        let mut homogeneous = [ClipVertex {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            u: 0.0,
            v: 0.0,
        }; 3];
        for (slot, source) in homogeneous.iter_mut().zip(view) {
            match self.projection.to_clip(source.view, source.u, source.v) {
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
                    s: projected.s,
                    t: projected.t,
                }
            });
            self.push(vertices, color.packed())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
