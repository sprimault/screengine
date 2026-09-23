// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! L'image ne dépend ni de la taille des tuiles, ni de leur ordre, ni de qui
//! les rend.
//!
//! La référence est [`Frame::region`] sur l'image entière : tous les triangles
//! dans l'ordre de soumission, sans répartition. Chaque autre façon de rendre
//! doit en donner les octets exacts.

use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::*;
use crate::context::{Config, TRIANGLE_CAPACITY};
use crate::math::fixed::{DEPTH_MARGIN, SUBPIXEL_SCALE};
use crate::raster::{NO_TEXTURE, Point, Vertex};
use crate::testing::Rng;
use crate::texture::Texture;

/// Largeur de l'image des tests, multiple ni de 32 ni de 64 : les tuiles de
/// bord sont partielles dans les deux découpages.
const W: u32 = 203;

/// Hauteur de l'image des tests.
const H: u32 = 150;

/// Un contexte à la taille des tests.
fn context(tile_size: u32) -> Context {
    Context::new(Config {
        max_width: W,
        max_height: H,
        width: W,
        height: H,
        tile_size,
        max_triangles: 0,
    })
    .expect("configuration saine")
}

/// Remplace la scène par des triangles tirés au hasard.
///
/// Petits et grands mêlés, débordant de l'image, qui se recouvrent et
/// s'interpénètrent. Un quart sont plats, à l'une de deux profondeurs seulement :
/// à égalité, c'est l'ordre de soumission qui décide du pixel, et la
/// répartition doit le conserver.
///
/// **Un triangle sur deux est texturé**, et ce n'est pas un raffinement : le
/// chemin texturé découpe la ligne en segments alignés sur la grille de
/// l'image, choisit un niveau de mipmap et trame les coordonnées. Rien de tout
/// cela n'existe sur le chemin uni, et c'est précisément là qu'une dépendance
/// au découpage se cacherait — une scène sans texture laisserait l'essentiel du
/// remplissage hors de la comparaison.
fn scene(context: &mut Context, seed: u64) {
    let mut rng = Rng::new(seed);
    context.triangles.clear();
    context.textures.clear();
    context.textures.push(alloc::sync::Arc::new(damier()));
    let span = (u32::MAX - 2 * DEPTH_MARGIN) as u64;
    for _ in 0..300 {
        let reach = if rng.next() % 4 == 0 { 400 } else { 40 };
        let cx = rng.coord(-20, W as i32 + 20);
        let cy = rng.coord(-20, H as i32 + 20);
        let flat = match rng.next() % 8 {
            0 => Some(1 << 30),
            1 => Some(3 << 30),
            _ => None,
        };
        let textured = rng.next() % 2 == 0;
        let mut vertex = || {
            let z = flat.unwrap_or(DEPTH_MARGIN + (rng.next() % span) as u32);
            // `s` et `t` sont `u·d` et `v·d` : les tirer à partir de la
            // profondeur du sommet les garde dans le domaine que la mise en
            // place attend, au lieu de valeurs qui satureraient les gradients.
            let scale = |c: i64| ((c * i64::from(z >> 16)) >> 4) as i32;
            Vertex {
                position: Point {
                    x: (cx + rng.coord(-reach, reach)) * SUBPIXEL_SCALE + rng.coord(0, 15),
                    y: (cy + rng.coord(-reach, reach)) * SUBPIXEL_SCALE + rng.coord(0, 15),
                },
                z,
                s: if textured {
                    scale(i64::from(rng.coord(0, 63)))
                } else {
                    0
                },
                t: if textured {
                    scale(i64::from(rng.coord(0, 63)))
                } else {
                    0
                },
            }
        };
        let (a, b, c) = (vertex(), vertex(), vertex());
        let color = rng.next() as u32 | 0xFF00_0000;
        let texture = if textured { 0 } else { NO_TEXTURE };
        // Les deux orientations, pour que la moitié des tirages ne soit pas
        // éliminée comme dos de face.
        context.push([a, b, c], color, texture).expect("capacité");
        context.push([a, c, b], color, texture).expect("capacité");
    }

    // Sans quoi la scène pourrait n'avoir aucun triangle texturé et toutes les
    // comparaisons resteraient vertes en ne mesurant que le chemin uni.
    let textures = context
        .triangles
        .iter()
        .filter(|t| t.texture() != NO_TEXTURE)
        .count();
    assert!(textures > 50, "{textures} triangles texturés, trop peu");
}

