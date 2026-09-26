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
use std::sync::Arc;

use screengine::{
    Affine3, BYTES_PER_PIXEL, Color, Config, Context, Filter, Light, Mesh, Rows, Texture, Triangle,
    Vec3, VertexUv, VertexUv2,
};

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

/// Le fichier de maillage que la mesure soumet : le même quadrilatère, chargé.
///
/// Les octets s'écrivent ici plutôt que par le constructeur de la scène : ce
/// binaire de test ne voit pas les modules du binaire de conformance, et un
/// quadrilatère est assez court pour que la recopie coûte moins qu'une
/// bibliothèque ouverte pour lui.
fn quad_mesh() -> Vec<u8> {
    // Deux sections : les poses animent, les coordonnées de texture non.
    let mut poses = 1u32.to_le_bytes().to_vec();
    let mut uvs = Vec::new();
    for (i, position) in VERTICES.iter().enumerate() {
        for value in [position.x, position.y, position.z] {
            poses.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32, 0.0, 1.0] {
            poses.extend_from_slice(&value.to_le_bytes());
        }
        for value in [((i & 1) * 64) as f32, ((i >> 1) * 64) as f32] {
            uvs.extend_from_slice(&value.to_le_bytes());
        }
    }

    let mut triangles = Vec::new();
    for triangle in TRIANGLES {
        for index in triangle.indices {
            triangles.extend_from_slice(&index.to_le_bytes());
        }
        let color = triangle.color;
        triangles.extend_from_slice(&[color.r, color.g, color.b, color.a]);
    }

    let mut groups = Vec::new();
    for value in [1u32, 0, TRIANGLES.len() as u32, 0] {
        groups.extend_from_slice(&value.to_le_bytes());
    }

    let mut names = 3u16.to_le_bytes().to_vec();
    names.extend_from_slice(b"mur");

    let sections = [
        (*b"FRMS", poses.as_slice()),
        (*b"SURF", groups.as_slice()),
        (*b"TEXN", names.as_slice()),
        (*b"TRIS", triangles.as_slice()),
        (*b"VTXS", uvs.as_slice()),
    ];
    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"MESH");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(sections.len() as u32).to_le_bytes());

    let mut offset = first;
    for (tag, body) in &sections {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        offset += body.len();
    }
    for (_, body) in &sections {
        bytes.extend_from_slice(body);
    }
    bytes
}

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

