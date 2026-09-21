// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'un triangle par fonctions de bord.
//!
//! Deux triangles qui partagent une arête doivent se partager ses pixels sans
//! trou ni recouvrement. C'est ce que la règle top-left obtient, et c'est le
//! défaut le plus coûteux du projet : invisible à l'arrêt, visible en mouvement
//! comme un scintillement de la couture, et découvert trop tard il est déjà sous
//! tout le reste du moteur.

use crate::math::fixed::{PIXEL_CENTER, SUBPIXEL_SCALE};

use super::plane::{GRADIENT_BITS, Plane};
use super::{Rect, Target};
use crate::texture::{MAX_TEXTURE_SIZE, Texture};

/// Une position projetée, en sous-pixels.
///
/// Deux dimensions seulement : c'est sur elle que portent les fonctions de bord
/// et la règle top-left, et leurs tests ne dépendent ainsi de rien de ce que
/// [`Vertex`] porte en plus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    /// Abscisse en sous-pixels, dans la bande de garde.
    pub x: i32,
    /// Ordonnée en sous-pixels, Y vers le bas.
    pub y: i32,
}

/// Un sommet projeté : sa position et ses attributs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vertex {
    /// La position, en sous-pixels.
    pub position: Point,
    /// La profondeur, `near/w` en 0.32, plus grand est plus proche, bornée par
    /// `to_depth`.
    pub z: u32,
    /// L'abscisse de texture multipliée par la profondeur, en 14.12.
    pub s: i32,
    /// L'ordonnée de texture multipliée par la profondeur, en 14.12.
    pub t: i32,
}

/// Un triangle prêt à être parcouru dans n'importe quelle fenêtre.
///
/// Ce qui ne dépend pas de la fenêtre se calcule une fois, à la soumission ;
/// ce qui en dépend — le point de départ du parcours — se recalcule par la
/// forme close à chaque fenêtre. C'est ce partage qui rend l'image
/// indépendante du découpage : une tuile ne reprend jamais une valeur
/// accumulée par sa voisine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prepared {
    v: [Point; 3],
    /// Pixels extrêmes dont le centre peut être couvert, bornes comprises, sans
    /// limitation par l'image.
    x0: i32,
    x1: i32,
    y0: i32,
    y1: i32,
    /// Le point de référence des trois plans, en sous-pixels.
    ///
    /// Commun, et c'est structurel : les plans d'un même triangle s'évaluent
    /// aux mêmes écarts, qui ne se calculent donc qu'une fois par pixel.
    ref_x: i32,
    ref_y: i32,
    depth: Plane,
    /// Les coordonnées de texture multipliées par la profondeur, `u·d` puis
    /// `v·d`, qui sont affines en espace écran là où `u` et `v` ne le sont pas.
    uv: [Plane; 2],
    color: u32,
    /// L'index de la texture dans la table du contexte, ou [`NO_TEXTURE`].
    texture: u16,
}

/// Cent vingt-huit octets, deux lignes de cache pleines. La répartition par
/// tuile parcourt ce tableau deux fois par image : sa taille compte.
const _: () = assert!(size_of::<Prepared>() == 128);

/// L'index que porte un triangle sans texture.
///
/// Une sentinelle plutôt qu'un `Option<u16>` : celui-ci ferait quatre octets
/// là où deux suffisent, et le test se fait une fois par triangle, hors de la
/// boucle de pixels.
pub const NO_TEXTURE: u16 = u16::MAX;

impl Prepared {
    /// Les pixels extrêmes que le triangle peut couvrir : `(x0, y0, x1, y1)`,
    /// bornes comprises.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        (self.x0, self.y0, self.x1, self.y1)
    }

    /// L'index de sa texture dans la table de l'image, ou [`NO_TEXTURE`].
    pub fn texture(&self) -> u16 {
        self.texture
    }
}

/// Produit vectoriel en deux dimensions, positif du côté intérieur de `a → b`.
///
/// `i64` et non `i32` : une coordonnée tient dans ±2¹⁶ en sous-pixels, un écart
/// dans ±2¹⁷, un produit dans ±2³⁴ et la différence dans ±2³⁵. Un `i32`
/// déborderait dès que la bande de garde sert, c'est-à-dire dès qu'un sommet
/// sort de l'écran.
fn edge(ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32) -> i64 {
    let (ax, ay) = (ax as i64, ay as i64);
    (bx as i64 - ax) * (py as i64 - ay) - (by as i64 - ay) * (px as i64 - ax)
}

