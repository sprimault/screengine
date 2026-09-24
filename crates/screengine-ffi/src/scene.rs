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

use screengine::{Affine3, Camera, Color, Filter, Light, Quat, Vec3, VertexUv, VertexUv2};

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
    /// Alpha. Ignored: the output is always fully opaque, as the ABI
    /// guarantees. The field only exists because a colour has four channels.
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
/// The world is right-handed with Z up. With the identity orientation
/// `{0, 0, 0, 1}`, the camera looks towards world +X, with the zenith towards
/// the top of the screen and world −Y towards the right. Naming the third axis
/// matters: "looking towards +X" alone leaves the roll undetermined, and roll
/// is what a sign mistake flips without changing where the camera points.
///
/// The quaternion is stored `x, y, z, w` — the real part last — and is
/// normalised by the engine, so it need not be unit.
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

/// A vertex carrying its texture coordinates.
///
/// Twenty bytes, offsets 0/4/8/12/16 on every target, with no padding: a
/// JavaScript binding writes them into linear memory byte by byte.
///
/// `u` and `v` are **in texels, not normalised**. That is the only choice that
/// makes them checkable against the vertex array alone: normalised, their bound
/// would depend on whichever texture the batch is finally drawn with.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgVertexUv {
    /// X coordinate, in object space.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
    /// Z coordinate.
    pub z: f32,
    /// Texture abscissa, in texels.
    pub u: f32,
    /// Texture ordinate, in texels.
    pub v: f32,
}

impl ScgVertexUv {
    /// Le sommet du noyau.
    pub(crate) fn to_core(self) -> VertexUv {
        VertexUv {
            position: Vec3::new(self.x, self.y, self.z),
            u: self.u,
            v: self.v,
        }
    }
}

/// A vertex carrying a second set of coordinates, the one a lightmap is read
/// with.
///
/// Twenty-eight bytes, offsets 0 to 24 on every target, with no padding.
///
/// **A separate type rather than two more fields on `ScgVertexUv`**, which is
/// published and never widens: a host that lights nothing keeps writing five
/// floats per vertex.
///
/// The two sets are independent, and that is the whole point: a wall repeats
/// its texture several times over and stretches its lightmap once across its
/// whole extent. Both are in texels of their own image, and both are bounded
/// the same way.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgVertexUv2 {
    /// X coordinate, in object space.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
    /// Z coordinate.
    pub z: f32,
    /// Texture abscissa, in texels.
    pub u: f32,
    /// Texture ordinate, in texels.
    pub v: f32,
    /// Lightmap abscissa, in texels of the lightmap.
    pub u2: f32,
    /// Lightmap ordinate, in texels of the lightmap.
    pub v2: f32,
}

impl ScgVertexUv2 {
    /// Le sommet du noyau.
    pub(crate) fn to_core(self) -> VertexUv2 {
        VertexUv2 {
            position: Vec3::new(self.x, self.y, self.z),
            u: self.u,
            v: self.v,
            u2: self.u2,
            v2: self.v2,
        }
    }
}

/// A point light that adds to the lighting of a surface.
///
/// Twenty bytes, offsets 0 to 19 on every target, with no padding.
///
/// **Its falloff is computed per vertex, not per pixel.** There is no distance
/// on the integer side of the pipeline, which begins at projection. A wall of
/// two triangles therefore renders a gradient between its corners rather than
/// a halo: split it into panels for a source that moves across it.
///
/// **Once a light is set, it holds for the whole scene.** A surface out of
/// range goes dark rather than keeping its colour — otherwise the boundary
/// between a surface a light reaches and one it does not would be a hard step
/// in the middle of continuous geometry.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgLight {
    /// X coordinate of its position, in world space.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
    /// Z coordinate.
    pub z: f32,
    /// Its radius, beyond which it lights nothing.
    ///
    /// Finite and strictly positive. Falloff reaches zero at the radius **with
    /// a zero derivative**, so no ring marks the edge — which a falloff linear
    /// in squared distance would draw.
    pub radius: f32,
    /// Red, in the memory order of the output pixels.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Reserved, must be zero.
    ///
    /// **Not an alpha**: a light adds, it does not blend, so a fourth channel
    /// would be a value the engine ignores. The byte exists to keep the
    /// structure free of implicit padding, and a later version may give it a
    /// meaning — which is why zero is required rather than merely advised.
    pub _reserved: u8,
}

impl ScgLight {
    /// La lumière du noyau.
    pub(crate) fn to_core(self) -> Result<Light, AbiError> {
        if self._reserved != 0 {
            return Err(AbiError::RESERVED);
        }
        Ok(Light {
            position: Vec3::new(self.x, self.y, self.z),
            radius: self.radius,
            color: Color::new(self.r, self.g, self.b, 0xFF),
        })
    }
}

