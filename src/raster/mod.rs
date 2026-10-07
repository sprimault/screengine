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
mod line;
mod plane;
pub mod simd;
mod triangle;

use plane::GRADIENT_BITS;

pub use bins::{Bins, Grid};
pub use clip::{MAX_CLIP_TRIANGLES, clip};
pub use line::{Segment, clip_segment, cover};
pub use simd::SimdPath;
pub(crate) use triangle::DITHER;
pub use triangle::{
    Lighting, Lit, MODULATED, NO_LIGHTING, NO_TEXTURE, Prepared, Sampling, Vertex, prepare,
    prepare_lit,
};

/// Remplit un triangle par le chemin demandé.
///
/// **C'est le seul endroit où un chemin se choisit**, et c'est voulu : la
/// sélection vit à la frontière du remplissage, pas à l'intérieur. Une variante
/// qui déciderait d'elle-même rendrait le forçage inopérant, et c'est lui qui
/// permet de jouer les trois chemins sur le même processeur.
///
/// **Toutes les variantes rendent la même image, au bit près.** Une divergence
/// est une variante fausse, jamais une différence acceptable — c'est ce que la
/// conformance vérifie en rejouant chaque scène par chaque chemin disponible.
pub fn fill<T: Target>(
    target: &mut T,
    window: Rect,
    triangle: &Prepared,
    sampling: Option<Sampling<'_>>,
    lit: Option<Lit<'_>>,
    simd: SimdPath,
) {
    // **Le chemin ne se résout pas ici, il descend jusqu'au span.** Ce qui se
    // vectorise est le remplissage d'une ligne unie, que le puits traite d'un
    // bloc parce que lui seul tient ses tampons ; la mise en place du triangle et
    // le parcours des lignes restent communs, et les dupliquer par chemin serait
    // recopier le rasteriseur pour en vectoriser la seule boucle intérieure.
    triangle::fill(target, window, triangle, sampling, lit, simd.resolve());
}

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
/// Remplit une ligne unie pixel par pixel : la référence du chemin par span.
///
/// **Une fonction libre et non le corps du défaut**, parce qu'un puits qui
/// surcharge [`Target::span`] doit pouvoir y revenir — Rust ne donne pas accès à
/// l'implémentation par défaut depuis une surcharge, et la recopier ferait
/// exactement ce que ce projet refuse : deux textes qui disent la même chose et
/// finissent par diverger. C'est ici qu'est écrit ce que toute variante doit
/// rendre, au bit près.
pub fn span_scalar<T: Target + ?Sized>(target: &mut T, span: Span) {
    let mut depth = span.depth;
    for x in span.x0..=span.x1 {
        // En un pixel couvert, la valeur tient dans [0, 2³²) : les sommets sont
        // bornés par `to_depth` avec une marge qui couvre l'arrondi des
        // gradients.
        let z = (depth >> GRADIENT_BITS) as u32;
        if span.modulated {
            if target.test_modulated(x, span.y, z.saturating_add(span.bias)) {
                target.modulate(x, span.y, span.color);
            }
        } else if target.test(x, span.y, z) {
            target.write(x, span.y, z, span.color);
        }
        depth = depth.wrapping_add(span.depth_x);
    }
}

/// Une ligne de pixels d'une surface **unie**, telle que le remplissage la
/// propose d'un bloc.
///
/// **Le seul chemin de remplissage qui se décrive sans lire une image** : la
/// couleur y est constante et la profondeur affine, si bien que tout le travail
/// tient dans ces champs. Les chemins texturés ou éclairés échantillonnent, donc
/// ils continuent de passer pixel par pixel.
#[derive(Debug, Clone, Copy)]
pub struct Span {
    /// L'ordonnée de la ligne, en coordonnées de l'image.
    pub y: i32,
    /// La première abscisse couverte, incluse.
    pub x0: i32,
    /// La dernière abscisse couverte, incluse.
    pub x1: i32,
    /// La profondeur au centre de `x0`, avant son décalage de gradient.
    pub depth: i64,
    /// Ce que la profondeur gagne d'un pixel au suivant.
    pub depth_x: i64,
    /// La couleur à écrire, constante sur toute la ligne.
    pub color: u32,
    /// La surface est-elle modulée ?
    ///
    /// Elle multiplie alors le tampon au lieu de l'écraser, et son test de
    /// profondeur est non strict et décalé de [`Span::bias`].
    pub modulated: bool,
    /// La tolérance de pente ajoutée à la profondeur d'une surface modulée.
    ///
    /// Nulle quand la surface ne l'est pas, et l'implémentation n'a pas à le
    /// vérifier : c'est le remplissage qui la pose.
    pub bias: u32,
    /// Le chemin de remplissage que le contexte a retenu.
    ///
    /// **Porté par le span et non lu du contexte**, parce que c'est le puits qui
    /// tient ses propres tampons et qui seul peut les traiter d'un bloc : il lui
    /// faut donc savoir par quel jeu d'instructions, et il n'a aucun autre moyen
    /// de l'apprendre.
    pub simd: SimdPath,
}

pub trait Target {
    /// Remplit une ligne entière d'une surface unie.
    ///
    /// **L'implémentation par défaut boucle sur [`Target::test`] et
    /// [`Target::write`]**, et c'est elle qui garde l'étanchéité vérifiable :
    /// les puits de comptage la conservent, donc ils voient toujours chaque
    /// proposition, y compris celles qu'une profondeur rejette. Un puits qui
    /// l'emporterait perdrait exactement ce que ce point de passage existe pour
    /// donner.
    ///
    /// **Ce que l'implémenter achète**, et c'est la seule raison de cette
    /// méthode : un puits qui range sa couleur et sa profondeur en tableaux
    /// contigus peut traiter plusieurs pixels par instruction. Un span est
    /// contigu par construction — les deux tableaux sont indexés
    /// `ligne × largeur + colonne`.
    ///
    /// **Ce que l'implémenter coûte** : le chemin rapide n'est plus éprouvé par
    /// les tests unitaires du rasteriseur, qui gardent le défaut. Sa justesse
    /// tient alors tout entière à l'égalité d'empreinte avec le chemin scalaire,
    /// que la conformance exige scène par scène.
    fn span(&mut self, span: Span) {
        span_scalar(self, span);
    }

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
    ///
    /// **L'égalité, elle, ne va pas de soi** : deux découpes d'un même plan
    /// n'en rendent pas les mêmes bits, et `z` arrive ici porteur de la
    /// tolérance de pente que le rasteriseur ajoute pour ces triangles. Voir
    /// `triangle::slope_bias`.
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