/// Vrai si l'arête de vecteur `(dx, dy)` est haute ou gauche.
///
/// En parcours horaire avec Y vers le bas, on va vers les x croissants en haut,
/// on descend à droite et on remonte à gauche : d'où le signe de `dy`, puis
/// celui de `dx` pour départager les arêtes horizontales. Le triangle étant
/// parcouru dans l'autre sens ici, c'est `-d` qu'on lui passe : voir `fill`.
///
/// La propriété qui rend l'arête partagée étanche est que pour un vecteur et son
/// opposé, **exactement un des deux** est haut-ou-gauche — sauf si les deux
/// composantes sont nulles, cas qui n'atteint jamais cette fonction puisqu'un
/// triangle d'aire nulle est écarté avant.
///
/// Ne jamais raccourcir en `dy <= 0` : les deux triangles d'une arête
/// horizontale la revendiqueraient alors tous les deux, et la ligne serait
/// rendue deux fois.
fn is_top_left(dx: i32, dy: i32) -> bool {
    dy < 0 || (dy == 0 && dx > 0)
}

/// Le biais que porte une arête : nul si elle est haute ou gauche, `-1` sinon.
///
/// Ajouté à la constante du setup, il transforme le test `E >= 0` en `E > 0`
/// pour les arêtes qui ne s'appartiennent pas — les deux écritures sont
/// équivalentes puisque `E` est entier. Le biais est retenu parce qu'il
/// disparaît dans la valeur initiale : la boucle garde un test unique sur le
/// signe, transposable en SIMD sans branche par arête.
fn bias(dx: i32, dy: i32) -> i64 {
    if is_top_left(dx, dy) { 0 } else { -1 }
}

/// Les trois fonctions de bord au centre du pixel `(x0, y0)`, et leurs pas
/// d'un pixel vers la droite puis vers le bas.
///
/// Extraite du remplissage parce que le test qui compare le span au balayage
/// naïf doit partir exactement des mêmes valeurs : recopiée, elle finirait par
/// diverger, et les deux se tromperaient ensemble sans que rien ne le dise.
fn setup(triangle: &Prepared, x0: i32, y0: i32) -> ([i64; 3], [i64; 3], [i64; 3]) {
    let v = triangle.v;
    // Les trois arêtes, dans le sens du parcours : un point intérieur est du bon
    // côté des trois à la fois.
    let e = [(0usize, 1usize), (1, 2), (2, 0)];

    let px = x0 * SUBPIXEL_SCALE + PIXEL_CENTER;
    let py = py_of(y0);

    let mut row = [0i64; 3];
    let mut step_x = [0i64; 3];
    let mut step_y = [0i64; 3];

    for (i, &(a, b)) in e.iter().enumerate() {
        let (dx, dy) = (v[b].x - v[a].x, v[b].y - v[a].y);
        // La fonction de bord est niée, et le biais se prend sur `-d` : c'est ce
        // qui classe l'arête sur le sens réellement parcouru. Pris sur `d`, il
        // le serait sur le sens inverse, et les deux triangles d'une arête la
        // revendiqueraient ensemble.
        row[i] = -edge(v[a].x, v[a].y, v[b].x, v[b].y, px, py) + bias(-dx, -dy);
        // Dérivées de la forme close niée, multipliées par le pas d'un pixel.
        // Les additions entières qui suivent sont exactes : parcourir vaut le
        // recalcul complet, bit pour bit, quel que soit le nombre de pas.
        step_x[i] = (dy as i64) * SUBPIXEL_SCALE as i64;
        step_y[i] = -(dx as i64) * SUBPIXEL_SCALE as i64;
    }
    (row, step_x, step_y)
}

/// Vrai si le pixel d'indice `k` depuis le début de la ligne est couvert.
///
/// La forme close, évaluée sans parcourir : les additions entières du parcours
/// donnent les mêmes bits, mais celle-ci se calcule en un point quelconque.
fn covered(row: &[i64; 3], step_x: &[i64; 3], k: i64) -> bool {
    let at = |i: usize| row[i] + step_x[i] * k;
    (at(0) | at(1) | at(2)) >= 0
}

