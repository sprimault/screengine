// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le tracé d'un segment, en coordonnées entières.
//!
//! **Une seconde famille de primitives, pas un triangle dégénéré.** Le triangle
//! préparé fait exactement cent vingt-huit octets et n'a plus un octet libre ;
//! et surtout une ligne n'a pas de couverture d'aire, donc rien de ce que les
//! fonctions de bord décident ne s'applique à elle.
//!
//! Tout ce qui est ici s'évalue en **coordonnées globales de l'image**, comme le
//! remplissage : une fenêtre ne borne que la boucle, jamais les valeurs, et deux
//! découpages différents allument exactement les mêmes pixels.

use crate::math::fixed::{PIXEL_CENTER, SUBPIXEL_SCALE};
use crate::math::projection::{ClipVertex, Frustum, PLANE_COUNT};

use super::Rect;

/// La demi-diagonale du losange inscrit dans un pixel, en sous-pixels.
///
/// Le losange est le carré du pixel tourné de 45° : ses quatre sommets sont au
/// milieu des côtés, donc à huit sous-pixels du centre sur chaque axe.
const DIAMOND: i64 = PIXEL_CENTER as i64;

/// Un segment prêt à être tracé, en coordonnées écran.
///
/// Les extrémités sont en sous-pixels, la profondeur en 0.32 comme celle d'un
/// triangle. Rien de ce que porte un triangle préparé — plans d'attributs,
/// index de texture, boîte d'éclairage — n'a de sens ici.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// L'abscisse de la première extrémité, en sous-pixels.
    pub x0: i32,
    /// Son ordonnée.
    pub y0: i32,
    /// Sa profondeur, en 0.32 — plus grand est plus proche.
    pub z0: u32,
    /// L'abscisse de la seconde extrémité.
    pub x1: i32,
    /// Son ordonnée.
    pub y1: i32,
    /// Sa profondeur.
    pub z1: u32,
    /// La couleur écrite, dans l'ordre mémoire des pixels de sortie.
    pub color: u32,
    /// Vrai quand le tracé teste la profondeur, faux quand il l'ignore.
    ///
    /// Un booléen et non un entier : c'est un type du noyau, que la frontière
    /// construit depuis la constante d'ABI après l'avoir validée. L'ABI, elle,
    /// n'a pas de `bool`.
    pub tested: bool,
}

impl Segment {
    /// La boîte englobante en pixels entiers, `(x0, y0, x1, y1)` inclusive.
    ///
    /// Elle borne la répartition par tuiles, donc elle doit **contenir** tout
    /// pixel que la couverture peut allumer : elle s'arrondit vers l'extérieur,
    /// et un pixel de trop ne coûte qu'une référence tandis qu'un pixel manquant
    /// trouerait la ligne dans une seule configuration de tuiles.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let (lo_x, hi_x) = if self.x0 < self.x1 {
            (self.x0, self.x1)
        } else {
            (self.x1, self.x0)
        };
        let (lo_y, hi_y) = if self.y0 < self.y1 {
            (self.y0, self.y1)
        } else {
            (self.y1, self.y0)
        };
        // `div_euclid` et non un décalage : un sous-pixel négatif doit tomber
        // sur le pixel qui le contient, et le décalage arithmétique d'un négatif
        // arrondit déjà vers le bas, mais l'écrire laisse la borne lisible.
        (
            lo_x.div_euclid(SUBPIXEL_SCALE),
            lo_y.div_euclid(SUBPIXEL_SCALE),
            hi_x.div_euclid(SUBPIXEL_SCALE),
            hi_y.div_euclid(SUBPIXEL_SCALE),
        )
    }
}

