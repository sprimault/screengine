// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La boucle d'événements : fenêtre, surface, pas fixe et recopie.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use screengine::{BYTES_PER_PIXEL, Context};
use softbuffer::Surface;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::clock::Clock;
use crate::input::Input;
use crate::scale::{Scale, Scaler};
use crate::{DEFAULT_WINDOW_FACTOR, Error, Play, Tick};

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

/// L'état de la boucle, et ce que l'appelant lui a confié.
struct Runner<S, U, R> {
    play: Play,
    context: Context,
    state: S,
    update: U,
    render: R,
    graphics: softbuffer::Context<OwnedDisplayHandle>,
    display: Option<Display>,
    pixels: Vec<u8>,
    scaler: Scaler,
    clock: Clock,
    input: Input,
    index: u64,
    last_wake: Option<Instant>,
    failure: Option<Error>,
    /// Vrai quand le curseur est réellement capturé par la fenêtre.
    captured: bool,
}

/// Lance la boucle, et rend la première erreur qui l'a arrêtée.
pub(crate) fn run<S, U, R>(
    play: Play,
    context: Context,
    state: S,
    update: U,
    render: R,
) -> Result<(), Error>
where
    U: FnMut(&mut S, &mut Tick<'_>),
    R: FnMut(&mut S, &mut Context),
{
    let event_loop = EventLoop::new().map_err(Error::EventLoop)?;
    let graphics = softbuffer::Context::new(event_loop.owned_display_handle())?;

    let (width, height) = context.resolution();
    // Le tampon est dimensionné sur le plafond, pas sur la résolution
    // d'ouverture : le jeu peut remonter entre deux images, et une
    // réallocation en cours de partie est précisément ce que le contexte
    // s'interdit de son côté.
    let config = context.config();
    let ceiling = config.max_width as usize * config.max_height as usize * BYTES_PER_PIXEL;
    let mut runner = Runner {
        pixels: vec![0; ceiling],
        scaler: Scaler::new(play.scale, width, height),
        clock: Clock::new(play.rate),
        play,
        context,
        state,
        update,
        render,
        graphics,
        display: None,
        input: Input::default(),
        index: 0,
        last_wake: None,
        failure: None,
        captured: false,
    };

    event_loop.run_app(&mut runner).map_err(Error::EventLoop)?;
    runner.failure.map_or(Ok(()), Err)
}

impl<S, U, R> Runner<S, U, R>
where
    U: FnMut(&mut S, &mut Tick<'_>),
    R: FnMut(&mut S, &mut Context),
{
    /// Retient l'erreur et arrête la boucle : `run` la rendra.
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Error) {
        self.failure.get_or_insert(error);
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
            .with_inner_size(PhysicalSize::new(size.0, size.1));
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(Error::Window)?,
        );
        let surface = Surface::new(&self.graphics, Rc::clone(&window))?;

        let mut display = Display {
            window,
            surface,
            visible: true,
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
    fn redraw(&mut self, display: &mut Display) -> Result<(), Error> {
        (self.render)(&mut self.state, &mut self.context);
        // Le jeu a pu changer la résolution interne dans `render` : toute la
        // disposition de la fenêtre en dépend, et la laisser derrière
        // afficherait l'image nouvelle à la géométrie de l'ancienne.
        let (width, height) = self.context.resolution();
        if self.scaler.source() != (width, height) {
            self.scaler.set_source(width, height);
        }
        self.context.frame_end(&mut self.pixels, width)?;

        let mut buffer = display.surface.buffer_mut()?;
        self.scaler.blit(&self.pixels, &mut buffer);
        buffer.present()?;
        Ok(())
    }
}

impl<S, U, R> ApplicationHandler for Runner<S, U, R>
where
    U: FnMut(&mut S, &mut Tick<'_>),
    R: FnMut(&mut S, &mut Context),
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
                exit: false,
                captured: self.captured,
                capture: None,
                title: None,
            };
            (self.update)(&mut self.state, &mut tick);
            let exit = tick.exit;
            if let Some(capture) = tick.capture {
                self.captured = grab_cursor(&display.window, capture);
            }
            if let Some(title) = &tick.title {
                display.window.set_title(title);
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
