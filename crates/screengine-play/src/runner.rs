// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La boucle d'événements : fenêtre, surface, pas fixe et recopie.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Duration;

use screengine::{BYTES_PER_PIXEL, Context};
use softbuffer::Surface;
// Celui de `std` panique sur wasm, faute d'horloge monotone, et c'est de toute
// façon celui-ci que `ControlFlow::WaitUntil` attend là-bas. Ailleurs, le crate
// réexporte `std::time::Instant` : le type est le même, et il n'y a pas deux
// chemins de temps à tenir.
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Icon as WinitIcon, Window, WindowId};

use crate::clock::Clock;
use crate::input::Input;
use crate::scale::{Scale, Scaler};
use crate::texture::Icon;
use crate::{DEFAULT_WINDOW_FACTOR, Error, FrameTime, Output, Play, Tick};

/// La fenêtre et sa surface, qui vivent et meurent ensemble.
///
/// La surface garde sa propre référence à la fenêtre : la détruire d'abord n'est
/// donc pas une question d'ordre des champs, mais les tenir dans le même
/// `Option` garantit qu'aucune des deux ne survit à l'autre dans l'état.
struct Display {
    window: Rc<Window>,
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    /// Faux quand la fenêtre est minimisée ou masquée : rien n'est rendu ni joué.
    visible: bool,
    /// Le contexte de dessin du canvas, par lequel l'image est posée.
    ///
    /// **La présentation de `softbuffer` n'est pas empruntée sur cette cible.**
    /// Elle reconstruit un tampon de plusieurs mégaoctets, pixel par pixel, à
    /// chaque image — vingt millisecondes mesurées sur un décor que le moteur
    /// remplit en douze. Son tampon, lui, reste la cible de la mise à l'échelle.
    #[cfg(target_arch = "wasm32")]
    context: web_sys::CanvasRenderingContext2d,
}

/// Le contexte de dessin du canvas que `winit` a créé pour la fenêtre.
#[cfg(target_arch = "wasm32")]
fn canvas_context(window: &Window) -> Result<web_sys::CanvasRenderingContext2d, Error> {
    use wasm_bindgen::JsCast;
    use winit::platform::web::WindowExtWebSys;

    let canvas = window
        .canvas()
        .ok_or_else(|| Error::Canvas(String::from("no canvas on this window")))?;
    canvas
        .get_context("2d")
        .map_err(|cause| Error::Canvas(format!("{cause:?}")))?
        .ok_or_else(|| Error::Canvas(String::from("no 2d context")))?
        .dyn_into()
        .map_err(|_| Error::Canvas(String::from("not a 2d context")))
}

/// Convertit le tampon de la mise à l'échelle en RGBA, dans un tampon réutilisé.
///
/// `softbuffer` range ses pixels en `0x00RRGGBB` quand le canvas attend quatre
/// octets dans l'ordre R, G, B, A — celui que le moteur produit déjà, et que
/// l'ABI garantit pour cette raison. **C'est tout ce qui reste du détour**, et
/// il disparaîtrait si la mise à l'échelle écrivait des octets ; la mesure dira
/// si cela vaut d'être fait.
///
/// **Compilée sur toutes les cibles bien qu'appelée sur une seule**, pour que
/// `cargo test` la couvre : un décalage d'un canal ne se verrait autrement que
/// dans un navigateur, et la doctrine de test du projet ne laisse pas une
/// conversion sans cas.
// `allow` et non `expect` : hors wasm, la fonction est morte pour la
// bibliothèque et vivante pour ses tests, si bien qu'une attente serait tantôt
// satisfaite tantôt non selon ce que la passe compile. Il se retire le jour où
// la mise à l'échelle écrit des octets, qui supprimera cette conversion.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn to_rgba(scaled: &[u32], frame: &mut Vec<u8>) {
    // N'alloue qu'à la première image, et à chaque changement de taille de
    // fenêtre : la capacité acquise reste.
    frame.resize(scaled.len() * 4, 0);
    for (pixel, out) in scaled.iter().zip(frame.chunks_exact_mut(4)) {
        out[0] = (pixel >> 16) as u8;
        out[1] = (pixel >> 8) as u8;
        out[2] = *pixel as u8;
        out[3] = 255;
    }
}

