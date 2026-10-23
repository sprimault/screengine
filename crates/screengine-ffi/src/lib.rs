// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Frontière C de Screengine.
//!
//! Ce crate convertit des types, enveloppe chaque point d'entrée de
//! `catch_unwind` et traduit les erreurs en codes. Il ne calcule rien : un
//! calcul écrit ici serait fait autrement par la conformance, qui appelle le
//! noyau sans passer par lui.
//!
//! **Deux points d'entrée s'en passent, et ils se comptent** :
//! `scg_abi_version`, qui rend une constante, et `scg_last_error`, qui rend un
//! pointeur sur un tampon déjà écrit et doit rester permise sur un objet
//! défaillant. Aucune des deux n'a de code de retour où loger une panique, et
//! chacune porte sa raison sur son corps ; « chaque point d'entrée » se lit donc
//! comme « chaque point d'entrée qui calcule », ce que `docs/abi.md` énonce dans
//! les mêmes termes.
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
    Affine3, Angle, Argument, Context, DepthMode, Error as CoreError, Lightmap, LightmapFault,
    Lightmaps, Mesh, Quat, Rect, SpriteOrientation, Surfaces, Sweep, Texture, Vec3, VertexUv,
    VertexUv2, Visibility, World, lightmap_fault,
};

use entry::AbiError;
use output::HostRows;
use world::paired;

pub use context::{ScgContext, ScgContextConfig};
pub use mesh::ScgMesh;
pub use scene::{
    SCG_BLEND_MODULATE, SCG_DEPTH_ALWAYS, SCG_DEPTH_TESTED, SCG_FILTER_BILINEAR, SCG_FILTER_DITHER,
    SCG_PICK_ALL, SCG_PICK_SOLID, SCG_SIMD_AUTO, SCG_SIMD_AVX2, SCG_SIMD_NEON, SCG_SIMD_SCALAR,
    SCG_SIMD_SIMD128, SCG_SIMD_SSE2, SCG_SPRITE_AXIAL, SCG_SPRITE_FACING, SCG_TEXTURE_FORMAT_RGBA8,
    SCG_TEXTURE_FORMAT_RGBA8_MASKED, ScgCamera, ScgGrade, ScgLight, ScgLine, ScgMat4, ScgPoint,
    ScgQuat, ScgSprite, ScgSweepHit, ScgTextureDesc, ScgTriangle, ScgVertex, ScgVertexUv,
    ScgVertexUv2, ScgVertexUvN,
};
// Les bornes d'entrée ont rejoint cette liste après coup : `cbindgen` lit la
// source de `status`, donc le header les portait déjà, mais rien ne les nommait
// depuis l'extérieur du crate — pas même un test de la frontière.
pub use status::{
    SCG_ERR_FAULTED, SCG_ERR_INVALID_ARGUMENT, SCG_ERR_INVALID_FORMAT, SCG_ERR_INVALID_STATE,
    SCG_ERR_NULL, SCG_ERR_OUT_OF_MEMORY, SCG_ERR_PANIC, SCG_ERR_POISONED, SCG_ERR_UNKNOWN_RESOURCE,
    SCG_ERR_UNSUPPORTED_FORMAT_VERSION, SCG_LIGHTMAP_ABSENT, SCG_LIGHTMAP_FAULT_AXIS_OFF_PLANE,
    SCG_LIGHTMAP_FAULT_DEGENERATE_AXIS, SCG_LIGHTMAP_FAULT_EXTENT_TOO_LARGE,
    SCG_LIGHTMAP_FAULT_NONE, SCG_LIGHTMAP_FAULT_ORIGIN_OFF_GRID, SCG_LIGHTMAP_FAULT_SKEWED_AXES,
    SCG_LIGHTMAP_READY, SCG_LIGHTMAP_STALE, SCG_LINE_CAPACITY, SCG_MAX_LIGHTMAP_SIZE,
    SCG_MAX_LIGHTS, SCG_MAX_OVERBRIGHT, SCG_MAX_RESOLUTION, SCG_MAX_TEXEL_COORD,
    SCG_MAX_TEXTURE_SIZE, SCG_OK, SCG_STATUS_INCOMPLETE, SCG_STATUS_NO_CELL, SCG_STATUS_NO_GAP,
    SCG_STATUS_START_SOLID, SCG_SWEEP_CELLS, SCG_TRAVERSAL_CELLS, SCG_TRAVERSAL_DEPTH,
    SCG_TRIANGLE_CAPACITY,
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
    // **Sans enveloppe, et c'est l'une des deux exceptions de la frontière**,
    // avec `scg_last_error`. Son corps rend une constante : rien n'y alloue,
    // rien n'y calcule, rien n'y panique. L'envelopper sauverait et
    // restaurerait l'environnement flottant pour retourner un entier — sur
    // l'appel que toute liaison fait en premier, avant même de savoir si la
    // bibliothèque lui convient.
    //
    // Sa signature l'interdit d'ailleurs : elle rend la version, pas un code de
    // retour, et n'a donc aucune valeur à donner à une panique.
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
    let set = |context: &mut Context| {
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise une
        // structure lisible, que rien d'autre ne modifie pendant l'appel.
        let camera = unsafe { camera.as_ref() }.ok_or(AbiError::NULL)?;
        context.set_camera(camera.to_core()?)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, set) }
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
    let set = |context: &mut Context| {
        context.set_resolution(width, height)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, set) }
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
    let set = |context: &mut Context| {
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise une
        // structure lisible, que rien d'autre ne modifie pendant l'appel.
        let grade = unsafe { grade.as_ref() }.ok_or(AbiError::NULL)?;
        let (gamma, gains, offsets) = grade.to_core()?;
        context.set_grade(gamma, gains, offsets)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, set) }
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
    let clear = |context: &mut Context| {
        context.clear_grade()?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, clear) }
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
    let set = |context: &mut Context| {
        context.set_filter(scene::filter_of(filter)?)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, set) }
}

/// Picks which instruction set fills triangles, from the next frames on.
///
/// `path` is `SCG_SIMD_AUTO`, the default, or one of `SCG_SIMD_SCALAR`,
/// `SCG_SIMD_SSE2`, `SCG_SIMD_AVX2`, `SCG_SIMD_NEON`, `SCG_SIMD_SIMD128`. An
/// unknown value is `SCG_ERR_INVALID_ARGUMENT`, and so is a known path this
/// machine does not provide — the two are told apart by `scg_last_error`.
///
/// **This never changes the image.** Every path renders the same bits as the
/// scalar one, which is the reference they are all validated against: what this
/// call selects is a cost, never a rendering. A digest that moved from one path
/// to another would mean a faulty path, not an acceptable difference.
///
/// **It exists so paths can be compared on one machine.** Without it, each path
/// would only ever be exercised where it runs, and SSE2 would never be measured
/// against the scalar one on a processor that also has AVX2 — that is, on almost
/// every one. A host gains from it the means to step around an instruction set
/// it holds to be at fault, without waiting for a release.
///
/// `SCG_SIMD_SCALAR` is available on every target; `SCG_SIMD_AUTO` picks the
/// widest path the machine provides and is therefore always available too.
///
/// Rejected with `SCG_ERR_INVALID_STATE` between `scg_frame_begin` and
/// `scg_frame_end`, for the reason of `scg_set_filter`: tiles are rendered from
/// threads the engine knows nothing about.
///
/// # Safety
///
/// `ctx` is a live handle used by no other thread during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_set_simd(ctx: *mut ScgContext, path: u32) -> i32 {
    let set = |context: &mut Context| {
        context.set_simd(scene::simd_path_of(path)?)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, set) }
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
    let set = |context: &mut Context| {
        context.set_overbright(shift)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, set) }
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
    let set = |context: &mut Context| {
        let color = screengine::Color::new(r, g, b, 0xFF);
        context.set_fog(color, start, end)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, set) }
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
    let clear = |context: &mut Context| {
        context.clear_fog()?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, clear) }
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
    let set = |context: &mut Context| {
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
        context.set_lights(&core_lights[..lights.len()])?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, set) }
}

/// Writes to `out` the world direction the centre of pixel `(x, y)` points at:
/// three floats, `x`, `y` then `z`, of unit length.
///
/// **The inverse of the projection, for picking with a cursor.** Form the
/// segment of a ray query by starting at the camera's position and adding this
/// vector scaled by the reach you want, then pass it to `scg_world_pick`. This
/// is the only step that goes back from the image to the world, and a host
/// cannot redo it without copying two of the engine's conventions: the aspect
/// ratio carried by the width alone, and the view basis of `docs/abi.md`.
///
/// **Unit length is what makes the reach a distance.** The raw direction has a
/// depth of one, so its length grows towards the edges of the image; scaled by a
/// reach it would carry further at a corner than at the centre, and a selection
/// would see further where you aim askew.
///
/// `x` and `y` are in the **internal resolution**, not the window: a pixel at or
/// beyond it is `SCG_ERR_INVALID_ARGUMENT`, and that refusal is the point — a
/// windowed host has both frames of reference at hand, and passing the wrong one
/// yields a plausible ray.
///
/// **Allowed while tiles render**, unlike every setting: it only reads a camera
/// and a resolution that no call can change during a frame.
///
/// # Safety
///
/// `ctx` is a live handle, and `out` must cover three writable floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_pixel_ray(ctx: *mut ScgContext, x: u32, y: u32, out: *mut f32) -> i32 {
    let ray = |core: entry::Core<'_>| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        let ray = core.shared().pixel_ray(x, y)?;
        let axes = [ray.x, ray.y, ray.z];
        // SAFETY: précondition de la fonction — `out` couvre trois flottants
        // inscriptibles, et le tableau source ne le recouvre pas.
        unsafe { ptr::copy_nonoverlapping(axes.as_ptr(), out, axes.len()) };
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_context(ctx, ray) }
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
    let submit = |context: &mut Context| {
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
        context
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
    unsafe { entry::with_exclusive(ctx, submit) }
}

