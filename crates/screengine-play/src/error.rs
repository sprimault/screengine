// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que [`Play::run`](crate::Play::run) peut rendre.

use std::fmt;

use screengine::Argument;

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
            Self::Engine(screengine::Error::InvalidArgument(argument)) => match argument {
                Argument::Resolution => f.write_str(
                    "internal resolution out of range: each side must be between 1 and 2048",
                ),
                Argument::TileSize => f.write_str("tile size must be 32 or 64"),
                Argument::Stride => f.write_str("stride is smaller than the internal width"),
                Argument::BufferLength => {
                    f.write_str("pixel buffer is shorter than stride x height x 4 bytes")
                }
                Argument::TileIndex => f.write_str("tile index beyond the tile count of the frame"),
                Argument::Region => f.write_str("region extends beyond the image"),
                Argument::ScratchLength => {
                    f.write_str("scratch buffer is shorter than the pixels of its region")
                }
                Argument::TriangleCapacity => {
                    f.write_str("more triangles submitted than the engine reserved")
                }
                Argument::VertexIndex => {
                    f.write_str("a triangle indexes a vertex beyond its batch")
                }
                Argument::Projection => f.write_str(
                    "field of view must be within ]0, pi[ radians, and the near plane positive",
                ),
                Argument::VertexCoordinate => {
                    f.write_str("a submitted vertex coordinate is NaN or infinite")
                }
                Argument::TextureCapacity => {
                    f.write_str("more distinct textures in one frame than the engine reserved")
                }
                Argument::TextureCoordinate => {
                    f.write_str("a texture coordinate is not finite, or beyond 16384 texels")
                }
                Argument::TextureSize => {
                    f.write_str("texture sides must be powers of two between 1 and 2048")
                }
                Argument::TextureLength => {
                    f.write_str("pixel block is not exactly width x height x 4 bytes")
                }
                Argument::Overbright => f.write_str("overbright shift must be 0, 1 or 2"),
                Argument::Fog => {
                    f.write_str("fog range must be finite, not negative, and end beyond start")
                }
                Argument::LightCapacity => {
                    f.write_str("more dynamic lights in one frame than the engine holds")
                }
                Argument::Light => f.write_str(
                    "a light has a non-finite position, or a radius that is not positive",
                ),
            },
            Self::Engine(screengine::Error::OutOfMemory) => {
                f.write_str("the engine could not allocate its buffers")
            }
            Self::Engine(screengine::Error::InvalidState) => {
                f.write_str("a tile was rendered twice in the same frame")
            }
            Self::Engine(screengine::Error::Faulted) => {
                f.write_str("a tile did not return from rendering; this frame is incomplete")
            }
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
            Self::EventLoop(e) => Some(e),
            Self::Window(e) => Some(e),
            Self::Surface(e) => Some(e),
            Self::Png(e) => Some(e),
            Self::Engine(_) | Self::Setting(_) | Self::ScaleTooLarge { .. } => None,
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
