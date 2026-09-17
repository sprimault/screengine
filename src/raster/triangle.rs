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

use super::{Rect, Target};

/// Un sommet projeté, en sous-pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    /// Abscisse en sous-pixels, dans la bande de garde.
    pub x: i32,
    /// Ordonnée en sous-pixels, Y vers le bas.
    pub y: i32,
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
    color: u32,
}

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
/// celui de `dx` pour départager les arêtes horizontales.
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
/// Les sommets sont en ordre horaire à l'écran, Y vers le bas. Un triangle
/// d'orientation inverse est le dos d'une face et n'est pas rendu ; pour une
/// surface à deux faces, l'appelant échange deux sommets avant d'appeler, il ne
/// nie pas les fonctions de bord — la négation laisserait la classification
/// haut-gauche calculée sur l'ancien sens de parcours, et l'arête partagée
/// serait revendiquée deux fois.
///
/// Rend `None` pour un triangle qui ne peut couvrir aucun centre de pixel.
pub fn prepare(v: [Point; 3], color: u32) -> Option<Prepared> {
    let area = edge(v[0].x, v[0].y, v[1].x, v[1].y, v[2].x, v[2].y);
    // Un seul test pour le dos et pour le dégénéré. Obligatoire et non
    // défensif : les équations de plan des attributs diviseront par cette aire,
    // et un triangle plat verrait ses trois fonctions de bord s'annuler le long
    // d'un segment, que les biais pourraient toutes satisfaire.
    if area <= 0 {
        return None;
    }

    let min_x = v[0].x.min(v[1].x).min(v[2].x);
    let max_x = v[0].x.max(v[1].x).max(v[2].x);
    let min_y = v[0].y.min(v[1].y).min(v[2].y);
    let max_y = v[0].y.max(v[1].y).max(v[2].y);

    let prepared = Prepared {
        v,
        x0: first_pixel(min_x),
        x1: last_pixel(max_x),
        y0: first_pixel(min_y),
        y1: last_pixel(max_y),
        color,
    };
    (prepared.x0 <= prepared.x1 && prepared.y0 <= prepared.y1).then_some(prepared)
}

/// Remplit la partie d'un triangle préparé qui tombe dans `window`.
///
/// `window` est en pixels de l'image, et tient dans la bande de garde.
pub fn fill<T: Target>(target: &mut T, window: Rect, triangle: &Prepared) {
    let v = triangle.v;
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

    // Les trois arêtes, dans le sens du parcours : un point intérieur est du bon
    // côté des trois à la fois.
    let e = [(0usize, 1usize), (1, 2), (2, 0)];

    let px = x0 * SUBPIXEL_SCALE + PIXEL_CENTER;
    let py = y0 * SUBPIXEL_SCALE + PIXEL_CENTER;

    let mut row = [0i64; 3];
    let mut step_x = [0i64; 3];
    let mut step_y = [0i64; 3];

    for (i, &(a, b)) in e.iter().enumerate() {
        let (dx, dy) = (v[b].x - v[a].x, v[b].y - v[a].y);
        row[i] = edge(v[a].x, v[a].y, v[b].x, v[b].y, px, py) + bias(dx, dy);
        // Dérivées de la forme close, multipliées par le pas d'un pixel. Les
        // additions entières qui suivent sont exactes : parcourir vaut le
        // recalcul complet, bit pour bit, quel que soit le nombre de pas.
        step_x[i] = -(dy as i64) * SUBPIXEL_SCALE as i64;
        step_y[i] = (dx as i64) * SUBPIXEL_SCALE as i64;
    }

    for y in y0..=y1 {
        let mut cell = row;
        for x in x0..=x1 {
            // Un point est intérieur quand les trois valeurs sont positives ou
            // nulles : leur OU binaire porte alors un bit de signe à zéro.
            if (cell[0] | cell[1] | cell[2]) >= 0 {
                target.put(x, y, color);
            }
            for i in 0..3 {
                cell[i] += step_x[i];
            }
        }
        for i in 0..3 {
            row[i] += step_y[i];
        }
    }
}

#[cfg(test)]
mod tests;