/// Refuse un lot dont un sommet texturé n'est pas fini.
///
/// Même raison que pour les sommets sans texture : le noyau n'a pas de mot
/// pour une coordonnée que l'hôte lui a donnée non finie, et la traiterait
/// comme une donnée.
pub(crate) fn check_finite_uv(vertices: &[ScgVertexUv]) -> Result<(), AbiError> {
    let finite = vertices
        .iter()
        .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite());
    if finite {
        Ok(())
    } else {
        Err(AbiError::VERTEX_NOT_FINITE)
    }
}

/// La même, pour les sommets éclairés.
pub(crate) fn check_finite_uv2(vertices: &[ScgVertexUv2]) -> Result<(), AbiError> {
    let finite = vertices
        .iter()
        .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite());
    if finite {
        Ok(())
    } else {
        Err(AbiError::VERTEX_NOT_FINITE)
    }
}

/// The only pixel format a texture is loaded from.
///
/// One, never zero: a description left zeroed is refused rather than read as a
/// valid format. Four bytes per texel, in the memory order of the output
/// pixels — a host never has two orders to keep straight.
pub const SCG_TEXTURE_FORMAT_RGBA8: u32 = 1;

/// Ordered dithering of texture coordinates: the default filter.
///
/// Zero, unlike `SCG_TEXTURE_FORMAT_RGBA8`, and for the opposite reason: a
/// context that is never configured must render what the engine renders by
/// default, so the default has to be the zero value.
pub const SCG_FILTER_DITHER: u32 = 0;

/// Bilinear blending of the four neighbouring texels, within one mipmap level.
///
/// A quality level above the default, at the price of four texel reads per
/// pixel instead of one. It replaces dithering rather than adding to it: there
/// is no staircase left to hide once coordinates are interpolated.
pub const SCG_FILTER_BILINEAR: u32 = 1;

/// Le filtrage du noyau que désigne une valeur de l'ABI.
///
/// La conversion vit ici et non dans le noyau, qui porte une énumération et
/// ignore qu'elle se transporte en entier — comme le format de texture, dont le
/// noyau ne connaît pas davantage la représentation.
pub(crate) fn filter_of(value: u32) -> Result<Filter, AbiError> {
    match value {
        SCG_FILTER_DITHER => Ok(Filter::Dither),
        SCG_FILTER_BILINEAR => Ok(Filter::Bilinear),
        _ => Err(AbiError::FILTER),
    }
}

/// What a texture load is given.
///
/// All fields are `uint32_t`: four-byte alignment on every target, no padding,
/// offsets 0 to 20 alike everywhere.
///
/// **Both sides must be powers of two**, independently of each other, from 1
/// to 2048. Texture coordinates then wrap by masking, with no division and no
/// comparison per texel, which is what makes textured filling affordable in
/// software.
///
/// Zero the whole structure before filling it: the reserved fields must be
/// zero, and that is what will allow one of them to be used later without
/// breaking bindings already written.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgTextureDesc {
    /// Width in texels, a power of two between 1 and 2048.
    pub width: u32,
    /// Height in texels, a power of two between 1 and 2048.
    pub height: u32,
    /// Pixel format: `SCG_TEXTURE_FORMAT_RGBA8`.
    pub format: u32,
    /// Reserved, must be zero.
    pub reserved0: u32,
    /// Reserved, must be zero.
    pub reserved1: u32,
    /// Reserved, must be zero.
    pub reserved2: u32,
}

impl ScgTextureDesc {
    /// Refuse une description que le moteur ne sait pas lire.
    ///
    /// Les champs réservés sont vérifiés nuls ici et non plus bas : c'est la
    /// clause qui permettra d'en employer un sans casser une liaison déjà
    /// écrite, et elle ne vaut que si personne ne l'a jamais laissée passer.
    pub(crate) fn validate(&self) -> Result<(), AbiError> {
        if self.format != SCG_TEXTURE_FORMAT_RGBA8 {
            return Err(AbiError::TEXTURE_FORMAT);
        }
        if self.reserved0 != 0 || self.reserved1 != 0 || self.reserved2 != 0 {
            return Err(AbiError::RESERVED);
        }
        Ok(())
    }
}

