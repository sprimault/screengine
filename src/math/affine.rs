// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les transformations affines, en 3×4.
//!
//! Vecteur colonne, `M·v`, stockage par colonnes : trois colonnes de rotation et
//! d'échelle, puis la translation. La projection n'en est pas une : c'est
//! `(sx, sy, cx, cy, near)` appliqué à part, qui épargne quatre produits par
//! sommet et une ligne dont les bits ne serviraient à rien.
//!
//! Chaque composante s'écrit à la main, sommée de gauche à droite dans l'ordre
//! des colonnes. C'est l'ordre naturel d'un chemin SIMD qui accumule colonne par
//! colonne, et rustc ne réassocie jamais une expression flottante : écrit ainsi,
//! le résultat est le même partout.

use super::quat::Quat;
use super::vector::Vec3;

/// Une transformation affine : `m[col * 3 + row]`, la translation en
/// `m[9..12]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine3 {
    /// Les douze coefficients, par colonnes.
    pub m: [f32; 12],
}

impl Affine3 {
    /// L'identité.
    pub const IDENTITY: Self = Self {
        m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    };

    /// La rotation d'un quaternion unitaire, suivie de la translation `t`.
    pub fn from_rotation_translation(q: Quat, t: Vec3) -> Self {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz) = (x * y, x * z, y * z);
        let (wx, wy, wz) = (w * x, w * y, w * z);
        Self {
            m: [
                1.0 - 2.0 * (yy + zz),
                2.0 * (xy + wz),
                2.0 * (xz - wy),
                2.0 * (xy - wz),
                1.0 - 2.0 * (xx + zz),
                2.0 * (yz + wx),
                2.0 * (xz + wy),
                2.0 * (yz - wx),
                1.0 - 2.0 * (xx + yy),
                t.x,
                t.y,
                t.z,
            ],
        }
    }

    /// Le produit `self · other` : `other` s'applique d'abord.
    ///
    /// Une composition se lit `parent.product(local)`. C'est ainsi que le
    /// contexte compose la vue avec la matrice modèle d'une soumission ; il n'y
    /// a pas de pile, et la matrice modèle-vue n'existe qu'ici.
    pub fn product(self, other: Self) -> Self {
        let a = &self.m;
        let b = &other.m;
        // Le coefficient `r` de la colonne qui commence en `col` : la ligne `r`
        // de `a` contre cette colonne de `b`.
        let row =
            |r: usize, col: usize| (a[r] * b[col] + a[3 + r] * b[col + 1]) + a[6 + r] * b[col + 2];
        let t = |r: usize| row(r, 9) + a[9 + r];
        Self {
            m: [
                row(0, 0),
                row(1, 0),
                row(2, 0),
                row(0, 3),
                row(1, 3),
                row(2, 3),
                row(0, 6),
                row(1, 6),
                row(2, 6),
                t(0),
                t(1),
                t(2),
            ],
        }
    }

    /// Transforme un point, translation comprise.
    pub fn transform_point(self, p: Vec3) -> Vec3 {
        let m = &self.m;
        Vec3::new(
            ((m[0] * p.x + m[3] * p.y) + m[6] * p.z) + m[9],
            ((m[1] * p.x + m[4] * p.y) + m[7] * p.z) + m[10],
            ((m[2] * p.x + m[5] * p.y) + m[8] * p.z) + m[11],
        )
    }

    /// Transforme une direction, sans la translation.
    pub fn transform_vector(self, v: Vec3) -> Vec3 {
        let m = &self.m;
        Vec3::new(
            (m[0] * v.x + m[3] * v.y) + m[6] * v.z,
            (m[1] * v.x + m[4] * v.y) + m[7] * v.z,
            (m[2] * v.x + m[5] * v.y) + m[8] * v.z,
        )
    }

    /// L'inverse d'une transformation rigide — rotation et translation seules.
    ///
    /// La transposée de la rotation, puis la translation ramenée par elle :
    /// aucune inverse générale, donc aucune division. Sur une transformation
    /// avec échelle, le résultat est faux, et c'est une précondition.
    pub fn inverse_rigid(self) -> Self {
        let m = &self.m;
        let r = [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]];
        let t = Vec3::new(m[9], m[10], m[11]);
        let back = |row: usize| -((r[row] * t.x + r[3 + row] * t.y) + r[6 + row] * t.z);
        let mut m = [0.0; 12];
        m[..9].copy_from_slice(&r);
        m[9..].copy_from_slice(&[back(0), back(1), back(2)]);
        Self { m }
    }
}
