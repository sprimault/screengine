// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La fenêtre de traversée : un rectangle de pixels que chaque portail resserre.
//!
//! **Elle borne la boucle de remplissage, jamais les valeurs.** Une fonction de
//! bord, un segment de perspective, un motif de tramage s'évaluent en
//! coordonnées d'image et ne savent rien du rectangle dans lequel on les
//! parcourt : une fenêtre de portail est donc, bit pour bit, de la même nature
//! qu'un rectangle de tuile, et l'image est identique avec ou sans elle.
//!
//! **Avec un z-buffer, une fenêtre n'est jamais un mécanisme de justesse,
//! seulement d'élimination.** Les cellules sont fermées et disjointes, et toutes
//! celles que la traversée visite sont dessinées : une fenêtre trop large rend
//! exactement la même image qu'une fenêtre exacte, les murs de la cellule
//! courante gagnant la profondeur. Seule une fenêtre **trop étroite** troue
//! l'image. L'asymétrie commande tout ce qui suit — on cherche la fenêtre la
//! moins chère qui soit conservatrice, pas la plus serrée.
//!
//! Le choix du rectangle contre un empilement de plans est arbitré dans
//! `docs/rust.md`, section « Côté entier ».

// Tout ce module attend son premier appelant, la traversée, qui dépile des
// (cellule, fenêtre) et réduit à chaque portail. Il se livre avant elle parce
// qu'il se teste seul — sans rendu, sans cellule et sans contexte —, et c'est
// précisément ce qui le rend vérifiable : une réduction fautive ne se lit pas
// dans une empreinte, elle se lit dans un trou.
#![allow(dead_code)]

use crate::math::fixed::SUBPIXEL_SCALE;
use crate::math::projection::ClipVertex;
use crate::math::{Affine3, Projection, Vec3};
use crate::raster::{Rect, clip};

/// Le nombre maximum de sommets qu'un triangle découpé par la fenêtre peut
/// porter.
///
/// Trois sommets et quatre bords : chaque bord ajoute au plus une arête, donc au
/// plus un sommet. La borne est prouvée et non majorée, comme celle du découpage
/// homogène — un tampon dimensionné au jugé serait soit du gaspillage, soit un
/// dépassement qu'aucun test ne rejoue.
const MAX_WINDOW_VERTICES: usize = 7;

/// Les bornes d'un rectangle en sous-pixels, inclusives.
///
/// Inclusives parce que c'est ainsi que le remplissage compte ses colonnes et
/// ses lignes, et qu'un rectangle vide se reconnaît alors à `min > max` sans
/// qu'aucune soustraction ne déborde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bounds {
    /// Abscisse minimale.
    min_x: i32,
    /// Abscisse maximale.
    max_x: i32,
    /// Ordonnée minimale.
    min_y: i32,
    /// Ordonnée maximale.
    max_y: i32,
}

impl Bounds {
    /// Les bornes vides, prêtes à accueillir un premier point.
    ///
    /// Volontairement croisées : tout point les corrige, et aucune n'est une
    /// valeur plausible qu'on oublierait de remplacer.
    const EMPTY: Self = Self {
        min_x: i32::MAX,
        max_x: i32::MIN,
        min_y: i32::MAX,
        max_y: i32::MIN,
    };

    /// Vrai si aucun point n'y est entré.
    fn is_empty(self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }

    /// Les bornes d'un rectangle de pixels, ramenées en sous-pixels.
    ///
    /// Le pixel `x` occupe les sous-pixels `[x·16, x·16 + 15]` : la borne haute
    /// prend donc le dernier sous-pixel du dernier pixel, et non le premier du
    /// suivant.
    fn of(rect: Rect) -> Self {
        let scale = SUBPIXEL_SCALE;
        Self {
            min_x: rect.x as i32 * scale,
            max_x: (rect.x + rect.width) as i32 * scale - 1,
            min_y: rect.y as i32 * scale,
            max_y: (rect.y + rect.height) as i32 * scale - 1,
        }
    }

    /// Étend les bornes jusqu'à contenir le point.
    fn add(&mut self, x: i32, y: i32) {
        // Par comparaisons écrites, comme partout dans ce projet : `min` et
        // `max` de la bibliothèque ne traitent pas NaN et −0 comme les chemins
        // SIMD, et la règle vaut même là où aucun flottant n'entre.
        if x < self.min_x {
            self.min_x = x;
        }
        if x > self.max_x {
            self.max_x = x;
        }
        if y < self.min_y {
            self.min_y = y;
        }
        if y > self.max_y {
            self.max_y = y;
        }
    }

