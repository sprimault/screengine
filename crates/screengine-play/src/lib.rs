// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Étage d'accueil de Screengine : une fenêtre, des entrées et une boucle
//! autour du moteur, pour faire un jeu en Rust sans écrire d'hôte.
//!
//! ```no_run
//! use screengine_play::{KeyCode, Play};
//!
//! fn main() -> Result<(), screengine_play::Error> {
//!     Play::new().title("Hello").run(
//!         0u64,
//!         |frames, tick| {
//!             *frames += 1;
//!             if tick.input().pressed(KeyCode::Escape) {
//!                 tick.exit();
//!             }
//!         },
//!         |_, _context| {},
//!     )
//! }
//! ```
//!
//! C'est l'un des deux chemins vers le moteur. L'autre, l'ABI C, laisse à l'hôte
//! sa fenêtre, sa boucle et ses entrées. Ce crate n'y ajoute que du
//! comportement — boucle à pas fixe, entrées, mise à l'échelle — et aucun type
//! de scène à lui : ce qu'il permet se fait aussi par l'ABI.
//!
//! Le noyau ignore son existence, et ses invariants — `no_std`, zéro allocation
//! par image, déterminisme — ne s'appliquent pas ici.

mod clock;
mod error;
mod input;
mod runner;
mod scale;

pub use error::Error;
pub use input::Input;
pub use scale::Scale;
pub use screengine;
// Réexportés et non redéfinis : ce crate ajoute du comportement, jamais des
// données. Deux modèles de scène qui divergeraient seraient la seule façon de
// le rater.
pub use screengine::{Affine3, Camera, Color, Quat, Triangle, Vec3};
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

use screengine::{Config, Context};

/// La résolution interne par défaut.
///
/// Remontée par trois, elle remplit exactement un écran 1920×1080.
const DEFAULT_RESOLUTION: (u32, u32) = (640, 360);

/// Le facteur de la fenêtre à l'ouverture, hors [`Scale::Fixed`].
const DEFAULT_WINDOW_FACTOR: u32 = 2;

/// Les réglages d'une session, puis son lancement.
///
/// Chaque réglage a un défaut qui marche : `Play::new().run(…)` ouvre une
/// fenêtre de 1280×720 sur une image de 640×360 mise à jour 60 fois par
/// seconde.
#[derive(Debug, Clone)]
pub struct Play {
    title: String,
    resolution: (u32, u32),
    tile_size: u32,
    scale: Scale,
    rate: u32,
    exit_on_escape: bool,
}

impl Default for Play {
    fn default() -> Self {
        Self::new()
    }
}

impl Play {
    /// Les réglages par défaut.
    pub fn new() -> Self {
        Self {
            title: String::from("Screengine"),
            resolution: DEFAULT_RESOLUTION,
            tile_size: 64,
            scale: Scale::Integer,
            rate: 60,
            exit_on_escape: true,
        }
    }

    /// Le titre de la fenêtre.
    pub fn title(mut self, title: &str) -> Self {
        self.title = title.to_owned();
        self
    }

    /// La résolution interne, en pixels : celle que rend le moteur, jamais
    /// celle de la fenêtre.
    pub fn resolution(mut self, width: u32, height: u32) -> Self {
        self.resolution = (width, height);
        self
    }

    /// Le côté des tuiles du moteur, 32 ou 64.
    pub fn tile_size(mut self, size: u32) -> Self {
        self.tile_size = size;
        self
    }

    /// Comment l'image remplit la fenêtre.
    pub fn scale(mut self, scale: Scale) -> Self {
        self.scale = scale;
        self
    }

    /// Le nombre de pas de mise à jour par seconde.
    pub fn tick_rate(mut self, rate: u32) -> Self {
        self.rate = rate;
        self
    }

    /// Fermer la fenêtre sur Échap, ce que fait le défaut.
    pub fn exit_on_escape(mut self, exit: bool) -> Self {
        self.exit_on_escape = exit;
        self
    }

    /// Ouvre la fenêtre et fait tourner la boucle jusqu'à sa fermeture.
    ///
    /// `update` s'exécute à pas fixe et reçoit les entrées ; `render` s'exécute
    /// après chaque réveil qui a fait avancer la partie, et reçoit le contexte
    /// du moteur, dont l'image est ensuite recopiée dans la fenêtre. Les deux
    /// partagent `state`.
    ///
    /// Les réglages invalides et la configuration refusée par le moteur sont
    /// rendus avant qu'aucune fenêtre ne s'ouvre.
    pub fn run<S, U, R>(self, state: S, update: U, render: R) -> Result<(), Error>
    where
        U: FnMut(&mut S, &mut Tick<'_>),
        R: FnMut(&mut S, &mut Context),
    {
        if self.rate == 0 {
            return Err(Error::Setting("tick rate must be at least 1"));
        }
        if self.scale == Scale::Fixed(0) {
            return Err(Error::Setting("scale factor must be at least 1"));
        }

        let (width, height) = self.resolution;
        let context = Context::new(Config {
            max_width: width,
            max_height: height,
            width,
            height,
            tile_size: self.tile_size,
            max_triangles: 0,
        })?;

        runner::run(self, context, state, update, render)
    }
}

/// Ce que reçoit un pas de mise à jour.
#[derive(Debug)]
pub struct Tick<'a> {
    input: &'a Input,
    index: u64,
    dt: f32,
    exit: bool,
}

impl Tick<'_> {
    /// L'état du clavier et de la souris.
    pub fn input(&self) -> &Input {
        self.input
    }

    /// Le numéro du pas, à partir de zéro.
    ///
    /// Il avance d'exactement un par pas, quel que soit le rythme des images :
    /// c'est lui, plutôt qu'un temps cumulé en flottant, qui rejoue une partie à
    /// l'identique.
    pub fn index(&self) -> u64 {
        self.index
    }

    /// La durée d'un pas, en secondes. Constante pour toute la session.
    pub fn dt(&self) -> f32 {
        self.dt
    }

    /// Demande la fermeture après ce pas.
    pub fn exit(&mut self) {
        self.exit = true;
    }
}
