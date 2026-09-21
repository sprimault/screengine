// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Une caméra qu'on déplace et qu'on oriente.
//!
//! Du comportement, pas un type de scène : elle produit la [`Camera`] du noyau
//! et n'en définit aucune autre. Ce que le moteur reçoit reste une position et
//! un quaternion — tout ce qui se fait ici se fait aussi depuis l'ABI C.

use screengine::{Affine3, Angle, Camera, Quat, Vec3};

use crate::{KeyCode, Tick};

/// L'avant du repère de vue une fois porté dans le monde par l'orientation
/// neutre : le +X.
const FORWARD: Vec3 = Vec3::new(1.0, 0.0, 0.0);

/// L'axe latéral de la caméra au repos, autour duquel le pitch tourne.
///
/// C'est l'image de « la droite de l'écran » dans le monde, et elle vaut −Y :
/// regardant l'est, le zénith en haut, on a le sud à sa droite.
const RIGHT: Vec3 = Vec3::new(0.0, -1.0, 0.0);

/// Le quart de tour, borne du pitch en radians.
const MAX_PITCH: f32 = core::f32::consts::FRAC_PI_2;

/// Une caméra qu'on dirige au clavier et à la souris.
///
/// Deux angles et une position, jamais une matrice accumulée : une orientation
/// qu'on compose image après image dérive, et se redresse mal. Ici, chaque
/// image reconstruit l'orientation depuis le lacet et le tangage, qui sont
/// l'état réel.
#[derive(Debug, Clone, Copy)]
pub struct FreeCamera {
    /// Où elle est, en coordonnées de monde.
    pub position: Vec3,
    /// Le lacet, en radians, autour du zénith. Positif vers le nord.
    pub yaw: f32,
    /// Le tangage, en radians, borné à un quart de tour. Positif vers le haut.
    pub pitch: f32,
    /// Vitesse de déplacement, en unités de monde par seconde.
    pub speed: f32,
    /// Radians de rotation par unité de déplacement de la souris.
    pub sensitivity: f32,
    /// Radians de rotation par seconde, aux touches.
    pub turn_rate: f32,
}

impl FreeCamera {
    /// Une caméra posée à `position`, regardant l'est.
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            speed: 4.0,
            sensitivity: 0.003,
            turn_rate: 2.0,
        }
    }

    /// Applique un pas d'entrées.
    ///
    /// Deux jeux de touches équivalents : **`W`, `A`, `S`, `D`** — donc `Z`,
    /// `Q`, `S`, `D` sur un clavier français, puisque les codes sont des
    /// positions physiques — et **les flèches**. Avancer, reculer, tourner à
    /// gauche, tourner à droite.
    ///
    /// Tourner et non le pas de côté : ce sont les seules touches qui
    /// permettent de s'orienter sans souris, et elles seules rejouent un
    /// déplacement à l'identique — la souris ne rend que des déplacements
    /// bruts, qu'aucun pas fixe ne reproduit.
    ///
    /// La souris n'oriente que lorsque le curseur est capturé : sans capture,
    /// il bute sur le bord de l'écran et la rotation s'arrêterait au milieu
    /// d'un mouvement, ce qui se lit comme un défaut du moteur.
    pub fn update(&mut self, tick: &Tick<'_>) {
        let dt = tick.dt();
        let input = tick.input();

        if tick.cursor_captured() {
            let (dx, dy) = input.mouse_delta();
            self.yaw -= dx * self.sensitivity;
            self.pitch -= dy * self.sensitivity;
        }
        let held = |key, other| -> f32 {
            match (input.down(key), input.down(other)) {
                (true, false) => 1.0,
                (false, true) => -1.0,
                _ => 0.0,
            }
        };
        let turn =
            held(KeyCode::ArrowLeft, KeyCode::ArrowRight) + held(KeyCode::KeyA, KeyCode::KeyD);
        self.yaw += turn.clamp(-1.0, 1.0) * self.turn_rate * dt;

        // Borné plutôt que replié : passé la verticale, l'image se retourne et
        // on croit à un défaut du moteur. Le roulis, lui, n'existe pas — il
        // n'est jamais accumulé, puisque l'orientation se reconstruit.
        self.pitch = self.pitch.clamp(-MAX_PITCH, MAX_PITCH);

        let ahead = held(KeyCode::ArrowUp, KeyCode::ArrowDown) + held(KeyCode::KeyW, KeyCode::KeyS);
        if ahead != 0.0 {
            // Le déplacement suit le lacet seul, pas le regard : dans un
            // couloir, avancer en regardant le plafond doit avancer, pas
            // monter.
            let heading = Affine3::from_rotation_translation(self.spin(), Vec3::ZERO);
            // Les deux touches d'un même axe se compensent, et le pas vaut au
            // plus un même si flèche et `W` sont tenues ensemble.
            let step = ahead.clamp(-1.0, 1.0) * self.speed * dt;
            self.position = self.position + heading.transform_vector(FORWARD) * step;
        }
    }

    /// Le lacet seul, sans le tangage.
    fn spin(&self) -> Quat {
        Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), Angle::from_radians(self.yaw))
    }

    /// La caméra du moteur : lacet puis tangage, autour de l'axe latéral.
    ///
    /// `spin.product(tilt)` et non l'inverse : le tangage s'applique d'abord,
    /// dans le repère de la caméra, et le lacet l'emporte ensuite dans le
    /// monde. Composé dans l'autre sens, le tangage pencherait autour d'un axe
    /// fixe et l'horizon basculerait dès qu'on tourne.
    pub fn camera(&self) -> Camera {
        let tilt = Quat::from_axis_angle(RIGHT, Angle::from_radians(self.pitch));
        Camera {
            position: self.position,
            orientation: self.spin().product(tilt),
            ..Camera::DEFAULT
        }
    }
}

#[cfg(test)]
mod tests;
