// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Frontière C de Screengine.
//!
//! Ce crate convertit des types, enveloppe chaque point d'entrée de
//! `catch_unwind` et traduit les erreurs en codes. Il ne calcule rien : un
//! calcul écrit ici serait fait autrement par la conformance, qui appelle le
//! noyau sans passer par lui.
//!
//! Le contrat est dans `docs/abi.md`. `include/screengine.h` est généré à partir
//! de ce crate seul, et la documentation des éléments exportés est en anglais
//! parce que `cbindgen` l'y recopie telle quelle.

mod context;
mod entry;
mod fpenv;
mod mesh;
mod message;
mod output;
mod scene;
mod status;
mod texture;
mod world;

use std::alloc::{self, Layout};
use std::ffi::c_char;
use std::ptr;
use std::slice;

use std::sync::Arc;

use screengine::{
    Argument, Context, Error as CoreError, Lightmap, Lightmaps, Mesh, Texture, Vec3, VertexUv,
    VertexUv2, Visibility, World,
};

use entry::AbiError;
use output::HostRows;

pub use context::{ScgContext, ScgContextConfig};
pub use mesh::ScgMesh;
pub use scene::{
    SCG_FILTER_BILINEAR, SCG_FILTER_DITHER, SCG_TEXTURE_FORMAT_RGBA8, ScgCamera, ScgGrade,
    ScgLight, ScgMat4, ScgTextureDesc, ScgTriangle, ScgVertex, ScgVertexUv, ScgVertexUv2,
};
pub use status::{
    SCG_ERR_FAULTED, SCG_ERR_INVALID_ARGUMENT, SCG_ERR_INVALID_FORMAT, SCG_ERR_INVALID_STATE,
    SCG_ERR_NULL, SCG_ERR_OUT_OF_MEMORY, SCG_ERR_PANIC, SCG_ERR_POISONED, SCG_ERR_UNKNOWN_RESOURCE,
    SCG_ERR_UNSUPPORTED_FORMAT_VERSION, SCG_LIGHTMAP_ABSENT, SCG_LIGHTMAP_READY,
    SCG_LIGHTMAP_STALE, SCG_OK, SCG_STATUS_INCOMPLETE, SCG_STATUS_NO_CELL, SCG_TRAVERSAL_CELLS,
    SCG_TRAVERSAL_DEPTH,
};
pub use texture::ScgTexture;
pub use world::{ScgLighting, ScgWorld};

/// ABI version this library implements.
///
/// Compare it for equality with the constant from the header you compiled
/// against, before any other call, and refuse a library that differs. Adding a
/// function or an error code does not change it; everything else does, so a
/// different value is a breaking change in either direction.
pub const SCG_ABI_VERSION: u32 = 1;

/// Alignment, in bytes, guaranteed by `scg_buffer_alloc`.
pub const SCG_BUFFER_ALIGNMENT: usize = 16;

/// Returns the ABI version of the loaded library.
///
/// Callable from any thread, at any time. Bindings that receive it as a signed
/// integer — JNI `jint`, JavaScript on wasm — must compare the unsigned value.
#[unsafe(no_mangle)]
pub extern "C" fn scg_abi_version() -> u32 {
    SCG_ABI_VERSION
}

/// Creates a rendering context and writes it to `out`.
///
/// Zero `*config` entirely before filling it in: its reserved fields must be
/// zero. On failure nothing is written to `out`, and the reason is available
/// from `scg_last_error(NULL)` on this same thread, read immediately.
///
/// # Safety
///
/// `config` points to a readable `ScgContextConfig`, and `out` to a writable
/// pointer. Both must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_create(
    config: *const ScgContextConfig,
    out: *mut *mut ScgContext,
) -> i32 {
    entry::without_context(|| {
        // SAFETY: précondition de la fonction — les deux pointeurs sont nuls ou
        // visent une valeur lisible, et rien d'autre ne les utilise pendant
        // l'appel.
        let config = unsafe { config.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: même précondition.
        let out = unsafe { out.as_mut() }.ok_or(AbiError::NULL)?;

        let context = Context::new(config.to_core()?)?;
        *out = Box::into_raw(Box::new(ScgContext::new(context)));
        Ok(())
    })
}

/// Releases a context.
///
/// Passing NULL does nothing, like `free`. A handle destroyed twice, or used
/// after destruction, is not detected: that is a precondition, not an error
/// case. Destroying a faulted context is allowed.
///
/// # Safety
///
/// `ctx` is NULL, or a handle returned by `scg_create` and not yet destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_destroy(ctx: *mut ScgContext) {
    // L'enveloppe englobe le test de nullité : elle vide l'emplacement par
    // thread en entrant, et un `scg_destroy(NULL)` qui ressortirait avant
    // laisserait le message d'une tâche précédente sur un thread de pool.
    entry::nothing(|| {
        if ctx.is_null() {
            return;
        }
        // SAFETY: précondition de la fonction — `ctx` vient de `Box::into_raw`
        // dans `scg_create` et n'a pas encore été rendu. C'est le seul endroit
        // du crate qui reprend cette allocation.
        drop(unsafe { Box::from_raw(ctx) });
    });
}

/// Sets the camera the next frames will render from.
///
/// Rejected with `SCG_ERR_INVALID_STATE` between `scg_frame_begin` and
/// `scg_frame_end`, and **as soon as a triangle of the current frame is kept**:
/// the camera holds for a whole frame, and every submission projects
/// immediately, so a camera changed midway would leave two screen spaces in the
/// same image, each one correct and the whole wrong. Set it before submitting,
/// or after `scg_frame_end`.
///
/// The field of view must be within ]0, pi[ radians and the near plane
/// positive, otherwise `SCG_ERR_INVALID_ARGUMENT`. The quaternion is normalised
/// by the engine.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call, and `camera`
/// is NULL or points to a readable `ScgCamera`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_set_camera(ctx: *mut ScgContext, camera: *const ScgCamera) -> i32 {
    let set = |mut core: entry::Core<'_>| {
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise une
        // structure lisible, que rien d'autre ne modifie pendant l'appel.
        let camera = unsafe { camera.as_ref() }.ok_or(AbiError::NULL)?;
        core.exclusive()?.set_camera(camera.to_core()?)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, set) }
}

/// Sets the internal resolution, from the next frames on.
///
/// Both sides must be at least 1 and within the `max_width` and `max_height`
/// given to `scg_create`, otherwise `SCG_ERR_INVALID_ARGUMENT` and the context
/// keeps the resolution it had. Nothing is allocated: every buffer a frame
/// needs was sized for the maximum, which is why that maximum is fixed once
/// and cannot be raised.
///
/// Rejected with `SCG_ERR_INVALID_STATE` between `scg_frame_begin` and
/// `scg_frame_end`, and as soon as a triangle of the current frame is kept —
/// the same rule as `scg_set_camera`, and for the same reason: projection
/// happens at submission time.
///
/// **The output buffer does not follow on its own.** After a raise, a buffer
/// left at its former size is too short, and the engine cannot detect it — it
/// never receives the length. Resize it, and pass the new `stride`. The tile
/// count changes too: call `scg_frame_begin` again rather than reusing the
/// count from the previous frame.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_set_resolution(ctx: *mut ScgContext, width: u32, height: u32) -> i32 {
    let set = |mut core: entry::Core<'_>| {
        core.exclusive()?.set_resolution(width, height)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, set) }
}

/// Sets the output curve applied while each tile is copied out.
///
/// See `ScgGrade` for what the fields mean and what the reserved ones may ever
/// hold. Out-of-range values, or a reserved field that is not zero, return
/// `SCG_ERR_INVALID_ARGUMENT` and the context keeps the curve it had.
///
/// Neutral by default, and turned back to neutral by `scg_clear_grade`. It
/// costs three table lookups per pixel once set, and nothing at all while it is
/// neutral. Rejected with `SCG_ERR_INVALID_STATE` between `scg_frame_begin` and
/// `scg_frame_end`, for the reason that holds for every frame-wide setting.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call, and `grade`
/// is NULL or points to a readable `ScgGrade`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_set_grade(ctx: *mut ScgContext, grade: *const ScgGrade) -> i32 {
    let set = |mut core: entry::Core<'_>| {
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise une
        // structure lisible, que rien d'autre ne modifie pendant l'appel.
        let grade = unsafe { grade.as_ref() }.ok_or(AbiError::NULL)?;
        let (gamma, gains, offsets) = grade.to_core()?;
        core.exclusive()?.set_grade(gamma, gains, offsets)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, set) }
}

