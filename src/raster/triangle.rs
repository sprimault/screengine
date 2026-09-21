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
pub fn fill<T: Target>(target: &mut T, window: Rect, triangle: &Prepared) {
    let color = triangle.color;

    // La fenêtre borne la boucle, jamais les valeurs : une fonction de bord
    // évaluée en un pixel ne dépend pas du rectangle dans lequel on la parcourt.
    let x0 = triangle.x0.max(window.x as i32);
    let x1 = triangle.x1.min((window.x + window.width) as i32 - 1);
    let y0 = triangle.y0.max(window.y as i32);
    let y1 = triangle.y1.min((window.y + window.height) as i32 - 1);
    if x0 > x1 || y0 > y1 {
        return;
    }

    let (mut row, step_x, step_y) = setup(triangle, x0, y0);
    let plane = &triangle.depth;
    let depth_x = plane.step_x(SUBPIXEL_SCALE);

    for y in y0..=y1 {
        if let Some((lo, hi)) = span(&row, &step_x, x0, x1) {
            // Les deux extrémités sont couvertes, et le pixel qui précède le
            // span ne l'est pas : c'est la coïncidence du span avec le test par
            // pixel, vérifiée en débogage plutôt que relue.
            debug_assert!(covered(&row, &step_x, (lo - x0) as i64));
            debug_assert!(covered(&row, &step_x, (hi - x0) as i64));
            debug_assert!(lo == x0 || !covered(&row, &step_x, (lo - x0 - 1) as i64));

            // La profondeur au centre du premier pixel du span, par la forme
            // close : elle ne se propage pas d'une ligne à l'autre, les spans
            // ne commençant pas à la même abscisse. Les pas entiers qui suivent
            // donnent les mêmes bits que l'évaluation directe en chaque pixel.
            let mut depth = plane.at(
                (lo * SUBPIXEL_SCALE + PIXEL_CENTER - triangle.ref_x) as i64,
                (py_of(y) - triangle.ref_y) as i64,
            );
            for x in lo..=hi {
                // En un pixel couvert, la valeur tient dans [0, 2³²) : les
                // sommets sont bornés par `to_depth` avec une marge qui couvre
                // l'arrondi des gradients.
                target.put(x, y, (depth >> GRADIENT_BITS) as u32, color);
                depth = depth.wrapping_add(depth_x);
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

#[cfg(test)]
mod tests;