/// Pose l'image sur le canvas.
#[cfg(target_arch = "wasm32")]
fn put_frame(
    context: &web_sys::CanvasRenderingContext2d,
    frame: &[u8],
    width: u32,
    height: u32,
) -> Result<(), Error> {
    let image = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
        wasm_bindgen::Clamped(frame),
        width,
        height,
    )
    .map_err(|cause| Error::Canvas(format!("{cause:?}")))?;
    context
        .put_image_data(&image, 0.0, 0.0)
        .map_err(|cause| Error::Canvas(format!("{cause:?}")))
}

/// Capture le curseur ou le relâche ; rend ce qui a été obtenu.
///
/// Deux modes essayés dans l'ordre, parce qu'aucun n'existe partout : `Locked`
/// n'est pas implémenté sous X11, `Confined` ne l'est pas sur macOS. Quand les
/// deux échouent, la fonction rend faux et la boucle continue — une caméra à la
/// souris devient alors inutilisable, mais les touches restent, et refuser de
/// démarrer pour autant serait pire.
///
/// Le curseur se cache dans tous les cas : winit ne garantit pas qu'un mode de
/// capture le masque.
fn grab_cursor(window: &Window, capture: bool) -> bool {
    if !capture {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
        return false;
    }
    let grabbed = window.set_cursor_grab(CursorGrabMode::Locked).is_ok()
        || window.set_cursor_grab(CursorGrabMode::Confined).is_ok();
    window.set_cursor_visible(!grabbed);
    grabbed
}

/// Convertit une icône décodée en celle de `winit`.
///
/// **`None` sur un refus plutôt qu'une erreur remontée**, et c'est le seul
/// endroit du crate où un échec se perd volontairement : les dimensions ont
/// déjà été vérifiées au décodage, donc `from_rgba` ne peut échouer que sur un
/// désaccord entre la longueur et les côtés, qui serait un défaut d'ici et non
/// de l'hôte. La fenêtre s'ouvre alors avec l'icône du système, ce qui est
/// exactement ce que l'absence d'icône donne — et c'est déjà ce qui arrive sous
/// Wayland, où la pose est ignorée.
fn to_winit_icon(icon: &Icon) -> Option<WinitIcon> {
    WinitIcon::from_rgba(icon.rgba.clone(), icon.width, icon.height).ok()
}

/// La période du relevé affiché.
const STATS_WINDOW: Duration = Duration::from_secs(1);

/// Une durée en millisecondes, au dixième.
///
/// Par les microsecondes entières plutôt que par `{:.1}` sur un flottant : ce
/// qui se lit dans une barre de titre française porte une virgule décimale, et
/// la passer par un `replace` après coup serait écrire le nombre deux fois.
fn millis(duration: Duration) -> String {
    let tenths = (duration.as_micros() + 50) / 100;
    format!("{},{}", tenths / 10, tenths % 10)
}

/// Le titre que porte la fenêtre : celui du jeu, puis le relevé s'il y en a un.
///
/// Le jeu garde le sien en entier — c'est lui qui porte les commandes, et une
/// mesure n'a pas à les chasser.
fn window_title(title: &str, stats: Option<&str>) -> String {
    match stats {
        Some(stats) => format!("{title} · {stats}"),
        None => title.to_owned(),
    }
}

/// Pose le titre du jeu et son relevé, chacun là où on le lit.
///
/// **Sur le web, `winit` écrit le titre dans l'attribut `alt` du canvas** — du
/// texte alternatif que personne ne regarde —, et le titre du document reste
/// celui de la page. L'étage d'accueil l'y porte donc lui-même, comme il insère
/// déjà le canvas que `winit` crée sans le placer.
fn set_window_title(window: &Window, title: &str, stats: Option<&str>) {
    let full = window_title(title, stats);
    window.set_title(&full);
    #[cfg(target_arch = "wasm32")]
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        document.set_title(&full);
        if let Some(stats) = stats {
            show_web_stats(&document, stats);
        }
    }
}

