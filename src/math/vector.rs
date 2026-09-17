// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les vecteurs à trois composantes.
//!
//! Main droite, Z en haut dans le monde. Les opérateurs se réservent à ce qui se
//! calcule composante par composante ; tout ce dont l'ordre des sommes compte a
//! un nom, et l'ordre écrit dans son corps est celui que le SIMD reproduira.

use core::ops::{Add, Mul, Neg, Sub};

use super::rsqrt::rsqrt;

/// Un vecteur ou un point.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// Abscisse.
    pub x: f32,
    /// Ordonnée.
    pub y: f32,
    /// Cote, vers le haut dans le monde.
    pub z: f32,
}

impl Vec3 {
    /// Le vecteur nul.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// Un vecteur de composantes données.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Le produit scalaire, sommé de gauche à droite.
    pub fn dot(self, other: Self) -> f32 {
        (self.x * other.x + self.y * other.y) + self.z * other.z
    }

    /// Le produit vectoriel, main droite.
    pub fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// Le vecteur de même direction et de longueur un, ou le vecteur nul quand
    /// sa longueur est négligeable.
    ///
    /// Le nul plutôt qu'une erreur : une direction dégénérée est un cas de
    /// données, et c'est à l'appelant de décider ce qu'il en fait.
    pub fn normalize(self) -> Self {
        self * rsqrt(self.dot(self))
    }
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Neg for Vec3 {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, k: f32) -> Self {
        Self::new(self.x * k, self.y * k, self.z * k)
    }
}