/// Aucune image texturée n'alloue non plus, dans les deux filtrages.
///
/// Le chemin sans texture ne suffit pas : `submit_textured` passe par la table
/// de textures du contexte, que `submit` ne touche jamais. Une table qui
/// grandirait au premier lot serait une allocation par image, et la mesure
/// voisine ne la verrait pas — alors que c'est le chemin que tout décor
/// emprunte depuis l'étape 2, et celui par lequel l'étape 3 fera entrer les
/// lightmaps.
///
/// Les deux filtrages sont mesurés parce qu'ils ne lisent pas la texture de la
/// même façon : le bilinéaire prend quatre texels au lieu d'un, et rien ne dit
/// a priori qu'aucun des deux n'a besoin d'un tampon.
#[test]
fn aucune_image_texturee_n_alloue() {
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

    // Hors mesure : charger une ressource est un appel nommé, qui alloue.
    let texture = Arc::new(
        Texture::load(64, 64, &vec![0x80u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );
    let vertices: Vec<VertexUv> = VERTICES
        .iter()
        .enumerate()
        .map(|(i, position)| VertexUv {
            position: *position,
            u: ((i & 1) * 64) as f32,
            v: ((i >> 1) * 64) as f32,
        })
        .collect();

    for filter in [Filter::Dither, Filter::Bilinear] {
        context.set_filter(filter).expect("hors image");
        let seen = allocations(|| {
            for _ in 0..3 {
                context
                    .submit_textured(Affine3::IDENTITY, &vertices, &TRIANGLES, &texture)
                    .expect("scène soumise");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        assert_eq!(seen, 0, "{seen} allocation(s) en {filter:?}");
    }
}

/// Aucune image soumise depuis un maillage n'alloue.
///
/// Le maillage se charge **hors de la mesure** — un chargement est un appel
/// nommé, qui a le droit d'allouer — et se soumet **dedans**. C'est là que la
/// promesse se joue : la soumission lit un tableau de textures qu'un décodeur
/// naïf recopierait dans un tampon, et parcourt des groupes dont un chemin
/// distrait ferait une liste intermédiaire.
///
/// Avec texture puis sans, comme pour les lots éclairés : les deux chemins ne
/// touchent pas la même table.
#[test]
fn aucune_image_de_maillage_n_alloue() {
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

    // Hors mesure : le chargement d'une ressource est un appel nommé.
    let mesh = Mesh::load(&quad_mesh()).expect("maillage valide");
    let texture = Arc::new(
        Texture::load(64, 64, &vec![0x80u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );

    for avec_texture in [true, false] {
        let seen = allocations(|| {
            for _ in 0..3 {
                context
                    .submit_mesh(Affine3::IDENTITY, &mesh, |_| {
                        avec_texture.then_some(&texture)
                    })
                    .expect("maillage soumis");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        assert_eq!(seen, 0, "{seen} allocation(s), texture : {avec_texture}");
    }
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

/// Changer de résolution sous le maximum n'alloue pas, aller comme retour.
///
/// C'est la clause que l'ABI promet et que seul ce test peut prouver : tout ce
/// que l'image consomme est dimensionné sur la résolution maximale, et la
/// grille se reconstruit dans de la capacité déjà réservée. Une seule de ces
/// deux réserves prise sur la résolution **courante** ferait réallouer au
/// premier agrandissement, chez un hôte qui ajuste sa résolution en cours de
/// partie — donc image après image.
///
/// Le tampon est alloué une fois pour le maximum et réutilisé : le
/// redimensionner dans la mesure ferait compter l'allocation du test.
#[test]
fn aucun_changement_de_resolution_n_alloue() {
    let (max_width, max_height) = (640u32, 360u32);
    let mut context = Context::new(Config {
        max_width,
        max_height,
        width: 320,
        height: 180,
        tile_size: 64,
        max_triangles: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; max_width as usize * max_height as usize * BYTES_PER_PIXEL];

    // Hors mesure : la première image de ce contexte, pour que rien de
    // paresseux ne vienne s'imputer au redimensionnement.
    context
        .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
        .expect("scène soumise");
    context.frame_end(&mut pixels, 320).expect("image rendue");

    let seen = allocations(|| {
        for (width, height) in [(640, 360), (320, 180), (480, 270), (640, 360)] {
            context
                .set_resolution(width, height)
                .expect("sous le maximum");
            context
                .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
                .expect("scène soumise");
            context.frame_end(&mut pixels, width).expect("image rendue");
        }
    });
    assert_eq!(seen, 0, "{seen} allocation(s) en changeant de résolution");
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

/// Une image éclairée n'alloue pas davantage : lightmap **et** lumières
/// dynamiques.
///
/// Ce que l'étape 3 a ajouté au contexte — le tableau annexe des plans
/// d'éclairage et la liste des lumières placées — n'était écrit sous aucune
/// mesure. L'invariant tenait, les deux étant dimensionnés par `Context::new`,
/// mais rien ne le prouvait : c'est exactement la forme du défaut que ce
/// fichier existe pour voir, une réserve qui grandit au premier lot qui s'en
/// sert.
///
/// **`set_lights` est dans la mesure**, et c'est le point : la lightmap est une
/// ressource, donc son chargement est un appel nommé qui a le droit d'allouer,
/// alors que régler des lumières se fait entre deux images, autant de fois que
/// le jeu veut déplacer une torche.
#[test]
fn aucune_image_eclairee_n_alloue() {
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

    // Hors mesure : deux ressources, donc deux appels nommés.
    let texture = Arc::new(
        Texture::load(64, 64, &vec![0x80u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );
    let lightmap = Arc::new(
        Texture::load(16, 16, &vec![0xC0u8; 16 * 16 * BYTES_PER_PIXEL]).expect("lightmap valide"),
    );

    let vertices: Vec<VertexUv2> = VERTICES
        .iter()
        .enumerate()
        .map(|(i, position)| VertexUv2 {
            position: *position,
            u: ((i & 1) * 64) as f32,
            v: ((i >> 1) * 64) as f32,
            u2: ((i & 1) * 16) as f32,
            v2: ((i >> 1) * 16) as f32,
        })
        .collect();

    let lights = [Light {
        position: Vec3::new(2.5, 0.0, 0.5),
        radius: 8.0,
        color: Color::new(0xFF, 0xC0, 0x60, 0xFF),
    }];

    // Avec texture puis sans : un mur uni éclairé est le premier cas de
    // l'étape, et il ne passe pas par la table de textures.
    for avec_texture in [true, false] {
        let seen = allocations(|| {
            for _ in 0..3 {
                context.set_lights(&lights).expect("hors image");
                context
                    .submit_lit(
                        Affine3::IDENTITY,
                        &vertices,
                        &TRIANGLES,
                        avec_texture.then_some(&texture),
                        &lightmap,
                    )
                    .expect("scène soumise");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        assert_eq!(seen, 0, "{seen} allocation(s), texture : {avec_texture}");
    }
}