/// Les abscisses extrêmes couvertes sur une ligne, bornes comprises, ou `None`
/// si la ligne ne porte aucun pixel.
///
/// **Exact, et pas par approximation.** Sur une ligne, chaque arête vaut
/// `C + k·S` avec `C` sa valeur à `x0` — biais top-left compris, qui ne fait
/// que translater le demi-plan — et le test est `C + k·S ≥ 0`. Résoudre en
/// entiers donne le plancher exact par `div_euclid`, donc chaque borne est
/// **la** solution, pas une majoration : l'intersection des trois est
/// rigoureusement ce que le test pixel par pixel retiendrait. Un span plus
/// large ferait diviser la perspective là où la profondeur n'a pas de sens ;
/// un span plus étroit trouerait le triangle.
///
/// Une arête horizontale — `S` nul — pose une condition constante sur toute la
/// ligne, et ne demande aucune division.
fn span(row: &[i64; 3], step_x: &[i64; 3], x0: i32, x1: i32) -> Option<(i32, i32)> {
    let (mut lo, mut hi) = (0i64, (x1 - x0) as i64);
    for i in 0..3 {
        let (c, s) = (row[i], step_x[i]);
        if s > 0 {
            // `k ≥ −C/S`, donc le plafond du quotient, qui est l'opposé du
            // plancher de son opposé.
            lo = lo.max(-c.div_euclid(s));
        } else if s < 0 {
            hi = hi.min(c.div_euclid(-s));
        } else if c < 0 {
            return None;
        }
        if lo > hi {
            return None;
        }
    }
    // `lo` et `hi` sont encadrés par `0` et `x1 − x0`, qui tiennent tous deux
    // dans un `i32` : la conversion ne peut pas déborder.
    Some((x0 + lo as i32, x0 + hi as i32))
}

/// Le plus petit pixel entier dont le centre atteint `subpixel`.
///
/// Par décalage arithmétique et non par division : `/ 16` tronque vers zéro et
/// perdrait la colonne d'abscisse `-1`.
fn first_pixel(subpixel: i32) -> i32 {
    (subpixel + (SUBPIXEL_SCALE - PIXEL_CENTER - 1)) >> 4
}

/// Le plus grand pixel entier dont le centre reste sous `subpixel`.
fn last_pixel(subpixel: i32) -> i32 {
    (subpixel - PIXEL_CENTER) >> 4
}

/// Prépare un triangle, sommets donnés en sous-pixels.
///
/// **La face avant est antihoraire dans les données**, donc horaire à l'écran
/// une fois l'axe Y retourné vers le bas : son aire signée est négative. Le
/// moteur la rend en niant les fonctions de bord plutôt qu'en permutant deux
/// sommets — l'ordre reçu reste l'ordre parcouru, et l'appelant n'a rien à
/// réarranger.
///
/// Nier et transposer sont la même règle, pas deux conventions voisines :
/// `edge` est exactement antisymétrique sur les entiers, et `is_top_left(d)` est
/// le complémentaire de `is_top_left(-d)`. Ce qui rend l'arête partagée étanche
/// n'est donc pas modifié, **à une condition qui ne se voit pas ici** : la
/// négation vaut pour tous les triangles, toujours. Deux triangles adjacents
/// dont l'un serait nié et l'autre non auraient des tests identiques au lieu de
/// complémentaires sur leur arête commune, qui serait alors revendiquée deux
/// fois ou pas du tout. C'est pourquoi une surface à deux faces se soumet par
/// son triangle miroir, et jamais en levant le test de signe ci-dessous.
///
/// Rend `None` pour un triangle qui ne peut couvrir aucun centre de pixel.
pub fn prepare(vertices: [Vertex; 3], color: u32, texture: u16) -> Option<Prepared> {
    let v = vertices.map(|vertex| vertex.position);
    let area = edge(v[0].x, v[0].y, v[1].x, v[1].y, v[2].x, v[2].y);
    // Un seul test pour le dos et pour le dégénéré. Obligatoire et non
    // défensif : les équations de plan des attributs diviseront par cette aire,
    // et un triangle plat verrait ses trois fonctions de bord s'annuler le long
    // d'un segment, que les biais pourraient toutes satisfaire.
    if area >= 0 {
        return None;
    }

    let min_x = v[0].x.min(v[1].x).min(v[2].x);
    let max_x = v[0].x.max(v[1].x).max(v[2].x);
    let min_y = v[0].y.min(v[1].y).min(v[2].y);
    let max_y = v[0].y.max(v[1].y).max(v[2].y);

    // Le plus petit sommet dans l'ordre (y, x), partagé par les trois plans :
    // pris sur `v[0]`, une permutation circulaire changerait l'arrondi en
    // chaque pixel sans changer le triangle.
    let reference = (0..3).min_by_key(|&i| (v[i].y, v[i].x)).unwrap_or(0);

    let prepared = Prepared {
        v,
        x0: first_pixel(min_x),
        x1: last_pixel(max_x),
        y0: first_pixel(min_y),
        y1: last_pixel(max_y),
        ref_x: v[reference].x,
        ref_y: v[reference].y,
        depth: Plane::new(
            v,
            vertices.map(|vertex| i64::from(vertex.z)),
            area,
            reference,
        ),
        uv: [
            Plane::new(
                v,
                vertices.map(|vertex| i64::from(vertex.s)),
                area,
                reference,
            ),
            Plane::new(
                v,
                vertices.map(|vertex| i64::from(vertex.t)),
                area,
                reference,
            ),
        ],
        color,
        texture,
    };
    (prepared.x0 <= prepared.x1 && prepared.y0 <= prepared.y1).then_some(prepared)
}