/// Découpe un segment contre le plan proche et la bande de garde.
///
/// Rend `None` quand rien n'en subsiste. **Bien plus simple que la découpe d'un
/// polygone** : un segment reste un segment, donc chaque plan ne fait que
/// déplacer l'une de ses deux extrémités, sans jamais en engendrer une
/// troisième. Il n'y a donc ni tampon, ni borne à prouver.
///
/// L'ordre des plans est celui du découpage des triangles, et il est écrit en
/// dur : deux ordres différents engendrent des extrémités différentes, et c'est
/// la constance de cet ordre qui garde une polyligne cohérente d'un segment au
/// suivant.
pub fn clip_segment(
    mut a: ClipVertex,
    mut b: ClipVertex,
    frustum: &Frustum,
) -> Option<(ClipVertex, ClipVertex)> {
    let (code_a, code_b) = (frustum.outcode(a), frustum.outcode(b));
    // Un demi-espace qui les exclut tous les deux : rien à découper.
    if code_a & code_b != 0 {
        return None;
    }
    if code_a | code_b == 0 {
        return Some((a, b));
    }

    for plane in 0..PLANE_COUNT {
        let da = frustum.distance(a, plane);
        let db = frustum.distance(b, plane);
        // Un sommet posé sur le plan est intérieur, ce qui fait tomber un `NaN`
        // du côté extérieur — toute comparaison avec lui étant fausse.
        match (da >= 0.0, db >= 0.0) {
            (true, true) => {}
            (false, false) => return None,
            (true, false) => b = Frustum::intersect(a, b, da, db),
            (false, true) => a = Frustum::intersect(a, b, da, db),
        }
    }
    Some((a, b))
}

/// Le quotient arrondi **vers le bas**, quel que soit le signe du diviseur.
///
/// `div_euclid` ne le donne pas : il force un reste positif, ce qui arrondit
/// vers le haut dès que le diviseur est négatif. Le signe se normalise donc
/// avant de diviser, et le plancher est alors celui qu'on attend des deux
/// côtés. Le défaut que cette fonction ferme ne se voyait que sur les segments
/// dont l'axe majeur décroît, et seulement à certaines pentes.
fn floor_div(num: i64, den: i64) -> i64 {
    if den < 0 {
        (-num).div_euclid(-den)
    } else {
        num.div_euclid(den)
    }
}

/// Le centre du pixel `(px, py)`, en sous-pixels.
fn center(px: i32, py: i32) -> (i64, i64) {
    (
        px as i64 * SUBPIXEL_SCALE as i64 + PIXEL_CENTER as i64,
        py as i64 * SUBPIXEL_SCALE as i64 + PIXEL_CENTER as i64,
    )
}

/// Vrai si le segment **sort** du losange inscrit dans le pixel `(px, py)`.
///
/// C'est la règle de couverture du tracé, et elle est à la ligne ce que la règle
/// top-left est au triangle : un pixel s'allume quand le segment quitte son
/// losange, si bien qu'une polyligne ne peint ses sommets partagés **qu'une
/// fois** — le segment qui arrive entre dans le losange du sommet et s'y
/// arrête, celui qui repart en sort.
///
/// Le losange est convexe, donc l'ensemble des paramètres du segment qui y
/// tombent est un intervalle : il suffit de l'intersecter avec `[0, 1]` et de
/// regarder si sa borne supérieure est atteinte **avant la fin du segment**.
/// Tout est en `i64` et en fractions comparées par produits croisés — aucun
/// flottant, aucune division.
///
/// Un segment de longueur nulle ne sort de rien : il n'allume aucun pixel, ce
/// qui est la réponse juste pour deux sommets confondus d'une polyligne.
pub fn exits_diamond(segment: &Segment, px: i32, py: i32) -> bool {
    let (cx, cy) = center(px, py);
    let (ax, ay) = (segment.x0 as i64 - cx, segment.y0 as i64 - cy);
    let (bx, by) = (segment.x1 as i64 - cx, segment.y1 as i64 - cy);
    let (dx, dy) = (bx - ax, by - ay);
    if dx == 0 && dy == 0 {
        return false;
    }

    // L'intervalle où le segment **prolongé** traverse le losange, en fractions
    // `num / den` de dénominateur positif. Il ne se borne pas à `[0, 1]` : c'est
    // la position de sa borne supérieure par rapport à `1` qui dit si le segment
    // sort du losange ou s'y arrête, et la borner d'avance perdrait exactement
    // cette information — le défaut que ce commentaire existe pour fermer.
    let mut lo: Option<(i64, i64)> = None;
    let mut hi: Option<(i64, i64)> = None;

    // Les quatre demi-plans `s·X + t·Y < 8`, avec `(s, t)` les quatre signes.
    for (s, t) in [(1i64, 1i64), (1, -1), (-1, 1), (-1, -1)] {
        let value = s * ax + t * ay;
        let step = s * dx + t * dy;
        if step == 0 {
            // Parallèle à ce bord : le segment entier est dedans ou dehors, et
            // ce bord ne borne donc rien.
            if value >= DIAMOND {
                return false;
            }
            continue;
        }
        // `value + step · param = 8` donne la borne de ce demi-plan.
        let num = DIAMOND - value;
        if step > 0 {
            let candidate = (num, step);
            hi = Some(match hi {
                Some(h) if h.0 * candidate.1 <= candidate.0 * h.1 => h,
                _ => candidate,
            });
        } else {
            // `step` négatif : la borne est inférieure, et diviser par un
            // négatif retourne le sens — d'où les deux signes rendus positifs
            // avant toute comparaison.
            let candidate = (-num, -step);
            lo = Some(match lo {
                Some(l) if l.0 * candidate.1 >= candidate.0 * l.1 => l,
                _ => candidate,
            });
        }
    }

    // Le losange est borné et la direction non nulle : au moins une paire de
    // bords opposés a des pas de signes contraires, donc les deux bornes
    // existent. Le cas contraire ne peut pas se produire, et il rend « pas de
    // sortie » plutôt qu'une valeur inventée.
    let (Some(lo), Some(hi)) = (lo, hi) else {
        return false;
    };

    // Intervalle vide : la droite passe à côté du losange.
    if lo.0 * hi.1 >= hi.0 * lo.1 {
        return false;
    }
    // Le losange est entièrement avant le départ, ou entièrement après
    // l'arrivée : le segment ne le rencontre pas.
    if hi.0 <= 0 || lo.0 >= lo.1 {
        return false;
    }
    // Et la sortie tombe dans le segment. Au sens large : un segment qui
    // s'arrête pile sur le bord en est sorti, le losange étant ouvert.
    hi.0 <= hi.1
}

