// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le vecteur des calculs hors image.
//!
//! **La règle d'usage est dans `docs/rust.md`**, au même endroit que la clause du
//! `f64` : `f32` dans le pipeline d'image, `f64` dans ce qui se calcule hors
//! image. [`Vec3`] sert le premier, celui-ci le second.
//!
//! Un type plutôt qu'une promotion champ par champ à chaque entrée de calcul :
//! celle-ci tient sur une expression courte — c'est ce que fait le contrôle de
//! repère de lightmap — et pas sur un enchaînement. Chaque promotion écrite à la
//! main est un endroit où elle peut être oubliée, et un oubli ne donne pas une
//! erreur de compilation mais un résultat en `f32` qui passe les tests et diverge
//! sur un cas limite. Le type le tient à la place de la relecture.
//!
//! **Le déterminisme n'y perd rien** : IEEE 754 impose les quatre opérations au
//! bit près en `f64` comme en `f32`, et une cible sans unité double passe par
//! `compiler-builtins`, correctement arrondi lui aussi.

use core::ops::{Add, Mul, Neg, Sub};

use super::Vec3;

/// Un vecteur ou un point, en double précision.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Vec3d {
    /// Abscisse.
    pub(crate) x: f64,
    /// Ordonnée.
    pub(crate) y: f64,
    /// Cote, vers le haut dans le monde.
    pub(crate) z: f64,
}

impl Vec3d {
    /// Le vecteur nul.
    pub(crate) const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// Un vecteur de composantes données.
    pub(crate) const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Le produit scalaire, sommé de gauche à droite.
    ///
    /// Même ordre que celui de [`Vec3`], et pour la même raison : il est
    /// contractuel, un chemin vectoriel devant l'accumuler composante par
    /// composante dans cet ordre.
    pub(crate) fn dot(self, other: Self) -> f64 {
        (self.x * other.x + self.y * other.y) + self.z * other.z
    }

    /// Le produit vectoriel, main droite.
    pub(crate) fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// Une composante, par son rang.
    ///
    /// Les trois axes d'une boîte se parcourent par leur rang : c'est ce qui
    /// permet à une seule boucle de servir les trois.
    pub(crate) fn axis(self, index: usize) -> f64 {
        match index {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
}

impl From<Vec3> for Vec3d {
    /// La promotion depuis le vecteur du pipeline.
    ///
    /// Exacte : tout `f32` fini a une image exacte en `f64`. C'est le seul point
    /// de passage entre les deux vocabulaires, et il est nommé pour cela.
    fn from(v: Vec3) -> Self {
        Self::new(f64::from(v.x), f64::from(v.y), f64::from(v.z))
    }
}

impl Add for Vec3d {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3d {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Neg for Vec3d {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<f64> for Vec3d {
    type Output = Self;

    fn mul(self, k: f64) -> Self {
        Self::new(self.x * k, self.y * k, self.z * k)
    }
}

#[cfg(test)]
mod tests;