/// Remplit la partie d'un triangle préparé qui tombe dans `window`.
///
/// `window` est en pixels de l'image, et tient dans la bande de garde.
pub fn fill<T: Target>(
    target: &mut T,
    window: Rect,
    triangle: &Prepared,
    texture: Option<&Texture>,
) {
    let color = triangle.color;

    // La fenêtre borne la boucle, jamais les valeurs : une fonction de bord
    // évaluée en un pixel ne dépend pas du rectangle dans lequel on la parcourt.
    let wx0 = triangle.x0.max(window.x as i32);
    let wx1 = triangle.x1.min((window.x + window.width) as i32 - 1);
    let y0 = triangle.y0.max(window.y as i32);
    let y1 = triangle.y1.min((window.y + window.height) as i32 - 1);
    if wx0 > wx1 || y0 > y1 {
        return;
    }

    // **Les fonctions de bord partent du span du triangle, pas de celui de la
    // fenêtre.** Le span sert aux segments de perspective, dont les extrémités
    // doivent être les mêmes quelle que soit la tuile : bornés par la fenêtre,
    // ils diviseraient à d'autres abscisses et la même surface se texturerait
    // autrement selon le découpage.
    let (mut row, step_x, step_y) = setup(triangle, triangle.x0, y0);
    let plane = &triangle.depth;
    let depth_x = plane.step_x(SUBPIXEL_SCALE);

    for y in y0..=y1 {
        if let Some((gl, gr)) = span(&row, &step_x, triangle.x0, triangle.x1) {
            // Les deux extrémités sont couvertes, et le pixel qui précède le
            // span ne l'est pas : c'est la coïncidence du span avec le test par
            // pixel, vérifiée en débogage plutôt que relue.
            debug_assert!(covered(&row, &step_x, (gl - triangle.x0) as i64));
            debug_assert!(covered(&row, &step_x, (gr - triangle.x0) as i64));
            debug_assert!(
                gl == triangle.x0 || !covered(&row, &step_x, (gl - triangle.x0 - 1) as i64)
            );

            // Le parcours, lui, s'arrête au bord de la fenêtre.
            let (lo, hi) = (gl.max(wx0), gr.min(wx1));
            if lo > hi {
                for i in 0..3 {
                    row[i] += step_y[i];
                }
                continue;
            }

            // La profondeur au centre du premier pixel du span, par la forme
            // close : elle ne se propage pas d'une ligne à l'autre, les spans
            // ne commençant pas à la même abscisse. Les pas entiers qui suivent
            // donnent les mêmes bits que l'évaluation directe en chaque pixel.
            let ey = (py_of(y) - triangle.ref_y) as i64;
            let ex = |x: i32| (x * SUBPIXEL_SCALE + PIXEL_CENTER - triangle.ref_x) as i64;
            match texture {
                None => {
                    let mut depth = plane.at(ex(lo), ey);
                    for x in lo..=hi {
                        // En un pixel couvert, la valeur tient dans [0, 2³²) :
                        // les sommets sont bornés par `to_depth` avec une marge
                        // qui couvre l'arrondi des gradients.
                        let z = (depth >> GRADIENT_BITS) as u32;
                        if target.test(x, y, z) {
                            target.write(x, y, z, color);
                        }
                        depth = depth.wrapping_add(depth_x);
                    }
                }
                Some(texture) => {
                    let mut x = lo;
                    while x <= hi {
                        // Le segment court d'un multiple de seize de l'image au
                        // suivant, **rabattu sur le span global** : au-delà, la
                        // profondeur prolongée hors du triangle peut s'annuler,
                        // et il n'y aurait aucun quotient à prendre. Ses bornes
                        // ne dépendent donc que du triangle et de la grille de
                        // l'image, jamais de la fenêtre où l'on parcourt.
                        let base = x - x.rem_euclid(SEGMENT);
                        let segment = (base.max(gl), (base + SEGMENT - 1).min(gr));
                        let last = segment.1.min(hi);
                        fill_segment(target, triangle, texture, y, segment, (x, last));
                        x = last + 1;
                    }
                }
            }
        }
        for i in 0..3 {
            row[i] += step_y[i];
        }
    }
}