/// Returns the output to its neutral state.
///
/// Doing so on a context that never set a curve is not an error: a host that
/// flattens its output at the end of a level does not have to remember whether
/// it had set one.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_clear_grade(ctx: *mut ScgContext) -> i32 {
    let clear = |mut core: entry::Core<'_>| {
        core.exclusive()?.clear_grade()?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, clear) }
}

/// Sets how textures are sampled from the next frames on.
///
/// `filter` is `SCG_FILTER_DITHER`, the default, or `SCG_FILTER_BILINEAR`; any
/// other value is `SCG_ERR_INVALID_ARGUMENT`, and the context keeps the filter
/// it had. A binding written against a later version therefore gets a plain
/// error rather than an image filtered otherwise than it believes.
///
/// Rejected with `SCG_ERR_INVALID_STATE` between `scg_frame_begin` and
/// `scg_frame_end`: tiles of one frame are rendered from threads the engine
/// knows nothing about, and a filter changed in between would leave part of the
/// image sampled one way and part the other.
///
/// The two filters are exclusive: bilinear replaces dithering, it does not add
/// to it.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_set_filter(ctx: *mut ScgContext, filter: u32) -> i32 {
    let set = |mut core: entry::Core<'_>| {
        core.exclusive()?.set_filter(scene::filter_of(filter)?)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, set) }
}

/// Sets how much lit surfaces are brightened, from the next frames on.
///
/// `shift` is `0`, `1` or `2`; anything else is `SCG_ERR_INVALID_ARGUMENT`,
/// and the context keeps the value it had.
///
/// **Zero is the default, and it is the faithful setting**: under full light a
/// texel comes out untouched, and never brighter. It is also a dull scene —
/// a real lightmap reaches white nowhere, so every surface ends up darker than
/// its texture. One or two double or quadruple the combined value, saturating
/// where light is strong, and that is what gives a lit scene its range.
///
/// Rejected with `SCG_ERR_INVALID_STATE` between `scg_frame_begin` and
/// `scg_frame_end`, for the same reason as `scg_set_filter`: a frame whose
/// tiles did not all share one setting is described by nothing.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_set_overbright(ctx: *mut ScgContext, shift: u32) -> i32 {
    let set = |mut core: entry::Core<'_>| {
        core.exclusive()?.set_overbright(shift)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, set) }
}

/// Turns on distance fog, from the next frames on.
///
/// `start` and `end` are view distances in world units, counted from the
/// camera: `start` must be finite and not negative, `end` finite and strictly
/// beyond `start`. Anything else is `SCG_ERR_INVALID_ARGUMENT`, and the
/// context keeps the fog it had.
///
/// **The image's background takes the fog colour on its own**, without the
/// host clearing with it: a pixel no triangle painted is infinitely distant,
/// and fog is applied where depth is already at hand. That is what makes
/// distant geometry meet the horizon with no dividing line — clearing
/// separately would draw back the very seam this avoids.
///
/// **Three channels, not four.** The output buffer is opaque by contract, so
/// an alpha on the fog colour would be a value the engine ignores, and a host
/// would rightly wonder what it does.
///
/// Off by default, and turned off again by `scg_clear_fog`. Rejected with
/// `SCG_ERR_INVALID_STATE` between `scg_frame_begin` and `scg_frame_end`, for
/// the reason that holds for every frame setting.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_set_fog(
    ctx: *mut ScgContext,
    r: u8,
    g: u8,
    b: u8,
    start: f32,
    end: f32,
) -> i32 {
    let set = |mut core: entry::Core<'_>| {
        let color = screengine::Color::new(r, g, b, 0xFF);
        core.exclusive()?.set_fog(color, start, end)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, set) }
}

/// Turns distance fog off, from the next frames on.
///
/// Calling it on a context that has no fog is not an error: a host that turns
/// fog off at the end of a level does not have to remember whether it turned
/// it on.
///
/// Rejected with `SCG_ERR_INVALID_STATE` during a frame, like every other
/// frame setting.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_clear_fog(ctx: *mut ScgContext) -> i32 {
    let clear = |mut core: entry::Core<'_>| {
        core.exclusive()?.clear_fog()?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, clear) }
}

/// Replaces the dynamic lights of the frames to come.
///
/// At most eight; beyond that the whole call is rejected rather than
/// truncated, since a half-lit scene looks exactly like one whose radii are
/// wrong. A count of zero turns lighting off.
///
/// **Falloff is computed per vertex, when a batch is submitted**: lights set
/// after a batch do not reach it. That is what lets a torch carried by the
/// player light a set without the set being submitted again — provided it is
/// set first.
///
/// **Once a light is set, it holds for the whole scene.** A surface out of
/// range goes dark rather than keeping its colour: the boundary between a
/// surface a light reaches and one it does not would otherwise be a hard step
/// in the middle of continuous geometry.
///
/// A light whose position is not finite, or whose radius is not finite and
/// strictly positive, is rejected: it would light nothing and divide by zero.
///
/// Rejected with `SCG_ERR_INVALID_STATE` during a frame, like every other
/// frame setting.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call, and
/// `lights` points to `count` readable `ScgLight`. A count of zero allows a
/// null pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_set_lights(
    ctx: *mut ScgContext,
    lights: *const ScgLight,
    count: u32,
) -> i32 {
    let set = |mut core: entry::Core<'_>| {
        // SAFETY: précondition de la fonction — le pointeur couvre son nombre
        // d'éléments, et un nombre nul autorise un pointeur nul.
        let lights = unsafe { slice_of(lights, count) };
        let mut core_lights = [screengine::Light {
            position: Vec3::ZERO,
            radius: 1.0,
            color: screengine::Color::new(0, 0, 0, 0xFF),
        }; screengine::MAX_LIGHTS];
        if lights.len() > core_lights.len() {
            return Err(AbiError::from(CoreError::InvalidArgument(
                Argument::LightCapacity,
            )));
        }
        for (slot, light) in core_lights.iter_mut().zip(lights) {
            *slot = light.to_core()?;
        }
        core.exclusive()?.set_lights(&core_lights[..lights.len()])?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, set) }
}

/// Submits a batch of triangles to the frame being recorded.
///
/// `model` carries the object into world space; the engine composes the view
/// from its own camera. Passing a model-view matrix here would apply the view
/// twice.
///
/// Each triangle indexes three vertices of `vertices` and carries its own
/// colour. **The batch is accepted or rejected as a whole**: an index at or
/// beyond `vertex_count`, a coefficient that is not finite, or a batch that
/// would exceed the triangle capacity leaves the frame exactly as it was. A
/// triangle whose vertices cannot be projected — beyond the guard band, behind
/// the near plane — simply does not appear, which is not an error.
///
/// A count of zero is accepted and submits nothing. Rejected with
/// `SCG_ERR_INVALID_STATE` between `scg_frame_begin` and `scg_frame_end`.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call; `vertices`
/// points to `vertex_count` readable `ScgVertex`, and `triangles` to
/// `triangle_count` readable `ScgTriangle`. A count of zero allows a null
/// pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    vertices: *const ScgVertex,
    vertex_count: u32,
    triangles: *const ScgTriangle,
    triangle_count: u32,
) -> i32 {
    let submit = |mut core: entry::Core<'_>| {
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise une
        // matrice lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — chaque pointeur couvre son
        // nombre d'éléments, et un compte nul admet le pointeur nul, pour
        // lequel `from_raw_parts` exigerait quand même un pointeur aligné.
        let (vertices, triangles) = unsafe {
            (
                slice_of(vertices, vertex_count),
                slice_of(triangles, triangle_count),
            )
        };
        scene::check_finite(vertices)?;
        core.exclusive()?
            .submit_each(model, triangles.len(), |i| {
                let triangle = triangles[i];
                let mut corners = [Vec3::ZERO; 3];
                for (corner, index) in
                    corners
                        .iter_mut()
                        .zip([triangle.i0, triangle.i1, triangle.i2])
                {
                    *corner = vertices
                        .get(index as usize)
                        .ok_or(CoreError::InvalidArgument(Argument::VertexIndex))?
                        .to_core();
                }
                Ok((corners, triangle.color()))
            })
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, submit) }
}

/// La tranche d'un tableau reçu de l'hôte, vide pour un compte nul.
///
/// # Safety
///
/// `ptr` couvre `count` éléments lisibles, ou `count` est nul.
unsafe fn slice_of<'a, T>(ptr: *const T, count: u32) -> &'a [T] {
    if count == 0 {
        return &[];
    }
    // SAFETY: précondition de la fonction. Le compte nul est traité avant, ce
    // qui évite d'exiger de l'hôte un pointeur aligné pour un tableau vide.
    unsafe { std::slice::from_raw_parts(ptr, count as usize) }
}

