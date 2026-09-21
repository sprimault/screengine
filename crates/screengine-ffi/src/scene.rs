// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce qu'une scène soumise met dans la mémoire de l'hôte.
//!
//! Quatre structures figées, toutes alignées sur quatre octets, sans le moindre
//! octet de bourrage sur les quatre cibles : une liaison JavaScript les écrit à
//! la main dans la mémoire linéaire, décalage par décalage.
//!
//! Aucun champ réservé. L'étape des textures apportera une structure de sommet
//! nouvelle et une fonction nouvelle, ce que la règle d'extension prévoit — un
//! champ qui dort en attendant ce jour-là serait un pari sur sa forme.

use screengine::{Affine3, Camera, Color, Quat, Vec3};

use crate::entry::AbiError;

/// A vertex position in object space.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgVertex {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
    /// Z coordinate.
    pub z: f32,
}

impl ScgVertex {
    /// Le point du noyau.
    pub(crate) fn to_core(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

/// Refuse un lot dont un sommet n'est pas fini.
///
/// En amont de la soumission, et sur le tableau entier plutôt que sur les seuls
/// sommets indexés : la soumission ne rend qu'une erreur du noyau, qui n'a pas
/// de mot pour une coordonnée que l'hôte lui a donnée non finie — il la
/// traiterait comme une donnée et ferait disparaître le triangle en silence.
pub(crate) fn check_finite(vertices: &[ScgVertex]) -> Result<(), AbiError> {
    let finite = vertices
        .iter()
        .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite());
    if finite {
        Ok(())
    } else {
        Err(AbiError::VERTEX_NOT_FINITE)
    }
}

/// A triangle: three indices into the vertex array, and its colour.
///
/// Indices are not optional: a host without indexed geometry writes 0, 1, 2
/// then 3, 4, 5. The colour belongs to the triangle rather than to the batch,
/// so a whole surface crosses the boundary in one call.
///
/// Vertices are counter-clockwise as seen from the front face; a triangle given
/// the other way round is a back face and is discarded.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgTriangle {
    /// Index of the first vertex.
    pub i0: u32,
    /// Index of the second vertex.
    pub i1: u32,
    /// Index of the third vertex.
    pub i2: u32,
    /// Red, in the memory order of the output pixels.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha. Written as given, never composited.
    pub a: u8,
}

impl ScgTriangle {
    /// La couleur du noyau.
    pub(crate) fn color(self) -> Color {
        Color::new(self.r, self.g, self.b, self.a)
    }
}

/// Where the camera is, and how wide it sees.
///
/// With the identity orientation `{0, 0, 0, 1}`, the camera looks towards world
/// +X with the zenith towards the top of the screen. The quaternion is stored
/// `x, y, z, w` — the real part last — and is normalised by the engine, so it
/// need not be unit.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgCamera {
    /// Position in world space: x, y, z.
    pub position: [f32; 3],
    /// Orientation as a quaternion: x, y, z, w.
    pub orientation: [f32; 4],
    /// Vertical field of view in radians, within ]0, pi[.
    ///
    /// Rejected on the radians themselves, before any conversion: three half
    /// turns would otherwise fold into one and be accepted silently.
    pub fov_y: f32,
    /// Near plane distance, positive and finite.
    ///
    /// Not named `near`: `windows.h` still defines `near` and `far` as empty
    /// macros, inherited from 16-bit segmented memory, and a field by that name
    /// vanishes in any translation unit that includes it first.
    pub near_plane: f32,
}

impl ScgCamera {
    /// La caméra du noyau, ou une erreur sur une valeur non finie.
    ///
    /// Le champ de vision et le plan proche ne sont pas vérifiés ici : le noyau
    /// les refuse déjà, avec le message qui nomme l'argument.
    pub(crate) fn to_core(self) -> Result<Camera, AbiError> {
        let finite = self
            .position
            .iter()
            .chain(&self.orientation)
            .all(|v| v.is_finite());
        if !finite {
            return Err(AbiError::CAMERA_NOT_FINITE);
        }
        Ok(Camera {
            position: Vec3::new(self.position[0], self.position[1], self.position[2]),
            orientation: Quat::new(
                self.orientation[0],
                self.orientation[1],
                self.orientation[2],
                self.orientation[3],
            ),
            fov_y: self.fov_y,
            near: self.near_plane,
        })
    }
}

/// A 4x4 model matrix, column-major: `m[column * 4 + row]`.
///
/// The last row — `m[3]`, `m[7]`, `m[11]`, `m[15]` — must be exactly
/// `0, 0, 0, 1`. The engine composes the view itself and inverts the camera
/// pose without a division, which only holds for a rigid transform: a
/// projection or a perspective matrix passed here would be treated as one.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgMat4 {
    /// The sixteen coefficients, column by column.
    pub m: [f32; 16],
}

impl ScgMat4 {
    /// La transformation du noyau, ou une erreur sur la dernière ligne.
    pub(crate) fn to_core(self) -> Result<Affine3, AbiError> {
        let m = &self.m;
        if !m.iter().all(|v| v.is_finite())
            || [m[3], m[7], m[11]] != [0.0, 0.0, 0.0]
            || m[15] != 1.0
        {
            return Err(AbiError::MATRIX);
        }
        Ok(Affine3 {
            m: [
                m[0], m[1], m[2], //
                m[4], m[5], m[6], //
                m[8], m[9], m[10], //
                m[12], m[13], m[14],
            ],
        })
    }
}

#[cfg(test)]
mod tests;