/// Une texture en damier de 64 texels de côté, dont la chaîne descend jusqu'à
/// 1×1.
///
/// Un damier plutôt qu'un aplat : deux texels voisins y diffèrent toujours,
/// donc un texel lu au mauvais endroit change la couleur du pixel. Sur un
/// aplat, une coordonnée fausse ne se verrait pas.
fn damier() -> Texture {
    let side = 64;
    let mut pixels = vec![0u8; side * side * 4];
    for (i, texel) in pixels.chunks_exact_mut(4).enumerate() {
        let (x, y) = (i % side, i / side);
        let value = if (x ^ y) & 1 == 0 { 0x20 } else { 0xE0 };
        texel.copy_from_slice(&[value, value, value, 0xFF]);
    }
    Texture::load(side as u32, side as u32, &pixels).expect("texture valide")
}

/// Un tampon d'hôte vide à la taille de l'image.
fn pixels() -> Vec<u8> {
    vec![0; W as usize * H as usize * BYTES_PER_PIXEL]
}

/// Ouvre une image sur la scène déjà soumise, sans le triangle en dur.
fn open(context: &mut Context) -> Frame<'_> {
    context.seal();
    Frame::new(context)
}

/// La référence : l'image entière rendue d'une région, sans répartition.
fn reference(context: &mut Context) -> Vec<u8> {
    let mut out = pixels();
    let mut color = vec![0u32; W as usize * H as usize];
    let mut depth = color.clone();
    let image = Rect {
        x: 0,
        y: 0,
        width: W,
        height: H,
    };
    open(context)
        .region(image, &mut color, &mut depth, &mut Rows::new(&mut out, W))
        .expect("région");
    out
}

/// Une résolution interne **inférieure au maximum** rend la même image que la
/// région de référence, dans les deux découpages.
///
/// Rien ne l'éprouvait : tous les autres cas rendent à la résolution maximale,
/// où la grille de tuiles couvre exactement l'image. Sous le maximum, la grille
/// se recalcule et les tampons réservés à la création restent plus grands que
/// ce qui sert — c'est là qu'un index calculé sur la mauvaise largeur se
/// cacherait. L'étape 3 rend cette résolution modifiable en cours de route, et
/// sur téléphone le rendu ne se fait jamais à la résolution de l'écran.
#[test]
fn une_resolution_sous_le_maximum_rend_la_reference() {
    let (width, height) = (W - 61, H - 47);
    for tile_size in [32, 64] {
        let mut context = Context::new(Config {
            max_width: W,
            max_height: H,
            width,
            height,
            tile_size,
            max_triangles: 0,
        })
        .expect("configuration saine");
        scene(&mut context, 7);

        let mut out = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];
        let mut color = vec![0u32; width as usize * height as usize];
        let mut depth = color.clone();
        let image = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        open(&mut context)
            .region(
                image,
                &mut color,
                &mut depth,
                &mut Rows::new(&mut out, width),
            )
            .expect("région");

        let mut tiled = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];
        open(&mut context)
            .end(&mut Rows::new(&mut tiled, width))
            .expect("image");

        assert!(tiled == out, "tuiles de {tile_size} sous le maximum");
        assert!(
            out.chunks_exact(BYTES_PER_PIXEL)
                .any(|p| p[..3] != [0, 0, 0]),
            "l'image est vide, le cas ne prouve rien"
        );
    }
}

/// Les tuiles de 32 et de 64, rendues par la fin seule, donnent la
/// référence octet pour octet.
#[test]
fn les_tuiles_de_32_et_de_64_rendent_la_reference() {
    for seed in 1..=12 {
        for tile_size in [32, 64] {
            let mut context = context(tile_size);
            scene(&mut context, seed);
            let expected = reference(&mut context);

            let mut out = pixels();
            open(&mut context)
                .end(&mut Rows::new(&mut out, W))
                .expect("image");
            assert!(out == expected, "graine {seed}, tuiles de {tile_size}");
        }
    }
}