/// La tranche d'un tableau reçu de l'hôte, vide pour un compte nul.
///
/// Rend au tas le handle opaque d'une ressource, un pointeur nul ne faisant
/// rien.
///
/// **Les quatre libérations de l'ABI avaient le même corps au nom du handle
/// près.** C'est l'enveloppe qui rend l'extraction obligatoire et pas seulement
/// souhaitable : `entry::nothing` doit être là — une panique dans un
/// destructeur traverserait la frontière —, et c'est précisément le genre de
/// ligne qui manque sur la libération écrite en dernier.
///
/// # Safety
///
/// `handle` est nul, ou vient de `Box::into_raw` et n'a pas encore été repris.
unsafe fn destroy<T>(handle: *mut T) {
    entry::nothing(|| {
        if handle.is_null() {
            return;
        }
        // SAFETY: précondition de la fonction — le handle vient de
        // `Box::into_raw` et n'a pas encore été rendu.
        drop(unsafe { Box::from_raw(handle) });
    });
}

/// Résout un rang dans le tableau de textures que l'hôte a passé.
///
/// **Cinq soumissions prenaient la même chaîne**, chacune avec son bloc `unsafe`
/// et son commentaire. Le rang vient du moteur, le tableau de l'appelant, et une
/// entrée nulle dit « pas de texture » — ce que les trois maillons expriment
/// ensemble et aucun seul. Un chemin ajouté sans résolution ne compilerait pas ;
/// un chemin ajouté avec une résolution **différente** — un rang non vérifié —
/// passait.
///
/// # Safety
///
/// Chaque entrée de `slots` est nulle, ou un handle de texture vivant.
unsafe fn slot_texture(slots: &[*const ScgTexture], rang: u32) -> Option<&Arc<Texture>> {
    slots
        .get(rang as usize)
        // SAFETY: précondition de la fonction — chaque entrée du tableau est
        // nulle ou un handle vivant.
        .and_then(|handle| unsafe { handle.as_ref() })
        .map(|texture| &texture.inner)
}

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

/// Renders an arbitrary rectangle of the begun frame, without binning.
///
/// **This is the tiles' oracle.** Every triangle and every line of the frame, in
/// submission order, against this rectangle alone: a tile that differs from it
/// has lost a primitive to the binning, or depends on how the image was cut. It
/// is to rendering what `scg_world_sweep_brute` is to collision — a slower path
/// that cannot be wrong about the thing the faster one might be wrong about.
///
/// **The two scratch buffers belong to the host**, `scratch` cells each and at
/// least `width × height`. That is the only difference from a tile, which keeps
/// its colour and depth on the stack of the call: a region may be the whole
/// image, and 2048 × 2048 cells of colour and depth would not fit there. One
/// length for both, because the engine requires them equal. **So this call asks
/// nothing of the stack**, where a tile and `scg_frame_end` need 128 KiB.
///
/// **It takes no tile, so it does not replace them.** `scg_frame_end` still
/// renders every tile nobody took, and those overlapping the region overwrite
/// what it wrote there; a host comparing the two reads its buffer in between.
///
/// Only between `scg_frame_begin` and `scg_frame_end`, like a tile — outside a
/// begun frame it returns `SCG_ERR_INVALID_STATE`. A rectangle reaching past the
/// image, or a `scratch` shorter than it, returns `SCG_ERR_INVALID_ARGUMENT`
/// without writing anything. The error message goes to the per-thread slot.
///
/// **This is not a frame path**, and the contract says so rather than letting
/// you measure it: every primitive of the frame is tested against the region,
/// without the binning that culls them per tile.
///
/// # Safety
///
/// `ctx` is a live handle, `color` and `depth` each point to `scratch` writable
/// `uint32_t` and **do not overlap each other**, and `pixels` points to a
/// writable buffer of at least `stride × height × 4` bytes.
///
/// The single `scratch` length for both does not mean one buffer serves both:
/// the engine takes an exclusive view of each, so passing the same pointer twice
/// is undefined behaviour before a byte is written. It is a precondition and not
/// an error case — the engine cannot see it, and the call that would prove a
/// check works is itself undefined.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_frame_region(
    ctx: *mut ScgContext,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: *mut u32,
    depth: *mut u32,
    scratch: usize,
    pixels: *mut u8,
    stride: u32,
) -> i32 {
    let render = |core: &Context| {
        if pixels.is_null() || color.is_null() || depth.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — chacun des deux couvre
        // `scratch` mots inscriptibles, et les deux tranches sont distinctes.
        let (color, depth) = unsafe {
            (
                slice::from_raw_parts_mut(color, scratch),
                slice::from_raw_parts_mut(depth, scratch),
            )
        };
        // SAFETY: précondition de la fonction — le tampon couvre l'image, et la
        // région n'en écrit que son rectangle.
        let mut out = unsafe { HostRows::new(pixels, stride) };
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        core.region(rect, color, depth, &mut out)?;
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
/// It renders tiles, and each keeps its colour and depth on the stack of the
/// call: the calling thread needs at least 128 KiB of stack, exactly as for
/// `scg_frame_tile`. A host without threads, which calls nothing but this
/// function, needs it too.
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
/// **That slot's lifetime is shorter than the context one's**: every entry point
/// clears it on the way in, whether it takes a context or not — every one but
/// the two the contract exempts from the wrapper, `scg_abi_version` and this
/// function, which compute nothing and so write no message. So it is valid until
/// the **next call that can fail, on the same thread**, where the context message
/// survives until the next call on that same context. A host that calls anything
/// else between the failure and the read gets the empty string, not a stale
/// message.
///
/// The two exemptions are what make this readable at all: a reader that cleared
/// the slot would erase the message it was called to fetch.
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
        let texture = if desc.format == SCG_TEXTURE_FORMAT_RGBA8_MASKED {
            Texture::load_masked(desc.width, desc.height, pixels)?
        } else {
            Texture::load(desc.width, desc.height, pixels)?
        };
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
    // SAFETY: précondition de la fonction — le handle est nul, ou vient de
    // `Box::into_raw` dans `scg_texture_load` et n'a pas encore été rendu.
    unsafe { destroy(texture) };
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
    let submit = |context: &mut Context| {
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
        context
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
    unsafe { entry::with_exclusive(ctx, submit) }
}

/// Submits a batch of triangles that **multiply** the buffer instead of
/// overwriting it.
///
/// Same contract as `scg_submit_textured`, with one difference that decides
/// everything: 255 is the neutral value, as it is for a lightmap. A modulated
/// surface darkens or does nothing — it never brightens.
///
/// **It tests depth without writing it, and the test is not strict.** The two
/// halves are set separately and each has its reason. Not strict, because a
/// stain is coplanar with the surface it marks and a strict test would lose it
/// at equal depth. No write, because a modulated surface occludes nothing: were
/// it to write depth, two stacked stains would multiply only once, in an order
/// that depended on tile binning.
///
/// **Submit the scenery before its stains.** That is the only ordering the
/// z-buffer does not remove for you, and it costs nothing new: submission order
/// is already contractual, and already independent of tile size and thread
/// count.
///
/// `texture` may be null, in which case each triangle's colour is the factor.
/// `blend` must be `SCG_BLEND_MODULATE`; zero and any unknown value are
/// refused, never silently mapped onto a default.
///
/// # Safety
///
/// Same preconditions as `scg_submit_textured`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_blended(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    vertices: *const ScgVertexUv,
    vertex_count: u32,
    triangles: *const ScgTriangle,
    triangle_count: u32,
    texture: *const ScgTexture,
    blend: u32,
) -> i32 {
    let submit = |context: &mut Context| {
        if blend != SCG_BLEND_MODULATE {
            return Err(AbiError::BLEND_MODE);
        }
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `texture` est nul ou vivant.
        let texture = unsafe { texture.as_ref() };
        // SAFETY: précondition de la fonction — chaque pointeur couvre son
        // nombre d'éléments.
        let (vertices, triangles) = unsafe {
            (
                slice_of(vertices, vertex_count),
                slice_of(triangles, triangle_count),
            )
        };
        scene::check_finite_uv(vertices)?;
        context
            .submit_each_blended(model, triangles.len(), texture.map(|t| &t.inner), |i| {
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
    unsafe { entry::with_exclusive(ctx, submit) }
}

/// Submits quads the engine orients on the camera.
///
/// The host gives a centre and two half-extents; the engine builds the quad.
/// That is the one thing here a host cannot do without computing: orienting
/// requires the camera's basis, which it would have to obtain by re-inverting
/// the pose it just passed in — normalising a quaternion with its own maths
/// library, so that two bindings would no longer render the same image.
///
/// **`model` places the centre, and nothing else.** Its linear part does not
/// orient the quad; the camera does, by definition. The parameter is there for
/// the symmetry of the submission family.
///
/// `orientation` must be `SCG_SPRITE_AXIAL` or `SCG_SPRITE_FACING`; zero and
/// any unknown value are refused, never silently mapped onto a default. The
/// axial mode degenerates when the camera looks straight down: the quads then
/// disappear, without an error, like a triangle that does not project.
///
/// `texture` may be null, in which case each sprite's colour fills its quad.
/// **Each sprite consumes two triangles** of the context's capacity, before
/// clipping.
///
/// # Safety
///
/// `ctx` is null or a live handle. `model` points at a readable `ScgMat4`.
/// `sprites` points at `sprite_count` readable `ScgSprite`. `texture` is null
/// or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_sprites(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    sprites: *const ScgSprite,
    sprite_count: u32,
    texture: *const ScgTexture,
    orientation: u32,
) -> i32 {
    let submit = |context: &mut Context| {
        let orientation = match orientation {
            SCG_SPRITE_AXIAL => SpriteOrientation::Axial,
            SCG_SPRITE_FACING => SpriteOrientation::Facing,
            _ => return Err(AbiError::SPRITE_ORIENTATION),
        };
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `texture` est nul ou vivant.
        let texture = unsafe { texture.as_ref() };
        // SAFETY: précondition de la fonction — `sprites` couvre son nombre
        // d'éléments.
        let sprites = unsafe { slice_of(sprites, sprite_count) };
        context
            .submit_each_sprite(
                model,
                sprites.len(),
                texture.map(|t| &t.inner),
                orientation,
                |i| Ok(sprites[i].to_core()),
            )
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, submit) }
}

/// Submits lines to draw, in the space `model` carries.
///
/// **A second primitive family, with a budget of its own**: lines and points
/// share `max_lines`, never `max_triangles`. One line costs one slot, and a
/// clipped line still costs one — unlike a triangle, whose clipping multiplies
/// the slots it consumes.
///
/// `depth` must be `SCG_DEPTH_TESTED` or `SCG_DEPTH_ALWAYS`; zero and any
/// unknown value are refused, never silently mapped onto a default.
///
/// Drawing happens **after all filling and after fog, before the output
/// curve**: fog would erase a guide at the end of a corridor, which is the
/// opposite of what a guide is for, while the curve applies because there is
/// only one output path. It never writes depth, in either mode.
///
/// The batch is accepted or refused whole: a non-finite coordinate or an
/// exceeded capacity leaves nothing in the frame. A line that does not
/// project — behind the near plane, outside the guard band — disappears
/// without an error, like a triangle that does not project.
///
/// # Safety
///
/// `ctx` is null or a live handle. `model` points at a readable `ScgMat4`.
/// `lines` points at `line_count` readable `ScgLine`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_lines(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    lines: *const ScgLine,
    line_count: u32,
    depth: u32,
) -> i32 {
    let submit = |context: &mut Context| {
        // **L'état d'abord, les arguments ensuite** : la convention de la
        // frontière, écrite dans `docs/abi.md`. Ces deux soumissions étaient les
        // seules à l'inverse, et pendant le rendu un mode fautif y rendait
        // `SCG_ERR_INVALID_ARGUMENT` là où les huit réglages rendaient
        // `SCG_ERR_INVALID_STATE`.
        let depth = depth_mode(depth)?;
        // SAFETY: précondition de la fonction — `model` est nul ou lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `lines` couvre son nombre
        // d'éléments.
        let lines = unsafe { slice_of(lines, line_count) };
        // Le tableau se lit sur place par une fonction d'accès, jamais recopié
        // ni réinterprété : une copie serait une allocation par image, et une
        // réinterprétation imposerait au noyau la disposition de la frontière.
        context
            .submit_each_line(model, lines.len(), depth, |i| lines[i].to_core())
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, submit) }
}

