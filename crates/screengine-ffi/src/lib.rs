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
mod message;
mod output;
mod scene;
mod status;
mod texture;

use std::alloc::{self, Layout};
use std::ffi::c_char;
use std::ptr;

use std::sync::Arc;

use screengine::{Argument, Context, Error as CoreError, Texture, Vec3, VertexUv, VertexUv2};

use entry::AbiError;
use output::HostRows;

pub use context::{ScgContext, ScgContextConfig};
pub use scene::{
    SCG_FILTER_BILINEAR, SCG_FILTER_DITHER, SCG_TEXTURE_FORMAT_RGBA8, ScgCamera, ScgLight, ScgMat4,
    ScgTextureDesc, ScgTriangle, ScgVertex, ScgVertexUv, ScgVertexUv2,
};
pub use status::{
    SCG_ERR_FAULTED, SCG_ERR_INVALID_ARGUMENT, SCG_ERR_INVALID_STATE, SCG_ERR_NULL,
    SCG_ERR_OUT_OF_MEMORY, SCG_ERR_PANIC, SCG_ERR_POISONED, SCG_OK,
};
pub use texture::ScgTexture;

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
/// `scg_frame_end`: the camera holds for a whole frame. The field of view must
/// be within ]0, pi[ radians and the near plane positive, otherwise
/// `SCG_ERR_INVALID_ARGUMENT`. The quaternion is normalised by the engine.
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
        if !core.shared().is_rendering() {
            core.exclusive()?.begin()?;
        }
        // SAFETY: précondition de la fonction — le tampon couvre l'image. Une
        // tuile qui tournerait encore fait refuser la fin avant toute écriture.
        let mut out = unsafe { HostRows::new(pixels, stride) };
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
