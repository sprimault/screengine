// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les lumières ponctuelles, ajoutées à l'éclairage d'une surface.
//!
//! **Tout se passe avant la projection**, du côté flottant du pipeline : il
//! n'existe aucune distance du côté entier, et la racine inverse du noyau vit
//! ici. Ce qui en sort est une couleur par sommet, que le rasteriseur
//! interpole comme n'importe quel autre attribut.

use crate::math::Vec3;
use crate::scene::Light;

/// Le nombre de lumières qu'une image peut porter.
///
/// Huit. Le calcul est par sommet, donc son coût suit le nombre de sommets
/// soumis et non celui des pixels : huit lumières sur un mur de quatre sommets
/// font trente-deux atténuations, ce qui ne se mesure pas. La borne existe
/// pour que le tableau vive dans le contexte sans allocation, et parce qu'une
/// scène qui en réclamerait davantage veut en réalité des lightmaps.
pub const MAX_LIGHTS: usize = 8;

/// Une lumière placée en espace de vue, prête à éclairer des sommets.
///
/// **La transformation a lieu une fois par lot, pas une fois par sommet.** La
/// vue est rigide, donc la distance y est la même qu'en monde : rien n'oblige
/// à ramener les sommets en coordonnées de monde, ce qui coûterait une matrice
/// de plus par sommet.
#[derive(Debug, Clone, Copy)]
pub struct Placed {
    /// Sa position en espace de vue.
    position: Vec3,
    /// L'inverse du carré de son rayon, qui est ce que l'atténuation emploie.
    ///
    /// Précalculé : la division vit ici, une fois par lumière et par lot, au
    /// lieu d'une fois par sommet et par lumière.
    inverse_square: f32,
    /// Sa couleur, en canaux de zéro à un.
    ///
    /// En flottant et non en octets : la somme de plusieurs lumières dépasse
    /// couramment l'unité, et saturer chaque contribution avant de les ajouter
    /// éteindrait les lumières faibles là où une forte domine.
    color: [f32; 3],
}

impl Placed {
    /// Place une lumière, la position étant déjà en espace de vue.
    ///
    /// Rend `None` pour un rayon nul, négatif ou non fini : une lumière sans
    /// étendue n'éclaire rien, et la garder ferait diviser par zéro.
    pub fn new(light: &Light, position: Vec3) -> Option<Self> {
        // `is_finite` d'abord : il écarte `NaN`, que la comparaison qui suit
        // laisserait passer puisqu'elle est fausse dans les deux sens.
        if !light.radius.is_finite() || light.radius <= 0.0 {
            return None;
        }
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            return None;
        }
        Some(Self {
            position,
            inverse_square: 1.0 / (light.radius * light.radius),
            color: [
                f32::from(light.color.r) / 255.0,
                f32::from(light.color.g) / 255.0,
                f32::from(light.color.b) / 255.0,
            ],
        })
    }

    /// Ce que cette lumière ajoute au sommet `point`, en espace de vue.
    ///
    /// L'atténuation est `(1 − d²/r²)²`, **et le carré n'est pas un
    /// raffinement** : `1 − d²/r²` s'annule en `r` avec une dérivée non nulle,
    /// ce qui dessine un anneau net au bord de la portée — un défaut qu'on
    /// prend pour une erreur de géométrie. Le carré l'annule en douceur.
    ///
    /// Aucune racine n'est appelée : c'est le carré de la distance qui entre
    /// dans la formule, et il s'obtient par un produit scalaire.
    fn contribution(&self, point: Vec3) -> [f32; 3] {
        let offset = Vec3::new(
            point.x - self.position.x,
            point.y - self.position.y,
            point.z - self.position.z,
        );
        let ratio = offset.dot(offset) * self.inverse_square;
        if ratio >= 1.0 {
            return [0.0; 3];
        }
        let falloff = (1.0 - ratio) * (1.0 - ratio);
        [
            self.color[0] * falloff,
            self.color[1] * falloff,
            self.color[2] * falloff,
        ]
    }
}

/// La somme des lumières au sommet `point`, en canaux de zéro à un, saturée.
///
/// **La saturation a lieu après la somme, pas avant.** Saturer chaque
/// contribution séparément éteindrait les lumières faibles partout où une
/// forte domine, alors que leur teinte est précisément ce qu'on veut voir se
/// mélanger.
pub fn sum(lights: &[Placed], point: Vec3) -> [f32; 3] {
    let mut total = [0.0f32; 3];
    for light in lights {
        let add = light.contribution(point);
        total[0] += add[0];
        total[1] += add[1];
        total[2] += add[2];
    }
    // Comparaison écrite, jamais `min` : celui-ci ne traite pas `NaN` de la
    // même façon en SSE et en NEON, et le noyau se l'interdit partout.
    let capped = |value: f32| if value > 1.0 { 1.0 } else { value };
    [capped(total[0]), capped(total[1]), capped(total[2])]
}

#[cfg(test)]
mod tests;
