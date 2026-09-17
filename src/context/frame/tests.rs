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

use super::*;
use crate::context::{Config, TRIANGLE_CAPACITY};
use crate::math::fixed::SUBPIXEL_SCALE;
use crate::raster::Point;
use crate::testing::Rng;

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
    })
    .expect("configuration saine")
}

/// Remplace la scène par des triangles tirés au hasard.
///
/// Petits et grands mêlés, débordant de l'image, et qui se recouvrent : sans
/// profondeur, c'est l'ordre de soumission qui décide du pixel, et la
/// répartition doit le conserver.
fn scene(context: &mut Context, seed: u64) {
    let mut rng = Rng::new(seed);
    context.triangles.clear();
    for _ in 0..300 {
        let reach = if rng.next() % 4 == 0 { 400 } else { 40 };
        let cx = rng.coord(-20, W as i32 + 20);
        let cy = rng.coord(-20, H as i32 + 20);
        let mut vertex = || Point {
            x: (cx + rng.coord(-reach, reach)) * SUBPIXEL_SCALE + rng.coord(0, 15),
            y: (cy + rng.coord(-reach, reach)) * SUBPIXEL_SCALE + rng.coord(0, 15),
        };
        let (a, b, c) = (vertex(), vertex(), vertex());
        let color = rng.next() as u32 | 0xFF00_0000;
        // Les deux orientations, pour que la moitié des tirages ne soit pas
        // éliminée comme dos de face.
        context.submit([a, b, c], color).expect("capacité");
        context.submit([a, c, b], color).expect("capacité");
    }
}

/// Un tampon d'hôte vide à la taille de l'image.
fn pixels() -> Vec<u8> {
    vec![0; W as usize * H as usize * BYTES_PER_PIXEL]
}

/// La référence : l'image entière rendue d'une région, sans répartition.
fn reference(context: &mut Context) -> Vec<u8> {
    let mut out = pixels();
    let mut scratch = vec![0u32; W as usize * H as usize];
    let frame = context.seal();
    let image = frame.grid.image();
    frame
        .region(image, &mut scratch, &mut Rows::new(&mut out, W))
        .expect("région");
    out
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
            context
                .seal()
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
        let frame = context.seal();
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
    let frame = context.seal();
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
    let mut scratch = vec![0u32; 100];
    let mut rows = Rows::new(&mut out, W);

    let rect = |x, width| Rect {
        x,
        y: 0,
        width,
        height: 10,
    };
    assert_eq!(
        frame.region(rect(W - 5, 10), &mut scratch, &mut rows),
        Err(Error::InvalidArgument(Argument::Region))
    );
    assert_eq!(
        frame.region(rect(u32::MAX, 2), &mut scratch, &mut rows),
        Err(Error::InvalidArgument(Argument::Region))
    );
    assert_eq!(
        frame.region(rect(0, 20), &mut scratch, &mut rows),
        Err(Error::InvalidArgument(Argument::ScratchLength))
    );
}

/// La capacité réservée est une limite rendue, pas une croissance : le
/// triangle de trop est refusé, et ceux d'avant restent.
#[test]
fn la_capacite_de_triangles_est_une_limite() {
    let mut context = context(64);
    context.triangles.clear();
    let p = |x: i32, y: i32| Point {
        x: x * SUBPIXEL_SCALE,
        y: y * SUBPIXEL_SCALE,
    };
    let v = [p(0, 0), p(10, 0), p(0, 10)];
    for _ in 0..TRIANGLE_CAPACITY {
        context.submit(v, 0).expect("sous la capacité");
    }
    assert_eq!(
        context.submit(v, 0),
        Err(Error::InvalidArgument(Argument::TriangleCapacity))
    );
    assert_eq!(context.triangles.len(), TRIANGLE_CAPACITY);
}