/// Submits points to draw, in the space `model` carries.
///
/// Same contract as `scg_submit_lines`, same budget, same depth modes. A point
/// lights the pixel that contains it, and only that one.
///
/// # Safety
///
/// `ctx` is null or a live handle. `model` points at a readable `ScgMat4`.
/// `points` points at `point_count` readable `ScgPoint`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_points(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    points: *const ScgPoint,
    point_count: u32,
    depth: u32,
) -> i32 {
    let submit = |context: &mut Context| {
        // L'état d'abord, comme la soumission de lignes et pour la même raison.
        let depth = depth_mode(depth)?;
        // SAFETY: précondition de la fonction — `model` est nul ou lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `points` couvre son nombre
        // d'éléments.
        let points = unsafe { slice_of(points, point_count) };
        context
            .submit_each_point(model, points.len(), depth, |i| points[i].to_core())
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, submit) }
}

/// La constante d'ABI traduite en mode du noyau, ou le refus.
///
/// Écrite une fois pour les deux soumissions de tracé : recopiée, elle finirait
/// par accepter dans l'une ce que l'autre refuse.
fn depth_mode(depth: u32) -> Result<DepthMode, AbiError> {
    match depth {
        SCG_DEPTH_TESTED => Ok(DepthMode::Tested),
        SCG_DEPTH_ALWAYS => Ok(DepthMode::Always),
        _ => Err(AbiError::DEPTH_MODE),
    }
}

/// Traduit le filtre de surfaces d'une interrogation.
///
/// Même forme que [`depth_mode`] et pour la même raison : zéro n'y vaut pas
/// défaut, et un mode inconnu est refusé plutôt que rabattu sur le plus
/// probable. Les deux interrogations le prenaient à l'identique, ce qui fait de
/// ce patron sa troisième occurrence et non sa seconde.
fn pick_filter(filter: u32) -> Result<Surfaces, AbiError> {
    match filter {
        SCG_PICK_SOLID => Ok(Surfaces::Solid),
        SCG_PICK_ALL => Ok(Surfaces::All),
        _ => Err(AbiError::PICK_FILTER),
    }
}

/// Submits a batch of triangles whose vertices carry a normal.
///
/// Same contract as `scg_submit_textured`, with two differences. The first is the
/// point of this path: dynamic lights then take the surface's orientation into
/// account. A face turned away from a light receives nothing, where without a
/// normal it received the same light as every other face at the same distance.
///
/// The second is easy to miss: **`texture` may be null here**, and each triangle
/// then renders its own colour. The same argument is `SCG_ERR_NULL` on
/// `scg_submit_textured`, where an untextured batch belongs on `scg_submit`
/// instead — but `scg_submit` carries no normal, so a shaded batch without a
/// texture has nowhere else to go.
///
/// **The normal need not be unit** — the engine normalises it, and it must,
/// since interpolating between frames denormalises it anyway. A zero-length
/// normal means no normal: the lighting falls back to distance alone. A
/// non-finite component rejects the whole batch, like a coordinate.
///
/// This changes nothing for lightmaps. What closes here is the angular term of
/// a **dynamic** light; occlusion remains the baking's business, and a baked
/// lamp and the same lamp as a dynamic light will never render quite the same.
///
/// # Safety
///
/// Same preconditions as `scg_submit_textured`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_shaded(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    vertices: *const ScgVertexUvN,
    vertex_count: u32,
    triangles: *const ScgTriangle,
    triangle_count: u32,
    texture: *const ScgTexture,
) -> i32 {
    let submit = |context: &mut Context| {
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `texture` est nul ou vivant.
        let texture = unsafe { texture.as_ref() };
        // SAFETY: précondition de la fonction — chaque pointeur couvre son
        // nombre d'éléments.
        let (vertices, triangles) = unsafe {
            (
                slice_of(vertices, vertex_count),
                slice_of(triangles, triangle_count),
            )
        };
        scene::check_finite_uvn(vertices)?;
        context
            .submit_each_shaded(model, triangles.len(), texture.map(|t| &t.inner), |i| {
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
            })
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, submit) }
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
    let submit = |context: &mut Context| {
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
        context
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
    unsafe { entry::with_exclusive(ctx, submit) }
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
    // SAFETY: précondition de la fonction — le handle est nul, ou vient de
    // `Box::into_raw` dans `scg_mesh_load` et n'a pas encore été rendu.
    unsafe { destroy(mesh) };
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
    let submit = |context: &mut Context| {
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

        context
            // SAFETY: précondition de la fonction — chaque entrée du tableau est
            // nulle ou un handle vivant. Le compte ayant été vérifié égal, le
            // rang est toujours dans les bornes.
            .submit_mesh(model, &mesh.inner, |slot| unsafe {
                slot_texture(slots, slot)
            })
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, submit) }
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
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let mesh = unsafe { mesh.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: mêmes préconditions que celles de `write_name`, que cet appel
        // porte telles quelles.
        unsafe {
            write_name(
                mesh.inner.texture_name(slot),
                AbiError::SLOT_RANGE,
                buf,
                cap,
                out_len,
            )
        }
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

/// Submits a mesh interpolated between two of its frames.
///
/// **`frame_a == frame_b` renders exactly what `scg_submit_mesh` renders of
/// that frame, for any `t`.** That is not a convenience: it is the theorem this
/// path validates against, as portal traversal validates against the raw path.
///
/// Two explicit indices rather than "the frame and the next one": a host may
/// loop from the last to the first, or blend two non-adjacent frames, without
/// the engine knowing anything of clips or sequences — nothing of the game
/// crosses the boundary.
///
/// `t` outside `[0, 1]` is **refused, never clamped**: silent clamping would
/// render an extrapolated pose without saying so, and extrapolating is the
/// game's decision. A frame index beyond `scg_mesh_frame_count` returns
/// `SCG_ERR_INVALID_ARGUMENT` and draws nothing — the file is fine, the call is
/// out of bounds.
///
/// Every other rule of `scg_submit_mesh` applies, the all-or-nothing refusal
/// included.
///
/// # Safety
///
/// Same preconditions as `scg_submit_mesh`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_mesh_frame(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    mesh: *const ScgMesh,
    textures: *const *const ScgTexture,
    texture_count: u32,
    frame_a: u32,
    frame_b: u32,
    t: f32,
) -> i32 {
    let submit = |context: &mut Context| {
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

        context
            .submit_mesh_frame(
                model,
                &mesh.inner,
                // SAFETY: mêmes préconditions que `scg_submit_mesh`, et le
                // compte a été vérifié égal.
                |slot| unsafe { slot_texture(slots, slot) },
                frame_a,
                frame_b,
                t,
            )
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, submit) }
}