/// Écrit le relevé dans la page, au-dessus du canvas.
///
/// **Un onglet ne montre que les premières lettres de son titre**, si bien que
/// le relevé y est coupé avant son premier chiffre : sur cette cible, le titre
/// ne suffit pas, et c'est pourtant celle que ce relevé sert d'abord.
///
/// L'élément est créé au premier relevé puis réutilisé. Une page qui en fournit
/// un sous le même identifiant garde le sien, **style compris** : c'est ainsi
/// qu'un jeu le place où il veut sans que l'étage d'accueil ait un réglage de
/// plus.
#[cfg(target_arch = "wasm32")]
fn show_web_stats(document: &web_sys::Document, line: &str) {
    /// L'identifiant de l'élément, qu'une page peut poser elle-même.
    const ID: &str = "screengine-stats";

    let element = match document.get_element_by_id(ID) {
        Some(element) => element,
        None => {
            let Ok(element) = document.create_element("div") else {
                return;
            };
            element.set_id(ID);
            // Le style est posé ici et non laissé à la page : celle d'un jeu ne
            // connaît pas cet identifiant, et un relevé invisible ne vaut pas
            // mieux qu'un relevé absent. Hors du flux, pour ne pas déplacer le
            // canvas que la page a centré.
            let _ = element.set_attribute(
                "style",
                "position:fixed;top:0;left:0;right:0;padding:4px 8px;\
                 font:13px/1.4 ui-monospace,monospace;color:#c8c8d0;\
                 background:#101014;text-align:center",
            );
            let Some(body) = document.body() else {
                return;
            };
            if body
                .insert_before(&element, body.first_child().as_ref())
                .is_err()
            {
                return;
            }
            element
        }
    };
    element.set_text_content(Some(line));
}

/// Ce que les images d'une seconde ont coûté, phase par phase.
///
/// Moyenné sur une seconde et non affiché par image : un chiffre qui change
/// soixante fois par seconde ne se lit pas, et la moyenne absorbe l'image
/// isolée qu'un ramasse-miettes de navigateur ou un autre processus a allongée.
#[derive(Default)]
struct Stats {
    /// Ce que le jeu a passé à soumettre sa scène.
    submit: Duration,
    /// Ce que le moteur a passé à remplir ses tuiles.
    fill: Duration,
    /// Le dessin de l'hôte, la mise à l'échelle et la présentation.
    screen: Duration,
    frames: u32,
    /// Le début de la fenêtre de mesure, posé par la première image.
    since: Option<Instant>,
}

impl Stats {
    /// Retient ce qu'une image vient de coûter.
    fn record(&mut self, submit: Duration, fill: Duration, screen: Duration, now: Instant) {
        self.submit += submit;
        self.fill += fill;
        self.screen += screen;
        self.frames += 1;
        self.since.get_or_insert(now);
    }

    /// Rend le relevé quand la période est écoulée, et repart de zéro.
    ///
    /// La moyenne ne divise jamais par zéro : `since` et `frames` sont posés par
    /// le même appel, donc le premier ne vaut `Some` qu'à partir de la première
    /// image comptée.
    fn take(&mut self, now: Instant) -> Option<String> {
        let since = self.since?;
        if now.duration_since(since) < STATS_WINDOW {
            return None;
        }
        let frames = self.frames;
        // Rapportées à la durée réelle de la fenêtre, et non affichées telles
        // quelles : celle-ci dure un peu plus d'une seconde — le relevé se prend
        // à la fin d'une image, pas à l'instant pile —, et le compte brut
        // annonçait soixante-deux images par seconde sur un écran qui en montre
        // soixante.
        let per_second = u128::from(frames) * 1_000_000 / now.duration_since(since).as_micros();
        let line = format!(
            "{per_second} img/s · soumission {} · remplissage {} · écran {} ms",
            millis(self.submit / frames),
            millis(self.fill / frames),
            millis(self.screen / frames),
        );
        *self = Self::default();
        Some(line)
    }
}