/// Une moitié des tuiles dans un ordre mélangé, puis la fin qui complète :
/// l'ordre ne change rien, et la fin ne rend que ce qui manque.
#[test]
fn l_ordre_des_tuiles_ne_change_rien() {
    for seed in 1..=12 {
        let mut context = context(32);
        scene(&mut context, seed);
        let expected = reference(&mut context);

        let mut out = pixels();
        let frame = open(&mut context);
        let mut order: Vec<u32> = (0..frame.tile_count()).collect();
        let mut rng = Rng::new(seed);
        for i in (1..order.len()).rev() {
            order.swap(i, (rng.next() % (i as u64 + 1)) as usize);
        }
        let mut rows = Rows::new(&mut out, W);
        for &index in &order[..order.len() / 2] {
            frame.tile(index, &mut rows).expect("tuile");
        }
        frame.end(&mut rows).expect("fin");
        assert!(out == expected, "graine {seed}");
    }
}

/// Des bandes rendues sur des threads distincts, chacune ses tuiles : le
/// partage de la `Frame` ne change aucun pixel.
#[test]
fn des_threads_rendent_la_reference() {
    extern crate std;

    let mut context = context(32);
    scene(&mut context, 7);
    let expected = reference(&mut context);

    let mut out = pixels();
    let frame = open(&mut context);
    let columns = W.div_ceil(32);
    let band = 32 * W as usize * BYTES_PER_PIXEL;
    std::thread::scope(|scope| {
        for (row, chunk) in out.chunks_mut(band).enumerate() {
            let frame = &frame;
            scope.spawn(move || {
                let mut rows = Rows::band(chunk, W, row as u32 * 32);
                for column in 0..columns {
                    frame
                        .tile(row as u32 * columns + column, &mut rows)
                        .expect("tuile");
                }
            });
        }
    });
    frame.end(&mut Rows::new(&mut out, W)).expect("fin");
    assert!(out == expected);
}

/// Une tuile se rend une fois par image : la seconde prise est un appel hors
/// séquence, pas un second rendu silencieux.
#[test]
fn une_tuile_prise_deux_fois_est_refusee() {
    let mut context = context(64);
    let mut out = pixels();
    let frame = context.frame_begin().expect("début");
    let mut rows = Rows::new(&mut out, W);
    frame.tile(2, &mut rows).expect("première prise");
    assert_eq!(frame.tile(2, &mut rows), Err(Error::InvalidState));
}

/// Hors d'une image commencée, une tuile et une fin sont des appels hors
/// séquence, et un second début aussi : sa répartition écraserait celle
/// qu'une tuile lit peut-être sur un autre thread.
#[test]
fn la_sequence_de_l_image_est_verifiee() {
    let mut context = context(64);
    let mut out = pixels();
    assert_eq!(
        context.tile(0, &mut Rows::new(&mut out, W)),
        Err(Error::InvalidState)
    );
    assert_eq!(
        context.end(&mut Rows::new(&mut out, W)),
        Err(Error::InvalidState)
    );

    assert_eq!(context.begin(), Ok(12), "4 colonnes et 3 lignes de 64");
    assert_eq!(context.begin(), Err(Error::InvalidState));
    assert_eq!(context.end(&mut Rows::new(&mut out, W)), Ok(()));
    assert_eq!(
        context.tile(0, &mut Rows::new(&mut out, W)),
        Err(Error::InvalidState),
        "une tuile après la fin"
    );
}

/// La fin attend que plus aucune tuile ne tourne : elle refuse, laisse
/// l'image ouverte, et aboutit une fois la tuile terminée.
#[test]
fn la_fin_refuse_pendant_qu_une_tuile_tourne() {
    let mut context = context(64);
    let mut out = pixels();
    context.begin().expect("début");

    context.in_flight.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        context.end(&mut Rows::new(&mut out, W)),
        Err(Error::InvalidState)
    );
    assert!(context.is_rendering(), "l'image reste ouverte");

    context.in_flight.fetch_sub(1, Ordering::SeqCst);
    assert_eq!(context.end(&mut Rows::new(&mut out, W)), Ok(()));
    assert!(!context.is_rendering());
}