/// Writes the number of animation frames the mesh carries to `out`.
///
/// A static mesh returns 1: the format has one shape, not two, and a decoder
/// with two paths is a decoder with two behaviours to keep in step.
///
/// It exists for the reason `scg_mesh_triangle_count` does — without it, the
/// only way to know is to try — and a host needs it before submitting: frame
/// indices beyond this count are refused.
///
/// # Safety
///
/// `mesh` must be a live handle from `scg_mesh_load`, and `out` must point to a
/// writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_mesh_frame_count(mesh: *const ScgMesh, out: *mut u32) -> i32 {
    // SAFETY: mêmes préconditions que ci-dessus.
    unsafe { mesh_count(mesh, out, Mesh::frame_count) }
}

/// Writes a frame's bounding box to `out`: the low corner's `x`, `y` and `z`,
/// then the high corner's, in the mesh's own space — a pose is not known here.
/// The centre and half-extents a sweep takes are one sum and one difference
/// away.
///
/// Six floats and not a structure, for the reason `scg_world_entity_pose`
/// gives: a box has no published layout to reuse, and inventing one would
/// freeze it forever for one accessor.
///
/// **Per frame, because no single box is true.** A static mesh has one and the
/// question does not arise; an animated mesh leaves the box of its first frame,
/// while the envelope of every frame covers what none of them fills. Only this
/// direction derives: the envelope comes from these boxes and
/// `scg_mesh_frame_count`, never the other way.
///
/// **This is not a collision test, and there is none against a posed mesh.** A
/// sweep tests a surface's polygon with the edges a map carries classified, and
/// a mesh has neither. What this gives is the extent its author wrote, so a host
/// stops guessing it.
///
/// # Safety
///
/// `mesh` must be a live handle from `scg_mesh_load`, and `out` must cover six
/// writable floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_mesh_bounds(mesh: *const ScgMesh, frame: u32, out: *mut f32) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let mesh = unsafe { mesh.as_ref() }.ok_or(AbiError::NULL)?;
        let [low, high] = mesh.inner.bounds(frame).ok_or(AbiError::FRAME_INDEX)?;
        let corners = [low.x, low.y, low.z, high.x, high.y, high.z];
        // SAFETY: précondition de la fonction — `out` couvre six flottants
        // inscriptibles, et le tableau source ne le recouvre pas.
        unsafe { ptr::copy_nonoverlapping(corners.as_ptr(), out, corners.len()) };
        Ok(())
    })
}

/// Le corps commun des accesseurs scalaires d'un maillage.
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

/// Les trois demi-étendues d'une boîte, refusées si elles sont négatives.
///
/// Séparée de [`read_point`] parce que le domaine n'est pas le même : une
/// coordonnée peut être négative, une demi-étendue non. Nulle est admise — c'est
/// ce qui fait du balayage d'une boîte plate, ou d'un rayon, le même appel.
///
/// **`NaN` se teste par la finitude, avant les comparaisons de bornes**, qui sont
/// fausses dans les deux sens et le laisseraient passer.
///
/// # Safety
///
/// `ptr` doit être nul ou viser trois `float` lisibles.
unsafe fn read_extents(ptr: *const f32) -> Result<Vec3, AbiError> {
    // SAFETY: même précondition, et la finitude y est déjà vérifiée.
    let values = unsafe { read_point(ptr) }?;
    if values.x < 0.0 || values.y < 0.0 || values.z < 0.0 {
        return Err(CoreError::InvalidArgument(Argument::VertexCoordinate).into());
    }
    Ok(values)
}

/// La lecture en deux temps d'un nom, partagée par les deux ressources.
///
/// Extraite dès sa seconde occurrence, contrairement à la règle habituelle : ce
/// n'est pas de la plomberie mais un protocole d'ABI — mesurer, refuser une
/// capacité trop courte sans rien écrire, terminer par un octet nul — et une
/// copie qui divergerait d'un mot ferait mentir le header pour l'une des deux
/// ressources.
///
/// **L'erreur d'absence vient de l'appelant**, et c'est ce qui distingue le
/// protocole de ce qu'il désigne : un emplacement de texture et un matériau
/// sortent d'un rang que la ressource déclare, une entité d'un index que la
/// carte compte. Portée ici, elle parlait d'emplacement à l'hôte qui lisait la
/// classe d'une entité — la règle que `entry.rs` s'écrit déjà pour la trame d'un
/// maillage, distincte par son message seul.
///
/// **Le refus des pointeurs est ici**, et non chez les quatre appelants : il y
/// était recopié mot pour mot, et c'est le genre de ligne qu'on oublie sur le
/// point d'entrée ajouté en dernier. L'ordre des deux refus ne se voit pas d'un
/// hôte, `AbiError::NULL` n'ayant qu'un message.
///
/// # Safety
///
/// `buf` est nul, ou couvre `cap` octets inscriptibles ; `out_len` est nul, ou
/// vise une `size_t` inscriptible. Le nom ne recouvre pas `buf`.
unsafe fn write_name(
    name: Option<&str>,
    absent: AbiError,
    buf: *mut c_char,
    cap: usize,
    out_len: *mut usize,
) -> Result<(), AbiError> {
    if out_len.is_null() || (buf.is_null() && cap != 0) {
        return Err(AbiError::NULL);
    }
    let name = name.ok_or(absent)?;
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
    // SAFETY: précondition de la fonction — le handle est nul, ou vient de
    // `Box::into_raw` dans `scg_world_load` et n'a pas encore été rendu.
    unsafe { destroy(world) };
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
    let submit = |context: &mut Context| {
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

        context
            // SAFETY: précondition de la fonction — chaque entrée du tableau est
            // nulle ou un handle vivant. Le compte ayant été vérifié égal et le
            // rang venant du chargement, il est toujours dans les bornes.
            .submit_world(model, &world.inner, |material| unsafe {
                slot_texture(slots, material)
            })
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, submit) }
}

/// Submits a whole map with its lightmaps, one batch per surface.
///
/// `textures` works exactly as in `scg_submit_world`, and `lighting` exactly as
/// in `scg_submit_world_visible` — null renders what `scg_submit_world` renders,
/// and a cell without an atlas falls back to the unlit path on its own.
///
/// **It exists so the two paths can be compared lit.** Traversal is validated
/// against the brute path: on a level where everything is visible both must
/// render the same image, and that equality is what catches a window narrowed
/// too far. Without this call the brute path would render an unlit level and the
/// traversed one a lit level, so the two would differ for a reason that has
/// nothing to do with what is being compared.
///
/// **`lighting` must come from `world`**, and a holder created from another map
/// returns `SCG_ERR_INVALID_ARGUMENT` before anything is submitted. The holder
/// is sized on the map it was created from, down to the slots of each cell's
/// atlas: an atlas read against a cell that carries one surface more would index
/// past its own slots.
///
/// **The map is submitted whole or not at all**, like `scg_submit_world`.
///
/// # Safety
///
/// Same preconditions as `scg_submit_world`, plus `lighting` being null or a
/// live handle from `scg_lighting_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_submit_world_lit(
    ctx: *mut ScgContext,
    model: *const ScgMat4,
    world: *const ScgWorld,
    textures: *const *const ScgTexture,
    texture_count: u32,
    lighting: *const ScgLighting,
) -> i32 {
    let submit = |context: &mut Context| {
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `world` est un handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — nul, ou un handle vivant rendu
        // par `scg_lighting_create`.
        let lighting = paired(unsafe { lighting.as_ref() }, world)?;
        if textures.is_null() && texture_count != 0 {
            return Err(AbiError::NULL);
        }
        if texture_count != world.inner.material_count() {
            return Err(AbiError::TEXTURE_COUNT);
        }
        // SAFETY: précondition de la fonction — le tableau couvre son nombre
        // d'éléments, le cas vide étant traité par `slice_of`.
        let slots = unsafe { slice_of(textures, texture_count) };

        context
            .submit_world_lit(
                model,
                &world.inner,
                // SAFETY: mêmes préconditions que `scg_submit_world`.
                |material| unsafe { slot_texture(slots, material) },
                lighting,
            )
            .map_err(AbiError::from)
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, submit) }
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
    // SAFETY: précondition de la fonction — le handle est nul, ou vient de
    // `Box::into_raw` dans `scg_lighting_create` et n'a pas encore été repris.
    unsafe { destroy(lighting) };
}

