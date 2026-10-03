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

mod camera;
mod clock;
mod error;
mod input;
mod output;
mod runner;
mod scale;
#[cfg(test)]
mod tests;
mod texture;

pub use camera::FreeCamera;
pub use error::Error;
pub use input::Input;
pub use output::Output;
pub use scale::Scale;
pub use screengine;
// Réexportés et non redéfinis : ce crate ajoute du comportement, jamais des
// données. Deux modèles de scène qui divergeraient seraient la seule façon de
// le rater.
//
// Le critère d'entrée est le **vocabulaire qu'un hôte écrit**, et non ce que la
// surface de ce crate oblige à nommer : `Hit`, `Surfaces`, `SWEEP_CELLS` et
// `sweep_skin` ne figurent dans aucune signature d'ici, et un hôte qui déplace un
// personnage les écrit tous les quatre. Le dernier y est entré sur le constat d'un
// intégrateur, qui consommait ce crate et non le header : une fonction d'ABI seule
// ne lui aurait rien rendu de lisible. Ce que la surface impose en fait partie — `Context` est le
// paramètre des rappels de rendu, `Visibility` ce que rend `submit_world_visible`,
// les deux capacités sont les défauts des budgets que `Play` règle — mais ne
// l'épuise pas, et l'annoncer comme le critère laissait trois entrées hors de lui.
// Tout le reste du noyau reste joignable par `screengine::`, qui est réexporté
// entier juste au-dessus — cette liste est un raccourci, pas une frontière.
//
// `screengine::Output` n'y entre pas : ce crate a le sien, et les deux se
// rendraient inutilisables par le même nom.
pub use screengine::{
    Affine3, Angle, Camera, Color, Context, DepthMode, Filter, Hit, LINE_CAPACITY, Light, Lightmap,
    Lightmaps, Line, Mesh, Point, Quat, SWEEP_CELLS, Sprite, SpriteOrientation, Surfaces,
    TRIANGLE_CAPACITY, Texture, Triangle, Vec3, VertexUv, VertexUv2, Visibility, World, sweep_skin,
};
pub use texture::{load_png, load_png_masked};
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