/// L'état de la boucle, et ce que l'appelant lui a confié.
struct Runner<S, U, R, O> {
    play: Play,
    context: Context,
    state: S,
    update: U,
    render: R,
    output: O,
    graphics: softbuffer::Context<OwnedDisplayHandle>,
    display: Option<Display>,
    pixels: Vec<u8>,
    scaler: Scaler,
    clock: Clock,
    input: Input,
    index: u64,
    last_wake: Option<Instant>,
    /// Vrai quand le curseur est réellement capturé par la fenêtre.
    captured: bool,
    stats: Stats,
    /// Ce que la dernière image a coûté, que le pas suivant lit par son `Tick`.
    ///
    /// Tenu à part du relevé, qui moyenne sur une seconde et se vide : ce sont
    /// deux grandeurs différentes, et les confondre donnerait au jeu une valeur
    /// qui change une fois par seconde au lieu d'une fois par image.
    frame_time: Option<FrameTime>,
    /// L'image en RGBA que le canvas reçoit, gardée d'une image à l'autre.
    #[cfg(target_arch = "wasm32")]
    frame: Vec<u8>,
    /// Le titre que le jeu a posé, gardé pour le réécrire sous le relevé
    /// suivant : sans lui, une mesure qui se rafraîchit effacerait ce que le
    /// jeu venait d'écrire.
    title: String,
    /// Le dernier relevé composé, absent tant qu'une seconde ne s'est pas
    /// écoulée.
    measured: Option<String>,
}