/// Computes the lightmaps of one cell, named by its stable identifier.
///
/// **By identifier and never by rank**: a cache stores identifiers, and editing
/// will name the cell it just changed. An identifier no cell carries is
/// `SCG_ERR_UNKNOWN_RESOURCE`.
///
/// **This allocates and takes time**, so it is a named call, and calling it
/// between the start and the end of a frame is forbidden. That is a precondition
/// and not an error you are told about: the holder belongs to no context, so
/// nothing here can know a frame is open. A cell that changes recomputes its
/// own — but **its immediate neighbours become wrong**, since the light coming
/// through the doorway was computed over there, and it is for the host to ask for
/// them again.
///
/// **It may return `SCG_ERR_INVALID_FORMAT` on a map that loaded cleanly**, when
/// the surfaces of that cell do not fit an atlas of `SCG_MAX_LIGHTMAP_SIZE` a
/// side. Loading bounds one surface at a time; the size of an atlas is only known
/// once its rectangles are packed, which happens here. No element is named — it is
/// the volume of every surface of the cell that overflows, not one of them. A host
/// that changes maps therefore bakes the new one **before** releasing the old.
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
/// **It allocates, like `scg_lighting_build`**, so the same precondition holds:
/// not between the start and the end of a frame, and you are not told about it —
/// the holder belongs to no context.
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
        let block = unsafe { slice_of_bytes(bytes, len) };
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

/// Writes to `out` what would make this lightmap frame be refused, or
/// `SCG_LIGHTMAP_FAULT_NONE`.
///
/// **The loader's own predicate, not a second writing of it.** Loading calls the
/// same code, so the two cannot drift. That is why it is exposed at all: a map
/// generator had to reimplement it — tolerance and Newell sum included — to find
/// which surface a map was refused for, and that copy would have gone silently
/// wrong the day a value moved.
///
/// **It takes no map handle, and that is the point**: a map that was refused
/// never produced one. A host passes the frame and corners it wrote itself, from
/// its own data, which is all it has left at that moment.
///
/// The five clauses are tested in the order loading refuses them, and the first
/// one met is the one reported. `corners` holds `corner_count` triples of
/// `float`, and **three corners are the minimum**: the fifth clause depends on
/// them, and below three it is wrong in both directions — no corner at all makes
/// the span come out of an empty set, inventing a fault on a sound frame, while
/// one or two keep it under the bound and would hide a real one.
///
/// # Safety
///
/// `origin`, `u` and `v` must each point to three readable `float`s, `corners`
/// must point to `corner_count` readable triples, and `out` must point to a
/// writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_lightmap_fault(
    origin: *const f32,
    u: *const f32,
    v: *const f32,
    corners: *const f32,
    corner_count: u32,
    out: *mut u32,
) -> i32 {
    entry::without_context(|| -> Result<(), AbiError> {
        if out.is_null() || corners.is_null() {
            return Err(AbiError::NULL);
        }
        if corner_count < 3 {
            return Err(AbiError::CORNER_COUNT);
        }
        // SAFETY: précondition de la fonction — trois flottants lisibles.
        let origin = unsafe { read_point(origin) }?;
        // SAFETY: idem.
        let axis_u = unsafe { read_point(u) }?;
        // SAFETY: idem.
        let axis_v = unsafe { read_point(v) }?;

        // **Les coins se recopient, et c'est permis ici** : un appel nommé a le
        // droit d'allouer, et celui-ci n'est pas dans une image. Ce qui ne se
        // fait pas est de réinterpréter la tranche de flottants en tranche de
        // `Vec3` — la disposition du noyau ne nous appartient pas. Le lecteur
        // par point porte en outre le refus du non fini, que le noyau suppose
        // écarté : sa documentation dit qu'au-delà une faute est rendue, mais
        // pas laquelle.
        // **`try_reserve_exact` et non `Vec::with_capacity`**, pour la raison
        // que `buffer::reserved` écrit côté noyau : le compte vient de l'hôte et
        // le contrat ne le borne pas, donc un échec d'allocation est atteignable
        // — et `with_capacity` y passe par `handle_alloc_error`, qui **avorte le
        // processus de l'hôte**. Un `abort` n'est pas un dépliage, donc
        // `catch_unwind` ne le voit pas : la panique ne traverse pas la
        // frontière, le processus meurt avant.
        //
        // C'est la classe que le noyau avait fermée sans qu'elle le soit ici, et
        // `alloc-verif` la ferme désormais pour toute la frontière.
        let mut points: Vec<Vec3> = Vec::new();
        points
            .try_reserve_exact(corner_count as usize)
            .map_err(|_| AbiError::from(CoreError::OutOfMemory))?;
        for index in 0..corner_count as usize {
            // SAFETY: précondition — `corner_count` triplets lisibles d'affilée.
            points.push(unsafe { read_point(corners.add(index * 3)) }?);
        }

        let code = match lightmap_fault(origin, axis_u, axis_v, &points) {
            None => status::SCG_LIGHTMAP_FAULT_NONE,
            Some(LightmapFault::DegenerateAxis) => status::SCG_LIGHTMAP_FAULT_DEGENERATE_AXIS,
            Some(LightmapFault::OriginOffGrid) => status::SCG_LIGHTMAP_FAULT_ORIGIN_OFF_GRID,
            Some(LightmapFault::SkewedAxes) => status::SCG_LIGHTMAP_FAULT_SKEWED_AXES,
            Some(LightmapFault::AxisOffPlane) => status::SCG_LIGHTMAP_FAULT_AXIS_OFF_PLANE,
            Some(LightmapFault::ExtentTooLarge) => status::SCG_LIGHTMAP_FAULT_EXTENT_TOO_LARGE,
        };

        // SAFETY: précondition — `out` vise un `uint32_t` inscriptible.
        unsafe { out.write(code) };
        Ok(())
    })
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

/// Writes how many luxels baking `cell_id` computes to `out`.
///
/// **What a bake costs, cell by cell.** `scg_lighting_build` is synchronous and
/// takes one cell, so a host that shows progress needs to weight each one: the
/// number of cells says nothing, a twenty-metre wall and a doorstep counting one
/// each. The value is derived at load, so asking for it costs nothing.
///
/// Gutters are excluded, and so are the surfaces that refuse a lightmap — the
/// bake does not visit them. An unknown identifier is `SCG_ERR_UNKNOWN_RESOURCE`
/// and writes nothing.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to a
/// writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_cell_luxel_count(
    world: *const ScgWorld,
    cell_id: u32,
    out: *mut u32,
) -> i32 {
    // L'erreur du noyau, et non un code propre à la frontière : une cellule
    // inconnue est la même faute ici que pour `scg_lighting_build`, qui la rend
    // par ce chemin.
    let read = |world: &ScgWorld| {
        world
            .inner
            .cell_luxel_count(cell_id)
            .ok_or_else(|| AbiError::from(CoreError::UnknownResource))
    };
    // SAFETY: mêmes préconditions que les autres accesseurs de la carte.
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
/// over. **Four things write `0`, and the list is closed**: leaving through a
/// wall, leaving through an unlinked portal, a move so long it crosses more cells
/// than this unfolds, and a `from_cell` that designates no cell — there is no
/// thread to follow from a cell that does not exist. The host's response is the
/// same for all four, so no code separates them.
///
/// **An identifier belongs to no map.** One taken from another map, where it names
/// a cell that exists, is neither rejected nor flagged: the cell is a precondition
/// here, and the engine cannot tell it apart from a correct one. A host keeps the
/// current cell in its game state and drops it when it changes maps.
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
/// **But it must come from `world`**: a holder created from another map returns
/// `SCG_ERR_INVALID_ARGUMENT` before anything is submitted. Falling back to the
/// unlit path there would render a plausible image from a mismatched map, which
/// is the one thing this clause does not do — it degrades for a cell that has
/// **nothing** computed, never for a holder that belongs elsewhere.
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
    let submit = |context: &mut Context| {
        // SAFETY: précondition de la fonction — chaque pointeur est nul ou vise
        // une valeur lisible.
        let model = unsafe { model.as_ref() }.ok_or(AbiError::NULL)?;
        let model = model.to_core()?;
        // SAFETY: précondition de la fonction — `world` est un handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — nul, ou un handle vivant rendu par
        // `scg_lighting_create`.
        let lighting = paired(unsafe { lighting.as_ref() }, world)?;
        if textures.is_null() && texture_count != 0 {
            return Err(AbiError::NULL);
        }
        if texture_count != world.inner.material_count() {
            return Err(AbiError::TEXTURE_COUNT);
        }
        // SAFETY: précondition de la fonction — le tableau couvre son nombre
        // d'éléments, le cas vide étant traité par `slice_of`.
        let slots = unsafe { slice_of(textures, texture_count) };

        let seen = context
            .submit_world_visible(
                model,
                &world.inner,
                cell_id,
                lighting,
                // SAFETY: mêmes préconditions que `scg_submit_world`.
                |material| unsafe { slot_texture(slots, material) },
            )
            .map_err(AbiError::from)?;

        Ok(match seen {
            Visibility::Complete => status::SCG_OK,
            Visibility::Incomplete => status::SCG_STATUS_INCOMPLETE,
            Visibility::NoCell => status::SCG_STATUS_NO_CELL,
        })
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant.
    unsafe { entry::with_exclusive(ctx, submit) }
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

/// Writes the stable identifier of one static light to `out`.
///
/// **A separate call rather than a field on `ScgLight`.** That structure crosses
/// the boundary **by value**: adding a field would move the offsets of a
/// published structure, which breaks the ABI. The identifier therefore comes by
/// rank — the same `index` `scg_world_light` takes.
///
/// Without it, a host that wrote the map can only match a light it reads back to
/// the one it placed by relying on write order, which nothing guarantees. An
/// `index` beyond `scg_world_light_count` returns `SCG_ERR_INVALID_ARGUMENT` and
/// writes nothing.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to
/// a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_light_id(
    world: *const ScgWorld,
    index: u32,
    out: *mut u32,
) -> i32 {
    let read = |world: &ScgWorld| world.inner.light_id(index).ok_or(AbiError::WORLD_INDEX);
    // SAFETY: mêmes préconditions que les autres accesseurs indexés.
    unsafe { world_value(world, out, read) }
}

/// Writes a material's stable identifier to `out`, by rank.
///
/// **The rank is the one `scg_world_material_name` takes**, and this is the
/// argument of `scg_world_light_id` transposed: without it, a host that wrote
/// the map can only match a material it reads back to the one it placed by
/// relying on write order, which nothing guarantees.
///
/// The gap was plainer here than for a light, because what
/// `scg_world_surface_material` returns **is** a rank: the table carried the
/// identifiers, the loader checked them unique, and nothing returned them. An
/// `index` beyond `scg_world_material_count` returns
/// `SCG_ERR_INVALID_ARGUMENT` and writes nothing.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to
/// a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_material_id(
    world: *const ScgWorld,
    index: u32,
    out: *mut u32,
) -> i32 {
    let read = |world: &ScgWorld| world.inner.material_id(index).ok_or(AbiError::SLOT_RANGE);
    // SAFETY: mêmes préconditions que les autres accesseurs indexés.
    unsafe { world_value(world, out, read) }
}

