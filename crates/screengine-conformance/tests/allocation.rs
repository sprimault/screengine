// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Zéro allocation par image, prouvé plutôt que relu.
//!
//! Un allocateur global compte les allocations du thread qui l'a armé. Le
//! contexte se crée librement — c'est un appel nommé —, puis aucune image ne
//! doit allouer, la première comprise : c'est là qu'un mipmap généré à la
//! demande ou un atlas agrandi se cacherait.
//!
//! Un binaire de test à part, parce qu'un allocateur global vaut pour tout le
//! binaire. Le compte est par thread, parce que le harnais de test alloue sur
//! les siens pendant que le test tourne.
//!
//! C'est le seul `unsafe` de la conformance : `GlobalAlloc` est un trait
//! `unsafe`, et il n'existe pas d'autre façon de voir une allocation.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use screengine::{BYTES_PER_PIXEL, Config, Context};

/// L'allocateur du système, qui compte au passage.
struct Counting;

thread_local! {
    /// Vrai pendant la mesure, sur le thread qui mesure.
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// Les allocations vues depuis que le thread a armé la mesure.
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Note une allocation si la mesure est armée sur ce thread.
///
/// `try_with` : pendant la destruction du stockage local d'un thread, l'accès
/// échouerait, et l'allocateur ne doit jamais paniquer.
fn note() {
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            COUNT.with(|count| count.set(count.get() + 1));
        }
    });
}

// SAFETY: chaque méthode délègue à `System` avec les arguments reçus, dont les
// préconditions sont celles de `GlobalAlloc` que l'appelant tient déjà ; le
// compte n'alloue rien et ne touche pas aux blocs.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: précondition de `GlobalAlloc::alloc`, transmise telle quelle.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: précondition de `GlobalAlloc::alloc_zeroed`, transmise telle quelle.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: précondition de `GlobalAlloc::realloc`, transmise telle quelle.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: précondition de `GlobalAlloc::dealloc`, transmise telle quelle.
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// L'allocateur de ce binaire de test.
#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Compte les allocations de `f` sur le thread courant.
fn allocations(f: impl FnOnce()) -> usize {
    COUNT.with(|count| count.set(0));
    ARMED.with(|armed| armed.set(true));
    f();
    ARMED.with(|armed| armed.set(false));
    COUNT.with(Cell::get)
}

/// Le compteur voit une allocation : sans ce témoin, le test suivant passerait
/// aussi avec un allocateur qui ne compte rien.
#[test]
fn le_compteur_voit_une_allocation() {
    let seen = allocations(|| {
        std::hint::black_box(Vec::<u8>::with_capacity(64));
    });
    assert!(seen >= 1, "aucune allocation vue");
}

/// Aucune image n'alloue, la première comprise, une fois le contexte créé et le
/// tampon de l'hôte fourni.
#[test]
fn aucune_image_n_alloue() {
    let (width, height) = (640, 360);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    let seen = allocations(|| {
        for _ in 0..3 {
            context.frame_end(&mut pixels, width).expect("image rendue");
        }
    });
    assert_eq!(seen, 0, "{seen} allocation(s) pendant trois images");
}
