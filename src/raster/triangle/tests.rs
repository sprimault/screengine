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
use crate::light::MAX_OVERBRIGHT;
use crate::math::fixed::DEPTH_MARGIN;
use crate::testing::Rng;

/// Le filtrage par défaut, qui est celui que ces tests éprouvent.
fn dithered(texture: &Texture) -> Option<Sampling<'_>> {
    Some(Sampling {
        texture,
        filter: Filter::Dither,
    })
}

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
    let vertices = v.map(|position| nu(position, 1 << 31));
    if let Some(triangle) = prepare(vertices, color, NO_TEXTURE) {
        fill(target, window, &triangle, None, None);
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

    /// Une surface modulée se compte comme une autre : c'est la couverture
    /// qu'on mesure ici, et le mode d'écriture n'y change rien.
    fn test_modulated(&mut self, x: i32, y: i32, z: u32) -> bool {
        self.test(x, y, z)
    }

    fn modulate(&mut self, _x: i32, _y: i32, _factor: u32) {}
}

/// Un point, en pixels entiers convertis en sous-pixels.
fn p(x: i32, y: i32) -> Point {
    Point {
        x: x * SUBPIXEL_SCALE,
        y: y * SUBPIXEL_SCALE,
    }
}

/// Un sommet nu à un point déjà en sous-pixels.
///
/// Ces tests raisonnent en points ; le constructeur prend deux coordonnées,
/// parce que ses autres appelants n'ont pas de `Point` sous la main.
fn nu(at: Point, z: u32) -> Vertex {
    Vertex::plain(at.x, at.y, z)
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
        let vertices = v.map(|position| nu(position, 1 << 31));
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
        p.to_clip(
            Vec3::new(cote, 1.7, devant),
            devant * 64.0,
            cote * 64.0,
            0.0,
            0.0,
            [0.0; 3],
        )
        .expect("sommet projetable")
    };
    // L'ordre donne la face avant : au sol, vue d'en haut, c'est le sens
    // horaire à l'écran une fois l'axe Y retourné.
    let corners = [sol(2.0, -3.0), sol(200.0, 40.0), sol(200.0, -40.0)];
    let projected = corners.map(|c| p.to_vertex(c));
    let vertices = projected.map(Vertex::from);
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

    fn test_modulated(&mut self, x: i32, y: i32, z: u32) -> bool {
        self.test(x, y, z)
    }

    fn modulate(&mut self, _x: i32, _y: i32, _factor: u32) {}
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
            let vertices = [0, 1, 2].map(|i| nu(v[k(i)], z[k(i)]).uv(s[k(i)], -s[k(i)]));
            let mut sink = Depths { seen: Vec::new() };
            if let Some(triangle) = prepare(vertices, 1, NO_TEXTURE) {
                fill(&mut sink, CLIP, &triangle, None, None);
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
        let vertices = v.map(|position| nu(position, 1 << 31));
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

    /// Non strict, et c'est la seule différence avec `test` : c'est elle que ce
    /// puits sert à éprouver, la tache coplanaire devant passer là où le test
    /// strict la rejetterait.
    fn test_modulated(&mut self, x: i32, y: i32, z: u32) -> bool {
        self.tests += 1;
        let passes = z >= self.depth[(y * W + x) as usize];
        self.accepted = passes.then_some((x, y));
        passes
    }

    /// La profondeur ne bouge pas : c'est ce que ce puits vérifie, en la
    /// laissant telle quelle pour que le test suivant la retrouve.
    fn modulate(&mut self, x: i32, y: i32, _factor: u32) {
        assert_eq!(
            self.accepted,
            Some((x, y)),
            "modulation en ({x}, {y}) sans test accepté"
        );
        self.writes += 1;
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
    let proche = devant.map(|position| nu(position, 3 << 30));
    let loin = devant.map(|position| nu(position, 1 << 30));
    for vertices in [proche, loin] {
        let triangle = prepare(vertices, 1, NO_TEXTURE).expect("triangle visible");
        fill(&mut sink, CLIP, &triangle, None, None);
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

    fn test_modulated(&mut self, x: i32, y: i32, z: u32) -> bool {
        z >= self.depth[(y * W + x) as usize]
    }

    fn modulate(&mut self, x: i32, y: i32, factor: u32) {
        let i = (y * W + x) as usize;
        self.color[i] = crate::light::modulate(self.color[i], factor, 0);
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
        p.to_clip(
            Vec3::new(cote, 1.2, avant),
            avant * densite,
            cote * densite,
            0.0,
            0.0,
            [0.0; 3],
        )
        .expect("sommet projetable")
    };
    let large = devant / 3.0;
    let corners = [sol(1.5, -2.0), sol(devant, large), sol(devant, -large)];
    let vertices = corners.map(|c| Vertex::from(p.to_vertex(c)));
    (prepare(vertices, 0, 0).expect("sol visible"), vertices)
}

/// Le filtre choisi arrive jusqu'au pixel, et les deux modes rendent bien deux
/// images différentes.
///
/// Le critère est ce qui sépare les deux techniques : sur une surface agrandie,
/// le tramage ne peut rendre que des texels existants — il déplace la
/// coordonnée, il ne fabrique pas de couleur —, alors que le bilinéaire en
/// interpole entre eux. Compter les valeurs distinctes le mesure sans dépendre
/// d'un pixel particulier.
#[test]
fn le_bilineaire_interpole_la_ou_le_tramage_choisit() {
    // Quatre texels de côté sur un sol qui en couvre des dizaines de pixels :
    // l'agrandissement est franc, et c'est là que les deux modes divergent. Le
    // damier donne le contraste maximal — `addressed` n'écarte ses texels que
    // d'une unité par canal, et il n'y a alors rien à interpoler entre eux.
    let (triangle, _) = sol_texture_fuyant(8.0, 4.0);
    let texture = Texture::load(
        4,
        4,
        &(0..16)
            .flat_map(|i: u32| {
                let noir = (i / 4 + i % 4) % 2 == 0;
                [if noir { 0x00 } else { 0xFF }; 3]
                    .into_iter()
                    .chain([0xFF])
            })
            .collect::<Vec<u8>>(),
    )
    .expect("texture valide");

    let rendu = |filter| {
        let mut paint = Paint::new();
        fill(
            &mut paint,
            CLIP,
            &triangle,
            Some(Sampling {
                texture: &texture,
                filter,
            }),
            None,
        );
        paint
    };
    let (tramage, bilineaire) = (rendu(Filter::Dither), rendu(Filter::Bilinear));

    let distinctes = |paint: &Paint| {
        let mut vues = Vec::new();
        for &c in paint.color.iter().filter(|c| **c != 0) {
            if !vues.contains(&c) {
                vues.push(c);
            }
        }
        vues.len()
    };
    let (avec, sans) = (distinctes(&bilineaire), distinctes(&tramage));

    // Le tramage ne rend que des texels existants : les deux du damier, plus
    // les gris que la réduction pose dans les niveaux suivants, que le sol
    // atteint en fuyant. Le bilinéaire, lui, fabrique tout l'intervalle.
    assert!(sans <= 4, "{sans} couleurs : le tramage en a fabriqué");
    assert!(
        avec > 16,
        "{avec} couleurs en bilinéaire contre {sans} : il n'interpole pas"
    );
    assert!(
        bilineaire.color != tramage.color,
        "le filtre ne change pas l'image"
    );
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
    fill(&mut entier, CLIP, &triangle, dithered(&texture), None);

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
        fill(&mut morceaux, window, &triangle, dithered(&texture), None);
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
    fill(&mut paint, CLIP, &triangle, dithered(&texture), None);

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
    fill(&mut entier, CLIP, &triangle, dithered(&texture), None);

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
        fill(&mut morceaux, window, &triangle, dithered(&texture), None);
    }

    let peints = entier.color.iter().filter(|c| **c != 0).count();
    assert!(peints > 200, "{peints} pixels, le cas ne couvre rien");
    assert!(
        morceaux.color == entier.color,
        "le motif de tramage a suivi la fenêtre"
    );
}

/// Un segment qui ne touche pas le bout du span prend son appui au pixel
/// suivant, et compte un pas par pixel du segment.
///
/// C'est le cas courant, celui des quinze seizièmes d'une ligne longue : la
/// pente obtenue est celle d'un pas de pixel, et non celle d'un pas de segment.
#[test]
fn un_segment_courant_prend_appui_sur_le_pixel_suivant() {
    assert_eq!(segment_slope_ends(32, 47, 200), (48, 16));
    assert_eq!(segment_slope_ends(0, 15, 16), (16, 16));
}

/// Un segment qui finit sur le bout du span prend son appui sur ce bout, et
/// perd le pas qui lui correspond.
///
/// Les deux vont ensemble : garder seize pas pour quinze intervalles
/// sous-estimerait la pente d'un seizième sur le dernier segment de chaque
/// ligne — le défaut que ce couple corrige. Et prendre appui un pixel plus loin
/// évaluerait `near/w` hors du triangle, où il n'a plus de sens.
#[test]
fn un_segment_qui_finit_au_span_recule_son_appui_et_son_diviseur() {
    assert_eq!(segment_slope_ends(32, 47, 47), (47, 15));
    assert_eq!(segment_slope_ends(0, 9, 9), (9, 9));
}

/// La borne est celle du span, jamais celle de la boîte englobante.
///
/// Sur toute ligne d'un triangle non rectangle, le span s'arrête avant la
/// boîte : c'est là que la distinction se joue, et une borne prise sur la boîte
/// ferait retomber ce cas dans le précédent.
#[test]
fn la_borne_est_le_span_et_non_la_boite() {
    let (sur_le_span, pas_du_span) = segment_slope_ends(32, 47, 47);
    let (au_dela, pas_au_dela) = segment_slope_ends(32, 47, 385);

    assert_eq!((sur_le_span, pas_du_span), (47, 15));
    assert_eq!((au_dela, pas_au_dela), (48, 16));
}

/// Un segment d'un seul pixel au bout du span n'a aucun pas, et rend une pente
/// nulle plutôt qu'une division par zéro.
#[test]
fn un_segment_d_un_seul_pixel_au_bout_du_span_ne_divise_pas_par_zero() {
    let (appui, pas) = segment_slope_ends(47, 47, 47);

    assert_eq!(appui, 47);
    assert_eq!(pas, 1, "le diviseur ne descend jamais à zéro");
}

/// Un triangle texturé **entièrement contenu dans la fenêtre**, pointe à
/// droite.
///
/// La géométrie compte : il faut que le sommet le plus à droite soit visible,
/// donc qu'au moins une ligne porte son span jusqu'à la boîte, et que les
/// autres s'arrêtent avant. Un sol qui déborde l'écran n'irait jamais jusqu'au
/// bout d'une ligne et laisserait le cas intéressant hors du parcours.
fn triangle_pointe_a_droite() -> Prepared {
    use crate::math::{Projection, Vec3};

    let p = Projection::new(W as u32, H as u32, 1.0, 0.1).unwrap_or_else(|_| unreachable!());
    let at = |cote: f32, haut: f32, avant: f32| {
        p.to_clip(
            Vec3::new(cote, haut, avant),
            avant * 32.0,
            cote * 32.0,
            0.0,
            0.0,
            [0.0; 3],
        )
        .expect("sommet projetable")
    };
    // Les deux sommets de droite partagent le même rapport `x/z`, donc la même
    // abscisse à l'écran : le bord droit est vertical, et **chaque** ligne
    // termine son span sur la boîte.
    let corners = [
        at(-1.0, 0.10, 2.0),
        at(0.80, 0.90, 2.0),
        at(0.40, 0.05, 1.0),
    ];
    let vertices = corners.map(|c| Vertex::from(p.to_vertex(c)));
    prepare(vertices, 0, 0).expect("triangle visible")
}

/// Le niveau de mipmap monte avec la distance le long d'une colonne.
///
/// La propriété que tout le filtrage par défaut repose dessus : plus loin, un
/// pixel couvre plus de texels, donc un niveau plus réduit. Elle ne tenait
/// jusqu'ici qu'à une empreinte de conformance, qui change aussi bien pour un
/// rendu faux.
#[test]
fn le_niveau_de_mipmap_monte_avec_la_distance() {
    let (triangle, _) = sol_texture_fuyant(60.0, 96.0);
    let (_, y0, _, y1) = triangle.bounds();

    // Une colonne au milieu du sol, du plus proche au plus lointain : à
    // l'écran, plus loin veut dire plus haut, donc `y` décroissant.
    let x = (triangle.x0 + triangle.x1) / 2;
    let mut niveaux = Vec::new();
    for y in (y0.max(0)..=y1.min(H - 1)).rev() {
        let ey = (py_of(y) - triangle.ref_y) as i64;
        let ex = (x * SUBPIXEL_SCALE + PIXEL_CENTER - triangle.ref_x) as i64;
        let depth = triangle.depth.at(ex, ey) >> GRADIENT_BITS;
        if depth <= 0 {
            continue;
        }
        let w = reciprocal(depth as u32);
        let read = |plane: &Plane| texel_coord(plane.at(ex, ey) >> GRADIENT_BITS, w);
        niveaux.push(mip_level(
            &triangle,
            &triangle.uv,
            read(&triangle.uv[0]),
            read(&triangle.uv[1]),
            w,
        ));
    }

    assert!(niveaux.len() > 8, "{} lignes, trop peu", niveaux.len());
    assert!(
        niveaux.windows(2).all(|w| w[1] >= w[0]),
        "le niveau redescend en s'éloignant : {niveaux:?}"
    );
    assert!(
        niveaux.last() > niveaux.first(),
        "le niveau ne monte pas du tout : {niveaux:?}"
    );
}

/// Le niveau tient compte de la dérivée **verticale**, et pas seulement de
/// l'horizontale.
///
/// C'est le critère de franchissement de l'étape 2, et il se manque exactement
/// là : sur un sol, `∂u/∂x` reste modérée le long d'une ligne alors que
/// `∂u/∂y` explose vers l'horizon. Un niveau choisi sur la seule horizontale
/// sous-sélectionne, et le sol scintille en mouvement — ce qu'aucune image fixe
/// ne montre.
///
/// La comparaison se fait contre un triangle dont les plans de texture n'ont
/// **aucune pente verticale**, la même géométrie par ailleurs : si le niveau ne
/// changeait pas, c'est que la dérivée verticale n'entre pas dans le calcul.
#[test]
fn le_niveau_de_mipmap_tient_compte_de_la_derivee_verticale() {
    let (triangle, _) = sol_texture_fuyant(60.0, 96.0);
    // Le même triangle, dont plus rien ne varie verticalement. La profondeur
    // s'aplatit avec les coordonnées : les deux termes verticaux du critère
    // sont `∂S/∂y − u·∂D/∂y`, et n'annuler que le premier en laisserait la
    // moitié debout — ce qui ne serait plus « sans dérivée verticale ».
    let mut plat = sol_texture_fuyant(60.0, 96.0).0;
    plat.depth.flatten_y();
    for plane in &mut plat.uv {
        plane.flatten_y();
    }

    let (_, y0, _, y1) = triangle.bounds();
    let x = (triangle.x0 + triangle.x1) / 2;
    let (mut mesures, mut distincts) = (0, 0);
    for y in y0.max(0)..=y1.min(H - 1) {
        let ey = (py_of(y) - triangle.ref_y) as i64;
        let ex = (x * SUBPIXEL_SCALE + PIXEL_CENTER - triangle.ref_x) as i64;
        let depth = triangle.depth.at(ex, ey) >> GRADIENT_BITS;
        if depth <= 0 {
            continue;
        }
        let w = reciprocal(depth as u32);
        let read = |plane: &Plane| texel_coord(plane.at(ex, ey) >> GRADIENT_BITS, w);
        let (u, v) = (read(&triangle.uv[0]), read(&triangle.uv[1]));

        let avec = mip_level(&triangle, &triangle.uv, u, v, w);
        let sans = mip_level(&plat, &plat.uv, u, v, w);
        assert!(avec >= sans, "en y={y}, {avec} avec et {sans} sans");
        mesures += 1;
        distincts += u32::from(avec > sans);
    }

    assert!(mesures > 8, "{mesures} lignes, trop peu");
    // Elle ne décide pas partout — près de la caméra, l'horizontale domine —,
    // mais là où elle décide, l'ignorer sous-sélectionne d'un niveau ou plus.
    // C'est exactement le sol qui scintille vers l'horizon.
    assert!(
        distincts > 0,
        "la dérivée verticale ne décide nulle part sur {mesures} lignes"
    );
}

/// L'image ne dépend pas de la boîte englobante du triangle, tant qu'elle
/// majore le span.
///
/// C'est le contrôle du **câblage**, celui que les cas ci-dessus ne peuvent pas
/// faire : ils donnent eux-mêmes la borne à la fonction, et resteraient verts
/// si le remplissage lui passait la boîte au lieu du span. Ici, élargir la
/// boîte sans toucher à un seul pixel couvert doit rendre exactement la même
/// image — ce qui est faux dès que la borne vient de la boîte, puisque la pente
/// du dernier segment de chaque ligne en dépend alors.
///
/// Élargir `x1` est licite et ne change rien d'autre : le span se résout par
/// les fonctions de bord et reste borné par les arêtes, la boîte ne servant
/// qu'à ouvrir le parcours.
#[test]
fn la_boite_englobante_ne_change_pas_la_texture() {
    let texture = addressed(64);

    let serre = triangle_pointe_a_droite();
    let mut etroite = Paint::new();
    fill(&mut etroite, CLIP, &serre, dithered(&texture), None);

    let mut large = triangle_pointe_a_droite();
    large.x1 = serre.x1 + 64;
    let mut desserree = Paint::new();
    fill(&mut desserree, CLIP, &large, dithered(&texture), None);

    let peints = etroite.color.iter().filter(|c| **c != 0).count();
    assert!(peints > 200, "{peints} pixels, le cas ne couvre rien");
    assert!(
        etroite.color == desserree.color,
        "la boîte englobante a changé la texture"
    );
}

/// Le second jeu de coordonnées suit exactement le chemin du premier.
///
/// Le même triangle est préparé deux fois : une fois avec ses coordonnées de
/// lightmap à leur place, une fois avec ces valeurs recopiées à la place des
/// coordonnées de texture. Les plans du tableau annexe doivent alors être ceux
/// de la texture, bit pour bit.
///
/// **C'est plus fort qu'une valeur attendue écrite à la main** : ce qui compte
/// n'est pas que les plans soient justes dans l'absolu, mais qu'ils partagent
/// l'aire, le sommet de référence et l'arrondi du premier jeu. Un sommet de
/// référence choisi séparément donnerait des plans presque identiques, et
/// l'écart n'apparaîtrait que sur quelques pixels d'une scène entière.
#[test]
fn les_plans_du_second_jeu_valent_ceux_du_premier_sur_les_memes_valeurs() {
    let v = [p(3, 2), p(5, 35), p(44, 9)];
    let (s2, t2) = ([1200, -700, 9000], [-40, 6100, 250]);

    let lit = [0, 1, 2].map(|i| nu(v[i], 1 << 31).uv(17, -17).uv2(s2[i], t2[i]));
    let temoin = [0, 1, 2].map(|i| nu(v[i], 1 << 31).uv(s2[i], t2[i]));

    let (triangle, lighting) =
        prepare_lit(lit, 0, NO_TEXTURE, 3, false, 7).expect("triangle visible");
    let temoin = prepare(temoin, 0, NO_TEXTURE).expect("triangle visible");

    assert_eq!(lighting.planes(), &temoin.uv);
    assert_eq!(lighting.lightmap(), 3);
    assert_eq!(triangle.lighting(), 7);
    // Le premier jeu n'a pas bougé pour autant : les deux familles de plans
    // sont indépendantes.
    assert_ne!(triangle.uv, temoin.uv);
}

/// Un triangle préparé sans éclairage ne désigne aucune place dans le tableau
/// annexe, et c'est la sentinelle qui le dit.
#[test]
fn un_triangle_sans_eclairage_porte_la_sentinelle() {
    let v = [p(2, 2), p(2, 30), p(40, 2)];
    let vertices = v.map(|position| nu(position, 1 << 31).uv2(500, -500));
    let triangle = prepare(vertices, 0, NO_TEXTURE).expect("triangle visible");
    assert_eq!(triangle.lighting(), NO_LIGHTING);
}

/// Un triangle vu de dos est refusé par les deux chemins de préparation, et
/// celui qui éclaire ne laisse donc rien à ranger.
#[test]
fn un_triangle_de_dos_ne_reserve_aucune_place() {
    let v = [p(2, 2), p(40, 2), p(2, 30)];
    let vertices = v.map(|position| nu(position, 1 << 31));
    assert!(prepare(vertices, 0, NO_TEXTURE).is_none());
    assert!(prepare_lit(vertices, 0, NO_TEXTURE, 0, false, 0).is_none());
}

/// Un sol éclairé, préparé par la chaîne complète : la texture à `densite`
/// texels par unité, la lightmap à `densite_lightmap`.
///
/// Les deux densités se règlent séparément, et c'est le point : une lightmap
/// est étirée là où une texture se répète, donc les deux jeux n'atteignent
/// jamais le même niveau de mipmap sur la même surface.
fn sol_eclaire(devant: f32, densite: f32, densite_lightmap: f32) -> (Prepared, Lighting) {
    sol_eclaire_glow(devant, densite, densite_lightmap, None)
}

/// Le même sol, dont chaque sommet peut porter une contribution de lumière
/// dynamique.
///
/// `glow` donne l'apport du sommet le plus proche de la caméra ; les deux autres
/// en reçoivent le tiers, pour que l'apport **décroisse le long de la surface**
/// comme le ferait une source posée devant elle. Un apport égal partout rendrait
/// un plan constant, et une rampe qui ne se déplacerait pas d'un pixel à l'autre
/// passerait tous les contrôles sans transporter quoi que ce soit.
///
/// `None` laisse les sommets à zéro **et** `glowing` à faux : c'est le cas des
/// tests d'avant, qui n'éprouvaient que les deux bras sans lumière dynamique.
fn sol_eclaire_glow(
    devant: f32,
    densite: f32,
    densite_lightmap: f32,
    glow: Option<[f32; 3]>,
) -> (Prepared, Lighting) {
    use crate::math::{Projection, Vec3};

    let p = Projection::new(W as u32, H as u32, 1.0, 0.1).unwrap_or_else(|_| unreachable!());
    let sol = |avant: f32, cote: f32, light: [f32; 3]| {
        p.to_clip(
            Vec3::new(cote, 1.2, avant),
            avant * densite,
            cote * densite,
            avant * densite_lightmap,
            cote * densite_lightmap,
            light,
        )
        .expect("sommet projetable")
    };
    let proche = glow.unwrap_or([0.0; 3]);
    let loin = proche.map(|c| c / 3.0);
    let large = devant / 3.0;
    let corners = [
        sol(1.5, -2.0, proche),
        sol(devant, large, loin),
        sol(devant, -large, loin),
    ];
    let vertices = corners.map(|c| Vertex::from(p.to_vertex(c)));
    prepare_lit(vertices, 0xFFFF_FFFF, 0, 0, glow.is_some(), 0).expect("sol visible")
}

/// Une lightmap unie, dont tous les texels valent `value` sur les trois canaux.
fn uniforme(side: u32, value: u8) -> Texture {
    let bytes: Vec<u8> = (0..side * side)
        .flat_map(|_| [value, value, value, 0xFF])
        .collect();
    Texture::load(side, side, &bytes).expect("lightmap valide")
}

/// L'éclairage d'un triangle, pour les cas où seuls la lightmap et le réglage
/// varient.
fn lighting<'a>(lightmap: &'a Texture, planes: &'a Lighting, overbright: u32) -> Option<Lit<'a>> {
    Some(Lit {
        lightmap: Some(lightmap),
        planes,
        overbright,
    })
}

/// **Un triangle non texturé mais éclairé prend le chemin des segments.**
///
/// C'est le chemin qu'on oublie : la couleur unie se peignait sans division de
/// perspective, et l'éclairage en a besoin. Un mur uni éclairé est pourtant le
/// cas le plus courant d'un décor, et sans ce chemin il ressortirait de sa
/// couleur brute — ce que personne ne prendrait pour un défaut, puisque c'est
/// exactement l'image d'avant la lumière.
#[test]
fn un_triangle_uni_eclaire_passe_par_les_segments() {
    let (triangle, planes) = sol_eclaire(30.0, 0.0, 1.0);
    let lightmap = addressed(64);

    let mut peint = Paint::new();
    fill(
        &mut peint,
        CLIP,
        &triangle,
        None,
        lighting(&lightmap, &planes, 0),
    );

    let mut brut = Paint::new();
    fill(&mut brut, CLIP, &triangle, None, None);

    let couverts = peint.color.iter().filter(|c| **c != 0).count();
    assert!(couverts > 200, "{couverts} pixels, le cas ne couvre rien");
    assert!(
        peint.color != brut.color,
        "la lightmap n'a rien changé à un triangle uni"
    );
    // Et l'éclairage varie d'un pixel à l'autre : une lightmap lue en un seul
    // point rendrait un aplat, ce que la comparaison ci-dessus ne verrait pas.
    let teintes = {
        let mut v: Vec<u32> = peint.color.iter().copied().filter(|c| *c != 0).collect();
        v.sort_unstable();
        v.dedup();
        v.len()
    };
    assert!(
        teintes > 20,
        "{teintes} teintes, la lightmap ne dégrade pas"
    );
}

/// Une lightmap blanche est transparente **au niveau du remplissage aussi**,
/// texture comprise.
///
/// Le même contrôle qu'au contexte, mais sur le seul chemin texturé : ici, un
/// écart ne peut venir que de la combinaison, là-bas il pourrait venir de la
/// soumission.
#[test]
fn une_lightmap_blanche_ne_change_pas_le_texel() {
    let (triangle, planes) = sol_eclaire(30.0, 24.0, 1.0);
    let texture = addressed(64);
    let blanche = uniforme(4, 0xFF);

    let mut avec = Paint::new();
    fill(
        &mut avec,
        CLIP,
        &triangle,
        dithered(&texture),
        lighting(&blanche, &planes, 0),
    );
    let mut sans = Paint::new();
    fill(&mut sans, CLIP, &triangle, dithered(&texture), None);

    let couverts = avec.color.iter().filter(|c| **c != 0).count();
    assert!(couverts > 200, "{couverts} pixels, le cas ne couvre rien");
    assert!(avec.color == sans.color, "la lightmap blanche a teinté");
}

/// **Une lightmap minifiée passe par ses mipmaps**, comme une texture.
///
/// C'est l'arbitrage du lot : une lightmap est presque toujours agrandie, mais
/// « presque » n'est pas « toujours » — un mur lointain ou vu très en biais la
/// minifie, et un niveau zéro forcé y scintillerait exactement comme une
/// texture sans chaîne. Le cas construit ici est ce mur : la lightmap y est si
/// dense que chaque pixel en couvre plusieurs texels.
///
/// **Le compte de teintes ne dirait rien** — un damier n'en produit que deux
/// quel que soit le niveau —, c'est leur **valeur** qui sépare les deux cas.
#[test]
fn une_lightmap_minifiee_passe_par_ses_mipmaps() {
    // Des cases de huit texels, et non d'un seul : la lightmap se lit en
    // bilinéaire, qui moyennerait un damier d'un texel avant même qu'un niveau
    // soit choisi. Il faut des plages franches pour que le contraste survive
    // au niveau zéro — sans quoi le test passerait quel que soit le niveau.
    let damier = {
        let (side, cell) = (64u32, 8u32);
        let mut bytes = Vec::new();
        for v in 0..side {
            for u in 0..side {
                let value = if ((u / cell) + (v / cell)) % 2 == 0 {
                    0x20
                } else {
                    0xE0
                };
                bytes.extend_from_slice(&[value, value, value, 0xFF]);
            }
        }
        Texture::load(side, side, &bytes).expect("lightmap valide")
    };

    // Texture absente et couleur blanche : la combinaison rend alors le texel
    // de lightmap presque tel quel, si bien qu'un canal de pixel se lit comme
    // une valeur de lightmap — 32 et 224 si les texels sont lus un à un, 128
    // s'ils ont été moyennés.
    let (triangle, planes) = sol_eclaire(30.0, 0.0, 512.0);
    let mut peint = Paint::new();
    fill(
        &mut peint,
        CLIP,
        &triangle,
        None,
        lighting(&damier, &planes, 0),
    );

    let canaux: Vec<u32> = peint
        .color
        .iter()
        .filter(|c| **c != 0)
        .map(|c| c & 0xFF)
        .collect();
    assert!(
        canaux.len() > 200,
        "{} pixels, le cas ne couvre rien",
        canaux.len()
    );
    let extremes = canaux.iter().filter(|c| **c < 64 || **c > 192).count();
    assert_eq!(
        extremes, 0,
        "{extremes} pixels aux teintes brutes du damier : le niveau n'a pas été pris"
    );
}

/// Le sur-éclairement traverse jusqu'au pixel, et il éclaircit.
#[test]
fn le_sur_eclairement_arrive_jusqu_au_pixel() {
    let (triangle, planes) = sol_eclaire(30.0, 0.0, 1.0);
    let demi = uniforme(4, 0x60);

    let peint = |overbright| {
        let mut paint = Paint::new();
        fill(
            &mut paint,
            CLIP,
            &triangle,
            None,
            lighting(&demi, &planes, overbright),
        );
        paint.color
    };

    let (sans, avec) = (peint(0), peint(MAX_OVERBRIGHT));
    let couverts = sans.iter().filter(|c| **c != 0).count();
    assert!(couverts > 200, "{couverts} pixels, le cas ne couvre rien");
    for (a, b) in sans.iter().zip(&avec) {
        for index in [0u32, 8, 16] {
            let (sombre, clair) = ((a >> index) & 0xFF, (b >> index) & 0xFF);
            assert!(clair >= sombre, "le sur-éclairement a assombri un canal");
        }
    }
    assert!(sans != avec, "le sur-éclairement n'a rien changé");
}

/// **La lightmap se lit toujours en bilinéaire**, quel que soit le filtre du
/// contexte.
///
/// Une lightmap de quatre texels étirée sur toute la surface : en bilinéaire
/// elle rend un dégradé continu, au plus proche voisin quatre aplats séparés
/// par des marches franches. Le tramage ne rattrape pas ce cas — il déplace la
/// coordonnée d'un demi-texel, ce qui ne fait rien contre une marche large de
/// plusieurs dizaines de pixels.
#[test]
fn la_lightmap_se_lit_en_bilineaire_meme_en_tramage() {
    let quatre = Texture::load(
        2,
        2,
        &[
            0x20, 0x20, 0x20, 0xFF, 0xE0, 0x40, 0x40, 0xFF, //
            0x40, 0xE0, 0x40, 0xFF, 0xE0, 0xE0, 0x20, 0xFF,
        ],
    )
    .expect("lightmap valide");

    // Une densité qui étire les deux texels sur toute la fuite du sol : c'est
    // le régime d'agrandissement où une lightmap vit toujours.
    let (triangle, planes) = sol_eclaire(30.0, 0.0, 0.03);
    let mut peint = Paint::new();
    fill(
        &mut peint,
        CLIP,
        &triangle,
        None,
        lighting(&quatre, &planes, 0),
    );

    let mut teintes: Vec<u32> = peint.color.iter().copied().filter(|c| *c != 0).collect();
    let couverts = teintes.len();
    assert!(couverts > 200, "{couverts} pixels, le cas ne couvre rien");
    teintes.sort_unstable();
    teintes.dedup();
    assert!(
        teintes.len() > 50,
        "{} teintes : la lightmap a été lue au plus proche voisin",
        teintes.len()
    );
}

/// Une surface que **seules** des lumières dynamiques éclairent passe par le
/// bras sans lightmap.
///
/// Le cas qu'aucun test du noyau n'atteignait : `Glow<false, true>`. Jusqu'aux
/// scènes `texture-dynamique` et `lumiere-dynamique`, il n'était exercé qu'à
/// l'échelle d'une image entière, où un canal faux de quelques unités ne se
/// distingue pas du bruit du tramage.
///
/// Trois assertions, parce que deux d'entre elles se laissent satisfaire par un
/// bras qui ne ferait rien de la lumière : que l'image change, qu'elle soit plus
/// claire — une contribution s'ajoute, elle ne retranche jamais —, et qu'elle
/// dégrade, puisque l'apport décroît d'un bout à l'autre de la surface.
#[test]
fn une_surface_sans_lightmap_prend_le_bras_des_lumieres_dynamiques() {
    let (triangle, planes) = sol_eclaire_glow(30.0, 0.0, 1.0, Some([0.75, 0.55, 0.30]));

    let mut peint = Paint::new();
    fill(
        &mut peint,
        CLIP,
        &triangle,
        None,
        Some(Lit {
            lightmap: None,
            planes: &planes,
            overbright: 0,
        }),
    );

    let (brut, _) = sol_eclaire_glow(30.0, 0.0, 1.0, None);
    let mut sans = Paint::new();
    fill(&mut sans, CLIP, &brut, None, None);

    let couverts = peint.color.iter().filter(|c| **c != 0).count();
    assert!(couverts > 200, "{couverts} pixels, le cas ne couvre rien");
    assert_ne!(
        peint.color, sans.color,
        "les lumières dynamiques n'ont rien changé à une surface sans lightmap"
    );

    let teintes = {
        let mut v: Vec<u32> = peint.color.iter().copied().filter(|c| *c != 0).collect();
        v.sort_unstable();
        v.dedup();
        v.len()
    };
    // Douze et non vingt, qui est le seuil du test voisin : sa lightmap varie
    // sur deux axes, quand une rampe de lumière ne varie que le long de la
    // surface et se quantifie sur huit bits. La mesure est de dix-sept ; ce
    // qu'il faut séparer, c'est une rampe d'un aplat — qui n'aurait qu'une
    // teinte — et de quelques paliers.
    assert!(
        teintes > 12,
        "{teintes} teintes : l'apport ne décroît pas le long de la surface"
    );
}

/// Une lightmap et des lumières dynamiques s'ajoutent sur la même surface.
///
/// Le quatrième bras, `Glow<true, true>`. Son défaut propre est d'en ignorer une
/// des deux sources : l'image serait alors celle de la lightmap seule, ou celle
/// des lumières seules, et les deux rendent une image parfaitement plausible.
/// C'est pourquoi la comparaison porte sur **les deux** cas simples, et non sur
/// une valeur attendue.
#[test]
fn la_lightmap_et_les_lumieres_dynamiques_s_ajoutent() {
    let lightmap = addressed(64);
    let glow = Some([0.60, 0.45, 0.25]);

    let peindre = |glow: Option<[f32; 3]>, avec_lightmap: bool| {
        let (triangle, planes) = sol_eclaire_glow(30.0, 0.0, 1.0, glow);
        let mut cible = Paint::new();
        fill(
            &mut cible,
            CLIP,
            &triangle,
            None,
            Some(Lit {
                lightmap: avec_lightmap.then_some(&lightmap),
                planes: &planes,
                overbright: 0,
            }),
        );
        cible.color
    };

    let ensemble = peindre(glow, true);
    let lightmap_seule = peindre(None, true);
    let lumieres_seules = peindre(glow, false);

    assert_ne!(
        ensemble, lightmap_seule,
        "les lumières dynamiques sont ignorées quand une lightmap est présente"
    );
    assert_ne!(
        ensemble, lumieres_seules,
        "la lightmap est ignorée quand des lumières dynamiques sont présentes"
    );
}

/// Une texture masquée dont la moitié gauche est transparente : quatre texels
/// de côté, les colonnes 0 et 1 invisibles, les deux autres blanches.
///
/// La frontière tombe au milieu d'un texel pour que le niveau zéro suffise et
/// que rien ne dépende du filtrage : ce sont les écritures qu'on mesure, pas
/// l'échantillonnage.
fn demi_masquee() -> Texture {
    let mut bytes = Vec::new();
    for _ in 0..4 {
        for u in 0..4 {
            let opaque = u >= 2;
            bytes.extend_from_slice(&[0xFF, 0xFF, 0xFF, if opaque { 0xFF } else { 0x00 }]);
        }
    }
    Texture::load_masked(4, 4, &bytes).expect("texture valide")
}

/// Un texel transparent n'écrit **ni la couleur ni la profondeur**.
///
/// La profondeur compte autant que la couleur, et c'est elle qu'on oublie : un
/// texel qui n'est pas peint mais qui inscrit sa profondeur masque ce qui est
/// derrière lui, et la silhouette découpe alors un trou dans le décor sans
/// qu'aucun pixel ne le montre.
#[test]
fn un_texel_transparent_n_ecrit_ni_couleur_ni_profondeur() {
    let (triangle, _) = sol_texture_fuyant(8.0, 1.0);
    let texture = demi_masquee();

    let mut paint = Paint::new();
    fill(&mut paint, CLIP, &triangle, dithered(&texture), None);

    let peints = paint.color.iter().filter(|c| **c != 0).count();
    let profonds = paint.depth.iter().filter(|z| **z != 0).count();
    assert!(peints > 0, "le cas ne peint rien du tout");
    assert_eq!(
        peints, profonds,
        "un pixel non peint a laissé sa profondeur"
    );

    let mut opaque = Paint::new();
    let entiere = addressed(4);
    fill(&mut opaque, CLIP, &triangle, dithered(&entiere), None);
    let couverts = opaque.color.iter().filter(|c| **c != 0).count();
    assert!(
        peints < couverts,
        "la moitié transparente n'a rien retiré : {peints} contre {couverts}"
    );
}

/// Deux surfaces masquées qui se croisent se résolvent par la profondeur, dans
/// n'importe quel ordre de soumission.
///
/// C'est la promesse que la transparence **binaire** laisse vraie, et la
/// raison pour laquelle l'étape ne fait pas de mélange fractionnaire : celui-ci
/// aurait exigé un tri par profondeur, c'est-à-dire ce que le z-buffer a
/// supprimé.
#[test]
fn l_ordre_de_soumission_ne_change_pas_une_scene_masquee() {
    let (proche, _) = sol_texture_fuyant(8.0, 1.0);
    let (loin, _) = sol_texture_fuyant(12.0, 1.0);
    let texture = demi_masquee();

    let peindre = |ordre: [&Prepared; 2]| {
        let mut paint = Paint::new();
        for triangle in ordre {
            fill(&mut paint, CLIP, triangle, dithered(&texture), None);
        }
        paint.color
    };

    assert_eq!(peindre([&proche, &loin]), peindre([&loin, &proche]));
}