/// La tranche d'octets d'un bloc reçu de l'hôte.
///
/// Séparée de [`slice_of`] parce que la longueur d'un bloc de pixels est une
/// `size_t` et non un compte d'éléments : c'est la seule longueur de l'ABI qui
/// change de largeur selon la cible.
///
/// # Safety
///
/// `ptr` couvre `len` octets lisibles, ou `len` est nul.
unsafe fn slice_of_bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        return &[];
    }
    // SAFETY: précondition de la fonction, le cas vide étant traité avant.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

/// Begins a frame: seals the submitted scene, bins it into tiles, and writes
/// the tile count to `tile_count`.
///
/// Tiles are numbered row by row, left to right then top to bottom; those of
/// the last column and row are partial when the internal resolution is not a
/// multiple of the tile size. A frame already begun returns
/// `SCG_ERR_INVALID_STATE`.
///
/// Rendering the tiles is optional: `scg_frame_end` renders every tile nobody
/// rendered.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call, and
/// `tile_count` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_frame_begin(ctx: *mut ScgContext, tile_count: *mut u32) -> i32 {
    let begin = |mut core: entry::Core<'_>| {
        if tile_count.is_null() {
            return Err(AbiError::NULL);
        }
        let count = core.exclusive()?.begin()?;
        // SAFETY: précondition de la fonction — `tile_count` est accessible en
        // écriture, et vient d'être vérifié non nul.
        unsafe { tile_count.write(count) };
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, begin) }
}

/// Renders tile `index` of the frame begun by `scg_frame_begin` into the host
/// buffer.
///
/// Callable from several threads at once on the same context, for distinct
/// indices, and only between `scg_frame_begin` and `scg_frame_end`; no other
/// call on the context is allowed meanwhile. A tile renders once per frame: an
/// index already taken, even by a concurrent call, returns
/// `SCG_ERR_INVALID_STATE`, as does a tile outside a begun frame. An index at or
/// beyond the tile count returns `SCG_ERR_INVALID_ARGUMENT`.
///
/// The error message of this call goes to the per-thread slot: read it with
/// `scg_last_error(NULL)` on the calling thread, immediately after the call.
/// A panic faults the context; the other tiles already running complete, and
/// `scg_frame_end` reports it.
///
/// Each tile keeps its colour and depth on the stack of the call: the calling
/// thread needs at least 128 KiB of stack.
///
/// `pixels` and `stride` are those of the whole image, the same for every tile
/// and for `scg_frame_end` of the frame: the tile writes only its own rectangle.
///
/// # Safety
///
/// `ctx` is a live handle, and `pixels` points to a writable buffer of at least
/// `stride × height × 4` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_frame_tile(
    ctx: *mut ScgContext,
    index: u32,
    pixels: *mut u8,
    stride: u32,
) -> i32 {
    let render = |core: &Context| {
        if pixels.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le tampon couvre l'image, et
        // chaque tuile n'en écrit que son rectangle, disjoint des autres.
        let mut out = unsafe { HostRows::new(pixels, stride) };
        core.tile(index, &mut out)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_tile(ctx, render) }
}

/// Ends the frame and writes the result into the host buffer.
///
/// It renders every tile nobody rendered, then closes the frame. Without
/// `scg_frame_begin`, it begins the frame itself: a host that only ever calls
/// this function always receives the whole image. While a tile is still being
/// rendered on another thread, it returns `SCG_ERR_INVALID_STATE` and leaves
/// the frame open. Its return code is what tells whether the image is good.
///
/// `stride` is in pixels and must be at least the current internal width. The
/// buffer holds at least `stride × height` pixels of four bytes each, in R, G,
/// B, A order with alpha always written. The engine never receives the buffer
/// length and cannot check it: that is a documented precondition.
///
/// # Safety
///
/// `ctx` is a live handle, and `pixels` points to a writable buffer of at least
/// `stride × height × 4` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_frame_end(ctx: *mut ScgContext, pixels: *mut u8, stride: u32) -> i32 {
    let render = |mut core: entry::Core<'_>| {
        if pixels.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le tampon couvre l'image. Une
        // tuile qui tournerait encore fait refuser la fin avant toute écriture.
        let mut out = unsafe { HostRows::new(pixels, stride) };
        if !core.shared().is_rendering() {
            // La sortie se vérifie avant d'ouvrir l'image : sans cela, un
            // `stride` refusé laisserait le contexte en rendu, et l'hôte qui
            // s'est seulement trompé de tampon verrait tous ses appels
            // suivants rendre `SCG_ERR_INVALID_STATE`.
            core.shared().check_output(&out)?;
            core.exclusive()?.begin()?;
        }
        core.shared().end(&mut out)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, render) }
}

/// Returns the last error message, as a NUL-terminated UTF-8 string.
///
/// The pointer is valid until the next call on the same context: copy the
/// message immediately if you need to keep it. It is never NULL — with no error
/// since the last call, the message is the empty string. The string belongs to
/// the engine; never free it.
///
/// Pass NULL to read the message of calls that have no context to attach it to,
/// such as a failed `scg_create`. That slot is per thread, not per call: read it
/// on the thread that made the failing call, immediately after it. A coroutine
/// that resumes on another thread of a pool will find it empty.
///
/// Allowed on a faulted context, so the host can learn the cause.
///
/// # Safety
///
/// `ctx` is NULL, or a handle returned by `scg_create` and not yet destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_last_error(ctx: *const ScgContext) -> *const c_char {
    // SAFETY: précondition de la fonction — `ctx` est nul ou valide. Cette
    // fonction ne passe pas par l'enveloppe : elle ne peut pas paniquer, et
    // elle doit rester permise sur un objet défaillant.
    match unsafe { ctx.as_ref() } {
        Some(ctx) => ctx.message().as_ptr(),
        None => message::orphan_ptr(),
    }
}

/// Allocates a buffer the host owns until `scg_buffer_free`.
///
/// Returns NULL on failure, and for a length of zero. The block is aligned to
/// `SCG_BUFFER_ALIGNMENT` bytes.
///
/// On wasm this is the only way to obtain a buffer the engine can write to: the
/// host cannot hand over an arbitrary pointer, since only the module's linear
/// memory is addressable. In JavaScript the returned pointer arrives signed —
/// test it with `ptr === 0`, never `ptr > 0`, and convert with `ptr >>> 0`
/// before building a view.
///
/// Callable from any thread.
#[unsafe(no_mangle)]
pub extern "C" fn scg_buffer_alloc(len: usize) -> *mut u8 {
    entry::producing(ptr::null_mut(), || {
        let Ok(layout) = Layout::from_size_align(len, SCG_BUFFER_ALIGNMENT) else {
            return ptr::null_mut();
        };
        if layout.size() == 0 {
            return ptr::null_mut();
        }
        // SAFETY: la taille est non nulle, ce que `alloc` exige. Un échec rend
        // le pointeur nul, que cette fonction transmet tel quel.
        unsafe { alloc::alloc(layout) }
    })
}

/// Releases a buffer obtained from `scg_buffer_alloc`.
///
/// `len` must be exactly the length passed to the allocation: the allocator
/// rebuilds the block description from it, and a different value is undefined
/// behaviour. Passing NULL does nothing. These blocks are freed through this
/// function and no other — the engine's allocator is not the host's, on desktop
/// either.
///
/// Callable from any thread.
///
/// # Safety
///
/// `ptr` is NULL, or a pointer returned by `scg_buffer_alloc` and not yet
/// freed, with the same `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_buffer_free(ptr: *mut u8, len: usize) {
    entry::nothing(|| {
        if ptr.is_null() {
            return;
        }
        let Ok(layout) = Layout::from_size_align(len, SCG_BUFFER_ALIGNMENT) else {
            return;
        };
        if layout.size() == 0 {
            return;
        }
        // SAFETY: précondition de la fonction — `ptr` vient de
        // `scg_buffer_alloc` avec cette même longueur, et l'alignement est une
        // constante de l'ABI, ce qui reconstruit à l'identique la description
        // de l'allocation.
        unsafe { alloc::dealloc(ptr, layout) }
    });
}