/// Une fin d'image après quelques tuiles rend exactement l'image d'une fin
/// seule : c'est la garantie qu'un hôte qui oublie une tuile ne reçoit pas
/// d'image trouée.
#[test]
fn la_fin_complete_les_tuiles_manquantes() {
    let mut full = pixels();
    context(32).frame_end(&mut full, W).expect("image seule");

    let mut context = context(32);
    let mut out = pixels();
    let count = context.begin().expect("début");
    for index in (0..count).step_by(3) {
        context
            .tile(index, &mut Rows::new(&mut out, W))
            .expect("tuile");
    }
    context.frame_end(&mut out, W).expect("fin");
    assert!(out == full);
}

/// Une `Frame` abandonnée sans fin rend le contexte à l'enregistrement : sans
/// quoi un `?` entre le début et la fin bloquerait le contexte pour toujours.
#[test]
fn une_frame_abandonnee_referme_l_image() {
    let mut context = context(64);
    drop(context.frame_begin().expect("début"));
    assert!(!context.is_rendering());
    assert!(context.frame_begin().is_ok());
}

/// Un index au-delà du nombre de tuiles est refusé, pas ramené dans
/// l'intervalle.
#[test]
fn un_index_hors_de_l_image_est_refuse() {
    let mut context = context(64);
    let mut out = pixels();
    let frame = context.frame_begin().expect("début");
    let count = frame.tile_count();
    assert_eq!(
        frame.tile(count, &mut Rows::new(&mut out, W)),
        Err(Error::InvalidArgument(Argument::TileIndex))
    );
}

/// Une sortie refusée ne consomme pas la tuile : l'appelant corrige son
/// tampon et la rend.
#[test]
fn une_sortie_refusee_ne_prend_pas_la_tuile() {
    let mut context = context(64);
    let mut out = pixels();
    let frame = context.frame_begin().expect("début");
    let mut short = [0u8; 16];
    assert_eq!(
        frame.tile(0, &mut Rows::new(&mut short, W)),
        Err(Error::InvalidArgument(Argument::BufferLength))
    );
    assert_eq!(frame.tile(0, &mut Rows::new(&mut out, W)), Ok(()));
}

/// Une bande qui ne contient pas les lignes de la tuile est refusée, au lieu
/// d'écrire la tuile ailleurs.
#[test]
fn une_bande_sans_les_lignes_de_la_tuile_est_refusee() {
    let mut context = context(64);
    let mut out = pixels();
    let frame = context.frame_begin().expect("début");
    let band = 64 * W as usize * BYTES_PER_PIXEL;
    let mut rows = Rows::band(&mut out[band..], W, 64);
    assert_eq!(
        frame.tile(0, &mut rows),
        Err(Error::InvalidArgument(Argument::BufferLength))
    );
}

/// Une région qui déborde de l'image, ou un tampon de travail trop court,
/// sont refusés avant d'écrire quoi que ce soit.
#[test]
fn une_region_invalide_est_refusee() {
    let mut context = context(64);
    let mut out = pixels();
    let frame = context.frame_begin().expect("début");
    let (mut color, mut depth) = (vec![0u32; 100], vec![0u32; 100]);
    let mut rows = Rows::new(&mut out, W);

    let rect = |x, width| Rect {
        x,
        y: 0,
        width,
        height: 10,
    };
    let mut region =
        |r, color: &mut [u32], depth: &mut [u32]| frame.region(r, color, depth, &mut rows);
    let (region_err, length_err) = (
        Err(Error::InvalidArgument(Argument::Region)),
        Err(Error::InvalidArgument(Argument::ScratchLength)),
    );
    assert_eq!(region(rect(W - 5, 10), &mut color, &mut depth), region_err);
    let wraps = rect(u32::MAX, 2);
    assert_eq!(region(wraps, &mut color, &mut depth), region_err);
    assert_eq!(region(rect(0, 20), &mut color, &mut depth), length_err);
    // Deux tampons de longueurs différentes, chacun assez long pour la région :
    // refusés quand même, jamais ramenés au plus court.
    let mut longer = vec![0u32; 101];
    assert_eq!(region(rect(0, 5), &mut color, &mut longer), length_err);
    assert_eq!(region(rect(0, 5), &mut color, &mut depth), Ok(()));
}