/// Les tailles et décalages que le header publie, vérifiés **à la compilation**.
///
/// En assertions de constante et non en test : un test ne s'exécute que sur la
/// cible hôte, alors qu'une liaison JavaScript reproduit ces décalages sur
/// wasm32 et une liaison JNI sur armv7. Ici, toute cible que la compilation
/// traverse les vérifie — `make lint` passe clippy sur wasm32 et sur les trois
/// ABI Android, et un champ qui bougerait y échouerait franchement.
///
/// Les mêmes assertions existent en C, injectées dans le header par `cbindgen`
/// et compilées par les hôtes. Celles-ci les attrapent avant que le header ne
/// soit même régénéré.
/// The output curve applied while each tile is copied out: an affine per
/// channel, then gamma.
///
/// Each channel goes through `x·gain + offset`, clamped to [0, 1], then
/// `^(1/gamma)`. The affine is the most general transform that stays local to
/// one pixel and one channel: it holds contrast, black level and tint at once.
/// A gain alone can lift neither a black nor lower a white, and gamma fixes
/// both ends — that family of images is reachable only this way.
///
/// `gamma` is the display gamma, in the sense where **2.2 brightens**, within
/// ]0, 8]. Gains are within [0, 4], offsets within [-1, 1], and the affine
/// applies **before** gamma: it corrects the source, gamma encodes for the
/// display, and that is the order in which they read. Gain and gamma commute up
/// to reparametrisation — `(a·x)^γ = a^γ·x^γ` — so no image is out of reach
/// either way; what the order fixes is what the number means. **It will never
/// change**: a host that had set its parameters would otherwise get a different
/// image with no version to warn it.
///
/// The curve never writes alpha, which the engine forces opaque as the contract
/// promises.
///
/// **What the reserved fields can ever hold.** They must be zero, and the
/// extension rule promises that a host written before one of them means
/// anything gets the default behaviour by passing zeros. A setting whose
/// neutral value is one — a further gain, a saturation — therefore **cannot**
/// go there as such: only additive settings can, whose neutral is zero. Should
/// a multiplicative one be wanted later, it has to be expressed as a departure
/// from neutral to fit. Written here so that the constraint steers the design
/// instead of being discovered against it — which is why the offsets are in
/// this struct from the start rather than left for later, where three fields
/// would no longer have fitted in two reserved ones.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScgGrade {
    /// Display gamma, within ]0, 8].
    pub gamma: f32,
    /// Red gain, within [0, 4], applied before gamma.
    pub gain_r: f32,
    /// Green gain.
    pub gain_g: f32,
    /// Blue gain.
    pub gain_b: f32,
    /// Red offset, within [-1, 1], added after the gain.
    pub offset_r: f32,
    /// Green offset.
    pub offset_g: f32,
    /// Blue offset.
    pub offset_b: f32,
    /// Reserved, must be zero.
    pub reserved0: u32,
    /// Reserved, must be zero.
    pub reserved1: u32,
}

impl ScgGrade {
    /// Le gamma, les trois gains et les trois décalages, réservés vérifiés.
    pub(crate) fn to_core(self) -> Result<(f32, [f32; 3], [f32; 3]), AbiError> {
        if self.reserved0 != 0 || self.reserved1 != 0 {
            return Err(AbiError::RESERVED);
        }
        Ok((
            self.gamma,
            [self.gain_r, self.gain_g, self.gain_b],
            [self.offset_r, self.offset_g, self.offset_b],
        ))
    }
}

const _: () = {
    use core::mem::{align_of, offset_of, size_of};

    assert!(size_of::<ScgVertex>() == 12 && align_of::<ScgVertex>() == 4);
    assert!(offset_of!(ScgVertex, z) == 8);

    assert!(size_of::<ScgTriangle>() == 16 && align_of::<ScgTriangle>() == 4);
    assert!(offset_of!(ScgTriangle, i2) == 8);
    assert!(offset_of!(ScgTriangle, r) == 12);
    assert!(offset_of!(ScgTriangle, a) == 15);

    assert!(size_of::<ScgCamera>() == 36 && align_of::<ScgCamera>() == 4);
    assert!(offset_of!(ScgCamera, orientation) == 12);
    assert!(offset_of!(ScgCamera, fov_y) == 28);
    assert!(offset_of!(ScgCamera, near_plane) == 32);

    assert!(size_of::<ScgMat4>() == 64 && align_of::<ScgMat4>() == 4);

    assert!(size_of::<ScgVertexUv>() == 20 && align_of::<ScgVertexUv>() == 4);
    assert!(offset_of!(ScgVertexUv, z) == 8);
    assert!(offset_of!(ScgVertexUv, u) == 12);
    assert!(offset_of!(ScgVertexUv, v) == 16);

    assert!(size_of::<ScgTextureDesc>() == 24 && align_of::<ScgTextureDesc>() == 4);
    assert!(offset_of!(ScgTextureDesc, height) == 4);
    assert!(offset_of!(ScgTextureDesc, format) == 8);
    assert!(offset_of!(ScgTextureDesc, reserved2) == 20);

    assert!(size_of::<ScgGrade>() == 36 && align_of::<ScgGrade>() == 4);
    assert!(offset_of!(ScgGrade, gain_r) == 4);
    assert!(offset_of!(ScgGrade, gain_b) == 12);
    assert!(offset_of!(ScgGrade, offset_r) == 16);
    assert!(offset_of!(ScgGrade, offset_b) == 24);
    assert!(offset_of!(ScgGrade, reserved0) == 28);
    assert!(offset_of!(ScgGrade, reserved1) == 32);
};
