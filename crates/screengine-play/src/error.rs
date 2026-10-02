// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que [`Play::run`](crate::Play::run) peut rendre.

use std::fmt;

/// Une erreur de l'étage d'accueil.
///
/// Rendue par `run`, jamais levée en panique : une boucle événementielle qui
/// panique ne laisse à l'appelant qu'une pile, là où une erreur lui laisse le
/// choix.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Le moteur a refusé la configuration, ou n'a pas pu allouer ses tampons.
    Engine(screengine::Error),
    /// Un réglage de [`Play`](crate::Play) est invalide : fréquence de mise à
    /// jour ou facteur d'échelle nul.
    Setting(&'static str),
    /// Le facteur imposé par [`Scale::Fixed`](crate::Scale::Fixed) donne une
    /// fenêtre plus grande que l'écran.
    ScaleTooLarge {
        /// Le facteur imposé.
        factor: u32,
        /// La taille de fenêtre qu'il demande, en pixels.
        window: (u32, u32),
        /// La taille de l'écran, en pixels.
        monitor: (u32, u32),
    },
    /// La boucle d'événements n'a pas pu démarrer, ou s'est arrêtée en erreur.
    EventLoop(winit::error::EventLoopError),
    /// La fenêtre n'a pas pu être créée.
    Window(winit::error::OsError),
    /// La surface de la fenêtre a refusé une opération.
    Surface(softbuffer::SoftBufferError),
    /// Un PNG passé à [`load_png`](crate::load_png) est illisible.
    ///
    /// Celui qui se décode mais que le moteur refuse — un côté qui n'est pas
    /// une puissance de deux — rend `Engine` : c'est la contrainte du moteur
    /// qui parle, pas le format.
    Png(png::DecodingError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Le noyau porte le texte de ses propres erreurs : le redire ici
            // donnait deux formulations du même refus, celle que lit un hôte Rust
            // et celle que `scg_last_error` rend à un hôte C.
            Self::Engine(error) => f.write_str(error.message()),
            Self::Setting(what) => f.write_str(what),
            Self::ScaleTooLarge {
                factor,
                window,
                monitor,
            } => write!(
                f,
                "scale factor {factor} needs a {}x{} window, larger than the {}x{} monitor",
                window.0, window.1, monitor.0, monitor.1
            ),
            Self::EventLoop(e) => write!(f, "event loop: {e}"),
            Self::Window(e) => write!(f, "window creation: {e}"),
            Self::Surface(e) => write!(f, "window surface: {e}"),
            Self::Png(e) => write!(f, "png decoding: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Engine(e) => Some(e),
            Self::EventLoop(e) => Some(e),
            Self::Window(e) => Some(e),
            Self::Surface(e) => Some(e),
            Self::Png(e) => Some(e),
            Self::Setting(_) | Self::ScaleTooLarge { .. } => None,
        }
    }
}

impl From<screengine::Error> for Error {
    fn from(error: screengine::Error) -> Self {
        Self::Engine(error)
    }
}

impl From<softbuffer::SoftBufferError> for Error {
    fn from(error: softbuffer::SoftBufferError) -> Self {
        Self::Surface(error)
    }
}

impl From<png::DecodingError> for Error {
    fn from(error: png::DecodingError) -> Self {
        Self::Png(error)
    }
}
