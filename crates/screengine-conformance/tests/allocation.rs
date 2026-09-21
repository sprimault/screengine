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

use screengine::{Affine3, BYTES_PER_PIXEL, Color, Config, Context, Rows, Texture, Triangle, Vec3};

/// Le quadrilatère de la scène de référence, resoumis à chaque image.
///
/// La soumission est mesurée avec le rendu : elle est un appel nommé, mais
/// c'est le contexte qui a réservé sa capacité à la création, et une liste de
/// dessin qui grandirait au premier lot vidé serait exactement le défaut que ce
/// fichier existe pour voir.
const VERTICES: [Vec3; 4] = [
    Vec3::new(2.0, 2.5, 1.6),
    Vec3::new(3.5, -2.5, 1.6),
    Vec3::new(3.5, -2.5, -1.6),
    Vec3::new(2.0, 2.5, -1.6),
];

/// Ses deux triangles, qui partagent l'arête des sommets 0 et 2.
const TRIANGLES: [Triangle; 2] = [
    Triangle {
        indices: [0, 2, 1],
        color: Color::new(0xE0, 0xA0, 0x30, 0xFF),
    },
    Triangle {
        indices: [0, 3, 2],
        color: Color::new(0xA0, 0xE0, 0x30, 0xFF),
    },
];

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

/// Une texture et toute sa chaîne de mipmaps tiennent dans une allocation, et
/// une seule.
///
/// Le chargement est un appel nommé : il a le droit d'allouer. Ce qu'on vérifie
/// est qu'il alloue **une fois**, donc que la chaîne entière est engendrée
/// maintenant. Un niveau produit plus tard, au premier affichage, n'est pas
/// visible dans le test des images tant qu'aucune n'échantillonne de texture —
/// mais il se verrait ici, en niveaux comptés séparément.
#[test]
fn une_texture_se_charge_en_une_seule_allocation() {
    let (width, height) = (64, 64);
    let pixels = vec![0x80u8; width as usize * height as usize * BYTES_PER_PIXEL];

    let mut texture = None;
    let seen = allocations(|| {
        texture = Some(Texture::load(width, height, &pixels).expect("texture valide"));
    });

    let texture = texture.expect("texture valide");
    assert_eq!(texture.level_count(), 7, "chaîne incomplète");
    assert_eq!(seen, 1, "{seen} allocation(s) pour une texture");
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
        max_triangles: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    let seen = allocations(|| {
        for _ in 0..3 {
            context
                .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
                .expect("scène soumise");
            context.frame_end(&mut pixels, width).expect("image rendue");
        }
    });
    assert_eq!(seen, 0, "{seen} allocation(s) pendant trois images");
}

/// Des tuiles rendues sur d'autres threads n'allouent pas davantage : chaque
/// thread arme sa propre mesure autour de ses tuiles, puisque la création
/// des threads, elle, alloue.
#[test]
fn aucune_tuile_n_alloue_sur_un_autre_thread() {
    let (width, height) = (640u32, 360u32);
    let tile = 32;
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: tile,
        max_triangles: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];
    let columns = width.div_ceil(tile);
    let band = tile as usize * width as usize * BYTES_PER_PIXEL;

    for _ in 0..2 {
        let mut frame = None;
        let begun = allocations(|| {
            context
                .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
                .expect("scène soumise");
            frame = Some(context.frame_begin().expect("début"));
        });
        let frame = frame.expect("début");
        assert_eq!(
            begun, 0,
            "{begun} allocation(s) à la soumission ou au début"
        );

        let seen: usize = std::thread::scope(|scope| {
            let workers: Vec<_> = pixels
                .chunks_mut(band)
                .enumerate()
                .map(|(row, chunk)| {
                    let frame = &frame;
                    scope.spawn(move || {
                        let mut rows = Rows::band(chunk, width, row as u32 * tile);
                        allocations(|| {
                            for column in 0..columns {
                                frame
                                    .tile(row as u32 * columns + column, &mut rows)
                                    .expect("tuile");
                            }
                        })
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().expect("thread de rendu"))
                .sum()
        });
        assert_eq!(seen, 0, "{seen} allocation(s) dans les threads de rendu");
    }
}