    /// Le rectangle de pixels qui contient ces bornes, élargi d'un sous-pixel.
    ///
    /// **Tout pixel que le polygone touche est inclus**, et non « tout pixel dont
    /// le centre est dedans ». Le portail partage ses arêtes avec les triangles
    /// du mur qui l'entoure, la règle top-left départage les centres tombant
    /// exactement dessus, et une fenêtre au centre près pourrait exclure un
    /// centre que la géométrie retient. Un trou est définitif, un pixel dilaté
    /// est gratuit.
    ///
    /// L'élargissement d'un sous-pixel absorbe l'arrondi de tous les croisements
    /// calculés plus haut, quel qu'en soit le sens : la fenêtre n'a besoin que
    /// d'être conservatrice.
    fn to_rect(self) -> Rect {
        if self.is_empty() {
            return Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            };
        }
        // `div_euclid` et non la division de Rust, qui tronque vers zéro : un
        // sous-pixel négatif tomberait alors du mauvais côté et la fenêtre
        // mordrait dans l'image.
        let x0 = (self.min_x - 1).div_euclid(SUBPIXEL_SCALE);
        let x1 = (self.max_x + 1).div_euclid(SUBPIXEL_SCALE);
        let y0 = (self.min_y - 1).div_euclid(SUBPIXEL_SCALE);
        let y1 = (self.max_y + 1).div_euclid(SUBPIXEL_SCALE);

        // Les bornes négatives se ramènent à zéro : la fenêtre ne sert qu'à
        // borner un parcours dans l'image, et ce qui est hors d'elle est déjà
        // traité par l'intersection avec la fenêtre reçue.
        let x0 = if x0 < 0 { 0 } else { x0 } as u32;
        let y0 = if y0 < 0 { 0 } else { y0 } as u32;
        if x1 < 0 || y1 < 0 {
            return Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            };
        }
        Rect {
            x: x0,
            y: y0,
            width: (x1 as u32).saturating_sub(x0) + 1,
            height: (y1 as u32).saturating_sub(y0) + 1,
        }
    }
}

/// Réduit une fenêtre à ce qu'un portail en laisse voir.
///
/// Les points sont ceux du portail en espace monde, dans son ordre
/// d'enroulement ; `view` porte la pose de la caméra déjà inversée, et
/// `projection` le cadrage courant.
///
/// **Le portail se découpe en éventail depuis son premier sommet**, ce que sa
/// convexité — vérifiée au chargement — rend licite sans découpe d'oreilles.
/// Chaque triangle passe par le chemin flottant existant, verbatim : aucune
/// opération flottante nouvelle n'entre dans le pipeline, donc aucun site
/// d'arrondi nouveau. La boîte d'une union étant l'union des boîtes, prendre les
/// extrema sur tout l'éventail rend rigoureusement la boîte du portail, sans
/// qu'aucun morceau ait à être recollé — et **l'ordre des triangles n'a même pas
/// d'importance**, minimum et maximum sur des entiers étant commutatifs. C'est le
/// seul calcul dérivé du projet dont l'ordre d'opérations ne soit pas
/// contractuel.
///
/// La réduction prend la boîte du portail **déjà découpé par la fenêtre reçue**,
/// et non l'intersection de sa boîte avec elle. Les deux donnent le même
/// rectangle dans presque tous les cas ; elles divergent d'un facteur deux et
/// demi sur un grand portail oblique vu à travers une porte étroite. Ce que cela
/// gagne n'est pas du remplissage, c'est le liseré où naissent les portails
/// faussement visibles, donc les cellules ramenées pour rien.
///
/// Rend une fenêtre vide — de largeur nulle — quand le portail n'en laisse rien :
/// la branche de traversée s'arrête là.
pub(crate) fn reduce(
    window: Rect,
    points: &[Vec3],
    view: Affine3,
    projection: &Projection,
) -> Rect {
    if points.len() < 3 || window.width == 0 || window.height == 0 {
        return Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }

    let limits = Bounds::of(window);
    let mut bounds = Bounds::EMPTY;

    for i in 1..points.len() - 1 {
        let corners = [points[0], points[i], points[i + 1]];
        let mut homogeneous = [ClipVertex::ZERO; 3];
        let mut projected = true;
        for (slot, corner) in homogeneous.iter_mut().zip(corners) {
            // Les attributs ne servent pas : une fenêtre ne porte ni texture ni
            // lumière, et seuls `x`, `y` et `w` décident de sa géométrie.
            match projection.to_clip(view.transform_point(corner), 0.0, 0.0, 0.0, 0.0, [0.0; 3]) {
                Some(vertex) => *slot = vertex,
                // Un sommet hors des limites de coordonnées ne réduit rien : le
                // triangle est abandonné, ce qui laisse la fenêtre plus large et
                // donc conservatrice.
                None => {
                    projected = false;
                    break;
                }
            }
        }
        if !projected {
            continue;
        }

        let polygon = clip(homogeneous, projection.frustum());
        for t in 0..polygon.triangle_count() {
            let vertices = polygon.triangle(t).map(|v| {
                let screen = projection.to_vertex(v);
                (screen.x, screen.y)
            });
            accumulate(&vertices, limits, &mut bounds);
        }
    }

    // La dilatation d'un sous-pixel peut pousser un bord juste au-delà de la
    // fenêtre reçue : l'intersection la ramène. C'est aussi ce qui rend la
    // réduction monotone, propriété dont la traversée a besoin pour terminer.
    bounds.to_rect().intersect(window)
}

