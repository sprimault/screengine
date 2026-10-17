// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que [`Play::run`](crate::Play::run) peut rendre.

use std::fmt;

/// Une erreur de l'étage d'accueil.
///
/// Jamais levée en panique : une boucle événementielle qui panique ne laisse à
/// l'appelant qu'une pile, là où une erreur lui laisse le choix.
///
/// **Deux chemins, et lequel dépend du moment** : ce qui se refuse avant la
/// fenêtre est rendu par `run` ; ce que la boucle lève ensuite part au puits de
/// [`Play::on_error`](crate::Play::on_error), parce que le lancement sur wasm
/// rend la main avant que la boucle tourne et qu'il n'y a alors plus de retour
/// à remplir.
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
    /// Le canvas du navigateur a refusé une opération : contexte de dessin,
    /// construction de l'image ou dépôt des pixels.
    ///
    /// Propre à wasm, où l'étage d'accueil pose lui-même l'image — la
    /// présentation de `softbuffer` y reconstruit son tampon à chaque image.
    /// Le message est celui que le navigateur a rendu.
    #[cfg(target_arch = "wasm32")]
    Canvas(String),
    /// Un PNG passé à [`load_png`](crate::load_png) est illisible.
    ///
    /// Celui qui se décode mais que le moteur refuse — un côté qui n'est pas
    /// une puissance de deux — rend `Engine` : c'est la contrainte du moteur
    /// qui parle, pas le format.
    Png(png::DecodingError),
}

/// Écrit le message d'une erreur là où la plateforme le montre.
///
/// Le défaut de [`Play::on_error`](crate::Play::on_error). Sur wasm il passe par
/// la console du navigateur : rien n'y mène la sortie d'erreur, et c'est la
/// cible où ce chemin est le seul — le lancement ayant rendu la main avant que
/// la boucle tourne, `run` n'a plus de retour à remplir.
pub(crate) fn report(error: &Error) {
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("screengine-play: {error}");
    #[cfg(target_arch = "wasm32")]
    web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
        "screengine-play: {error}"
    )));
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
            #[cfg(target_arch = "wasm32")]
            Self::Canvas(e) => write!(f, "canvas: {e}"),
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
            // Le canvas rend un `JsValue` et non une erreur Rust : ce qu'il a
            // dit est déjà dans le message, et il n'y a pas de source à chaîner.
            #[cfg(target_arch = "wasm32")]
            Self::Canvas(_) => None,
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
