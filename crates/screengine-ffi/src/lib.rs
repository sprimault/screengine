// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

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
mod status;

use std::alloc::{self, Layout};
use std::ffi::c_char;
use std::ptr;
use std::slice;

use screengine::{BYTES_PER_PIXEL, Context, Error};

use entry::AbiError;

pub use context::{ScgContext, ScgContextConfig};
pub use status::{
    SCG_ERR_INVALID_ARGUMENT, SCG_ERR_INVALID_STATE, SCG_ERR_NULL, SCG_ERR_OUT_OF_MEMORY,
    SCG_ERR_PANIC, SCG_ERR_POISONED, SCG_OK,
};

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
/// case. Destroying a poisoned context is allowed.
///
/// # Safety
///
/// `ctx` is NULL, or a handle returned by `scg_create` and not yet destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_destroy(ctx: *mut ScgContext) {
    if ctx.is_null() {
        return;
    }
    // SAFETY: précondition de la fonction — `ctx` vient de `Box::into_raw` dans
    // `scg_create` et n'a pas encore été rendu. C'est le seul endroit du crate
    // qui reprend cette allocation.
    drop(unsafe { Box::from_raw(ctx) });
}

/// Ends the frame and writes the result into the host buffer.
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
    let render = |core: &mut Context| {
        if pixels.is_null() {
            return Err(AbiError::NULL);
        }

        let (_, height) = core.resolution();
        // Avant la tranche, pas après : un produit qui déborde donnerait une
        // longueur repliée, et une tranche plus courte que le tampon qu'elle
        // décrit est un accès hors limites en puissance.
        let len = (stride as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(BYTES_PER_PIXEL))
            .ok_or(AbiError::from(Error::InvalidArgument))?;

        // SAFETY: précondition documentée dans le header — l'appelant garantit
        // `stride × hauteur` pixels de quatre octets accessibles en écriture,
        // et le moteur n'en conserve rien au retour.
        let buffer = unsafe { slice::from_raw_parts_mut(pixels, len) };
        core.frame_end(buffer, stride)?;
        Ok(())
    };

    // SAFETY: précondition de la fonction — `ctx` est nul ou un handle vivant,
    // utilisé par ce seul thread pendant l'appel.
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
/// Allowed on a poisoned context, so the host can learn the cause.
///
/// # Safety
///
/// `ctx` is NULL, or a handle returned by `scg_create` and not yet destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scg_last_error(ctx: *const ScgContext) -> *const c_char {
    // SAFETY: précondition de la fonction — `ctx` est nul ou valide. Cette
    // fonction ne passe pas par l'enveloppe : elle ne peut pas paniquer, et
    // elle doit rester permise sur un objet empoisonné.
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
    let Ok(layout) = Layout::from_size_align(len, SCG_BUFFER_ALIGNMENT) else {
        return ptr::null_mut();
    };
    if layout.size() == 0 {
        return ptr::null_mut();
    }
    // SAFETY: la taille est non nulle, ce que `alloc` exige. Un échec rend le
    // pointeur nul, que cette fonction transmet tel quel.
    unsafe { alloc::alloc(layout) }
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
    if ptr.is_null() {
        return;
    }
    let Ok(layout) = Layout::from_size_align(len, SCG_BUFFER_ALIGNMENT) else {
        return;
    };
    if layout.size() == 0 {
        return;
    }
    // SAFETY: précondition de la fonction — `ptr` vient de `scg_buffer_alloc`
    // avec cette même longueur, et l'alignement est une constante de l'ABI, ce
    // qui reconstruit à l'identique la description de l'allocation.
    unsafe { alloc::dealloc(ptr, layout) }
}