/// Loads a texture from a block of pixels and writes its handle to `out`.
///
/// `pixels` holds `width * height` texels, rows contiguous, four bytes each in
/// R, G, B, A order — the memory order of the output pixels. The block is
/// copied: the host may free it as soon as this call returns.
///
/// **Both sides must be powers of two**, independently, from 1 to 2048. The
/// whole mipmap chain is built here, down to 1x1, by averaging texels; nothing
/// is ever generated later, which is what keeps a frame free of allocation.
///
/// **Takes no context.** A texture belongs to none, and the same one may be
/// submitted to several from several threads. On failure the message therefore
/// goes to the per-thread slot: read it with `scg_last_error(NULL)`, on the
/// calling thread, before any other call on that thread.
///
/// # Safety
///
/// `desc` must point to a readable description, zeroed before being filled in.
/// `pixels` must cover exactly `width * height * 4` readable bytes, and `out` a
/// writable handle. Nothing is written to `out` on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_texture_load(
    desc: *const ScgTextureDesc,
    pixels: *const u8,
    len: usize,
    out: *mut *mut ScgTexture,
) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise une
        // description lisible.
        let desc = unsafe { desc.as_ref() }.ok_or(AbiError::NULL)?;
        desc.validate()?;
        if pixels.is_null() && len != 0 {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — `pixels` couvre `len` octets
        // lisibles. Un `len` nul admet le pointeur nul, que `from_raw_parts`
        // exigerait quand même aligné.
        let pixels = unsafe { slice_of_bytes(pixels, len) };
        let texture = Texture::load(desc.width, desc.height, pixels)?;
        let handle = Box::into_raw(Box::new(ScgTexture {
            inner: Arc::new(texture),
        }));
        // SAFETY: précondition de la fonction — `out` vise un handle
        // inscriptible, et rien n'y a été écrit avant ce point.
        unsafe { out.write(handle) };
        Ok(())
    })
}

/// Releases a texture.
///
/// `scg_texture_destroy(NULL)` does nothing, like `free(NULL)`. Destroying a
/// texture a frame still references is harmless: the engine holds its own
/// reference until that frame ends.
///
/// # Safety
///
/// `texture` must be null, or a handle returned by `scg_texture_load` and not
/// yet destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_texture_destroy(texture: *mut ScgTexture) {
    entry::nothing(|| {
        if texture.is_null() {
            return;
        }
        // SAFETY: précondition de la fonction — le handle vient de
        // `Box::into_raw` dans `scg_texture_load` et n'a pas encore été rendu.
        drop(unsafe { Box::from_raw(texture) });
    });
}

/// Submits a batch of triangles dressed with a texture.
///
/// Same contract as `scg_submit`, with two differences: the vertices carry
/// their texture coordinates, in texels, and the texture applies to the whole
/// batch. Coordinates beyond 16384 texels, or not finite, reject the batch.
///
/// Each triangle's colour is **ignored** on this path: the texel decides, and
/// the colour does not tint it. Fill `r`, `g`, `b` and `a` with anything.
///
/// # Safety
///
/// Same preconditions as `scg_submit`, and `texture` must be a live handle from
/// `scg_texture_load`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_textured(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    vertices: *const ScgVertexUv,
    vertex_count: u32,
    triangles: *const ScgTriangle,
    triangle_count: u32,
    texture: *const ScgTexture,
) -> i32 {
    let submit = |mut core: entry::Core<'_>| {
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `texture` est un handle vivant.
        let texture = unsafe { texture.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — chaque pointeur couvre son
        // nombre d'éléments.
        let (vertices, triangles) = unsafe {
            (
                slice_of(vertices, vertex_count),
                slice_of(triangles, triangle_count),
            )
        };
        scene::check_finite_uv(vertices)?;
        core.exclusive()?
            .submit_each_uv(model, triangles.len(), Some(&texture.inner), |i| {
                let triangle = triangles[i];
                let mut corners = [VertexUv::untextured(Vec3::ZERO); 3];
                for (corner, index) in
                    corners
                        .iter_mut()
                        .zip([triangle.i0, triangle.i1, triangle.i2])
                {
                    *corner = vertices
                        .get(index as usize)
                        .ok_or(CoreError::InvalidArgument(Argument::VertexIndex))?
                        .to_core();
                }
                Ok((corners, triangle.color()))
            })
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, submit) }
}

/// Submits a batch of triangles lit by a lightmap.
///
/// Same contract as `scg_submit_textured`, with two differences: the vertices
/// carry a second set of coordinates, and the batch carries a lightmap on top
/// of its texture. Both apply to the whole batch.
///
/// **`texture` may be null, and `lightmap` may not.** That asymmetry is
/// deliberate and is the one thing to get right here: a plain wall lit by a
/// lightmap is the commonest surface of a set, and it renders `colour x
/// lightmap`, each triangle's own colour standing in for the texel. A null
/// `lightmap`, on the other hand, is `SCG_ERR_NULL` — a batch without one has
/// no business on this path, and `scg_submit` or `scg_submit_textured` renders
/// it.
///
/// Where `texture` is given, each triangle's colour is ignored exactly as on
/// `scg_submit_textured`.
///
/// The lightmap is read bilinearly whatever `scg_set_filter` says, and through
/// its own mipmap chain. The engine keeps its own strong reference to both
/// images until the end of the frame: a host may destroy them on return.
///
/// # Safety
///
/// Same preconditions as `scg_submit`, `vertices` pointing to `vertex_count`
/// readable `ScgVertexUv2`; `lightmap` must be a live handle from
/// `scg_texture_load`, and `texture` must be null or such a handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_lit(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    vertices: *const ScgVertexUv2,
    vertex_count: u32,
    triangles: *const ScgTriangle,
    triangle_count: u32,
    texture: *const ScgTexture,
    lightmap: *const ScgTexture,
) -> i32 {
    let submit = |mut core: entry::Core<'_>| {
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `lightmap` est un handle
        // vivant, `texture` est nul ou un handle vivant.
        let (texture, lightmap) = unsafe { (texture.as_ref(), lightmap.as_ref()) };
        let lightmap = lightmap.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — chaque pointeur couvre son
        // nombre d'éléments.
        let (vertices, triangles) = unsafe {
            (
                slice_of(vertices, vertex_count),
                slice_of(triangles, triangle_count),
            )
        };
        scene::check_finite_uv2(vertices)?;
        core.exclusive()?
            .submit_each_lit(
                model,
                triangles.len(),
                texture.map(|t| &t.inner),
                &lightmap.inner,
                |i| {
                    let triangle = triangles[i];
                    let mut corners = [VertexUv2::unlit(VertexUv::untextured(Vec3::ZERO)); 3];
                    for (corner, index) in
                        corners
                            .iter_mut()
                            .zip([triangle.i0, triangle.i1, triangle.i2])
                    {
                        *corner = vertices
                            .get(index as usize)
                            .ok_or(CoreError::InvalidArgument(Argument::VertexIndex))?
                            .to_core();
                    }
                    Ok((corners, triangle.color()))
                },
            )
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, submit) }
}

/// Loads a mesh from a block of bytes.
///
/// The engine copies what it keeps: the host may free `bytes` as soon as the
/// call returns, and no lifetime crosses the boundary. `len` must be exactly the
/// length the file declares in its header — a longer or shorter block is
/// rejected, tail bytes included.
///
/// The mesh belongs to no context, so the failure is read with
/// `scg_last_error(NULL)`. Two codes say different things to the host:
/// `SCG_ERR_INVALID_FORMAT` means the content is bad, to report as a bad asset;
/// `SCG_ERR_UNSUPPORTED_FORMAT_VERSION` means this library cannot read that
/// version, so take a newer one or export the data again.
///
/// An empty mesh is a valid file, not an error.
///
/// # Safety
///
/// `bytes` must cover `len` readable bytes, or `len` must be zero. `out` must
/// point to a writable handle; nothing is written unless the call succeeds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_mesh_load(
    bytes: *const u8,
    len: usize,
    out: *mut *mut ScgMesh,
) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        if bytes.is_null() && len != 0 {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — `bytes` couvre `len` octets
        // lisibles. Un `len` nul admet le pointeur nul, que `from_raw_parts`
        // exigerait quand même aligné.
        let bytes = unsafe { slice_of_bytes(bytes, len) };
        let mesh = Mesh::load(bytes)?;
        let handle = Box::into_raw(Box::new(ScgMesh { inner: mesh }));
        // SAFETY: précondition de la fonction — `out` vise un handle
        // inscriptible, et rien n'y a été écrit avant ce point.
        unsafe { out.write(handle) };
        Ok(())
    })
}