/// L'ordonnée du centre du pixel `y`, en sous-pixels.
fn py_of(y: i32) -> i32 {
    y * SUBPIXEL_SCALE + PIXEL_CENTER
}

/// Pixels entre deux divisions de perspective.
///
/// Seize, la valeur d'époque : la division coûte alors un seizième de pixel, et
/// l'écart à la perspective exacte reste sous le texel sur un segment de cette
/// longueur. Les points de division sont les **multiples de seize de l'abscisse
/// dans l'image**, jamais un décompte reparti du bord de la tuile ou du
/// triangle — sinon la même surface se texturerait autrement selon le
/// découpage.
const SEGMENT: i32 = 16;

/// Bits fractionnaires d'une coordonnée de texture par pixel.
const UV_BITS: u32 = 16;

/// Décalage qui ramène `S · W` en 16.16.
///
/// `S` est en 14.12 et vaut `u · d · 2¹²` ; `W` vaut `2⁵⁶ / D` avec `D = d ·
/// 2³²`. Leur produit vaut donc `u · 2³⁶`, et `u` en 16.16 s'en tire par un
/// décalage de vingt bits.
///
/// **Le produit ne déborde pas, et ce n'est pas par la borne des facteurs** :
/// pris séparément, `S` atteint 2²⁶ et `W` 2⁵⁰, dont le produit serait hors de
/// l'`i64`. Mais les deux sont **anticorrélés** — la profondeur qui fait
/// grandir `W` fait rétrécir `S` dans la même proportion —, si bien que leur
/// produit vaut exactement `u · 2³⁶` et reste sous 2⁵⁰ pour une coordonnée
/// bornée à 2¹⁴ texels.
const UV_SHIFT: u32 = 20;

