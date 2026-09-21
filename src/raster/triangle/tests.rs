// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La preuve que deux triangles partagent une arête sans trou ni recouvrement.
//!
//! Dans son fichier parce qu'il pèse plus que le rasteriseur qu'il vérifie, et
//! sous-module parce que les fonctions de bord, le biais et la classification
//! haut-gauche sont privés : un fichier extérieur ne les verrait pas.
//!
//! Chacun de ces tests a été vu échouer. Neutraliser le biais fait tomber les
//! cinq cas de partage ; le raccourcir en `dy <= 0` fait tomber le cas
//! horizontal et les positions sous-pixel, et eux seuls.

use alloc::vec;
use alloc::vec::Vec;

use super::*;
use crate::testing::Rng;

/// Largeur de la fenêtre des tests : assez petite pour que le balayage de
/// référence reste instantané, assez grande pour porter des arêtes obliques.
const W: i32 = 50;

/// Hauteur de la fenêtre des tests.
const H: i32 = 40;

/// La fenêtre des tests, à l'origine de l'image.
const CLIP: Rect = Rect {
    x: 0,
    y: 0,
    width: W as u32,
    height: H as u32,
};

/// Prépare puis remplit, comme le fait une image entière. La profondeur est
/// constante : l'étanchéité ne porte que sur les positions.
fn fill_triangle<T: Target>(target: &mut T, window: Rect, v: [Point; 3], color: u32) {
    let vertices = v.map(|position| Vertex {
        position,
        z: 1 << 31,
        s: 0,
        t: 0,
    });
    if let Some(triangle) = prepare(vertices, color) {
        fill(target, window, &triangle);
    }
}

/// Une fenêtre qui ne part pas de l'origine borne le parcours sans changer
/// un seul pixel : la couverture d'un triangle découpé en quatre quarts est
/// exactement celle du triangle entier. C'est la propriété dont les tuiles
/// dépendent.
#[test]
fn quatre_fenetres_couvrent_comme_une_seule() {
    let v = [
        shift([p(25, 2)], 3, 11)[0],
        shift([p(3, 30)], 7, 9)[0],
        shift([p(47, 37)], 5, 1)[0],
    ];
    let mut whole = Coverage::new();
    fill_triangle(&mut whole, CLIP, v, 1);

    let mut quarters = Coverage::new();
    let quarter = [
        (0, 0, 17, 13),
        (17, 0, 33, 13),
        (0, 13, 17, 27),
        (17, 13, 33, 27),
    ];
    for (x, y, width, height) in quarter {
        let window = Rect {
            x,
            y,
            width,
            height,
        };
        fill_triangle(&mut quarters, window, v, 1);
    }

    assert!(whole.stats().1 > 0, "le cas de test ne couvre rien");
    assert_eq!(quarters.hits, whole.hits);
}

/// Compte les écritures par pixel.
///
/// Comparer deux images ne servirait à rien : un pixel écrit deux fois rend
/// exactement la même couleur qu'un pixel écrit une fois. C'est le comptage,
/// et lui seul, qui rend un recouvrement visible.
struct Coverage {
    hits: Vec<u16>,
}

impl Coverage {
    /// Une couverture vide, à la taille de la fenêtre.
    fn new() -> Self {
        Self {
            hits: vec![0; (W * H) as usize],
        }
    }

    /// Pixels touchés, écritures totales, et maximum sur un pixel.
    ///
    /// Les trois vont ensemble : un trou et un recouvrement se compensent
    /// dans le total, et le maximum seul ne dit rien des pixels manquants.
    fn stats(&self) -> (u32, u32, u16) {
        let touched = self.hits.iter().filter(|n| **n > 0).count() as u32;
        let total = self.hits.iter().map(|n| *n as u32).sum();
        let max = self.hits.iter().copied().max().unwrap_or(0);
        (touched, total, max)
    }
}

impl Target for Coverage {
    fn put(&mut self, x: i32, y: i32, _z: u32, _color: u32) {
        assert!(
            (0..W).contains(&x) && (0..H).contains(&y),
            "écriture hors fenêtre en ({x}, {y})"
        );
        self.hits[(y * W + x) as usize] += 1;
    }
}

/// Un point, en pixels entiers convertis en sous-pixels.
fn p(x: i32, y: i32) -> Point {
    Point {
        x: x * SUBPIXEL_SCALE,
        y: y * SUBPIXEL_SCALE,
    }
}