/// Releases a mesh.
///
/// `scg_mesh_destroy(NULL)` does nothing, like `free(NULL)`. Destroying a mesh
/// during a frame is harmless, but **not for the same reason as a texture**:
/// nothing reads a mesh once the submission has returned, whereas the engine
/// holds a reference to a texture until the frame ends. Do not assume the two
/// follow one rule.
///
/// # Safety
///
/// `mesh` must be null, or a handle returned by `scg_mesh_load` and not yet
/// destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_mesh_destroy(mesh: *mut ScgMesh) {
    entry::nothing(|| {
        if mesh.is_null() {
            return;
        }
        // SAFETY: précondition de la fonction — le handle vient de
        // `Box::into_raw` dans `scg_mesh_load` et n'a pas encore été rendu.
        drop(unsafe { Box::from_raw(mesh) });
    });
}

/// Writes the number of texture slots the mesh asks for to `out`.
///
/// Slots are numbered from zero. Read each name with `scg_mesh_texture_name`,
/// load what you want with your own files, and pass the handles in slot order.
///
/// # Safety
///
/// `mesh` must be a live handle from `scg_mesh_load`, and `out` must point to a
/// writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_mesh_texture_count(mesh: *const ScgMesh, out: *mut u32) -> i32 {
    // SAFETY: précondition de la fonction — `mesh` est nul ou un handle vivant,
    // `out` est nul ou inscriptible.
    unsafe { mesh_count(mesh, out, Mesh::texture_count) }
}

/// Submits a mesh, one batch per surface group, with a texture per slot.
///
/// `textures` holds `texture_count` handles in slot order, and `texture_count`
/// must **equal** `scg_mesh_texture_count`, not merely reach it. A null entry
/// means "no texture" for that slot, and the triangle colours from the file
/// decide instead. The array is read in place and never copied, so nothing is
/// allocated during the call.
///
/// `model` places the mesh in the world; the engine composes its camera itself.
///
/// **The mesh is submitted whole or not at all.** If it does not fit in the
/// remaining triangle capacity, the call returns `SCG_ERR_INVALID_ARGUMENT` and
/// leaves nothing behind — not even the groups it had already placed. Size the
/// capacity with `scg_mesh_triangle_count` before creating the context.
///
/// Nothing in the mesh is validated again here: indices and group bounds were
/// checked once, when it was loaded.
///
/// # Safety
///
/// `ctx` must be null or a live handle. `model` must point to a readable matrix,
/// `mesh` must be a live handle from `scg_mesh_load`, and `textures` must be null
/// with `texture_count` zero, or cover `texture_count` readable pointers, each
/// null or a live handle from `scg_texture_load`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_mesh(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    mesh: *const ScgMesh,
    textures: *const *const ScgTexture,
    texture_count: u32,
) -> i32 {
    let submit = |mut core: entry::Core<'_>| {
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `mesh` est un handle vivant.
        let mesh = unsafe { mesh.as_ref() }.ok_or(AbiError::NULL)?;
        if textures.is_null() && texture_count != 0 {
            return Err(AbiError::NULL);
        }
        if texture_count != mesh.inner.texture_count() {
            return Err(AbiError::TEXTURE_COUNT);
        }
        // SAFETY: précondition de la fonction — le tableau couvre son nombre
        // d'éléments, le cas vide étant traité par `slice_of`.
        let slots = unsafe { slice_of(textures, texture_count) };

        core.exclusive()?
            .submit_mesh(model, &mesh.inner, |slot| {
                // SAFETY: précondition de la fonction — chaque entrée du tableau
                // est nulle ou un handle vivant. Le compte ayant été vérifié
                // égal, `get` ne rend jamais `None` ici.
                slots
                    .get(slot as usize)
                    .and_then(|handle| unsafe { handle.as_ref() })
                    .map(|texture| &texture.inner)
            })
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, submit) }
}

/// Reads the name of one texture slot, in two steps.
///
/// Call it once with `buf` null and `cap` zero: it writes the length of the name
/// to `out_len`, not counting the terminator. Call it again with a buffer of at
/// least `*out_len + 1` bytes: it writes the name and a null terminator.
///
/// A `cap` too small for the name and its terminator returns
/// `SCG_ERR_INVALID_ARGUMENT` and **writes nothing**, `out_len` included — the
/// measuring call is how you learn the length. A `slot` beyond
/// `scg_mesh_texture_count` returns `SCG_ERR_INVALID_ARGUMENT` too: the file is
/// fine, the index is not. `out_len` is required in both steps.
///
/// The name is what the file calls the slot, never a path: the engine opens
/// nothing, and the host decides what it loads for that slot.
///
/// # Safety
///
/// `mesh` must be a live handle from `scg_mesh_load`. `buf` must be null with
/// `cap` zero, or cover `cap` writable bytes. `out_len` must point to a writable
/// `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_mesh_texture_name(
    mesh: *const ScgMesh,
    slot: u32,
    buf: *mut c_char,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    entry::without_context(|| {
        if out_len.is_null() || (buf.is_null() && cap != 0) {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let mesh = unsafe { mesh.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: mêmes préconditions que celles de `write_name`, que cet appel
        // porte telles quelles.
        unsafe { write_name(mesh.inner.texture_name(slot), buf, cap, out_len) }
    })
}

/// Writes the number of triangles the mesh carries to `out`.
///
/// It is what a host needs to size `max_triangles` before creating the context
/// it will submit to: without it, the only way to know is to try.
///
/// # Safety
///
/// `mesh` must be a live handle from `scg_mesh_load`, and `out` must point to a
/// writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_mesh_triangle_count(mesh: *const ScgMesh, out: *mut u32) -> i32 {
    // SAFETY: mêmes préconditions que ci-dessus.
    unsafe { mesh_count(mesh, out, Mesh::triangle_count) }
}

/// Le corps commun des deux accesseurs scalaires d'un maillage.
///
/// # Safety
///
/// `mesh` est nul ou un handle vivant, `out` est nul ou vise un `u32`
/// inscriptible.
unsafe fn mesh_count(mesh: *const ScgMesh, out: *mut u32, count: impl Fn(&Mesh) -> u32) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let mesh = unsafe { mesh.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — `out` vise un `u32`
        // inscriptible.
        unsafe { out.write(count(&mesh.inner)) };
        Ok(())
    })
}

/// Le corps commun des deux accesseurs scalaires d'une carte.
///
/// # Safety
///
/// `world` est nul ou un handle vivant, `out` est nul ou vise un `u32`
/// inscriptible.
unsafe fn world_count(world: *const ScgWorld, out: *mut u32, count: impl Fn(&World) -> u32) -> i32 {
    // SAFETY: mêmes préconditions, transmises telles quelles.
    unsafe { world_value(world, out, |world| Ok(count(&world.inner))) }
}

/// La lecture d'un entier de la carte qui peut refuser, partagée par les
/// accesseurs indexés et par les interrogations géométriques.
///
/// `world_count` en est le cas dégénéré, celui d'une lecture qui ne refuse jamais :
/// elle passe par ici plutôt qu'à côté, faute de quoi la vérification du pointeur
/// de sortie et celle du handle existeraient à deux endroits.
///
/// # Safety
///
/// `world` est nul ou un handle vivant, et `out` vise un `u32` inscriptible.
unsafe fn world_value(
    world: *const ScgWorld,
    out: *mut u32,
    read: impl FnOnce(&ScgWorld) -> Result<u32, AbiError>,
) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        let value = read(world)?;
        // SAFETY: précondition de la fonction — `out` vise un `u32`
        // inscriptible.
        unsafe { out.write(value) };
        Ok(())
    })
}

/// Les trois flottants d'un point, refusés s'ils ne sont pas finis.
///
/// Le refus est ici et non dans le noyau : une position non finie rendrait toutes
/// les comparaisons du comptage de traversées fausses dans les deux sens, et la
/// caméra serait déclarée nulle part sans qu'on sache pourquoi.
///
/// # Safety
///
/// `ptr` est nul, ou vise trois `float` lisibles.
unsafe fn read_point(ptr: *const f32) -> Result<Vec3, AbiError> {
    if ptr.is_null() {
        return Err(AbiError::NULL);
    }
    // SAFETY: précondition de la fonction — trois flottants lisibles.
    let values = unsafe { [ptr.read(), ptr.add(1).read(), ptr.add(2).read()] };
    if values.iter().any(|value| !value.is_finite()) {
        return Err(CoreError::InvalidArgument(Argument::VertexCoordinate).into());
    }
    Ok(Vec3::new(values[0], values[1], values[2]))
}