/// Allume les pixels du segment qui tombent dans `window`.
///
/// Le parcours est un pas entier sur l'axe majeur, et **la règle de sortie du
/// losange décide seule de ce qui s'allume** : le pas propose, la règle
/// dispose. Les deux premières et les deux dernières positions sont celles où
/// l'une et l'autre peuvent diverger, et c'est précisément là que la règle
/// existe — ailleurs, le pixel proposé est toujours celui dont le losange est
/// traversé.
///
/// **Rien n'est rebasé sur la fenêtre.** Le pas part de l'extrémité du segment,
/// en coordonnées globales, et la fenêtre ne fait qu'écarter ce qui tombe
/// dehors : une tuile qui redémarrerait le pas à son bord décalerait la ligne
/// d'un pixel à chaque couture.
///
/// La profondeur s'interpole linéairement entre les deux extrémités, ce qui est
/// juste : `near/w` est affine en espace écran, donc le long d'un segment de
/// l'image.
pub fn cover<F: FnMut(i32, i32, u32)>(segment: &Segment, window: Rect, mut pixel: F) {
    let (dx, dy) = (
        segment.x1 as i64 - segment.x0 as i64,
        segment.y1 as i64 - segment.y0 as i64,
    );
    if dx == 0 && dy == 0 {
        return;
    }

    let (bx0, by0, bx1, by1) = segment.bounds();
    let major_x = dx.abs() >= dy.abs();
    // Le parcours va toujours dans le sens croissant de l'axe majeur : la règle
    // de sortie du losange ne dépend pas du sens, et un parcours unique évite
    // d'avoir deux boucles dont une seule serait éprouvée.
    let (first, last) = if major_x { (bx0, bx1) } else { (by0, by1) };

    let steps = dx.abs().max(dy.abs());
    for major in first..=last {
        // Le pixel candidat de cette colonne — ou de cette ligne —, pris sur
        // l'axe mineur par la forme close : aucune accumulation, donc aucune
        // dérive, et la valeur est la même quel que soit le point de départ de
        // la boucle.
        let minor = minor_at(segment, major, major_x);
        // Deux candidats, et non un : au voisinage d'un sommet de losange, le
        // pixel dont le losange est traversé peut être le voisin de celui que
        // l'arrondi désigne. Les proposer tous les deux et laisser la règle
        // trancher coûte un test et supprime la classe entière de ces cas.
        for candidate in [minor, minor + 1] {
            let (x, y) = if major_x {
                (major, candidate)
            } else {
                (candidate, major)
            };
            if x < window.x as i32
                || y < window.y as i32
                || x >= (window.x + window.width) as i32
                || y >= (window.y + window.height) as i32
            {
                continue;
            }
            if !exits_diamond(segment, x, y) {
                continue;
            }
            pixel(x, y, depth_at(segment, x, y, major_x, steps));
        }
    }
}