/// Translate un polygone, en sous-pixels.
fn shift<const N: usize>(poly: [Point; N], dx: i32, dy: i32) -> [Point; N] {
    poly.map(|q| Point {
        x: q.x + dx,
        y: q.y + dy,
    })
}

/// La couverture attendue d'un quadrilatère convexe, par balayage direct de
/// ses quatre arêtes.
///
/// Référence indépendante du découpage : mêmes fonctions de bord, même
/// biais, mais aucune diagonale. Si les deux triangles la reproduisent
/// exactement, l'arête qu'ils partagent est étanche.
fn quad_area(q: [Point; 4]) -> u32 {
    let mut count = 0;
    for y in 0..H {
        for x in 0..W {
            let px = x * SUBPIXEL_SCALE + PIXEL_CENTER;
            let py = y * SUBPIXEL_SCALE + PIXEL_CENTER;
            let inside = (0..4).all(|i| {
                let (a, b) = (q[i], q[(i + 1) % 4]);
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                // La même règle niée que `fill`, biais pris sur `-d` : la
                // référence doit suivre la convention du moteur, sinon elle ne
                // mesurerait que l'écart entre deux conventions.
                -edge(a.x, a.y, b.x, b.y, px, py) + bias(-dx, -dy) >= 0
            });
            if inside {
                count += 1;
            }
        }
    }
    count
}

/// Retourne le sens de parcours d'un quadrilatère.
///
/// Les cas de test s'écrivent dans l'ordre de lecture — haut-gauche,
/// haut-droite, bas-droite, bas-gauche —, qui est horaire à l'écran ; la face
/// avant du moteur est l'autre. Le retournement se fait donc en un point.
fn reversed(q: [Point; 4]) -> [Point; 4] {
    [q[0], q[3], q[2], q[1]]
}

/// Vrai si le quadrilatère est convexe et parcouru dans le sens horaire.
fn is_convex(q: [Point; 4]) -> bool {
    (0..4).all(|i| {
        let (a, b, c) = (q[i], q[(i + 1) % 4], q[(i + 2) % 4]);
        edge(a.x, a.y, b.x, b.y, c.x, c.y) > 0
    })
}

/// Remplit les deux triangles d'un découpage et rend leur couverture.
fn split(q: [Point; 4], first: usize) -> Coverage {
    let i = |k: usize| q[(first + k) % 4];
    let mut cov = Coverage::new();
    fill_triangle(&mut cov, CLIP, [i(0), i(1), i(2)], 1);
    fill_triangle(&mut cov, CLIP, [i(0), i(2), i(3)], 1);
    cov
}

/// Le cœur de la preuve : les deux découpages d'un quadrilatère couvrent
/// exactement son aire, chaque pixel une fois et une seule.
///
/// La marge est nulle et doit le rester : tout est entier, la référence
/// s'évalue avec les mêmes fonctions de bord, et il n'y a aucune source
/// d'écart légitime. Une tolérance d'un seul pixel masquerait exactement le
/// défaut cherché.
fn assert_seamless(q: [Point; 4], label: &str) {
    let q = reversed(q);
    let expected = quad_area(q);
    assert!(expected > 0, "{label} : le cas de test ne couvre rien");

    for first in [0usize, 1] {
        let (touched, total, max) = split(q, first).stats();
        assert_eq!(max, 1, "{label} (diagonale {first}) : recouvrement");
        assert_eq!(touched, expected, "{label} (diagonale {first}) : trou");
        assert_eq!(total, expected, "{label} (diagonale {first}) : total");
    }
}

/// Les cas nommés se décalent d'un demi-pixel, et ce décalage fait tout leur
/// intérêt.
///
/// Sur des coordonnées entières, une arête tombe sur un multiple de la
/// grille et ne passe par aucun centre de pixel : le biais n'y décide rien,
/// et le cas reste vert même quand la règle top-left est neutralisée. Décalé
/// de `PIXEL_CENTER`, il place l'arête pile sur la ligne des centres, là où
/// le biais est seul à départager.
const ON_CENTERS: i32 = PIXEL_CENTER;

/// Un quadrilatère à arêtes obliques, le cas de base.
#[test]
fn partage_une_arete_diagonale() {
    let rect = [p(10, 10), p(40, 10), p(40, 30), p(10, 30)];
    assert_seamless(shift(rect, ON_CENTERS, ON_CENTERS), "rectangle");

    let lozenge = [p(25, 6), p(44, 20), p(25, 34), p(6, 20)];
    assert_seamless(shift(lozenge, ON_CENTERS, 0), "losange");
}