/// La lecture en deux temps d'un nom, partagée par les deux ressources.
///
/// Extraite dès sa seconde occurrence, contrairement à la règle habituelle : ce
/// n'est pas de la plomberie mais un protocole d'ABI — mesurer, refuser une
/// capacité trop courte sans rien écrire, terminer par un octet nul — et une
/// copie qui divergerait d'un mot ferait mentir le header pour l'une des deux
/// ressources.
///
/// # Safety
///
/// `buf` est nul avec `cap` nul, ou couvre `cap` octets inscriptibles ; `out_len`
/// vise une `size_t` inscriptible. Le nom ne recouvre pas `buf`.
unsafe fn write_name(
    name: Option<&str>,
    buf: *mut c_char,
    cap: usize,
    out_len: *mut usize,
) -> Result<(), AbiError> {
    let name = name.ok_or(AbiError::TEXTURE_SLOT)?;
    if !buf.is_null() {
        if cap < name.len() + 1 {
            return Err(AbiError::NAME_CAPACITY);
        }
        // SAFETY: précondition de la fonction — `buf` couvre `cap` octets
        // inscriptibles, et `cap` vient d'être vérifié plus grand que le nom et
        // son terminateur. Le nom vit dans la ressource, que `buf` ne recouvre
        // pas.
        unsafe {
            ptr::copy_nonoverlapping(name.as_ptr(), buf.cast::<u8>(), name.len());
            buf.add(name.len()).write(0);
        }
    }
    // SAFETY: précondition de la fonction — `out_len` vise une `size_t`
    // inscriptible.
    unsafe { out_len.write(name.len()) };
    Ok(())
}

/// Loads a map from a block of bytes.
///
/// Same contract as `scg_mesh_load`, and deliberately so: two resources loaded
/// from a block have no reason to behave differently, and a binding written for
/// one reads the same for the other. The engine copies what it keeps, the map
/// belongs to no context, and the failure is read with `scg_last_error(NULL)`.
///
/// Loading derives what the file does not store: portal links, triangles and
/// texture coordinates. Nothing of it is a cache that could go stale.
///
/// # Safety
///
/// `bytes` must cover `len` readable bytes, or `len` must be zero. `out` must
/// point to a writable handle; nothing is written unless the call succeeds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_load(
    bytes: *const u8,
    len: usize,
    out: *mut *mut ScgWorld,
) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        if bytes.is_null() && len != 0 {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — `bytes` couvre `len` octets
        // lisibles, le cas vide étant traité par `slice_of_bytes`.
        let bytes = unsafe { slice_of_bytes(bytes, len) };
        let world = World::load(bytes)?;
        let handle = Box::into_raw(Box::new(ScgWorld {
            inner: Arc::new(world),
        }));
        // SAFETY: précondition de la fonction — `out` vise un handle
        // inscriptible, et rien n'y a été écrit avant ce point.
        unsafe { out.write(handle) };
        Ok(())
    })
}

/// Releases a map.
///
/// `scg_world_destroy(NULL)` does nothing, like `free(NULL)`. Same rule as a
/// mesh: nothing reads a map once the submission has returned.
///
/// # Safety
///
/// `world` must be null, or a handle returned by `scg_world_load` and not yet
/// destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_destroy(world: *mut ScgWorld) {
    entry::nothing(|| {
        if world.is_null() {
            return;
        }
        // SAFETY: précondition de la fonction — le handle vient de
        // `Box::into_raw` dans `scg_world_load` et n'a pas encore été rendu.
        drop(unsafe { Box::from_raw(world) });
    });
}

/// Writes the number of materials the map asks for to `out`.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to
/// a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_material_count(world: *const ScgWorld, out: *mut u32) -> i32 {
    // SAFETY: précondition de la fonction — `world` est nul ou un handle
    // vivant, `out` est nul ou inscriptible.
    unsafe { world_count(world, out, World::material_count) }
}

/// Writes the number of triangles the map carries to `out`.
///
/// All cells together: the map is submitted whole at this stage, with no
/// culling, so this is what a host sizes `max_triangles` on.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to
/// a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_triangle_count(world: *const ScgWorld, out: *mut u32) -> i32 {
    // SAFETY: mêmes préconditions que ci-dessus.
    unsafe { world_count(world, out, World::triangle_count) }
}

/// Submits a whole map, one batch per surface.
///
/// `textures` holds `texture_count` handles in material order — the order
/// `scg_world_material_name` walks — and `texture_count` must **equal**
/// `scg_world_material_count`. A null entry means "no texture" for that
/// material. The array is read in place and never copied.
///
/// **Every cell, no culling**, and it is not deprecated. Portal traversal is
/// `scg_submit_world_visible`; this one stays as the path traversal is validated
/// against — on a level where everything is visible, both must render the same
/// image. It takes no starting cell, because a parameter that does nothing is a
/// parameter whose meaning would change.
///
/// **The map is submitted whole or not at all**, like a mesh: size the capacity
/// with `scg_world_triangle_count` before creating the context.
///
/// # Safety
///
/// `ctx` must be null or a live handle. `model` must point to a readable matrix,
/// `world` must be a live handle from `scg_world_load`, and `textures` must be
/// null with `texture_count` zero, or cover `texture_count` readable pointers,
/// each null or a live handle from `scg_texture_load`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_world(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    world: *const ScgWorld,
    textures: *const *const ScgTexture,
    texture_count: u32,
) -> i32 {
    let submit = |mut core: entry::Core<'_>| {
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `world` est un handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        if textures.is_null() && texture_count != 0 {
            return Err(AbiError::NULL);
        }
        if texture_count != world.inner.material_count() {
            return Err(AbiError::TEXTURE_COUNT);
        }
        // SAFETY: précondition de la fonction — le tableau couvre son nombre
        // d'éléments, le cas vide étant traité par `slice_of`.
        let slots = unsafe { slice_of(textures, texture_count) };

        core.exclusive()?
            .submit_world(model, &world.inner, |material| {
                // SAFETY: précondition de la fonction — chaque entrée du tableau
                // est nulle ou un handle vivant. Le compte ayant été vérifié
                // égal et le rang venant du chargement, `get` ne rend jamais
                // `None` ici.
                slots
                    .get(material as usize)
                    .and_then(|handle| unsafe { handle.as_ref() })
                    .map(|texture| &texture.inner)
            })
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, submit) }
}

/// Creates the holder of a map's computed lightmaps, without computing any.
///
/// The allocation happens here, in a named call, and never at the first
/// computation: that is how a host knows when it pays. The handle keeps `world`
/// alive, so the two may be destroyed in either order.
///
/// It takes no context — a resource belongs to none — and writes its error to the
/// thread-local slot, read with `scg_last_error(NULL)`.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to a
/// writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_lighting_create(
    world: *const ScgWorld,
    out: *mut *mut ScgLighting,
) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — `world` est nul ou vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        let inner = Lightmaps::new(&world.inner).map_err(AbiError::from)?;
        let handle = Box::into_raw(Box::new(ScgLighting {
            inner,
            world: world.inner.clone(),
        }));
        // SAFETY: précondition de la fonction — `out` vise un pointeur
        // inscriptible.
        unsafe { out.write(handle) };
        Ok(())
    })
}

/// Releases a lightmap holder. `scg_lighting_destroy(NULL)` does nothing.
///
/// # Safety
///
/// `lighting` must be null, or a handle from `scg_lighting_create` that has not
/// been destroyed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_lighting_destroy(lighting: *mut ScgLighting) {
    if lighting.is_null() {
        return;
    }
    // SAFETY: précondition de la fonction — le handle vient de `Box::into_raw` et
    // n'a pas encore été repris.
    drop(unsafe { Box::from_raw(lighting) });
}

/// Computes the lightmaps of one cell, named by its stable identifier.
///
/// **By identifier and never by rank**: a cache stores identifiers, and editing
/// will name the cell it just changed. An identifier no cell carries is
/// `SCG_ERR_UNKNOWN_RESOURCE`.
///
/// **This allocates and takes time**, so it is a named call and nothing allows it
/// between the start and the end of a frame. A cell that changes recomputes its
/// own — but **its immediate neighbours become wrong**, since the light coming
/// through the doorway was computed over there, and it is for the host to ask for
/// them again.
///
/// # Safety
///
/// `lighting` must be a live handle from `scg_lighting_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_lighting_build(lighting: *mut ScgLighting, cell_id: u32) -> i32 {
    entry::without_context(|| {
        // SAFETY: précondition de la fonction — le handle est nul ou vivant, et
        // aucun autre appel ne le touche en même temps.
        let lighting = unsafe { lighting.as_mut() }.ok_or(AbiError::NULL)?;
        let world = lighting.world.clone();
        lighting
            .inner
            .build(&world, cell_id)
            .map_err(AbiError::from)
    })
}