/// Remplit la part de `draw` qui tombe dans le segment `segment`, bornes
/// comprises.
///
/// Les coordonnées de texture se divisent aux **deux extrémités du segment** et
/// s'interpolent affinement entre elles : c'est le compromis d'époque, une
/// division pour seize pixels au lieu d'une par pixel, et l'écart à la
/// perspective exacte reste sous le texel sur une longueur pareille.
///
/// `segment` ne dépend que du triangle et de la grille de l'image ; `draw` en
/// est la part que la fenêtre laisse voir. Les séparer est ce qui rend la
/// texture indépendante du découpage : tout part de la forme close, rien ne
/// s'accumule d'une tuile à l'autre.
fn fill_segment<T: Target>(
    target: &mut T,
    triangle: &Prepared,
    texture: &Texture,
    y: i32,
    segment: (i32, i32),
    draw: (i32, i32),
) {
    // Les écarts se recalculent ici plutôt que de traverser la signature :
    // deux soustractions par segment, contre deux paramètres de plus dans une
    // liste qui en compte déjà six.
    let ey = (py_of(y) - triangle.ref_y) as i64;
    let ex = |x: i32| (x * SUBPIXEL_SCALE + PIXEL_CENTER - triangle.ref_x) as i64;
    let depth_at = |x: i32| triangle.depth.at(ex(x), ey);
    let uv_at = |x: i32| {
        let w = reciprocal((depth_at(x) >> GRADIENT_BITS) as u32);
        let read = |plane: &Plane| i64::from(texel_coord(plane.at(ex(x), ey) >> GRADIENT_BITS, w));
        [read(&triangle.uv[0]), read(&triangle.uv[1])]
    };

    let (from, to) = segment;
    let steps = (to - from + 1) as i64;
    let first = uv_at(from);
    // **Le niveau se prend au maximum des deux extrémités**, et non au seul
    // point de division gauche : quand la densité double sur seize pixels, un
    // niveau pris à gauche sous-sélectionne toute la moitié droite du segment.
    // La réciproque de droite est calculée de toute façon pour la pente, donc
    // ce maximum coûte quatre multiplications et aucune division.
    let level = {
        let at = |x: i32, uv: [i64; 2]| {
            mip_level(
                triangle,
                uv[0] as i32,
                uv[1] as i32,
                reciprocal((depth_at(x) >> GRADIENT_BITS) as u32),
            )
        };
        at(from, first).max(at(to, uv_at(to)))
    };
    // La valeur de fin se prend au pixel **suivant** le segment, pour que la
    // pente soit celle d'un pas de pixel et non d'un pas de segment. Ce point
    // est dans le span tant que le segment s'y termine ; à la fin du span, il
    // vaut le dernier pixel couvert, faute de profondeur au-delà.
    let after = uv_at(if to < triangle.x1 { to + 1 } else { to });
    // `div_euclid` : la pente s'arrondit vers le bas des deux côtés de zéro, là
    // où `/` ferait un pas double autour de l'origine de la texture.
    let slope = [
        (after[0] - first[0]).div_euclid(steps),
        (after[1] - first[1]).div_euclid(steps),
    ];

    let mut depth = depth_at(draw.0);
    let depth_x = triangle.depth.step_x(SUBPIXEL_SCALE);
    let skipped = (draw.0 - from) as i64;
    let mut uv = [first[0] + slope[0] * skipped, first[1] + slope[1] * skipped];
    for x in draw.0..=draw.1 {
        let z = (depth >> GRADIENT_BITS) as u32;
        if target.test(x, y, z) {
            // Le décalage de niveau porte sur la coordonnée **interpolée**, et
            // non sur les extrémités du segment : appliqué à celles-ci, il
            // quantifierait la pente par 2ⁿ, soit cinq bits perdus au niveau 5.
            // Les deux décalages restent séparés — le tramage s'insérera entre
            // eux, et appliqué avant celui du niveau il serait divisé par 2ⁿ et
            // s'éteindrait dès le niveau 2.
            let coord = |c: i64| ((c >> level) >> UV_BITS) as i32;
            let texel = texture.texel(level as usize, coord(uv[0]), coord(uv[1]));
            target.write(x, y, z, texel);
        }
        depth = depth.wrapping_add(depth_x);
        uv[0] += slope[0];
        uv[1] += slope[1];
    }
}

/// La réciproque de la profondeur, `2⁵⁶ / D`.
///
/// La seule division du remplissage, et elle a lieu une fois par segment de
/// seize pixels. `D` est strictement positif en un pixel couvert : les sommets
/// sont bornés par `to_depth` à distance des bornes, et une combinaison convexe
/// reste dans l'intervalle.
fn reciprocal(depth: u32) -> u64 {
    debug_assert!(depth > 0, "profondeur nulle en un pixel couvert");
    (1u64 << 56) / depth.max(1) as u64
}

