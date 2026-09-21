// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! L'équation de plan : bornes, arrondi, et indépendance de l'ordre des
//! sommets.

use super::*;
use crate::math::fixed::{DEPTH_MARGIN, PIXEL_CENTER, SUBPIXEL_SCALE};
use crate::testing::Rng;

/// L'aire signée, comme le rasteriseur la calcule.
fn area(v: [Point; 3]) -> i64 {
    let (x, y) = (v.map(|p| p.x as i64), v.map(|p| p.y as i64));
    (x[1] - x[0]) * (y[2] - y[0]) - (y[1] - y[0]) * (x[2] - x[0])
}

/// La valeur exacte en un point, en rationnel, multipliée par l'aire et
/// décalée de [`GRADIENT_BITS`] : la référence sans arrondi.
fn exact(v: [Point; 3], z: [u32; 3], px: i32, py: i32) -> i128 {
    let (x, y) = (v.map(|p| p.x as i128), v.map(|p| p.y as i128));
    let (px, py) = (px as i128, py as i128);
    // Coordonnées barycentriques non normalisées : chaque poids est l'aire du
    // sous-triangle opposé au sommet.
    let w0 = (x[1] - px) * (y[2] - py) - (y[1] - py) * (x[2] - px);
    let w1 = (x[2] - px) * (y[0] - py) - (y[2] - py) * (x[0] - px);
    let w2 = (x[0] - px) * (y[1] - py) - (y[0] - py) * (x[1] - px);
    (w0 * z[0] as i128 + w1 * z[1] as i128 + w2 * z[2] as i128) << GRADIENT_BITS
}

/// Un point en sous-pixels tiré dans la bande de garde.
fn point(rng: &mut Rng) -> Point {
    let reach = 4096 * SUBPIXEL_SCALE;
    Point {
        x: rng.coord(-reach, reach),
        y: rng.coord(-reach, reach),
    }
}

/// Une profondeur de sommet dans l'intervalle que `to_depth` garantit.
fn depth(rng: &mut Rng) -> u32 {
    DEPTH_MARGIN + (rng.next() % (u32::MAX - 2 * DEPTH_MARGIN) as u64) as u32
}

/// Aux extrêmes de la bande de garde, écarts de profondeur sur toute la plage
/// et triangles aussi fins que le tirage les rend : en chaque centre de pixel
/// intérieur, l'évaluation enveloppante reste à moins de 64 unités de la
/// valeur exacte, calculée en `i128`, et dans [0, 2³²). C'est la borne sur
/// laquelle repose la marge de `to_depth`.
#[test]
fn l_erreur_reste_sous_la_marge_aux_extremes() {
    let mut rng = Rng::new(37);
    let mut checked = 0;
    while checked < 200_000 {
        let v = [point(&mut rng), point(&mut rng), point(&mut rng)];
        let a = area(v);
        if a <= 0 {
            continue;
        }
        let z = [depth(&mut rng), depth(&mut rng), depth(&mut rng)];
        let plane = Plane::new(v, z, a);
        // Un centre de pixel tiré au hasard, gardé s'il est dans le triangle.
        let px = rng.coord(-4096, 4095) * SUBPIXEL_SCALE + PIXEL_CENTER;
        let py = rng.coord(-4096, 4095) * SUBPIXEL_SCALE + PIXEL_CENTER;
        let target = exact(v, z, px, py);
        if target < 0 || target > (a as i128) * ((u32::MAX as i128) << GRADIENT_BITS) {
            continue;
        }
        let inside = {
            let q = Point { x: px, y: py };
            area([v[0], v[1], q]) >= 0 && area([v[1], v[2], q]) >= 0 && area([v[2], v[0], q]) >= 0
        };
        if !inside {
            continue;
        }
        let got = plane.at(px, py);
        let error = ((got as i128 * a as i128 - target).abs() >> GRADIENT_BITS) / a as i128;
        assert!(error < 64, "écart de {error} unités");
        let range = 0..1i64 << (32 + GRADIENT_BITS);
        assert!(range.contains(&got), "hors plage : {got}");
        checked += 1;
    }
}

/// Les trois permutations circulaires d'un même triangle rendent les mêmes
/// bits en chaque point : c'est la raison du point de référence canonique.
#[test]
fn les_permutations_circulaires_rendent_les_memes_bits() {
    let mut rng = Rng::new(41);
    for _ in 0..100_000 {
        let v = [point(&mut rng), point(&mut rng), point(&mut rng)];
        let a = area(v);
        if a <= 0 {
            continue;
        }
        let z = [depth(&mut rng), depth(&mut rng), depth(&mut rng)];
        let (px, py) = (rng.coord(-65536, 65535), rng.coord(-65536, 65535));
        let base = Plane::new(v, z, a).at(px, py);
        for shift in [1, 2] {
            let rotate = |k: usize| (k + shift) % 3;
            let v2 = [v[rotate(0)], v[rotate(1)], v[rotate(2)]];
            let z2 = [z[rotate(0)], z[rotate(1)], z[rotate(2)]];
            assert_eq!(Plane::new(v2, z2, area(v2)).at(px, py), base);
        }
    }
}

/// Le pas d'un pixel ajouté pas à pas rend les bits de la forme close : c'est
/// ce qui permet au parcours de partir de n'importe quel pixel d'une ligne.
///
/// Horizontalement seulement, parce que c'est le seul sens que le remplissage
/// parcourt : chaque ligne repart de la forme close au premier pixel de son
/// span, et il n'y a donc rien à propager d'une ligne à l'autre.
#[test]
fn le_parcours_pas_a_pas_rend_la_forme_close() {
    let mut rng = Rng::new(43);
    for _ in 0..10_000 {
        let v = [point(&mut rng), point(&mut rng), point(&mut rng)];
        let a = area(v);
        if a <= 0 {
            continue;
        }
        let plane = Plane::new(v, [depth(&mut rng), depth(&mut rng), depth(&mut rng)], a);
        let (px, py) = (rng.coord(-60000, 60000), rng.coord(-60000, 60000));
        let steps_x = rng.coord(0, 200);
        let mut value = plane.at(px, py);
        for _ in 0..steps_x {
            value = value.wrapping_add(plane.step_x(SUBPIXEL_SCALE));
        }
        assert_eq!(value, plane.at(px + steps_x * SUBPIXEL_SCALE, py));
    }
}

/// Le gradient s'arrondit vers le bas des deux côtés de zéro : −1365,33 donne
/// −1366, pas −1365. Tronqué vers zéro, un gradient qui change de signe entre
/// deux triangles voisins ferait un pas double autour de zéro ; l'erreur reste
/// sous l'unité dans les deux cas, et c'est pourquoi seul ce test la voit.
#[test]
fn le_gradient_s_arrondit_vers_le_bas_des_deux_cotes_de_zero() {
    let v = [
        Point { x: 0, y: 0 },
        Point { x: 3, y: 0 },
        Point { x: 0, y: 3 },
    ];
    assert_eq!(Plane::new(v, [1, 0, 0], area(v)).dx, -1366);
    assert_eq!(Plane::new(v, [0, 1, 0], area(v)).dx, 1365);
}

/// Un plan constant est exact : les gradients sont nuls, et la valeur est
/// celle des sommets partout.
#[test]
fn un_plan_constant_est_exact() {
    let v = [
        Point { x: 0, y: 0 },
        Point { x: 1600, y: 0 },
        Point { x: 0, y: 1600 },
    ];
    let plane = Plane::new(v, [123_456_789; 3], area(v));
    assert_eq!(plane.at(4000, -3000) >> GRADIENT_BITS, 123_456_789);
}