/// L'arête horizontale, que la règle top-left rate le plus souvent : les
/// deux triangles la voient avec `dy == 0` et ne se départagent que par le
/// signe de `dx`.
#[test]
fn partage_une_arete_horizontale() {
    let lozenge = [p(25, 6), p(44, 20), p(25, 34), p(6, 20)];
    assert_seamless(shift(lozenge, 0, ON_CENTERS), "horizontale");
}

/// L'arête verticale, symétrique du cas précédent.
#[test]
fn partage_une_arete_verticale() {
    let lozenge = [p(6, 20), p(25, 6), p(44, 20), p(25, 34)];
    assert_seamless(shift(lozenge, ON_CENTERS, 0), "verticale");
}

/// Les deux cent cinquante-six positions sous-pixel.
///
/// Un biais faux ne se voit souvent qu'à un décalage sur seize, quand une
/// arête tombe exactement sur une colonne ou une ligne de centres de pixels.
#[test]
fn partage_une_arete_a_toutes_les_positions_sous_pixel() {
    let base = [p(25, 6), p(44, 20), p(25, 34), p(6, 20)];
    for dy in 0..SUBPIXEL_SCALE {
        for dx in 0..SUBPIXEL_SCALE {
            assert_seamless(shift(base, dx, dy), "sous-pixel");
        }
    }
}

/// Un quadrilatère à cheval sur le bord : la fenêtre borne la boucle, elle
/// ne change aucune valeur, donc le partage doit tenir à l'identique.
#[test]
fn partage_une_arete_au_bord_de_l_image() {
    let base = [p(25, 6), p(44, 20), p(25, 34), p(6, 20)];
    for (dx, dy) in [(-15, -10), (20, 12), (-30, 0), (0, 25)] {
        let shifted = shift(base, dx * SUBPIXEL_SCALE, dy * SUBPIXEL_SCALE);
        if quad_area(reversed(shifted)) == 0 {
            continue;
        }
        assert_seamless(shifted, "bord");
    }
}

/// Un éventail de six triangles autour d'un centre.
///
/// C'est là qu'un biais asymétrique se trahit : entre deux triangles, deux
/// erreurs peuvent se compenser ; entre six, elles s'accumulent.
#[test]
fn un_eventail_couvre_son_hexagone_sans_recouvrement() {
    let center = p(25, 20);
    // Antihoraire à l'écran, comme toute face avant : le sommet du haut, puis
    // vers la gauche.
    let rim = [
        p(25, 6),
        p(13, 13),
        p(13, 27),
        p(25, 34),
        p(37, 27),
        p(37, 13),
    ];

    let mut fan = Coverage::new();
    for i in 0..6 {
        fill_triangle(&mut fan, CLIP, [center, rim[i], rim[(i + 1) % 6]], 1);
    }

    // Référence : l'hexagone découpé autrement, en deux trapèzes.
    let mut reference = Coverage::new();
    fill_triangle(&mut reference, CLIP, [rim[0], rim[1], rim[2]], 1);
    fill_triangle(&mut reference, CLIP, [rim[0], rim[2], rim[3]], 1);
    fill_triangle(&mut reference, CLIP, [rim[0], rim[3], rim[4]], 1);
    fill_triangle(&mut reference, CLIP, [rim[0], rim[4], rim[5]], 1);

    let (touched, total, max) = fan.stats();
    assert_eq!(max, 1, "éventail : recouvrement");
    assert_eq!(total, touched, "éventail : total");

    let (expected, _, max_ref) = reference.stats();
    assert_eq!(max_ref, 1, "la référence elle-même se recouvre");
    assert_eq!(touched, expected, "éventail : couverture différente");
}

/// Un triangle d'aire nulle n'écrit rien et ne panique pas.
///
/// Le test d'aire est obligatoire et non défensif : les équations de plan
/// des attributs diviseront par cette aire, et une division entière par zéro
/// panique dans les deux profils.
#[test]
fn un_triangle_degenere_n_ecrit_rien() {
    let cases = [
        ([p(10, 10), p(20, 20), p(30, 30)], "colinéaires"),
        ([p(10, 10), p(10, 10), p(30, 20)], "sommets confondus"),
        ([p(10, 10), p(10, 10), p(10, 10)], "point unique"),
    ];

    for (v, label) in cases {
        let mut cov = Coverage::new();
        fill_triangle(&mut cov, CLIP, v, 1);
        assert_eq!(cov.stats().1, 0, "{label}");
    }
}