/// Writes what a cell has as a lightmap to `out`.
///
/// One of `SCG_LIGHTMAP_ABSENT`, `SCG_LIGHTMAP_READY` or `SCG_LIGHTMAP_STALE`.
///
/// # Safety
///
/// `lighting` must be a live handle from `scg_lighting_create`, and `out` must
/// point to a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_lighting_state(
    lighting: *const ScgLighting,
    cell_id: u32,
    out: *mut u32,
) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le handle est nul ou vivant.
        let lighting = unsafe { lighting.as_ref() }.ok_or(AbiError::NULL)?;
        let state = lighting
            .inner
            .state(&lighting.world, cell_id)
            .map_err(AbiError::from)?;
        let value = match state {
            Lightmap::Absent => status::SCG_LIGHTMAP_ABSENT,
            Lightmap::Ready => status::SCG_LIGHTMAP_READY,
            Lightmap::Stale => status::SCG_LIGHTMAP_STALE,
        };
        // SAFETY: précondition de la fonction — `out` vise un `u32` inscriptible.
        unsafe { out.write(value) };
        Ok(())
    })
}

/// Writes the lightmap cache to `buf`.
///
/// Call it once with `buf` null and `cap` zero: it writes the required length to
/// `out_len`. Call it again with a buffer of at least `*out_len` bytes: it writes
/// the block and writes its length to `out_len` again. The length is known
/// analytically — the measuring call serialises nothing.
///
/// A `cap` too short returns `SCG_ERR_INVALID_ARGUMENT` and **writes nothing**,
/// `out_len` included, so a host that ignores the first call cannot half-fill a
/// buffer. A longer buffer is fine; only the block is written, and the block
/// carries its own length in its header. Measure and fill must see the same state:
/// a lightmap computed in between changes the required length.
///
/// A carrier where nothing has been computed writes a valid, empty block. On wasm
/// the buffer comes from `scg_buffer_alloc`, as everywhere else.
///
/// # Safety
///
/// `lighting` must be a live handle from `scg_lighting_create`. `buf` must be null
/// with `cap` zero, or cover `cap` writable bytes. `out_len` must point to a
/// writable `size_t`, in both steps.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_lighting_save(
    lighting: *const ScgLighting,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    entry::without_context(|| {
        if out_len.is_null() || (buf.is_null() && cap != 0) {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le handle est nul ou vivant.
        let lighting = unsafe { lighting.as_ref() }.ok_or(AbiError::NULL)?;
        let len = lighting
            .inner
            .save_len(&lighting.world)
            .map_err(AbiError::from)?;

        if !buf.is_null() {
            if cap < len {
                return Err(AbiError::CACHE_CAPACITY);
            }
            // SAFETY: précondition de la fonction — `buf` couvre `cap` octets
            // inscriptibles, et `cap` vaut la longueur qu'on vient de mesurer. Le
            // cache vit dans le porteur, que `buf` ne recouvre pas.
            let out = unsafe { slice::from_raw_parts_mut(buf, len) };
            lighting
                .inner
                .save(&lighting.world, out)
                .map_err(AbiError::from)?;
        }
        // SAFETY: précondition de la fonction — `out_len` vise une `size_t`
        // inscriptible.
        unsafe { out_len.write(len) };
        Ok(())
    })
}

/// Restores a lightmap cache, and writes how many entries were taken to
/// `out_accepted`.
///
/// **An entry whose fingerprint no longer matches is dropped, and that is not an
/// error**: the call returns `SCG_OK` and counts it out. So is an entry naming a
/// cell the map no longer carries. A partly stale cache is an editor's normal
/// case, and refusing the whole block would recompute a level for one moved wall.
/// Cells that were dropped stay absent, and `scg_lighting_state` says which.
///
/// Only a malformed block is an error, `SCG_ERR_INVALID_FORMAT`. What the block
/// carries decides nothing on its own: every entry is checked against the loaded
/// map.
///
/// # Safety
///
/// `lighting` must be a live handle from `scg_lighting_create`, `bytes` must cover
/// `len` readable bytes, and `out_accepted` must point to a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_lighting_restore(
    lighting: *mut ScgLighting,
    bytes: *const u8,
    len: usize,
    out_accepted: *mut u32,
) -> i32 {
    entry::without_context(|| {
        if bytes.is_null() || out_accepted.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le handle est nul ou vivant, et
        // aucun autre appel ne le touche en même temps.
        let lighting = unsafe { lighting.as_mut() }.ok_or(AbiError::NULL)?;
        let world = lighting.world.clone();
        // SAFETY: précondition de la fonction — `bytes` couvre `len` octets
        // lisibles. Ce qu'ils portent est hostile, et le décodeur le traite comme
        // tel ; seule la couverture est promise par l'appelant.
        let block = unsafe { slice::from_raw_parts(bytes, len) };
        let accepted = lighting
            .inner
            .restore(&world, block)
            .map_err(AbiError::from)?;
        // SAFETY: précondition de la fonction — `out_accepted` vise un `u32`
        // inscriptible.
        unsafe { out_accepted.write(accepted) };
        Ok(())
    })
}

/// Writes the number of cells the map carries to `out`.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to a
/// writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_cell_count(world: *const ScgWorld, out: *mut u32) -> i32 {
    // SAFETY: mêmes préconditions que les autres comptes de la carte.
    unsafe { world_count(world, out, World::cell_count) }
}

/// Writes the stable identifier of the cell at `index` to `out`.
///
/// `index` is a rank in what `scg_world_cell_count` returned, and it is **not**
/// stable across loads: it enumerates, it does not designate. The identifier does.
/// An index beyond the count is `SCG_ERR_INVALID_ARGUMENT` and writes nothing.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to a
/// writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_cell_id(
    world: *const ScgWorld,
    index: u32,
    out: *mut u32,
) -> i32 {
    let read = |world: &ScgWorld| world.inner.cell_id(index).ok_or(AbiError::WORLD_INDEX);
    // SAFETY: mêmes préconditions que les autres accesseurs indexés.
    unsafe { world_value(world, out, read) }
}

/// Writes the identifier of the cell containing `position` to `out`, or `0`.
///
/// **Zero means "nowhere"**, which is a clause and not an error: a host may
/// legitimately place a camera in a gap while a level is being edited. Pass what
/// this writes to `scg_submit_world_visible`.
///
/// **Two overlapping cells may hold the same point**, and the first one in file
/// order wins. This walks every cell and every face, so it is meant for loading a
/// map or for picking the thread back up — between two frames,
/// `scg_world_track` costs far less.
///
/// It takes no context and writes its error to the thread-local slot, read with
/// `scg_last_error(NULL)`.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, `position` must point to
/// three readable `float`s, and `out` must point to a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_locate(
    world: *const ScgWorld,
    position: *const f32,
    out: *mut u32,
) -> i32 {
    let read = |world: &ScgWorld| {
        // SAFETY: précondition de la fonction — trois flottants lisibles.
        let position = unsafe { read_point(position) }?;
        Ok(world.inner.locate(position))
    };
    // SAFETY: mêmes préconditions que les autres accesseurs de la carte.
    unsafe { world_value(world, out, read) }
}

/// Writes the identifier of the cell a move ends in to `out`, or `0`.
///
/// `from_cell` is where the move starts. Crossing a linked portal carries the cell
/// over; leaving through a wall or an unlinked portal writes `0`, and so does a
/// `from_cell` that designates no cell — there is no thread to follow from a cell
/// that does not exist.
///
/// **Several cells may be crossed in one move**, and this follows them. The engine
/// never relocates a camera on its own: when this writes `0`, it is for the host
/// to call `scg_world_locate` again.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, `from` and `to` must each
/// point to three readable `float`s, and `out` must point to a writable
/// `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_track(
    world: *const ScgWorld,
    from_cell: u32,
    from: *const f32,
    to: *const f32,
    out: *mut u32,
) -> i32 {
    let read = |world: &ScgWorld| {
        // SAFETY: précondition de la fonction — trois flottants lisibles chacun.
        let start = unsafe { read_point(from) }?;
        // SAFETY: idem.
        let end = unsafe { read_point(to) }?;
        Ok(world.inner.track(from_cell, start, end))
    };
    // SAFETY: mêmes préconditions que les autres accesseurs de la carte.
    unsafe { world_value(world, out, read) }
}