/// Sweeps an axis-aligned box from `from` to `to`, starting in `from_cell`.
///
/// **The collision module needs no rendering context**, which is the whole point:
/// a game server loads a map, sweeps, and never allocates a frame buffer. It
/// takes no context and writes its error to the thread-local slot, read with
/// `scg_last_error(NULL)`.
///
/// **It is callable from any thread, on the same handle, at the same time.** A
/// loaded map is immutable and this only reads it; nothing is retained between
/// calls, and the result depends on the arguments alone. This is the first entry
/// point in the ABI to allow that on a shared object.
///
/// **Returns a positive status, not only `SCG_OK`.** Judge the result by the sign
/// of the code: `SCG_STATUS_NO_CELL` when `from_cell` is `0`, which means
/// "nowhere" and reports a free move; `SCG_STATUS_INCOMPLETE` when the sweep hit
/// `SCG_SWEEP_CELLS`, which **truncates** the move rather than reporting it free;
/// `SCG_STATUS_START_SOLID` when the box was already inside solid geometry; and
/// `SCG_STATUS_NO_GAP` when the box is too small, where it moves, to keep any gap.
/// That last one is reported only while nothing more pressing applies — ask
/// `scg_sweep_reach` at load time rather than waiting for it.
///
/// `half_extents` of zero sweeps a ray. The box never rotates: give one that
/// encloses every orientation your character takes.
///
/// **What stops the box is the map, not a filter you pass.** Surfaces flagged
/// non-solid are ignored, and a collision layer mask is a game notion that
/// belongs on your side.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, `half_extents`, `from`
/// and `to` must each point to three readable `float`s, and `out` must point to a
/// writable `ScgSweepHit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_sweep(
    world: *const ScgWorld,
    from_cell: u32,
    half_extents: *const f32,
    from: *const f32,
    to: *const f32,
    out: *mut ScgSweepHit,
) -> i32 {
    // Le type de retour est annoté : cette enveloppe accepte aussi bien `()` que
    // le code d'un statut, et sans annotation l'inférence choisit le premier.
    entry::without_context(|| -> Result<i32, AbiError> {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — trois flottants lisibles, et
        // chacun doit être fini et positif ou nul.
        let half = unsafe { read_extents(half_extents) }?;
        // SAFETY: précondition de la fonction — trois flottants lisibles.
        let start = unsafe { read_point(from) }?;
        // SAFETY: idem.
        let end = unsafe { read_point(to) }?;

        // **Le départage vient du noyau**, par `World::sweep_checked` : « nulle
        // part » contre « cette cellule n'existe pas » est la distinction que
        // `Sweep` porte, et elle vivait ici en second exemplaire — la
        // documentation de `sweep_checked` dit elle-même qu'une seconde copie
        // finirait par diverger sur le cas ajouté en dernier.
        let hit = match world.inner.sweep_checked(from_cell, half, start, end)? {
            Sweep::Reached(hit) => hit,
            // Rien n'a été examiné, et inventer un mur serait inventer de la
            // géométrie : le déplacement se rend libre jusqu'à son arrivée.
            Sweep::NoCell => {
                // SAFETY: précondition — `out` vise une `ScgSweepHit` inscriptible.
                unsafe { out.write(ScgSweepHit::free(end)) };
                return Ok(status::SCG_STATUS_NO_CELL);
            }
        };

        // SAFETY: précondition — `out` vise une `ScgSweepHit` inscriptible, et
        // rien n'y a été écrit avant ce point.
        unsafe { out.write(ScgSweepHit::from_core(&hit)) };

        // **Le plus actionnable quand plusieurs s'appliquent** : un départ dans le
        // solide demande à l'hôte de se dégager, là où une région tronquée ne lui
        // laisse aucun levier — la borne n'étant pas réglable.
        //
        // Le jeu perdu vient en dernier, et il est **masqué dans le cas qu'il
        // décrit** : une boîte sans jeu se repose dans le solide, donc le balayage
        // suivant rend le départ solide. Ce n'est pas un défaut de cet ordre, c'est
        // ce qui rend `scg_sweep_reach` nécessaire — un état ne se publie pas par
        // un canal qui ne transporte qu'un événement à la fois.
        Ok(if hit.start_solid {
            status::SCG_STATUS_START_SOLID
        } else if hit.incomplete {
            status::SCG_STATUS_INCOMPLETE
        } else if hit.no_gap {
            status::SCG_STATUS_NO_GAP
        } else {
            status::SCG_OK
        })
    })
}

/// Writes to `out` the same sweep, against **every** cell and without traversal.
///
/// **Half of an oracle, not a production path.** On a well-formed map this and
/// `scg_world_sweep` return the same bits, and that equality is the only check
/// that catches a traversal window grown too narrow — an integrator who suspects
/// one compares the two on their own map, which is what this project's
/// conformance suite does on every run.
///
/// It takes **no starting cell**, and that is what makes it a reference: knowing
/// none, it cannot pick the wrong one. So it never returns `SCG_STATUS_NO_CELL`,
/// and never `SCG_STATUS_INCOMPLETE` either — examining every cell, it has no
/// budget to exhaust. `SCG_STATUS_START_SOLID` and `SCG_STATUS_NO_GAP` keep the
/// meaning and the order they have on `scg_world_sweep`.
///
/// **It costs every surface of every cell**, where traversal stops at the first
/// contact: calling it once per frame on a large map is not what it is for.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, `half_extents`, `from`
/// and `to` must each point to three readable `float`s, and `out` must point to a
/// writable `ScgSweepHit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_sweep_brute(
    world: *const ScgWorld,
    half_extents: *const f32,
    from: *const f32,
    to: *const f32,
    out: *mut ScgSweepHit,
) -> i32 {
    entry::without_context(|| -> Result<i32, AbiError> {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — trois flottants lisibles, et
        // chacun doit être fini et positif ou nul.
        let half = unsafe { read_extents(half_extents) }?;
        // SAFETY: précondition de la fonction — trois flottants lisibles.
        let start = unsafe { read_point(from) }?;
        // SAFETY: idem.
        let end = unsafe { read_point(to) }?;

        // **Aucune cellule à connaître, donc aucune à manquer**, et c'est tout
        // ce qui distingue cet appel du précédent : il n'y a ni cellule nulle à
        // traiter avant le noyau, ni identifiant inconnu à refuser, ni budget de
        // cellules à épuiser. Les deux statuts qui restent sont ceux qui
        // décrivent la boîte, non la région examinée.
        let hit = world.inner.sweep_brute(half, start, end);

        // SAFETY: précondition — `out` vise une `ScgSweepHit` inscriptible, et
        // rien n'y a été écrit avant ce point.
        unsafe { out.write(ScgSweepHit::from_core(&hit)) };

        Ok(if hit.start_solid {
            status::SCG_STATUS_START_SOLID
        } else if hit.no_gap {
            status::SCG_STATUS_NO_GAP
        } else {
            status::SCG_OK
        })
    })
}