/// Un sommet à une profondeur donnée, en pixels entiers.
fn at(x: i32, y: i32, z: u32) -> Vertex {
    Vertex {
        position: Point {
            x: x * SUBPIXEL_SCALE,
            y: y * SUBPIXEL_SCALE,
        },
        z,
        s: 0,
        t: 0,
    }
}

/// La couleur rendue au pixel `(x, y)` de la référence.
fn color_at(image: &[u8], x: u32, y: u32) -> u32 {
    let i = (y * W + x) as usize * BYTES_PER_PIXEL;
    u32::from_le_bytes([image[i], image[i + 1], image[i + 2], image[i + 3]])
}

/// Deux triangles inclinés qui se croisent : chacun passe devant l'autre d'un
/// côté de la ligne d'intersection, quel que soit l'ordre de soumission.
#[test]
fn deux_triangles_inclines_s_interpenetrent() {
    let (red, blue) = (0xFF00_00FF, 0xFFFF_0000);
    let (near, far) = (3 << 30, 1 << 30);
    // Le rouge se rapproche vers la droite, le bleu s'en éloigne. Sommets
    // antihoraires à l'écran, comme toute face avant.
    let a = [at(10, 10, far), at(10, 140, far), at(190, 10, near)];
    let b = [at(10, 20, near), at(10, 130, near), at(190, 20, far)];
    for order in [[(a, red), (b, blue)], [(b, blue), (a, red)]] {
        let mut context = context(32);
        context.triangles.clear();
        for (triangle, color) in order {
            context.push(triangle, color, NO_TEXTURE).expect("capacité");
        }
        let image = reference(&mut context);
        assert_eq!(color_at(&image, 20, 30), blue, "bleu devant à gauche");
        assert_eq!(color_at(&image, 150, 30), red, "rouge devant à droite");
    }
}

/// À profondeur égale, le premier triangle soumis reste : le test est strict,
/// et inverser l'ordre inverse le résultat.
#[test]
fn a_egalite_le_premier_soumis_reste() {
    let (red, blue) = (0xFF00_00FF, 0xFFFF_0000);
    let z = 1 << 31;
    let triangle = [at(10, 10, z), at(10, 140, z), at(190, 10, z)];
    for (first, second) in [(red, blue), (blue, red)] {
        let mut context = context(64);
        context.triangles.clear();
        context.push(triangle, first, NO_TEXTURE).expect("capacité");
        context
            .push(triangle, second, NO_TEXTURE)
            .expect("capacité");
        assert_eq!(color_at(&reference(&mut context), 30, 30), first);
    }
}

/// Quand les profondeurs sont toutes distinctes, l'ordre de soumission ne
/// change rien à l'image.
#[test]
fn l_ordre_de_soumission_ne_compte_pas_a_profondeurs_distinctes() {
    let mut rng = Rng::new(31);
    let triangles: Vec<([Vertex; 3], u32)> = (0..40u32)
        .map(|i| {
            let z = DEPTH_MARGIN + (i + 1) * 97_000_000;
            let (x, y) = (rng.coord(0, 150), rng.coord(0, 100));
            let v = [at(x, y, z), at(x + 60, y, z), at(x, y + 50, z)];
            (v, rng.next() as u32 | 0xFF00_0000)
        })
        .collect();
    let render = |reverse: bool| {
        let mut context = context(32);
        context.triangles.clear();
        let mut list = triangles.clone();
        if reverse {
            list.reverse();
        }
        for (v, color) in list {
            context.push(v, color, NO_TEXTURE).expect("capacité");
        }
        reference(&mut context)
    };
    assert!(render(false) == render(true));
}

/// La capacité réservée est une limite rendue, pas une croissance : le
/// triangle de trop est refusé, et ceux d'avant restent.
#[test]
fn la_capacite_de_triangles_est_une_limite() {
    let mut context = context(64);
    context.triangles.clear();
    let v = [at(0, 0, 1 << 31), at(0, 10, 1 << 31), at(10, 0, 1 << 31)];
    for _ in 0..TRIANGLE_CAPACITY {
        context.push(v, 0, NO_TEXTURE).expect("sous la capacité");
    }
    assert_eq!(
        context.push(v, 0, NO_TEXTURE),
        Err(Error::InvalidArgument(Argument::TriangleCapacity))
    );
    assert_eq!(context.triangles.len(), TRIANGLE_CAPACITY);
}
