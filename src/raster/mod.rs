// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage, en coordonnées entières.
//!
//! Tout ce qui est ici s'évalue en coordonnées globales de l'image. Rien ne
//! dépend d'un découpage : une valeur rebasée à l'origine d'une tuile reste
//! exacte parce qu'une translation entière l'est, mais son point de départ se
//! calcule toujours par la forme close, au coin global.

mod bins;
mod clip;
mod plane;
mod triangle;

pub use bins::{Bins, Grid};
pub use clip::{MAX_CLIP_TRIANGLES, clip};
pub(crate) use triangle::DITHER;
pub use triangle::{
    Lighting, Lit, MODULATED, NO_LIGHTING, NO_TEXTURE, Prepared, Sampling, Vertex, fill, prepare,
    prepare_lit,
};

/// Un rectangle de l'image, en pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Abscisse du coin haut gauche.
    pub x: u32,
    /// Ordonnée du coin haut gauche.
    pub y: u32,
    /// Largeur.
    pub width: u32,
    /// Hauteur.
    pub height: u32,
}

impl Rect {
    /// Le rectangle vide, que le remplissage traverse sans rien écrire.
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    /// L'intersection de deux rectangles, vide quand ils ne se croisent pas.
    ///
    /// Par comparaisons écrites plutôt que par `min` et `max`, comme partout
    /// ici, et sans soustraction qui puisse déborder : les bords se comparent
    /// avant d'être retranchés, alors qu'une largeur calculée d'abord passerait
    /// par un `u32` négatif.
    pub fn intersect(self, other: Self) -> Self {
        let x0 = if self.x > other.x { self.x } else { other.x };
        let y0 = if self.y > other.y { self.y } else { other.y };
        let right = self.x + self.width;
        let other_right = other.x + other.width;
        let x1 = if right < other_right {
            right
        } else {
            other_right
        };
        let bottom = self.y + self.height;
        let other_bottom = other.y + other.height;
        let y1 = if bottom < other_bottom {
            bottom
        } else {
            other_bottom
        };

        if x1 <= x0 || y1 <= y0 {
            return Self::EMPTY;
        }
        Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        }
    }
}

/// Où le remplissage écrit ses pixels.
///
/// Un puits plutôt qu'une tranche, et ce n'est pas de l'abstraction gratuite :
/// un pixel écrit deux fois est invisible dans le tampon final, donc un
/// recouvrement d'arête partagée ne se détecte qu'en comptant les écritures.
/// Sans ce point de passage, le défaut le plus coûteux du projet n'aurait aucun
/// test capable de l'attraper.
///
/// La généricité est résolue à la compilation : il n'y a pas d'appel indirect
/// dans la boucle de remplissage.
pub trait Target {
    /// Propose un pixel couvert, en coordonnées entières de l'image, avec sa
    /// profondeur en 0.32 : rend vrai s'il passe le test de profondeur.
    ///
    /// Les coordonnées sont toujours dans la fenêtre passée au remplissage :
    /// l'implémentation n'a pas à les vérifier. C'est elle qui fait le test,
    /// pour que le puits de comptage des tests d'étanchéité voie **toutes** les
    /// propositions, y compris celles qu'une profondeur rejette.
    fn test(&mut self, x: i32, y: i32, z: u32) -> bool;

    /// Écrit un pixel que [`Target::test`] vient d'accepter.
    ///
    /// **Séparée du test, et c'est ce qui rend le texturage abordable** : entre
    /// les deux, le remplissage échantillonne la texture, et il ne le fait donc
    /// que pour les pixels qui survivent à la profondeur. Fondues en un seul
    /// appel, les deux obligeraient à échantillonner avant de savoir si le
    /// pixel est visible — sur une scène où le décor se recouvre, la plus
    /// grande partie du travail irait à des pixels occultés.
    ///
    /// La profondeur est redonnée ici plutôt que retenue entre les deux appels :
    /// un état caché dans le puits ferait dépendre l'écriture d'un test qui l'a
    /// précédée, et rien dans le type ne le garantirait.
    fn write(&mut self, x: i32, y: i32, z: u32, color: u32);

    /// Propose un pixel d'une surface **modulée**, dont le test est non strict.
    ///
    /// Non strict parce qu'une surface modulée est coplanaire avec celle
    /// qu'elle marque — une tache d'ombre sur le sol qui la porte —, et que le
    /// test strict la perdrait à égalité de profondeur. Le décor se soumet donc
    /// avant ses taches, ce qui ne demande aucune garantie nouvelle : l'ordre
    /// de soumission est déjà contractuel.
    fn test_modulated(&mut self, x: i32, y: i32, z: u32) -> bool;

    /// Multiplie le pixel déjà écrit par `factor`, **sans toucher la
    /// profondeur**.
    ///
    /// Une surface modulée n'occulte rien. Si elle inscrivait sa profondeur,
    /// deux taches superposées ne se multiplieraient plus qu'une fois, dans un
    /// ordre qui dépendrait de la répartition en tuiles — donc l'image
    /// dépendrait de la taille des tuiles, ce que l'invariant interdit.
    ///
    /// C'est le puits qui porte l'opération et non le remplissage, comme pour
    /// [`Target::write`] : lui seul sait où sa couleur est rangée, et un
    /// accesseur en lecture obligerait chaque implémentation à l'exposer.
    fn modulate(&mut self, x: i32, y: i32, factor: u32);
}