// `Context` n'est plus ici : il entre dans la portée par le réexport public
// ci-dessus, et l'importer deux fois ne compile pas.
use screengine::Config;

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
    /// Le plafond des changements de résolution, ou la résolution d'ouverture.
    max_resolution: Option<(u32, u32)>,
    tile_size: u32,
    /// Les deux budgets d'une image, `0` valant chacun le défaut du moteur.
    /// Séparés comme le noyau les sépare : une ligne n'est pas un triangle.
    max_triangles: u32,
    max_lines: u32,
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
            max_resolution: None,
            tile_size: 64,
            max_triangles: 0,
            max_lines: 0,
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

    /// Le plafond que [`Context::set_resolution`] pourra atteindre en cours de
    /// partie. Sans lui, c'est la résolution d'ouverture.
    ///
    /// Le moteur dimensionne à la création tout ce qu'une image consomme, sur
    /// ce plafond : le relever coûte de la mémoire une fois, et c'est ce qui
    /// permet ensuite de baisser puis de remonter sans jamais allouer. Un jeu
    /// qui n'en veut pas ne perd rien — il peut toujours descendre.
    ///
    /// [`Context::set_resolution`]: screengine::Context::set_resolution
    pub fn max_resolution(mut self, width: u32, height: u32) -> Self {
        self.max_resolution = Some((width, height));
        self
    }

    /// Le côté des tuiles du moteur, 32 ou 64.
    pub fn tile_size(mut self, size: u32) -> Self {
        self.tile_size = size;
        self
    }

    /// Les triangles qu'une image peut recevoir, `0` valant le défaut du moteur.
    ///
    /// Le défaut suffit à tout ce que cet étage montre, et c'est pourquoi il est
    /// resté seul longtemps. Il ne suffit plus dès qu'un décor part **sans
    /// traversée** : le chemin qui soumet toutes les cellules d'un coup compte
    /// les triangles de la carte entière, pas ceux que la vue atteint, et
    /// quelques milliers de cellules en viennent à bout. C'est aussi ce chemin
    /// que la conformance compare au chemin déplié, donc un hôte qui ne peut pas
    /// relever ce budget ne peut plus éprouver l'égalité des deux.
    ///
    /// La valeur compte des triangles **préparés** : le plan proche en découpe un
    /// en jusqu'à six.
    pub fn max_triangles(mut self, count: u32) -> Self {
        self.max_triangles = count;
        self
    }

    /// Les primitives de tracé qu'une image peut recevoir, `0` valant le défaut
    /// du moteur.
    ///
    /// Un budget à part, et non une part du précédent : c'est le noyau qui en
    /// décide ainsi, et un calque d'éditeur qui trace mille repères ne doit pas
    /// vider la capacité de son décor.
    pub fn max_lines(mut self, count: u32) -> Self {
        self.max_lines = count;
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
        self.run_with_output(state, update, render, |_, _| {})
    }

    /// La même boucle, avec un rappel de plus : l'image finie avant la fenêtre.
    ///
    /// **Trois rappels et non deux**, parce qu'il y a trois temps dans une image
    /// et que le troisième n'appartient pas au moteur. `update` joue, `render`
    /// soumet, `output` écrit par-dessus ce que le moteur a rendu — voir
    /// [`Output`] pour ce qui va là et pourquoi.
    ///
    /// **Une méthode de plus plutôt qu'un paramètre de plus à [`run`](Self::run)**
    /// : un programme qui n'écrit rien par-dessus n'a pas à porter un rappel
    /// vide, et c'est la forme minimale qui fait la valeur de cet étage.
    pub fn run_with_output<S, U, R, O>(
        self,
        state: S,
        update: U,
        render: R,
        output: O,
    ) -> Result<(), Error>
    where
        U: FnMut(&mut S, &mut Tick<'_>),
        R: FnMut(&mut S, &mut Context),
        O: FnMut(&mut S, &mut Output<'_>),
    {
        if self.rate == 0 {
            return Err(Error::Setting("tick rate must be at least 1"));
        }
        if self.scale == Scale::Fixed(0) {
            return Err(Error::Setting("scale factor must be at least 1"));
        }

        let context = Context::new(self.config())?;

        runner::run(self, context, state, update, render, output)
    }

    /// La configuration que ces réglages demandent au moteur.
    ///
    /// À part du lancement, parce que c'est le seul endroit où un réglage peut se
    /// perdre en silence — et que le reste de `run` exige une fenêtre, donc ne se
    /// vérifie pas. Rien n'est validé ici : `Context::new` refuse, et
    /// [`run_with_output`](Self::run_with_output) rend ce refus avant d'ouvrir
    /// quoi que ce soit.
    fn config(&self) -> Config {
        let (width, height) = self.resolution;
        let (max_width, max_height) = self.max_resolution.unwrap_or(self.resolution);
        Config {
            max_width,
            max_height,
            width,
            height,
            tile_size: self.tile_size,
            max_triangles: self.max_triangles,
            max_lines: self.max_lines,
        }
    }
}

/// Ce que reçoit un pas de mise à jour.
#[derive(Debug)]
pub struct Tick<'a> {
    input: &'a Input,
    index: u64,
    dt: f32,
    exit: bool,
    captured: bool,
    capture: Option<bool>,
    title: Option<String>,
}

impl<'a> Tick<'a> {
    /// Un pas fabriqué de toutes pièces, pour les tests du crate.
    ///
    /// La boucle est le seul endroit qui construit un `Tick` en vrai, et elle
    /// exige une fenêtre. Sans cette porte, tout ce qui consomme un pas —
    /// au premier rang la caméra libre — ne se testerait qu'en réécrivant ses
    /// calculs dans le test, qui ne vérifierait alors que lui-même.
    #[cfg(test)]
    pub(crate) fn for_test(input: &'a Input, dt: f32, captured: bool) -> Self {
        Self {
            input,
            index: 0,
            dt,
            exit: false,
            captured,
            capture: None,
            title: None,
        }
    }

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

    /// Vrai quand le curseur est capturé : caché, et son déplacement rendu à la
    /// fenêtre plutôt qu'à l'écran.
    pub fn cursor_captured(&self) -> bool {
        self.captured
    }

    /// Capture le curseur, ou le relâche, à la fin de ce pas.
    ///
    /// Une caméra à la souris en a besoin : sans capture, le curseur bute sur
    /// le bord de l'écran et la rotation s'arrête. La demande est appliquée
    /// après le pas, et `cursor_captured` dit au pas suivant ce qui a
    /// réellement été obtenu — aucune plateforme ne garantit la capture, et
    /// celle qui la refuse laisse l'exemple jouable aux flèches plutôt que de
    /// refuser de démarrer.
    pub fn capture_cursor(&mut self, capture: bool) {
        self.capture = Some(capture);
    }

    /// Change le titre de la fenêtre, à la fin de ce pas.
    ///
    /// La barre de titre est le seul endroit où un jeu peut écrire tant que le
    /// moteur ne dessine pas de texte : un mode, un compteur, l'état d'un
    /// réglage qu'on vient de basculer. Appelée plusieurs fois dans le même
    /// pas, c'est la dernière qui compte.
    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_owned());
    }
}
