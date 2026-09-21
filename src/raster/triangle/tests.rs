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
use crate::math::fixed::DEPTH_MARGIN;
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
    if let Some(triangle) = prepare(vertices, color, NO_TEXTURE) {
        fill(target, window, &triangle, None);
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
    /// Le comptage porte sur le **test**, pas sur l'écriture : c'est là que
    /// passent toutes les propositions, et un puits qui compterait les
    /// écritures manquerait celles qu'une profondeur rejette — donc tout ce
    /// qu'un recouvrement d'arête produirait.
    fn test(&mut self, x: i32, y: i32, _z: u32) -> bool {
        assert!(
            (0..W).contains(&x) && (0..H).contains(&y),
            "écriture hors fenêtre en ({x}, {y})"
        );
        self.hits[(y * W + x) as usize] += 1;
        true
    }

    fn write(&mut self, _x: i32, _y: i32, _z: u32, _color: u32) {}
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

/// Le span rend exactement l'ensemble que le test des trois fonctions de bord
/// retiendrait, ligne par ligne, sur des triangles tirés au hasard.
///
/// C'est la propriété dont dépend tout le schéma des segments de perspective :
/// une extrémité qui déborderait ferait diviser la profondeur là où elle n'a
/// pas de sens, et une extrémité trop courte trouerait le triangle. Le
/// comparer au balayage naïf, et non à une autre formule, est ce qui interdit
/// aux deux de se tromper ensemble.
#[test]
fn le_span_coincide_avec_le_test_par_pixel() {
    let mut rng = Rng::new(0x5A11);
    let mut lignes = 0;
    for _ in 0..3_000 {
        let v = [
            p(rng.coord(-10, W + 10), rng.coord(-10, H + 10)),
            p(rng.coord(-10, W + 10), rng.coord(-10, H + 10)),
            p(rng.coord(-10, W + 10), rng.coord(-10, H + 10)),
        ];
        let vertices = v.map(|position| Vertex {
            position,
            z: 1 << 31,
            s: 0,
            t: 0,
        });
        let Some(triangle) = prepare(vertices, 0, NO_TEXTURE) else {
            continue;
        };

        // Les mêmes bornes et les mêmes fonctions de bord que `fill`, pour que
        // la comparaison porte sur le span seul.
        let (bx0, by0, bx1, by1) = triangle.bounds();
        let x0 = bx0.max(0);
        let x1 = bx1.min(W - 1);
        let y0 = by0.max(0);
        let y1 = by1.min(H - 1);
        if x0 > x1 || y0 > y1 {
            continue;
        }

        let (mut row, step_x, step_y) = setup(&triangle, x0, y0);
        for y in y0..=y1 {
            let naif: Vec<i32> = (x0..=x1)
                .filter(|x| covered(&row, &step_x, (x - x0) as i64))
                .collect();
            match span(&row, &step_x, x0, x1) {
                Some((lo, hi)) => {
                    assert_eq!(
                        (lo, hi),
                        (naif[0], naif[naif.len() - 1]),
                        "graine 0x5A11, ligne {y}"
                    );
                    // Contigu : sans quoi les extrémités seules ne diraient
                    // rien de ce qu'il y a entre elles.
                    assert_eq!(naif.len() as i32, hi - lo + 1, "ligne {y} trouée");
                    lignes += 1;
                }
                None => assert!(
                    naif.is_empty(),
                    "ligne {y} : span vide mais pixels couverts"
                ),
            }
            for i in 0..3 {
                row[i] += step_y[i];
            }
        }
    }
    assert!(lignes > 2_000, "{lignes} lignes, échantillon trop maigre");
}

/// L'erreur des attributs interpolés, mesurée en texels sur un sol qui fuit
/// vers l'horizon.
///
/// C'est la question que le format en virgule fixe laissait ouverte :
/// l'équation de plan se trompe de moins de 64 unités d'attribut, ce qui est
/// une fraction de texel de près, et le facteur `1/d` l'amplifie au loin. La
/// réponse est mesurée ici, et elle est **nulle** : `S` et `D` portent la même
/// erreur relative et leur quotient l'absorbe. Un accumulateur exact par plan —
/// une addition et une comparaison par pixel et par attribut — serait donc
/// payé pour rien.
///
/// Le zéro est un seuil et non une coïncidence à contempler : le jour où il
/// devient un, c'est que quelque chose a changé dans les formats, et c'est ce
/// jour-là qu'il faut rouvrir la question.
///
/// Le sol part du plan proche et file jusqu'à deux cents unités : c'est la
/// géométrie qui maltraite le plus la profondeur, et celle du critère de
/// franchissement de l'étape.
#[test]
fn l_erreur_des_attributs_reste_sous_un_texel() {
    use crate::math::{Projection, Vec3};

    let p = Projection::new(640, 360, 1.0, 0.1).unwrap_or_else(|_| unreachable!());
    // Un sol à 1,7 unité sous la caméra — Y va vers le bas en espace de vue —,
    // texturé à soixante-quatre texels par unité : un carrelage serré, qui rend
    // l'erreur lisible.
    let sol = |devant: f32, cote: f32| {
        p.to_clip(Vec3::new(cote, 1.7, devant), devant * 64.0, cote * 64.0)
            .expect("sommet projetable")
    };
    // L'ordre donne la face avant : au sol, vue d'en haut, c'est le sens
    // horaire à l'écran une fois l'axe Y retourné.
    let corners = [sol(2.0, -3.0), sol(200.0, 40.0), sol(200.0, -40.0)];
    let projected = corners.map(|c| p.to_vertex(c));
    let vertices = projected.map(|v| Vertex {
        position: Point { x: v.x, y: v.y },
        z: v.z,
        s: v.s,
        t: v.t,
    });
    let triangle = prepare(vertices, 0, NO_TEXTURE).expect("sol visible");

    let (x0, y0, x1, y1) = triangle.bounds();
    let (mut pire, mut mesures) = (0i128, 0u32);
    for y in y0.max(0)..=y1.min(359) {
        for x in x0.max(0)..=x1.min(639) {
            let (px, py) = (x * SUBPIXEL_SCALE + PIXEL_CENTER, py_of(y));
            // La face avant a une aire **négative** à l'écran : les poids
            // portent ce signe, et c'est en les niant qu'on retrouve des
            // coordonnées barycentriques positives à l'intérieur.
            let w = barycentric(vertices.map(|v| v.position), px, py).map(|n| -n);
            if w.iter().any(|n| *n < 0) {
                continue;
            }

            // Exact : `S` et `D` étant tous deux affines en espace écran, leur
            // rapport *est* l'interpolation perspective-correcte, et les aires
            // au dénominateur se simplifient.
            let sum = |f: fn(&Vertex) -> i128| (0..3).map(|i| w[i] * f(&vertices[i])).sum::<i128>();
            let (num_s, num_d) = (sum(|v| v.s as i128), sum(|v| v.z as i128));
            if num_d <= 0 {
                continue;
            }
            let exact = (num_s << 20) / num_d;

            let (ex, ey) = ((px - triangle.ref_x) as i64, (py - triangle.ref_y) as i64);
            let s = (triangle.uv[0].at(ex, ey) >> GRADIENT_BITS) as i128;
            let d = (triangle.depth.at(ex, ey) >> GRADIENT_BITS) as i128;
            let obtenu = (s << 20) / d;

            pire = pire.max((obtenu - exact).abs());
            mesures += 1;
        }
    }
    assert!(
        mesures > 10_000,
        "{mesures} pixels, échantillon trop maigre"
    );
    assert_eq!(pire, 0, "écart de {pire} texels sur {mesures} pixels");
}

/// Les trois poids barycentriques non normalisés d'un point, en `i128` : l'aire
/// du sous-triangle opposé à chaque sommet.
fn barycentric(v: [Point; 3], px: i32, py: i32) -> [i128; 3] {
    let (x, y) = (v.map(|p| p.x as i128), v.map(|p| p.y as i128));
    let (px, py) = (px as i128, py as i128);
    let w = |a: usize, b: usize| (x[a] - px) * (y[b] - py) - (y[a] - py) * (x[b] - px);
    [w(1, 2), w(2, 0), w(0, 1)]
}

/// Les profondeurs écrites par un puits, pour comparer deux remplissages
/// autrement que par leur couverture.
struct Depths {
    seen: Vec<(i32, i32, u32)>,
}

impl Target for Depths {
    fn test(&mut self, x: i32, y: i32, z: u32) -> bool {
        self.seen.push((x, y, z));
        true
    }

    fn write(&mut self, _x: i32, _y: i32, _z: u32, _color: u32) {}
}

/// Les trois permutations circulaires d'un même triangle rendent les mêmes
/// pixels avec les mêmes profondeurs, au bit près.
///
/// Une permutation circulaire ne change ni le triangle ni son orientation :
/// deux soumissions du même mur, écrites dans un ordre différent par un
/// exportateur, doivent donner la même image. C'est la seule raison d'être du
/// point de référence canonique — pris sur `v[0]`, l'arrondi des équations de
/// plan changerait en chaque pixel.
#[test]
fn les_permutations_circulaires_rendent_la_meme_image() {
    let mut rng = Rng::new(0xC1FC);
    let mut compares = 0;
    for _ in 0..500 {
        let v = [
            p(rng.coord(0, W), rng.coord(0, H)),
            p(rng.coord(0, W), rng.coord(0, H)),
            p(rng.coord(0, W), rng.coord(0, H)),
        ];
        let z = [
            DEPTH_MARGIN + (rng.next() % 0x4000_0000) as u32,
            DEPTH_MARGIN + (rng.next() % 0x4000_0000) as u32,
            DEPTH_MARGIN + (rng.next() % 0x4000_0000) as u32,
        ];
        let s = [
            rng.coord(-9000, 9000),
            rng.coord(-9000, 9000),
            rng.coord(-9000, 9000),
        ];

        let render = |shift: usize| {
            let k = |i: usize| (i + shift) % 3;
            let vertices = [0, 1, 2].map(|i| Vertex {
                position: v[k(i)],
                z: z[k(i)],
                s: s[k(i)],
                t: -s[k(i)],
            });
            let mut sink = Depths { seen: Vec::new() };
            if let Some(triangle) = prepare(vertices, 1, NO_TEXTURE) {
                fill(&mut sink, CLIP, &triangle, None);
            }
            sink.seen
        };

        let base = render(0);
        if base.is_empty() {
            continue;
        }
        assert_eq!(render(1), base, "permutation de 1");
        assert_eq!(render(2), base, "permutation de 2");
        compares += 1;
    }
    assert!(
        compares > 100,
        "{compares} triangles, échantillon trop maigre"
    );
}

/// Un triangle préparé porte l'index de texture qu'on lui donne, sentinelle
/// comprise.
///
/// Le seul endroit où cet index se vérifie pour l'instant : c'est le contexte
/// qui le distribue, mais ses triangles préparés ne montrent pas leurs champs
/// hors de ce module.
#[test]
fn un_triangle_prepare_porte_son_index_de_texture() {
    let v = [p(2, 2), p(2, 30), p(40, 2)];
    for index in [NO_TEXTURE, 0, 1, 65_534] {
        let vertices = v.map(|position| Vertex {
            position,
            z: 1 << 31,
            s: 0,
            t: 0,
        });
        let triangle = prepare(vertices, 0, index).expect("triangle visible");
        assert_eq!(triangle.texture, index);
    }
}

/// Un puits qui tient vraiment une profondeur, et qui refuse toute écriture
/// que son test n'a pas acceptée juste avant.
struct Strict {
    depth: Vec<u32>,
    tests: u32,
    writes: u32,
    /// Le pixel que le dernier test a accepté, s'il en a accepté un.
    accepted: Option<(i32, i32)>,
}

impl Strict {
    /// Un puits vide, dont tous les pixels sont au fond.
    fn new() -> Self {
        Self {
            depth: vec![0; (W * H) as usize],
            tests: 0,
            writes: 0,
            accepted: None,
        }
    }
}

impl Target for Strict {
    fn test(&mut self, x: i32, y: i32, z: u32) -> bool {
        self.tests += 1;
        let passes = z > self.depth[(y * W + x) as usize];
        self.accepted = passes.then_some((x, y));
        passes
    }

    fn write(&mut self, x: i32, y: i32, z: u32, _color: u32) {
        assert_eq!(
            self.accepted,
            Some((x, y)),
            "écriture en ({x}, {y}) sans test accepté"
        );
        self.writes += 1;
        self.depth[(y * W + x) as usize] = z;
        self.accepted = None;
    }
}

/// Un pixel occulté est testé mais pas écrit.
///
/// C'est tout l'objet de la scission : entre le test et l'écriture viendra
/// l'échantillonnage de la texture, et le faire pour un pixel qu'une
/// profondeur rejette serait payer le plus cher du remplissage pour rien. Un
/// puits qui écrirait sans test accepté est attrapé par l'assertion de `write`.
#[test]
fn un_pixel_occulte_est_teste_mais_pas_ecrit() {
    let devant = [p(2, 2), p(2, 34), p(44, 2)];
    let mut sink = Strict::new();

    // Le même triangle deux fois : le premier passe partout, le second est
    // derrière lui au sens strict du test, donc refusé partout.
    let proche = devant.map(|position| Vertex {
        position,
        z: 3 << 30,
        s: 0,
        t: 0,
    });
    let loin = devant.map(|position| Vertex {
        position,
        z: 1 << 30,
        s: 0,
        t: 0,
    });
    for vertices in [proche, loin] {
        let triangle = prepare(vertices, 1, NO_TEXTURE).expect("triangle visible");
        fill(&mut sink, CLIP, &triangle, None);
    }

    let couverts = sink.writes;
    assert!(couverts > 200, "{couverts} pixels, le cas ne couvre rien");
    assert_eq!(
        sink.tests,
        couverts * 2,
        "les deux triangles doivent proposer autant de pixels l'un que l'autre"
    );
    assert_eq!(
        sink.writes, couverts,
        "le triangle du fond a écrit malgré la profondeur"
    );
}

/// Un puits qui garde la couleur écrite par pixel, avec un test de profondeur
/// réel.
struct Paint {
    color: Vec<u32>,
    depth: Vec<u32>,
}

impl Paint {
    /// Un puits vide, tous pixels au fond.
    fn new() -> Self {
        Self {
            color: vec![0; (W * H) as usize],
            depth: vec![0; (W * H) as usize],
        }
    }
}

impl Target for Paint {
    fn test(&mut self, x: i32, y: i32, z: u32) -> bool {
        z > self.depth[(y * W + x) as usize]
    }

    fn write(&mut self, x: i32, y: i32, z: u32, color: u32) {
        self.depth[(y * W + x) as usize] = z;
        self.color[(y * W + x) as usize] = color;
    }
}

/// Une texture dont chaque texel porte ses propres coordonnées : le texel
/// `(u, v)` vaut `u | v << 8`, si bien qu'un pixel dit lequel il a lu.
fn addressed(side: u32) -> Texture {
    let mut bytes = Vec::new();
    for v in 0..side {
        for u in 0..side {
            bytes.extend_from_slice(&[u as u8, v as u8, 0, 0xFF]);
        }
    }
    Texture::load(side, side, &bytes).expect("texture valide")
}

/// Un sol texturé vu en perspective, préparé par la chaîne complète, avec les
/// sommets dont il sort : un triangle préparé ne garde que ses plans, et
/// l'interpolation exacte de référence a besoin des valeurs aux sommets.
///
/// `devant` dit jusqu'où le sol fuit et `densite` combien de texels couvrent
/// une unité de monde. Les deux se règlent par test : une fuite lointaine et
/// une densité forte font travailler le mipmap, une fuite courte et une
/// densité faible gardent tout au niveau 0, ce qu'il faut pour mesurer
/// l'interpolation seule.
fn sol_texture_fuyant(devant: f32, densite: f32) -> (Prepared, [Vertex; 3]) {
    use crate::math::{Projection, Vec3};

    let p = Projection::new(W as u32, H as u32, 1.0, 0.1).unwrap_or_else(|_| unreachable!());
    let sol = |avant: f32, cote: f32| {
        p.to_clip(Vec3::new(cote, 1.2, avant), avant * densite, cote * densite)
            .expect("sommet projetable")
    };
    let large = devant / 3.0;
    let corners = [sol(1.5, -2.0), sol(devant, large), sol(devant, -large)];
    let vertices = corners.map(|c| {
        let v = p.to_vertex(c);
        Vertex {
            position: Point { x: v.x, y: v.y },
            z: v.z,
            s: v.s,
            t: v.t,
        }
    });
    (prepare(vertices, 0, 0).expect("sol visible"), vertices)
}

/// **Le test central du lot.** Les segments de perspective s'alignent sur la
/// grille de l'image, pas sur la fenêtre où l'on parcourt.
///
/// Rendu en quatre fenêtres, le même sol doit donner exactement les mêmes
/// couleurs qu'en une seule. Un segment qui repartirait du bord de la fenêtre
/// diviserait à d'autres abscisses, et la même surface se texturerait autrement
/// selon la taille des tuiles — une couture que la conformance ne verrait que
/// dans une configuration.
#[test]
fn les_segments_s_alignent_sur_la_grille_de_l_image() {
    let (triangle, _) = sol_texture_fuyant(60.0, 96.0);
    let texture = addressed(64);

    let mut entier = Paint::new();
    fill(&mut entier, CLIP, &triangle, Some(&texture));

    let mut morceaux = Paint::new();
    for (x, y, width, height) in [
        (0, 0, 17, 13),
        (17, 0, 33, 13),
        (0, 13, 17, 27),
        (17, 13, 33, 27),
    ] {
        let window = Rect {
            x,
            y,
            width,
            height,
        };
        fill(&mut morceaux, window, &triangle, Some(&texture));
    }

    let peints = entier.color.iter().filter(|c| **c != 0).count();
    assert!(peints > 200, "{peints} pixels, le cas ne couvre rien");
    assert!(
        morceaux.color == entier.color,
        "le découpage de la fenêtre a changé la texture"
    );
}

/// L'interpolation affine entre deux divisions ne s'écarte pas de la
/// perspective exacte de plus d'un texel.
///
/// C'est ce que le segment de seize pixels achète : une division pour seize
/// pixels au lieu d'une par pixel. Le comparer à l'exact, calculé en `i128`
/// sur les mêmes sommets, est ce qui dit si le compromis tient — et c'est le
/// même critère qu'un sol qui fuit vers l'horizon impose.
#[test]
fn l_interpolation_par_segments_reste_sous_le_texel() {
    let (triangle, vertices) = sol_texture_fuyant(8.0, 6.0);
    let side = 64;
    let texture = addressed(side);

    let mut paint = Paint::new();
    fill(&mut paint, CLIP, &triangle, Some(&texture));

    let (mut pire, mut mesures) = (0i128, 0u32);
    // Le quart bas de l'image seulement : c'est le premier plan, où le sol est
    // agrandi et le niveau de mipmap reste le zéro. Plus loin, le texel lu est
    // une moyenne dont la coordonnée n'est plus celle du niveau zéro, et la
    // comparaison ne porterait plus sur l'interpolation mais sur le filtrage.
    for y in (H * 3 / 4)..H {
        for x in 0..W {
            let got = paint.color[(y * W + x) as usize];
            if got == 0 {
                continue;
            }
            let (px, py) = (x * SUBPIXEL_SCALE + PIXEL_CENTER, py_of(y));
            let w = barycentric(triangle.v, px, py).map(|n| -n);
            if w.iter().any(|n| *n < 0) {
                continue;
            }
            let sum = |f: fn(&Vertex) -> i128| (0..3).map(|i| w[i] * f(&vertices[i])).sum::<i128>();
            let num_d = sum(|v| i128::from(v.z));
            if num_d <= 0 {
                continue;
            }
            // `S / D` est l'interpolation perspective-correcte exacte, les aires
            // au dénominateur se simplifiant. Ramenée en 16.16 comme les
            // coordonnées du remplissage : `S` est en 14.12 et `D` en 0.32, donc
            // le quotient se décale de 36 bits.
            let exact = (sum(|v| i128::from(v.s)) << 36) / num_d;

            // Le texel lu porte ses coordonnées : `u` est son octet de poids
            // faible, replié comme le remplissage l'a replié.
            let lu = (got & 0xFF) as i128;
            let attendu = (exact >> 16).rem_euclid(side as i128);
            let ecart = (lu - attendu).rem_euclid(side as i128);
            pire = pire.max(ecart.min(side as i128 - ecart));
            mesures += 1;
        }
    }
    assert!(mesures > 60, "{mesures} pixels, échantillon trop maigre");
    assert!(pire <= 1, "écart de {pire} texels sur {mesures} pixels");
}

/// La table de tramage est une permutation des seize niveaux, et somme à zéro.
///
/// La permutation dit plus que la somme : une entrée recopiée sur sa voisine,
/// compensée ailleurs, laisserait la somme nulle tout en laissant un niveau
/// jamais visité — donc un biais directionnel dans le bloc de quatre.
#[test]
fn la_table_de_tramage_est_une_permutation_de_somme_nulle() {
    assert_eq!(DITHER.iter().map(|&d| i64::from(d)).sum::<i64>(), 0);

    let mut vus: Vec<i32> = DITHER.to_vec();
    vus.sort_unstable();
    let attendus: Vec<i32> = (0..16).map(|k| (2 * k - 15) << 11).collect();
    assert_eq!(vus, attendus);
}

/// Le décalage reste sous le demi-texel, des deux côtés.
///
/// C'est l'amplitude qui sépare le tramage du bruit : au-delà, un pixel lirait
/// un texel à deux de distance et détruirait en minification ce que le mipmap
/// vient de moyenner.
#[test]
fn le_tramage_deplace_de_moins_d_un_demi_texel() {
    let demi = 1 << (UV_BITS - 1);
    for &d in &DITHER {
        assert!(d.abs() < demi, "décalage de {d}, demi-texel à {demi}");
    }
}

/// `v` prend la transposée de l'index de `u`, et les deux ne coïncident que
/// sur la diagonale du bloc.
///
/// Avec le même index, le déplacement serait toujours porté par la diagonale de
/// l'espace de texture : sur une surface où `u` vaut `v`, la texture ne
/// montrerait que sa propre diagonale, et tout le reste serait invisible.
#[test]
fn le_tramage_de_v_est_la_transposee_de_celui_de_u() {
    let mut egaux = 0;
    for y in 0..4 {
        for x in 0..4 {
            let [du, dv] = dither_offsets(x, y);
            assert_eq!(dv, dither_offsets(y, x)[0], "en ({x}, {y})");
            if du == dv {
                egaux += 1;
            }
        }
    }
    assert_eq!(egaux, 4, "seule la diagonale du bloc doit coïncider");
}

/// **Le test que la conformance ne peut pas remplacer.** Le motif de tramage
/// suit la position dans l'image, pas celle dans la fenêtre.
///
/// Les deux tailles de tuile de la suite de conformance, 32 et 64, sont des
/// multiples de quatre : un index pris sur la position locale à la tuile
/// rendrait exactement la même image dans ses cinq passes, et `make conform`
/// resterait vert. Seules des fenêtres commençant à des abscisses **non
/// multiples de quatre** — 17 et 13 ici, choisies pour cela — font apparaître
/// le décalage du motif.
#[test]
fn le_tramage_ne_depend_pas_du_decoupage() {
    let (triangle, _) = sol_texture_fuyant(12.0, 10.0);
    let texture = addressed(64);

    let mut entier = Paint::new();
    fill(&mut entier, CLIP, &triangle, Some(&texture));

    let mut morceaux = Paint::new();
    for (x, y, width, height) in [
        (0, 0, 17, 13),
        (17, 0, 33, 13),
        (0, 13, 17, 27),
        (17, 13, 33, 27),
    ] {
        let window = Rect {
            x,
            y,
            width,
            height,
        };
        fill(&mut morceaux, window, &triangle, Some(&texture));
    }

    let peints = entier.color.iter().filter(|c| **c != 0).count();
    assert!(peints > 200, "{peints} pixels, le cas ne couvre rien");
    assert!(
        morceaux.color == entier.color,
        "le motif de tramage a suivi la fenêtre"
    );
}
