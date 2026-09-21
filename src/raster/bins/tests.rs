// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le découpage en tuiles et la répartition, vus tuile par tuile.

use alloc::vec::Vec;

use super::*;
use crate::math::fixed::SUBPIXEL_SCALE;
use crate::raster::{Point, Vertex, prepare};

/// Un triangle rectangle qui couvre les pixels `x0..x1` × `y0..y1`, dans le
/// sens de la face avant — antihoraire à l'écran.
fn triangle(x0: i32, y0: i32, x1: i32, y1: i32) -> Prepared {
    let p = |x: i32, y: i32| Vertex {
        position: Point {
            x: x * SUBPIXEL_SCALE,
            y: y * SUBPIXEL_SCALE,
        },
        z: 1 << 31,
        s: 0,
        t: 0,
    };
    prepare([p(x0, y0), p(x0, y1), p(x1, y0)], 0).expect("triangle visible")
}

/// Les triangles d'une tuile, dans l'ordre où elle les dessinera.
fn tile(bins: &Bins, index: u32) -> Vec<u32> {
    bins.tile(index).collect()
}

/// La numérotation est contractuelle — ligne par ligne —, et les tuiles de
/// bord sont partielles : une tuile qui dépasserait de l'image écrirait hors
/// du tampon de l'hôte.
#[test]
fn numerote_ligne_par_ligne_avec_des_tuiles_de_bord_partielles() {
    let grid = Grid::new(100, 70, 32);
    assert_eq!(grid.count(), 12);
    let rect = |index| {
        let r = grid.rect(index);
        (r.x, r.y, r.width, r.height)
    };
    assert_eq!(rect(1), (32, 0, 32, 32));
    assert_eq!(rect(3), (96, 0, 4, 32));
    assert_eq!(rect(4), (0, 32, 32, 32));
    assert_eq!(rect(11), (96, 64, 4, 6));
}

/// Un petit triangle n'est référencé que dans les tuiles que sa boîte
/// touche, et un triangle hors de l'image nulle part.
#[test]
fn un_petit_triangle_ne_va_que_dans_ses_tuiles() {
    let grid = Grid::new(128, 64, 32);
    let triangles = [triangle(20, 5, 40, 20), triangle(-50, -50, -10, -10)];
    let mut bins = Bins::new(grid.count() as usize, triangles.len()).expect("réserve");
    bins.build(&grid, &triangles);

    for index in 0..grid.count() {
        let expected: &[u32] = if index <= 1 { &[0] } else { &[] };
        assert_eq!(tile(&bins, index), expected, "tuile {index}");
    }
}

/// Au-delà de `LARGE_TILES` tuiles, un triangle passe dans la liste commune :
/// chaque tuile le voit, et la mémoire de répartition reste bornée.
#[test]
fn un_grand_triangle_passe_dans_la_liste_commune() {
    let grid = Grid::new(320, 320, 32);
    let triangles = [triangle(0, 0, 320, 320)];
    let mut bins = Bins::new(grid.count() as usize, triangles.len()).expect("réserve");
    bins.build(&grid, &triangles);

    assert!(bins.refs.is_empty());
    assert_eq!(tile(&bins, 99), [0]);
}

/// La fusion rend l'ordre de soumission, petits et grands mêlés. C'est
/// l'ordre qui départage deux triangles qui se recouvrent : perdu, l'image
/// d'une tuile cesserait d'être celle de l'image entière.
#[test]
fn la_fusion_rend_l_ordre_de_soumission() {
    let grid = Grid::new(320, 320, 32);
    let triangles = [
        triangle(0, 0, 320, 320),
        triangle(1, 1, 20, 20),
        triangle(0, 0, 300, 300),
        triangle(2, 2, 25, 25),
        triangle(0, 0, 310, 310),
    ];
    let mut bins = Bins::new(grid.count() as usize, triangles.len()).expect("réserve");
    bins.build(&grid, &triangles);

    assert_eq!(tile(&bins, 0), [0, 1, 2, 3, 4]);
    assert_eq!(tile(&bins, 1), [0, 2, 4]);
}

/// Une seconde répartition ne garde rien de la première : les tampons se
/// vident, ils ne s'accumulent pas.
#[test]
fn une_nouvelle_repartition_repart_de_zero() {
    let grid = Grid::new(128, 64, 32);
    let mut bins = Bins::new(grid.count() as usize, 2).expect("réserve");
    bins.build(&grid, &[triangle(20, 5, 40, 20), triangle(0, 0, 128, 64)]);
    bins.build(&grid, &[triangle(100, 40, 120, 60)]);

    assert_eq!(tile(&bins, 0), [] as [u32; 0]);
    assert_eq!(tile(&bins, 7), [0]);
}
