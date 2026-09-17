// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les quaternions d'orientation.
//!
//! Ils portent l'orientation de la caméra et des objets, et s'interpolent entre
//! deux pas de simulation. Convertis en rotation une fois par objet et par
//! image, jamais par sommet.

use super::angle::Angle;
use super::rsqrt::rsqrt;
use super::vector::Vec3;

/// Un quaternion `x·i + y·j + z·k + w`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    /// Composante en `i`.
    pub x: f32,
    /// Composante en `j`.
    pub y: f32,
    /// Composante en `k`.
    pub z: f32,
    /// Partie réelle.
    pub w: f32,
}

impl Quat {
    /// L'orientation neutre.
    pub const IDENTITY: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// Un quaternion de composantes données.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// La rotation de `angle` autour de `axis`, dans le sens direct.
    ///
    /// L'axe est normalisé ici ; un axe nul donne l'identité plutôt qu'un
    /// quaternion nul, qui ne décrit aucune rotation.
    pub fn from_axis_angle(axis: Vec3, angle: Angle) -> Self {
        let axis = axis.normalize();
        if axis == Vec3::ZERO {
            return Self::IDENTITY;
        }
        let half = angle.half();
        let (s, c) = (half.sin(), half.cos());
        Self::new(axis.x * s, axis.y * s, axis.z * s, c)
    }

    /// Le produit `self · other` : la rotation `other` d'abord, puis `self`.
    ///
    /// Une méthode et non `Mul` : l'ordre compte, et il se lit mieux nommé.
    pub fn product(self, other: Self) -> Self {
        let (a, b) = (self, other);
        Self {
            x: ((a.w * b.x + a.x * b.w) + a.y * b.z) - a.z * b.y,
            y: ((a.w * b.y - a.x * b.z) + a.y * b.w) + a.z * b.x,
            z: ((a.w * b.z + a.x * b.y) - a.y * b.x) + a.z * b.w,
            w: ((a.w * b.w - a.x * b.x) - a.y * b.y) - a.z * b.z,
        }
    }

    /// Le conjugué, qui est l'inverse d'un quaternion unitaire.
    pub fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Le produit scalaire à quatre composantes, sommé de gauche à droite.
    pub fn dot(self, other: Self) -> f32 {
        ((self.x * other.x + self.y * other.y) + self.z * other.z) + self.w * other.w
    }

    /// Le quaternion unitaire de même orientation, ou l'identité pour un
    /// quaternion de norme négligeable.
    ///
    /// Le moteur normalise à la réception plutôt que d'exiger une norme unité :
    /// l'exiger imposerait une tolérance, forcément arbitraire.
    pub fn normalize(self) -> Self {
        let k = rsqrt(self.dot(self));
        if k == 0.0 {
            return Self::IDENTITY;
        }
        Self::new(self.x * k, self.y * k, self.z * k, self.w * k)
    }

    /// L'interpolation normalisée de `self` vers `other`, par le plus court
    /// chemin.
    ///
    /// Plutôt que l'interpolation sphérique : pour dix degrés entre deux pas,
    /// l'écart reste sous le millième de degré, et le sphérique exigerait un
    /// arc cosinus et une division par un sinus qui tend vers zéro.
    pub fn nlerp(self, other: Self, t: f32) -> Self {
        // `q` et `-q` sont la même orientation : on prend celui qui est du même
        // côté, sans quoi l'interpolation ferait le grand tour.
        let b = if self.dot(other) < 0.0 {
            Self::new(-other.x, -other.y, -other.z, -other.w)
        } else {
            other
        };
        let a = self;
        Self::new(
            a.x + (b.x - a.x) * t,
            a.y + (b.y - a.y) * t,
            a.z + (b.z - a.z) * t,
            a.w + (b.w - a.w) * t,
        )
        .normalize()
    }
}