/// Le pixel de l'axe mineur au pas `major`, par la forme close.
///
/// Arrondi vers le bas par `div_euclid` : le candidat et son voisin du dessus
/// sont tous deux proposés, et c'est la règle du losange qui tranche. Un arrondi
/// au plus proche choisirait à sa place, et se tromperait aux sommets.
fn minor_at(segment: &Segment, major: i32, major_x: bool) -> i32 {
    let (from_major, from_minor) = if major_x {
        (segment.x0, segment.y0)
    } else {
        (segment.y0, segment.x0)
    };
    let (d_major, d_minor) = if major_x {
        (
            segment.x1 as i64 - segment.x0 as i64,
            segment.y1 as i64 - segment.y0 as i64,
        )
    } else {
        (
            segment.y1 as i64 - segment.y0 as i64,
            segment.x1 as i64 - segment.x0 as i64,
        )
    };
    if d_major == 0 {
        return from_minor.div_euclid(SUBPIXEL_SCALE);
    }
    // Le centre de la colonne `major`, en sous-pixels globaux.
    let at = major as i64 * SUBPIXEL_SCALE as i64 + PIXEL_CENTER as i64;
    // Le plancher, et non la troncature de Rust : une position négative
    // tomberait d'un pas du mauvais côté, et le candidat proposé ne serait plus
    // voisin du pixel que la règle retient.
    let offset = floor_div((at - from_major as i64) * d_minor, d_major);
    let minor = from_minor as i64 + offset;
    minor.div_euclid(SUBPIXEL_SCALE as i64) as i32
}

/// La profondeur au pixel `(x, y)`, interpolée entre les deux extrémités.
///
/// Par la position **sur l'axe majeur**, qui est celui dont l'étendue est la
/// plus grande : c'est ce qui garde le quotient borné et le rend exact à ses
/// deux bouts.
fn depth_at(segment: &Segment, x: i32, y: i32, major_x: bool, steps: i64) -> u32 {
    if steps == 0 {
        return segment.z0;
    }
    let (from, at) = if major_x {
        (
            segment.x0 as i64,
            x as i64 * SUBPIXEL_SCALE as i64 + PIXEL_CENTER as i64,
        )
    } else {
        (
            segment.y0 as i64,
            y as i64 * SUBPIXEL_SCALE as i64 + PIXEL_CENTER as i64,
        )
    };
    let span = if major_x {
        segment.x1 as i64 - segment.x0 as i64
    } else {
        segment.y1 as i64 - segment.y0 as i64
    };
    if span == 0 {
        return segment.z0;
    }
    let t = at - from;
    // Bornage écrit : un pixel du bout peut tomber un demi-pixel au-delà de
    // l'extrémité, et la profondeur ne doit pas sortir de l'intervalle des deux
    // bouts — une extrapolation ferait passer un repère devant ce qui le cache.
    // Les deux sens sont écrits séparément parce que `span` porte le sens du
    // segment, et qu'un bornage unique supposerait qu'il soit positif.
    let t = if span > 0 {
        if t < 0 {
            0
        } else if t > span {
            span
        } else {
            t
        }
    } else if t > 0 {
        0
    } else if t < span {
        span
    } else {
        t
    };
    let z0 = segment.z0 as i64;
    let z1 = segment.z1 as i64;
    // Le même plancher que pour la position : le sens d'arrondi d'une division
    // entière est contractuel, et `span` porte le sens du segment.
    let z = z0 + floor_div((z1 - z0) * t, span);
    if z < 0 {
        0
    } else if z > u32::MAX as i64 {
        u32::MAX
    } else {
        z as u32
    }
}

#[cfg(test)]
mod tests;