/// Un triangle parcouru à l'envers est le dos d'une face : il n'est pas
/// rendu. Ce qui est décidé s'asserte, sans quoi une inversion accidentelle
/// passerait inaperçue.
#[test]
fn un_triangle_de_dos_n_est_pas_rendu() {
    let mut cov = Coverage::new();
    // Horaire à l'écran, donc le dos : la face avant est antihoraire dans les
    // données, et le moteur la rend en niant les fonctions de bord.
    fill_triangle(&mut cov, CLIP, [p(10, 10), p(40, 10), p(10, 30)], 1);
    assert_eq!(cov.stats().1, 0);

    // Et son miroir est bien rendu, sans quoi ce test passerait aussi sur un
    // moteur qui n'affiche plus rien.
    let mut face = Coverage::new();
    fill_triangle(&mut face, CLIP, [p(10, 10), p(10, 30), p(40, 10)], 1);
    assert!(face.stats().1 > 0, "la face avant doit être rendue");
}

/// Un triangle entièrement hors de la fenêtre n'écrit rien, et sa boîte
/// vide ne produit aucune boucle.
#[test]
fn un_triangle_hors_fenetre_n_ecrit_rien() {
    let mut cov = Coverage::new();
    fill_triangle(&mut cov, CLIP, [p(-40, -40), p(-10, -40), p(-10, -10)], 1);
    assert_eq!(cov.stats().1, 0);
}

/// Un triangle plus petit qu'un pixel, placé entre deux centres, ne couvre
/// aucun échantillon — et c'est correct : le pixel est échantillonné en son
/// centre, pas par son aire.
#[test]
fn un_triangle_sans_centre_couvert_n_ecrit_rien() {
    let base = Point {
        x: 10 * SUBPIXEL_SCALE + 9,
        y: 10 * SUBPIXEL_SCALE + 9,
    };
    let v = [
        base,
        Point {
            x: base.x + 5,
            y: base.y,
        },
        Point {
            x: base.x + 5,
            y: base.y + 5,
        },
    ];

    let mut cov = Coverage::new();
    fill_triangle(&mut cov, CLIP, v, 1);
    assert_eq!(cov.stats().1, 0);
}

/// Des quadrilatères convexes tirés au hasard, chacun découpé selon ses deux
/// diagonales.
///
/// Un tirage trouve mieux qu'une liste écrite à la main les pentes d'arête
/// qui tombent pile sur une colonne de centres, et la propriété ne demande
/// aucune aire analytique : chaque découpage est la référence de l'autre.
#[test]
fn des_quadrilateres_aleatoires_se_decoupent_sans_couture() {
    let mut tested = 0;

    for seed in 1..=400u64 {
        let mut rng = Rng::new(seed);
        let q = [
            Point {
                x: rng.coord(2 * SUBPIXEL_SCALE, 24 * SUBPIXEL_SCALE),
                y: rng.coord(2 * SUBPIXEL_SCALE, 18 * SUBPIXEL_SCALE),
            },
            Point {
                x: rng.coord(26 * SUBPIXEL_SCALE, 47 * SUBPIXEL_SCALE),
                y: rng.coord(2 * SUBPIXEL_SCALE, 18 * SUBPIXEL_SCALE),
            },
            Point {
                x: rng.coord(26 * SUBPIXEL_SCALE, 47 * SUBPIXEL_SCALE),
                y: rng.coord(21 * SUBPIXEL_SCALE, 37 * SUBPIXEL_SCALE),
            },
            Point {
                x: rng.coord(2 * SUBPIXEL_SCALE, 24 * SUBPIXEL_SCALE),
                y: rng.coord(21 * SUBPIXEL_SCALE, 37 * SUBPIXEL_SCALE),
            },
        ];

        // Le tirage par quadrant ne garantit pas la convexité : un quad
        // concave rendrait la référence fausse, pas le rasteriseur.
        if !is_convex(q) {
            continue;
        }

        let expected = quad_area(reversed(q));
        if expected == 0 {
            continue;
        }

        for first in [0usize, 1] {
            let (touched, total, max) = split(reversed(q), first).stats();
            assert_eq!(max, 1, "graine {seed}, diagonale {first} : recouvrement");
            assert_eq!(touched, expected, "graine {seed}, diagonale {first} : trou");
            assert_eq!(total, expected, "graine {seed}, diagonale {first} : total");
        }
        tested += 1;
    }

    assert!(tested > 200, "trop peu de cas retenus : {tested}");
}
