// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! L'état du clavier et de la souris, vu depuis un pas de mise à jour.

use std::collections::HashSet;

use winit::event::MouseButton;
use winit::keyboard::KeyCode;

/// Ce que la mise à jour sait des entrées.
///
/// Les touches sont physiques : [`KeyCode::KeyW`] désigne la touche placée là
/// où se trouve W sur un clavier QWERTY, quelle que soit la disposition. C'est
/// ce qu'un déplacement attend — Z sur un clavier AZERTY.
///
/// Les fronts — [`pressed`](Self::pressed), [`released`](Self::released) — valent
/// pour un seul pas. Quand un réveil de la boucle en exécute trois, seul le
/// premier les voit ; quand il n'en exécute aucun, ils attendent le suivant. Un
/// appui n'est donc jamais vu deux fois, ni perdu.
#[derive(Debug, Default)]
pub struct Input {
    keys_down: HashSet<KeyCode>,
    keys_pressed: HashSet<KeyCode>,
    keys_released: HashSet<KeyCode>,
    buttons_down: HashSet<MouseButton>,
    buttons_pressed: HashSet<MouseButton>,
    buttons_released: HashSet<MouseButton>,
    mouse_delta: (f64, f64),
    mouse_position: Option<(u32, u32)>,
}

impl Input {
    /// La touche est tenue.
    pub fn down(&self, key: KeyCode) -> bool {
        self.keys_down.contains(&key)
    }

    /// La touche a été enfoncée depuis le pas précédent.
    pub fn pressed(&self, key: KeyCode) -> bool {
        self.keys_pressed.contains(&key)
    }

    /// La touche a été relâchée depuis le pas précédent.
    pub fn released(&self, key: KeyCode) -> bool {
        self.keys_released.contains(&key)
    }

    /// Le bouton de la souris est tenu.
    pub fn button_down(&self, button: MouseButton) -> bool {
        self.buttons_down.contains(&button)
    }

    /// Le bouton a été enfoncé depuis le pas précédent.
    pub fn button_pressed(&self, button: MouseButton) -> bool {
        self.buttons_pressed.contains(&button)
    }

    /// Le bouton a été relâché depuis le pas précédent.
    pub fn button_released(&self, button: MouseButton) -> bool {
        self.buttons_released.contains(&button)
    }

    /// Le déplacement brut de la souris depuis le pas précédent.
    ///
    /// Brut : sans accélération du système, et indépendant de la position du
    /// curseur, qui peut buter sur le bord de l'écran. C'est ce qu'une caméra
    /// attend.
    pub fn mouse_delta(&self) -> (f32, f32) {
        (self.mouse_delta.0 as f32, self.mouse_delta.1 as f32)
    }

    /// La position du curseur en pixels de la résolution interne, ou `None`
    /// quand il est hors de l'image — dans une bande noire, ou hors de la
    /// fenêtre.
    pub fn mouse_position(&self) -> Option<(u32, u32)> {
        self.mouse_position
    }

    /// Enregistre l'appui ou le relâchement d'une touche.
    ///
    /// La répétition automatique du système n'arrive pas jusqu'ici : elle ne
    /// dit rien de plus que [`down`](Self::down).
    pub(crate) fn key(&mut self, key: KeyCode, down: bool) {
        if down {
            if self.keys_down.insert(key) {
                self.keys_pressed.insert(key);
            }
        } else if self.keys_down.remove(&key) {
            self.keys_released.insert(key);
        }
    }

    /// Enregistre l'appui ou le relâchement d'un bouton de la souris.
    pub(crate) fn button(&mut self, button: MouseButton, down: bool) {
        if down {
            if self.buttons_down.insert(button) {
                self.buttons_pressed.insert(button);
            }
        } else if self.buttons_down.remove(&button) {
            self.buttons_released.insert(button);
        }
    }

    /// Cumule un déplacement brut de la souris.
    pub(crate) fn motion(&mut self, dx: f64, dy: f64) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
    }

    /// Place le curseur, déjà ramené à la résolution interne.
    pub(crate) fn position(&mut self, position: Option<(u32, u32)>) {
        self.mouse_position = position;
    }

    /// Relâche tout ce qui est tenu, à la perte du focus.
    ///
    /// Une touche relâchée pendant que la fenêtre n'a pas le focus n'envoie
    /// aucun événement : sans ce relâchement, le personnage continuerait à
    /// avancer au retour.
    pub(crate) fn release_all(&mut self) {
        for key in self.keys_down.drain() {
            self.keys_released.insert(key);
        }
        for button in self.buttons_down.drain() {
            self.buttons_released.insert(button);
        }
    }

    /// Clôt un pas : les fronts et le déplacement ont été vus.
    pub(crate) fn end_step(&mut self) {
        self.keys_pressed.clear();
        self.keys_released.clear();
        self.buttons_pressed.clear();
        self.buttons_released.clear();
        self.mouse_delta = (0.0, 0.0);
    }
}

#[cfg(test)]
mod tests;