/// Submits only what the camera sees of a map, from the cell it stands in.
///
/// `textures` works exactly as in `scg_submit_world`. `cell_id` is the stable
/// identifier of the camera's cell, never an index.
///
/// **Traversal decides first, submission follows.** Each retained cell is
/// submitted **once**, in file order — the order that settles two coplanar
/// surfaces — so the total stays within `scg_world_triangle_count`, which remains
/// a valid way to size the context.
///
/// **Returns a positive status, not only `SCG_OK`.** Judge the result by the sign
/// of the code: `SCG_STATUS_INCOMPLETE` when traversal hit `SCG_TRAVERSAL_DEPTH`
/// or `SCG_TRAVERSAL_CELLS`, and `SCG_STATUS_NO_CELL` when `cell_id` is `0`,
/// which means "nowhere" and submits nothing. A host that tests `!= SCG_OK`
/// treats both as failures.
///
/// An identifier that no cell carries is `SCG_ERR_UNKNOWN_RESOURCE` — the
/// difference between "the camera is nowhere", which happens while a level is
/// being edited, and "that cell does not exist", which is a fault in the call.
///
/// `lighting` may be `NULL`, in which case the level is submitted unlit. A cell
/// whose lightmaps are not computed is submitted unlit too, surface by surface:
/// **a partially relit level stays displayable**, which is exactly when an editor
/// needs to see it.
///
/// # Safety
///
/// `ctx` must be null or a live handle, `model` must point to a readable
/// `ScgMat4`, `world` must be a live handle from `scg_world_load`, and `textures`
/// must cover `texture_count` entries, each null or a live texture handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_world_visible(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    world: *const ScgWorld,
    textures: *const *const ScgTexture,
    texture_count: u32,
    lighting: *const ScgLighting,
    cell_id: u32,
) -> i32 {
    let submit = |mut core: entry::Core<'_>| {
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `world` est un handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — nul, ou un handle vivant rendu par
        // `scg_lighting_create`.
        let lighting = unsafe { lighting.as_ref() };
        if textures.is_null() && texture_count != 0 {
            return Err(AbiError::NULL);
        }
        if texture_count != world.inner.material_count() {
            return Err(AbiError::TEXTURE_COUNT);
        }
        // SAFETY: précondition de la fonction — le tableau couvre son nombre
        // d'éléments, le cas vide étant traité par `slice_of`.
        let slots = unsafe { slice_of(textures, texture_count) };

        let seen = core
            .exclusive()?
            .submit_world_visible(
                model,
                &world.inner,
                cell_id,
                lighting.map(|lighting| &lighting.inner),
                |material| {
                    // SAFETY: mêmes préconditions que `scg_submit_world`.
                    slots
                        .get(material as usize)
                        .and_then(|handle| unsafe { handle.as_ref() })
                        .map(|texture| &texture.inner)
                },
            )
            .map_err(AbiError::from)?;

        Ok(match seen {
            Visibility::Complete => status::SCG_OK,
            Visibility::Incomplete => status::SCG_STATUS_INCOMPLETE,
            Visibility::NoCell => status::SCG_STATUS_NO_CELL,
        })
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, submit) }
}

/// Writes the number of static lights the map carries to `out`.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to
/// a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_light_count(world: *const ScgWorld, out: *mut u32) -> i32 {
    // SAFETY: mêmes préconditions que les autres comptes d'une carte.
    unsafe { world_count(world, out, World::light_count) }
}

/// Writes one static light of the map to `out`.
///
/// **The engine fills a structure you own**, in the very shape you hand back to
/// `scg_set_lights`: a map's lights are meant to be re-submitted, not rebuilt.
/// The reserved byte is written zero, as the contract requires of you.
///
/// An `index` beyond `scg_world_light_count` returns
/// `SCG_ERR_INVALID_ARGUMENT` and writes nothing.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to
/// a writable `ScgLight`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_light(
    world: *const ScgWorld,
    index: u32,
    out: *mut ScgLight,
) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        let light = world.inner.light(index).ok_or(AbiError::WORLD_INDEX)?;
        // SAFETY: précondition de la fonction — `out` vise une `ScgLight`
        // inscriptible, et rien n'y a été écrit avant ce point.
        unsafe { out.write(ScgLight::from_core(light)) };
        Ok(())
    })
}

/// Writes the number of entities the map carries to `out`.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to
/// a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_entity_count(world: *const ScgWorld, out: *mut u32) -> i32 {
    // SAFETY: mêmes préconditions que les autres comptes d'une carte.
    unsafe { world_count(world, out, World::entity_count) }
}

/// Writes an entity's own identifier and that of its cell.
///
/// Both are stable identifiers the editor assigned, never array indices: that is
/// what lets an editor undo and save part of a map without renumbering anything.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and both `id` and `cell`
/// must point to writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_entity_ids(
    world: *const ScgWorld,
    index: u32,
    id: *mut u32,
    cell: *mut u32,
) -> i32 {
    entry::without_context(|| {
        if id.is_null() || cell.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        let (own, home) = world.inner.entity_ids(index).ok_or(AbiError::WORLD_INDEX)?;
        // SAFETY: précondition de la fonction — les deux pointeurs visent des
        // `uint32_t` inscriptibles.
        unsafe {
            id.write(own);
            cell.write(home);
        }
        Ok(())
    })
}

/// Writes an entity's pose to `out`: three floats of position, then four of a
/// normalised quaternion.
///
/// Seven floats and not a structure: a pose has no published layout to reuse,
/// and inventing one would freeze it forever for the sake of one accessor.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must cover
/// seven writable floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_entity_pose(
    world: *const ScgWorld,
    index: u32,
    out: *mut f32,
) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        let (position, orientation) = world
            .inner
            .entity_pose(index)
            .ok_or(AbiError::WORLD_INDEX)?;
        let pose = [
            position.x,
            position.y,
            position.z,
            orientation.x,
            orientation.y,
            orientation.z,
            orientation.w,
        ];
        // SAFETY: précondition de la fonction — `out` couvre sept flottants
        // inscriptibles, et le tableau source ne le recouvre pas.
        unsafe { ptr::copy_nonoverlapping(pose.as_ptr(), out, pose.len()) };
        Ok(())
    })
}

/// Reads an entity's class, in two steps.
///
/// Same protocol as the other names. **The engine never interprets this
/// string**: it carries what the editor wrote, and what it means is the host's
/// business.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`. `buf` must be null with
/// `cap` zero, or cover `cap` writable bytes. `out_len` must point to a writable
/// `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_entity_class(
    world: *const ScgWorld,
    index: u32,
    buf: *mut c_char,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    entry::without_context(|| {
        if out_len.is_null() || (buf.is_null() && cap != 0) {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: mêmes préconditions que celles de `write_name`.
        unsafe { write_name(world.inner.entity_class(index), buf, cap, out_len) }
    })
}

/// Reads an entity's opaque bytes, in two steps.
///
/// Same protocol as a name, **without a terminator**: these are bytes, not a
/// string, and the engine copied them without reading one. `out_len` is their
/// length, and a `cap` below it returns `SCG_ERR_INVALID_ARGUMENT` without
/// writing anything.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`. `buf` must be null with
/// `cap` zero, or cover `cap` writable bytes. `out_len` must point to a writable
/// `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_entity_data(
    world: *const ScgWorld,
    index: u32,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    entry::without_context(|| {
        if out_len.is_null() || (buf.is_null() && cap != 0) {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        let data = world
            .inner
            .entity_data(index)
            .ok_or(AbiError::WORLD_INDEX)?;

        if !buf.is_null() {
            if cap < data.len() {
                return Err(AbiError::NAME_CAPACITY);
            }
            // SAFETY: précondition de la fonction — `buf` couvre `cap` octets
            // inscriptibles, et `cap` vient d'être vérifié au moins aussi grand
            // que le bloc. Celui-ci vit dans la ressource, que `buf` ne recouvre
            // pas.
            unsafe { ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len()) };
        }
        // SAFETY: précondition de la fonction — `out_len` vise une `size_t`
        // inscriptible.
        unsafe { out_len.write(data.len()) };
        Ok(())
    })
}

/// Reads the name of one material, in two steps.
///
/// Same protocol as `scg_mesh_texture_name`: call once with `buf` null and `cap`
/// zero to learn the length, then again with a buffer of at least `*out_len + 1`
/// bytes. A `cap` too small returns `SCG_ERR_INVALID_ARGUMENT` and writes
/// nothing, `out_len` included; an `index` beyond `scg_world_material_count`
/// returns the same code.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`. `buf` must be null with
/// `cap` zero, or cover `cap` writable bytes. `out_len` must point to a writable
/// `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_material_name(
    world: *const ScgWorld,
    index: u32,
    buf: *mut c_char,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    entry::without_context(|| {
        if out_len.is_null() || (buf.is_null() && cap != 0) {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: mêmes préconditions que celles de `write_name`, que cet appel
        // porte telles quelles.
        unsafe { write_name(world.inner.material_name(index), buf, cap, out_len) }
    })
}