/// Accumule dans `bounds` la boîte de l'intersection d'un triangle avec
/// `limits`.
///
/// Découpage de Sutherland-Hodgman contre les quatre bords, en sous-pixels : les
/// écarts entre sommets atteignent 2¹⁷ et les produits 2³⁴, d'où les `i64` —
/// mêmes bornes que les fonctions de bord, et la même marge.
fn accumulate(triangle: &[(i32, i32); 3], limits: Bounds, bounds: &mut Bounds) {
    let mut current = [(0i32, 0i32); MAX_WINDOW_VERTICES];
    let mut next = [(0i32, 0i32); MAX_WINDOW_VERTICES];
    let mut len = 3;
    current[..3].copy_from_slice(triangle);

    // Chaque bord est un demi-plan aligné sur un axe : le côté se lit sur une
    // seule coordonnée, et le croisement ne demande qu'une division.
    let edges = [
        (true, limits.min_x, true),
        (true, limits.max_x, false),
        (false, limits.min_y, true),
        (false, limits.max_y, false),
    ];

    for (vertical, bound, keep_greater) in edges {
        let inside = |p: (i32, i32)| {
            let value = if vertical { p.0 } else { p.1 };
            if keep_greater {
                value >= bound
            } else {
                value <= bound
            }
        };

        let mut count = 0;
        for i in 0..len {
            let a = current[i];
            let b = current[(i + 1) % len];
            let a_in = inside(a);
            if a_in {
                next[count] = a;
                count += 1;
            }
            if a_in != inside(b) {
                next[count] = cross(a, b, bound, vertical);
                count += 1;
            }
        }

        len = count;
        if len == 0 {
            return;
        }
        current[..len].copy_from_slice(&next[..len]);
    }

    for point in &current[..len] {
        bounds.add(point.0, point.1);
    }
}

/// Le croisement du segment `a → b` avec la droite `bound`, verticale ou
/// horizontale.
///
/// La division tronque vers zéro, et ce n'est pas corrigé ici : la boîte
/// obtenue est élargie d'un sous-pixel à la conversion en pixels, ce qui absorbe
/// l'arrondi dans le sens conservateur quel qu'il soit. Corriger chaque
/// croisement selon son bord coûterait quatre cas de plus pour un résultat que
/// la fenêtre n'utilise pas plus finement.
fn cross(a: (i32, i32), b: (i32, i32), bound: i32, vertical: bool) -> (i32, i32) {
    let (from, to, other_from, other_to) = if vertical {
        (a.0, b.0, a.1, b.1)
    } else {
        (a.1, b.1, a.0, b.0)
    };
    let span = i64::from(to) - i64::from(from);
    // Un segment parallèle au bord n'a pas de croisement à donner ; il ne peut
    // pas en avoir été demandé, ses deux extrémités étant du même côté.
    let other = if span == 0 {
        i64::from(other_from)
    } else {
        i64::from(other_from)
            + (i64::from(bound) - i64::from(from)) * (i64::from(other_to) - i64::from(other_from))
                / span
    };
    // Les deux coordonnées restent dans la bande de garde, dont les bornes sont
    // celles du format 28.4 : la conversion ne peut pas saturer.
    let other = other as i32;
    if vertical {
        (bound, other)
    } else {
        (other, bound)
    }
}

#[cfg(test)]
mod tests;