/// Ouvre la boucle, et rend ce qui a été refusé avant toute fenêtre.
///
/// Ce que la boucle lève ensuite part au puits de `play` : voir
/// [`Play::on_error`](crate::Play::on_error) pour la raison, qui est le
/// lancement sur wasm.
pub(crate) fn run<S, U, R, O>(
    play: Play,
    context: Context,
    state: S,
    update: U,
    render: R,
    output: O,
) -> Result<(), Error>
where
    S: 'static,
    U: FnMut(&mut S, &mut Tick<'_>) + 'static,
    R: FnMut(&mut S, &mut Context) + 'static,
    O: FnMut(&mut S, &mut Output<'_>) + 'static,
{
    // Pris avant que `play` entre dans le runner, et seulement là où quelque
    // chose le lira : sur wasm le lancement emporte le runner, et c'est la
    // boucle elle-même qui appellera le puits par `fail`.
    #[cfg(not(target_arch = "wasm32"))]
    let on_error = play.on_error;
    let event_loop = EventLoop::new().map_err(Error::EventLoop)?;
    let graphics = softbuffer::Context::new(event_loop.owned_display_handle())?;

    let (width, height) = context.resolution();
    // Le tampon est dimensionné sur le plafond, pas sur la résolution
    // d'ouverture : le jeu peut remonter entre deux images, et une
    // réallocation en cours de partie est précisément ce que le contexte
    // s'interdit de son côté.
    let config = context.config();
    let ceiling = config.max_width as usize * config.max_height as usize * BYTES_PER_PIXEL;
    let runner = Runner {
        pixels: vec![0; ceiling],
        scaler: Scaler::new(play.scale, width, height),
        clock: Clock::new(play.rate),
        title: play.title.clone(),
        play,
        context,
        state,
        update,
        render,
        output,
        graphics,
        display: None,
        input: Input::default(),
        index: 0,
        last_wake: None,
        captured: false,
        stats: Stats::default(),
        frame_time: None,
        #[cfg(target_arch = "wasm32")]
        frame: Vec::new(),
        measured: None,
    };

    // La boucle ne se lance pas de la même façon des deux côtés, et ce n'est pas
    // un réglage : un navigateur ne peut pas bloquer le fil qui l'appelle, donc
    // `spawn_app` prend le runner et rend la main tout de suite. C'est de là que
    // viennent les bornes `'static`, et c'est pourquoi le puits reçoit ce que la
    // boucle lève — y compris, en natif, l'arrêt de la boucle elle-même, dont le
    // retour n'existe pas là-bas.
    #[cfg(not(target_arch = "wasm32"))]
    {
        // `run_app` emprunte en exclusif là où `spawn_app` prend : le `mut` ne
        // vaut que de ce côté, et il est ici plutôt que sur la construction pour
        // qu'aucune cible ne le porte sans l'employer.
        let mut app = runner;
        if let Err(error) = event_loop.run_app(&mut app) {
            on_error(&Error::EventLoop(error));
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;

        event_loop.spawn_app(runner);
    }
    Ok(())
}

impl<S, U, R, O> Runner<S, U, R, O>
where
    U: FnMut(&mut S, &mut Tick<'_>),
    R: FnMut(&mut S, &mut Context),
    O: FnMut(&mut S, &mut Output<'_>),
{
    /// Passe l'erreur au puits et arrête la boucle.
    ///
    /// Au puits et non dans un champ que `run` relirait : sur wasm le lancement
    /// rend la main avant que la boucle tourne, et ce champ ne serait jamais
    /// relu. Un seul chemin sur toutes les cibles plutôt qu'un retour ici et un
    /// puits là-bas.
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Error) {
        (self.play.on_error)(&error);
        event_loop.exit();
    }

    /// Ouvre la fenêtre à la résolution interne multipliée par le facteur
    /// d'ouverture, et sa surface.
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<Display, Error> {
        let (width, height) = self.context.resolution();
        let factor = match self.play.scale {
            Scale::Fixed(factor) => factor,
            Scale::Integer | Scale::Fill => DEFAULT_WINDOW_FACTOR,
        };
        let size = (width.saturating_mul(factor), height.saturating_mul(factor));

        // Seul un facteur imposé fait de l'écran trop petit une erreur. Sans
        // écran principal connu — Wayland n'en expose pas —, rien ne se vérifie
        // et le système réduit la fenêtre.
        if let (Scale::Fixed(factor), Some(monitor)) =
            (self.play.scale, event_loop.primary_monitor())
        {
            let screen = monitor.size();
            if size.0 > screen.width || size.1 > screen.height {
                return Err(Error::ScaleTooLarge {
                    factor,
                    window: size,
                    monitor: (screen.width, screen.height),
                });
            }
        }

        // En pixels physiques : une taille logique serait multipliée par le
        // facteur d'échelle de l'écran, et le facteur entier ne tomberait plus
        // juste.
        let attributes = Window::default_attributes()
            .with_title(self.play.title.clone())
            .with_window_icon(self.play.icon.as_ref().and_then(to_winit_icon))
            .with_inner_size(PhysicalSize::new(size.0, size.1));

        // winit crée bien le canvas, mais ne l'insère pas dans la page : c'est
        // son défaut, et il laisse à l'appelant le soin de le placer. Or
        // softbuffer retrouve le canvas en interrogeant le document par
        // l'attribut que winit y a posé — hors du document, la création de
        // surface échoue sur « No canvas found with the given id ». Insérer est
        // donc la seule façon d'avoir un défaut qui rende une image.
        #[cfg(target_arch = "wasm32")]
        let attributes = {
            use winit::platform::web::WindowAttributesExtWebSys;

            attributes.with_append(true)
        };
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(Error::Window)?,
        );
        // Le titre d'ouverture, que les attributs ci-dessus ne portent que pour
        // une vraie barre de titre : sur le web, c'est celui de la page qui
        // resterait.
        set_window_title(&window, &self.play.title, None);
        let surface = Surface::new(&self.graphics, Rc::clone(&window))?;
        #[cfg(target_arch = "wasm32")]
        let context = canvas_context(&window)?;

        let mut display = Display {
            window,
            surface,
            visible: true,
            #[cfg(target_arch = "wasm32")]
            context,
        };
        let inner = display.window.inner_size();
        self.resize(&mut display, inner)?;
        Ok(display)
    }

    /// Suit la taille de la fenêtre. Une taille nulle est une fenêtre
    /// minimisée : la surface ne l'accepte pas, et il n'y a rien à montrer.
    fn resize(&mut self, display: &mut Display, size: PhysicalSize<u32>) -> Result<(), Error> {
        match (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
            (Some(width), Some(height)) => {
                display.surface.resize(width, height)?;
                self.scaler.resize(size.width, size.height);
                self.show(display, true);
                // La surface vient d'être redimensionnée et son contenu ne vaut
                // plus rien. Sans image demandée ici, la fenêtre garde la
                // précédente à l'ancienne géométrie jusqu'au prochain pas joué,
                // que `about_to_wait` est seul à provoquer : la zone découverte
                // reste noire le temps d'une période de mise à jour.
                display.window.request_redraw();
            }
            _ => self.show(display, false),
        }
        Ok(())
    }

    /// Passe la fenêtre à visible ou non.
    ///
    /// Au retour, le temps écoulé pendant l'absence est abandonné : il n'a pas
    /// été joué, et le rattraper ferait avancer la partie d'un coup.
    fn show(&mut self, display: &mut Display, visible: bool) {
        if visible && !display.visible {
            self.clock.reset();
            self.last_wake = None;
            display.window.request_redraw();
        }
        display.visible = visible;
    }

    /// Fait rendre le moteur, puis recopie son image dans la fenêtre.
    ///
    /// Les quatre lectures d'horloge sont prises même quand aucun relevé ne les
    /// affiche : elles ne se mesurent pas à côté d'une image, et les rendre
    /// conditionnelles couperait cette fonction en deux chemins pour en
    /// économiser le millième.
    fn redraw(&mut self, display: &mut Display) -> Result<(), Error> {
        let opened = Instant::now();
        (self.render)(&mut self.state, &mut self.context);
        let submitted = Instant::now();
        // Le jeu a pu changer la résolution interne dans `render` : toute la
        // disposition de la fenêtre en dépend, et la laisser derrière
        // afficherait l'image nouvelle à la géométrie de l'ancienne.
        let (width, height) = self.context.resolution();
        if self.scaler.source() != (width, height) {
            self.scaler.set_source(width, height);
        }
        self.context.frame_end(&mut self.pixels, width)?;
        let filled = Instant::now();

        // **L'hôte écrit par-dessus l'image finie, avant la recopie.** C'est le
        // pendant exact de ce qu'un hôte C fait après `scg_frame_end`, et le seul
        // endroit où une interface — jauge, score, arme en main — a sa place : le
        // moteur ne la connaît pas, et le tracé de lignes ne la sert pas, qui
        // prend des coordonnées de monde.
        //
        // Le tampon est dimensionné sur le plafond, donc plus grand que l'image :
        // c'est `Output` qui en borne l'accès sur la résolution courante.
        (self.output)(
            &mut self.state,
            &mut Output::new(&mut self.pixels, width, height, width),
        );

        let mut buffer = display.surface.buffer_mut()?;
        self.scaler.blit(&self.pixels, &mut buffer);
        #[cfg(not(target_arch = "wasm32"))]
        buffer.present()?;
        #[cfg(target_arch = "wasm32")]
        {
            // Le tampon n'est pas présenté : son contenu vient d'être recopié,
            // et c'est `present` que ce chemin remplace. Rien ne reste à
            // défaire — il ne porte pas de `Drop`, et son emprunt s'arrête à la
            // ligne ci-dessus.
            to_rgba(&buffer, &mut self.frame);
            let (width, height) = self.scaler.target();
            put_frame(&display.context, &self.frame, width, height)?;
        }

        let presented = Instant::now();
        // Posées quoi que vaille `stats` : le relevé affiché est un service de
        // l'étage d'accueil, ces durées sont ce que l'ABI donne déjà à un hôte
        // C. Les conditionner reviendrait à faire dépendre une capacité d'un
        // réglage d'affichage.
        self.frame_time = Some(FrameTime {
            submit: submitted - opened,
            fill: filled - submitted,
            screen: presented - filled,
        });
        if self.play.stats {
            self.stats.record(
                submitted - opened,
                filled - submitted,
                presented - filled,
                presented,
            );
            if let Some(line) = self.stats.take(presented) {
                self.measured = Some(line);
                set_window_title(&display.window, &self.title, self.measured.as_deref());
            }
        }
        Ok(())
    }
}

impl<S, U, R, O> ApplicationHandler for Runner<S, U, R, O>
where
    U: FnMut(&mut S, &mut Tick<'_>),
    R: FnMut(&mut S, &mut Context),
    O: FnMut(&mut S, &mut Output<'_>),
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.display.is_some() {
            return;
        }
        match self.open(event_loop) {
            Ok(display) => self.display = Some(display),
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.display = None;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(mut display) = self.display.take() else {
            return;
        };

        let result = match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                Ok(())
            }
            WindowEvent::Resized(size) => self.resize(&mut display, size),
            WindowEvent::Occluded(hidden) => {
                self.show(&mut display, !hidden);
                Ok(())
            }
            WindowEvent::Focused(false) => {
                self.input.release_all();
                Ok(())
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let down = event.state == ElementState::Pressed;
                    if down && code == KeyCode::Escape && self.play.exit_on_escape {
                        event_loop.exit();
                    }
                    if !event.repeat {
                        self.input.key(code, down);
                    }
                }
                Ok(())
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.input.button(button, state == ElementState::Pressed);
                Ok(())
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input
                    .position(self.scaler.to_source(position.x, position.y));
                Ok(())
            }
            WindowEvent::CursorLeft { .. } => {
                self.input.position(None);
                Ok(())
            }
            WindowEvent::RedrawRequested if display.visible => self.redraw(&mut display),
            _ => Ok(()),
        };

        self.display = Some(display);
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.input.motion(delta.0, delta.1);
        }
    }

    /// Joue les pas dus, demande une image s'il y en a eu, et dort jusqu'au
    /// suivant.
    ///
    /// Rendre sans pas joué ne montrerait que l'image précédente : sans
    /// interpolation entre deux pas, le rythme des images suit celui de la
    /// mise à jour.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(display) = &self.display else {
            return;
        };
        if !display.visible {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }

        let now = Instant::now();
        let elapsed = self
            .last_wake
            .map_or_else(Default::default, |last| now - last);
        self.last_wake = Some(now);

        let steps = self.clock.advance(elapsed);
        for _ in 0..steps {
            let mut tick = Tick {
                input: &self.input,
                index: self.index,
                dt: self.clock.step_seconds(),
                frame: self.frame_time,
                // La disposition que la dernière image a servie : le pas qui la
                // lit décide de la résolution que le rappel de rendu demandera,
                // et c'est `redraw` qui la reprend juste après.
                window: self.scaler.target(),
                viewport: self.scaler.extent(),
                exit: false,
                captured: self.captured,
                capture: None,
                title: None,
                icon: None,
            };
            (self.update)(&mut self.state, &mut tick);
            let exit = tick.exit;
            if let Some(capture) = tick.capture {
                self.captured = grab_cursor(&display.window, capture);
            }
            if let Some(title) = tick.title.take() {
                self.title = title;
                set_window_title(&display.window, &self.title, self.measured.as_deref());
            }
            if let Some(icon) = &tick.icon {
                display.window.set_window_icon(to_winit_icon(icon));
            }
            self.input.end_step();
            self.index += 1;
            if exit {
                event_loop.exit();
                return;
            }
        }

        if steps > 0 {
            display.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(now + self.clock.until_next()));
    }
}

#[cfg(test)]
mod tests;