/// Writes to `out` how much a sweep grows a box of these half extents.
///
/// **The residual gap a host cannot guess.** A sweep places the box just short of
/// contact, never on it, and a host writing a "am I standing on ground" probe
/// needs the order of magnitude of that gap to choose its length and threshold.
///
/// A function rather than a constant, because what gets copied wrong is not the
/// value but the rule: the factor applies to the **largest** half extent, not to
/// each of them, so that a flat box — a disc, a blade — does not keep a zero
/// thickness that nothing would ever separate from the floor.
///
/// What it writes is the margin **along each axis**. The real gap along a surface
/// normal is that margin times the sum of the absolute values of that normal: one
/// margin on an axis-aligned floor, √2 on a 45° ramp, up to √3 in a corner — which
/// is why a body floats a little further above a slope than above a flat floor.
///
/// It is the margin of a **face**. The rim of a panel — the edge that ends it, the
/// corner where it stops — stops half a margin later, which is what lets a body
/// resting against a wall move along it instead of catching on the end of the very
/// panel it follows.
///
/// **A box too small for where it stands keeps no gap at all.** Re-placing a body
/// at the returned fraction goes through `float` positions, whose step grows with
/// the coordinate, and once that step covers the gap the body lands inside the
/// solid. Speed does not enter, only smallness does — a vehicle is safer than a
/// walker. Ask `scg_sweep_reach` how far your box may go rather than reproducing
/// the threshold: a copied threshold is wrong on exactly the cases it decides.
///
/// The value is not part of the contract and may change between versions; what it
/// means does not. Call it rather than caching it.
///
/// # Safety
///
/// `half_extents` must point to three readable floats and `out` to a writable
/// `float`. Their **values** are not a precondition: a half extent that is
/// negative or not finite is checked and refused with
/// `SCG_ERR_INVALID_ARGUMENT`, where breaking the preconditions above is
/// undefined behaviour — a binding must not confuse the two.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_sweep_skin(half_extents: *const f32, out: *mut f32) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — trois flottants lisibles, et
        // chacun doit être fini et positif ou nul. Le même lecteur que le
        // balayage, pour que les deux refusent exactement les mêmes entrées.
        let half = unsafe { read_extents(half_extents) }?;
        // SAFETY: précondition — `out` vise un `float` inscriptible.
        unsafe { out.write(screengine::sweep_skin(half)) };
        Ok(())
    })
}

/// Writes to `out` how far from the origin a box of these half extents keeps a gap.
///
/// **Ask this once, at load time, and compare it to how far your level reaches.**
/// A sweep leaves the box a half margin short of the solid; putting it back at the
/// returned fraction goes through `float` positions, whose step is `|p| * 2^-24`.
/// Past this distance that step covers the gap, the body lands inside the solid,
/// and the next sweep starts penetrating. Widening the box or moving the level
/// closer to the origin is the fix, and both are decisions a host makes early.
///
/// **A value, not a predicate on a position.** What a host compares this to is the
/// extent of its level, which it knows from generating it — so the question is
/// asked once, before a map even exists. A predicate would have to be called per
/// body and per frame to learn the same thing.
///
/// **The gap holds strictly below this distance**: at that coordinate it is already
/// lost. Measured, and in half extents, since that is what you pass: one unit
/// reaches eight thousand units, five centimetres — a ten-centimetre projectile —
/// four hundred.
///
/// `SCG_STATUS_NO_GAP` reports the same condition per call, but it is the last of
/// the four statuses: a box in that state lands in the solid and is then reported
/// as `SCG_STATUS_START_SOLID`, which masks it. This function is the one that
/// answers in time.
///
/// A ray — zero half extents — writes zero: carrying no gap, it has no distance at
/// which to lose one.
///
/// The value is not part of the contract and may change between versions; what it
/// means does not. Call it rather than caching it.
///
/// # Safety
///
/// `half_extents` must point to three readable floats and `out` to a writable
/// `float`. Their **values** are checked, not assumed: see `scg_sweep_skin`,
/// which refuses the same domain with the same code.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_sweep_reach(half_extents: *const f32, out: *mut f32) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — trois flottants lisibles, et
        // chacun doit être fini et positif ou nul. Le même lecteur que le
        // balayage et que la marge, pour que les trois refusent les mêmes entrées.
        let half = unsafe { read_extents(half_extents) }?;
        // SAFETY: précondition — `out` vise un `float` inscriptible.
        unsafe { out.write(screengine::sweep_reach(half)) };
        Ok(())
    })
}

/// Writes to `out` the rotation of `angle` about `axis`.
///
/// **The angle is binary, not radians: a full turn is 2³², so one unit is a
/// turn divided by 2³².** Converting degrees or radians is the caller's step,
/// and that is the point — the conversion from a float to this integer
/// saturates differently on a scalar path and on SSE, so the engine keeps it
/// out of its own arithmetic. A host that animates a pose should carry the
/// binary angle and add to it, where wrapping is exact and free, rather than
/// accumulate radians and convert each frame.
///
/// **This is what makes a rotation reproducible across languages.** Computing
/// it with the host's own `sin` and `cos` does not give the same bits twice:
/// the engine's sine comes from its own tables, and the whole point of the
/// bit-exact guarantee is that two bindings render the same image. Every
/// function here delegates to the engine and adds nothing.
///
/// `axis` need not be unit; it is normalised. A zero or degenerate axis yields
/// the identity rather than an error: there is no rotation to describe, and
/// refusing would make a host branch on a case it cannot always foresee.
///
/// # Safety
///
/// `axis` must point to three readable floats and `out` to a writable
/// `ScgQuat`. Their **values** are checked, not assumed: a coordinate that is
/// not finite is refused with `SCG_ERR_INVALID_ARGUMENT`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_quat_from_axis_angle(
    axis: *const f32,
    angle: u32,
    out: *mut ScgQuat,
) -> i32 {
    entry::without_context(|| {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — trois flottants lisibles, dont
        // le fini est vérifié. Le même lecteur que les positions, l'axe étant
        // une direction de monde et non une étendue.
        let axis = unsafe { read_point(axis) }?;
        let q = Quat::from_axis_angle(axis, Angle(angle));
        // SAFETY: précondition — `out` vise un `ScgQuat` inscriptible.
        unsafe { out.write(ScgQuat::from_core(q)) };
        Ok(())
    })
}

/// Writes to `out` the rotation `a` followed by the rotation `b`.
///
/// The order is the one a reader expects of a composition of poses, not the
/// algebraic one: applying the result turns by `a` first. Neither argument need
/// be unit, and the result is not normalised — compose freely, the engine
/// normalises where it consumes a pose.
///
/// # Safety
///
/// `a`, `b` and `out` must each point to a readable — writable for `out` —
/// `ScgQuat`. A component that is not finite is refused with
/// `SCG_ERR_INVALID_ARGUMENT`. `out` may alias `a` or `b`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_quat_product(
    a: *const ScgQuat,
    b: *const ScgQuat,
    out: *mut ScgQuat,
) -> i32 {
    entry::without_context(|| {
        if a.is_null() || b.is_null() || out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — deux `ScgQuat` lisibles. Les
        // deux lectures précèdent l'écriture, donc `out` peut viser l'un d'eux.
        let (a, b) = unsafe { (a.read(), b.read()) };
        let product = b.to_core()?.product(a.to_core()?);
        // SAFETY: précondition — `out` vise un `ScgQuat` inscriptible.
        unsafe { out.write(ScgQuat::from_core(product)) };
        Ok(())
    })
}