/// Le niveau de mipmap d'un segment, depuis ses dérivées et sa réciproque.
///
/// **Les quatre dérivées se prennent en forme close, sans division nouvelle.**
/// `u = S/D` n'est pas affine en espace écran, mais `S` et `D` le sont, et
/// `∂u/∂x = (S_x − u·D_x)/D`. Le facteur `1/D` étant commun aux quatre et le
/// critère étant un **maximum** — distance de Chebyshev, sans racine carrée —,
/// on le sort du maximum : quatre numérateurs entiers, un seul `max`, et une
/// seule multiplication par la réciproque déjà calculée.
///
/// **La dérivée verticale est indispensable.** Sur un sol, `∂u/∂x` reste
/// modérée le long d'une ligne alors que `∂u/∂y` explose vers l'horizon : un
/// niveau choisi sur la seule horizontale sous-sélectionne, et le sol
/// scintille. C'est le critère de franchissement de l'étape, manqué exactement
/// là.
///
/// Écartée : la différence finie verticale, qui exigerait une réciproque un
/// pixel plus bas — donc hors du triangle dès la dernière ligne, où la
/// profondeur prolongée peut s'annuler. Le quotient y enveloppe en silence, et
/// le scintillement reviendrait précisément à l'horizon.
fn mip_level(triangle: &Prepared, u: i32, v: i32, reciprocal: u64) -> u32 {
    let pixel = SUBPIXEL_SCALE;
    let (dx, dy) = (
        triangle.depth.step_x(pixel) >> GRADIENT_BITS,
        triangle.depth.step_y(pixel) >> GRADIENT_BITS,
    );
    // `S` est en 14.12 et la coordonnée en 16.16 : le décalage met les deux
    // termes à la même échelle avant la soustraction. `step_x` n'est pas encore
    // réduit de `GRADIENT_BITS`, ce qui laisse les quatre bits de marge dont ce
    // décalage a besoin.
    let numerator = |plane: &Plane, coord: i32, depth_step: i64| {
        let slope = plane.step_x(pixel) << (UV_BITS - GRADIENT_BITS);
        slope - ((i64::from(coord).saturating_mul(depth_step)) >> UV_SHIFT)
    };
    let worst = [
        numerator(&triangle.uv[0], u, dx),
        numerator(&triangle.uv[1], v, dx),
        {
            let slope = triangle.uv[0].step_y(pixel) << (UV_BITS - GRADIENT_BITS);
            slope - ((i64::from(u).saturating_mul(dy)) >> UV_SHIFT)
        },
        {
            let slope = triangle.uv[1].step_y(pixel) << (UV_BITS - GRADIENT_BITS);
            slope - ((i64::from(v).saturating_mul(dy)) >> UV_SHIFT)
        },
    ]
    .into_iter()
    .map(|n| n.unsigned_abs())
    .max()
    .unwrap_or(0);

    // `ρ·2¹⁶ = M·W/2³⁶`, donc le niveau vaut `bit_length(ρ) − 1`, soit
    // `11 − leading_zeros(M·W)`. Une dérivée nulle donne `leading_zeros = 64`
    // et le niveau 0, sans cas particulier ; la saturation du produit donne le
    // niveau 11, ce qui est inoffensif : elle demanderait plus de 2¹² texels
    // par pixel, donc un niveau qu'aucune chaîne ne porte.
    LEVEL_BIAS.saturating_sub(worst.saturating_mul(reciprocal).leading_zeros())
}

/// Le biais du logarithme de `mip_level`.
///
/// **Ce onze n'est pas un réglage** : il sort de la largeur de l'`u64` et des
/// échelles de `S`, de `W` et des coordonnées. Qu'il vaille aussi le dernier
/// niveau d'une texture de [`MAX_TEXTURE_SIZE`] est une coïncidence — et c'est
/// l'assertion ci-dessous qui la surveille, faute de quoi porter cette borne à
/// 4096 plafonnerait le niveau sans que rien ne le signale.
const LEVEL_BIAS: u32 = 11;

const _: () = assert!(MAX_TEXTURE_SIZE.trailing_zeros() == LEVEL_BIAS);

/// La coordonnée de texture en 16.16 d'un attribut `S` à la profondeur `D`.
///
/// **Par décalage et non par division** : `/` tronque vers zéro, donc
/// changerait de sens d'arrondi de part et d'autre de l'origine de la texture,
/// et une couture y apparaîtrait sur une ligne que rien d'autre ne distingue.
/// Le décalage arithmétique, lui, arrondit vers le bas des deux côtés.
fn texel_coord(s: i64, reciprocal: u64) -> i32 {
    ((s.wrapping_mul(reciprocal as i64)) >> UV_SHIFT) as i32
}

#[cfg(test)]
mod tests;