/// Writes to `out` the model matrix of a rotation and a translation.
///
/// This is the matrix a submission takes, built the way the engine builds it.
/// Writing it by hand is where a binding's own trigonometry creeps back in,
/// and where two languages stop rendering the same image.
///
/// The quaternion need not be unit; it is normalised here. The last row of the
/// result is exactly `0, 0, 0, 1`, which is what a submission requires.
///
/// # Safety
///
/// `q` must point to a readable `ScgQuat`, `translation` to three readable
/// floats, and `out` to a writable `ScgMat4`. Values that are not finite are
/// refused with `SCG_ERR_INVALID_ARGUMENT`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_mat4_from_rotation_translation(
    q: *const ScgQuat,
    translation: *const f32,
    out: *mut ScgMat4,
) -> i32 {
    entry::without_context(|| {
        if q.is_null() || out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — un `ScgQuat` lisible.
        let rotation = unsafe { q.read() }.to_core()?;
        // SAFETY: précondition — trois flottants lisibles, dont le fini est
        // vérifié.
        let t = unsafe { read_point(translation) }?;
        let m = Affine3::from_rotation_translation(rotation, t);
        // SAFETY: précondition — `out` vise un `ScgMat4` inscriptible.
        unsafe { out.write(ScgMat4::from_core(m)) };
        Ok(())
    })
}

/// Writes to `out` the matrix `a` followed by the matrix `b`.
///
/// Same reading order as `scg_quat_product`: a point goes through `a` first.
/// This is how a host composes a placement with a local transform without
/// multiplying matrices itself — four lines of host code that two languages
/// would not agree on at the last bit.
///
/// **Neither is the view matrix.** The engine composes its camera itself, and a
/// matrix passed to a submission is the model matrix alone; folding a view into
/// it would be treated as a rigid transform and silently misrender.
///
/// # Safety
///
/// `a`, `b` and `out` must each point to a readable — writable for `out` —
/// `ScgMat4`. A matrix that is not finite, or whose last row is not exactly
/// `0, 0, 0, 1`, is refused with `SCG_ERR_INVALID_ARGUMENT`. `out` may alias
/// `a` or `b`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_mat4_product(
    a: *const ScgMat4,
    b: *const ScgMat4,
    out: *mut ScgMat4,
) -> i32 {
    entry::without_context(|| {
        if a.is_null() || b.is_null() || out.is_null() {
            return Err(AbiError::NULL);
        }
        // SAFETY: précondition de la fonction — deux `ScgMat4` lisibles. Les
        // deux lectures précèdent l'écriture, donc `out` peut viser l'un d'eux.
        let (a, b) = unsafe { (a.read(), b.read()) };
        let product = b.to_core()?.product(a.to_core()?);
        // SAFETY: précondition — `out` vise un `ScgMat4` inscriptible.
        unsafe { out.write(ScgMat4::from_core(product)) };
        Ok(())
    })
}

/// Picks the scene with a ray from `from` to `to`, starting in `from_cell`.
///
/// **This is the sweep of a zero-extent box**, and one function for both: a ray
/// is that case, and two paths would have been the same code to validate against
/// itself. It fills the same `ScgSweepHit`, with the same rules — judge the result
/// by the sign of the code. It carries three of the sweep's four statuses, never
/// `SCG_STATUS_NO_GAP`: a ray is not grown, so it has no gap to lose.
///
/// **The safety margin is relative to the half-extent, so a ray carries none.**
/// It hits what it crosses, never what it grazes: picking must land on the
/// surface the user aimed at, not on its neighbour.
///
/// `filter` must be `SCG_PICK_SOLID` or `SCG_PICK_ALL`; zero and any unknown
/// value are refused, never silently mapped onto a default. `SCG_PICK_ALL` also
/// sees surfaces flagged **non-solid** — a grate, a pane, a trigger volume —
/// which collision passes through by construction and which an editor must be
/// able to select.
///
/// **It takes no context and is callable from any thread**, like
/// `scg_world_sweep` and for the same reason: a loaded map is immutable and this
/// only reads it. Its error is read with `scg_last_error(NULL)`.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, `from` and `to` must
/// each point to three readable `float`s, and `out` must point to a writable
/// `ScgSweepHit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_pick(
    world: *const ScgWorld,
    from_cell: u32,
    from: *const f32,
    to: *const f32,
    filter: u32,
    out: *mut ScgSweepHit,
) -> i32 {
    // Annoté comme le balayage : sans cela l'inférence choisit `()` et le statut
    // ne remonterait pas.
    entry::without_context(|| -> Result<i32, AbiError> {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        let surfaces = pick_filter(filter)?;
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — trois flottants lisibles.
        let start = unsafe { read_point(from) }?;
        // SAFETY: idem.
        let end = unsafe { read_point(to) }?;

        // Le départage vient du noyau, exactement comme pour le balayage, et par
        // le site qu'il partage avec lui.
        let hit = match world.inner.pick_checked(from_cell, start, end, surfaces)? {
            Sweep::Reached(hit) => hit,
            Sweep::NoCell => {
                // SAFETY: précondition — `out` vise une `ScgSweepHit` inscriptible.
                unsafe { out.write(ScgSweepHit::free(end)) };
                return Ok(status::SCG_STATUS_NO_CELL);
            }
        };

        // SAFETY: précondition — `out` vise une `ScgSweepHit` inscriptible, et
        // rien n'y a été écrit avant ce point.
        unsafe { out.write(ScgSweepHit::from_core(&hit)) };

        // Le même ordre de priorité que le balayage, et il est contractuel. Le
        // jeu perdu n'y figure pas, et c'est une propriété du rayon : sa
        // dilatation est nulle, donc le noyau ne lève jamais ce drapeau ici.
        Ok(if hit.start_solid {
            status::SCG_STATUS_START_SOLID
        } else if hit.incomplete {
            status::SCG_STATUS_INCOMPLETE
        } else {
            status::SCG_OK
        })
    })
}

/// Writes to `out` the same ray, against **every** cell and without traversal.
///
/// **The other half of the oracle**, and the counterpart of
/// `scg_world_sweep_brute` for picking: on a well-formed map this and
/// `scg_world_pick` return the same bits, and that equality is what catches a
/// traversal window grown too narrow.
///
/// It takes **no starting cell**, so it never returns `SCG_STATUS_NO_CELL`, nor
/// `SCG_STATUS_INCOMPLETE` — examining every cell, it has no budget to exhaust.
/// `SCG_STATUS_START_SOLID` is therefore the only status it reports: a ray is
/// never grown, so `SCG_STATUS_NO_GAP` is as out of reach here as it is on
/// `scg_world_pick`.
///
/// `filter` takes the same two values and is refused the same way, and the cost
/// is every surface of every cell — not a path to call once per frame.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, `from` and `to` must each
/// point to three readable `float`s, and `out` must point to a writable
/// `ScgSweepHit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_pick_brute(
    world: *const ScgWorld,
    from: *const f32,
    to: *const f32,
    filter: u32,
    out: *mut ScgSweepHit,
) -> i32 {
    // Annoté comme les trois autres : sans cela l'inférence choisit `()`.
    entry::without_context(|| -> Result<i32, AbiError> {
        if out.is_null() {
            return Err(AbiError::NULL);
        }
        let surfaces = pick_filter(filter)?;
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: précondition de la fonction — trois flottants lisibles.
        let start = unsafe { read_point(from) }?;
        // SAFETY: idem.
        let end = unsafe { read_point(to) }?;

        // Le rayon brut, sans cellule de départ : voir `scg_world_sweep_brute`
        // pour ce que cette absence retire — ici elle laisse le seul statut qui
        // décrive le point de départ et non la région examinée.
        let hit = world.inner.pick_brute(start, end, surfaces);

        // SAFETY: précondition — `out` vise une `ScgSweepHit` inscriptible, et
        // rien n'y a été écrit avant ce point.
        unsafe { out.write(ScgSweepHit::from_core(&hit)) };

        Ok(if hit.start_solid {
            status::SCG_STATUS_START_SOLID
        } else {
            status::SCG_OK
        })
    })
}

/// Writes the material of a surface to `out`, by its stable identifier.
///
/// The counterpart of `ScgSweepHit::surface_id`: without it that field would be
/// an identifier no function translates. What it writes indexes the map's
/// material table, which `scg_world_material_name` names.
///
/// An identifier no surface carries is `SCG_ERR_UNKNOWN_RESOURCE`.
///
/// # Safety
///
/// `world` must be a live handle from `scg_world_load`, and `out` must point to a
/// writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_world_surface_material(
    world: *const ScgWorld,
    surface_id: u32,
    out: *mut u32,
) -> i32 {
    let read = |world: &ScgWorld| {
        world
            .inner
            .surface_material(surface_id)
            .ok_or_else(|| AbiError::from(CoreError::UnknownResource))
    };
    // SAFETY: mêmes préconditions que les autres accesseurs de la carte.
    unsafe { world_value(world, out, read) }
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
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: mêmes préconditions que celles de `write_name`.
        unsafe {
            write_name(
                world.inner.entity_class(index),
                AbiError::WORLD_INDEX,
                buf,
                cap,
                out_len,
            )
        }
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
                return Err(AbiError::BYTES_CAPACITY);
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
        // SAFETY: précondition de la fonction — le pointeur est nul ou vise un
        // handle vivant.
        let world = unsafe { world.as_ref() }.ok_or(AbiError::NULL)?;
        // SAFETY: mêmes préconditions que celles de `write_name`, que cet appel
        // porte telles quelles.
        unsafe {
            write_name(
                world.inner.material_name(index),
                AbiError::SLOT_RANGE,
                buf,
                cap,
                out_len,
            )
        }
    })
}
