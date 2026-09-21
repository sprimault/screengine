// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La projection, et le tronc de vue qu'elle découpe.
//!
//! Le repère de vue est X à droite, Y vers le bas, Z vers l'avant, et `w` est
//! la profondeur de vue elle-même. La projection n'est pas une matrice : cinq
//! valeurs suffisent, et garder `w = z_vue` exactement réduit le clipping du
//! plan proche à une comparaison au lieu d'un test de plan général.
//!
//! Le plan lointain est à l'infini. Ce n'est pas un raffinement : à distance
//! finie, la profondeur cesse d'être un multiple constant de `1/w`, et
//! [`to_depth`] recevrait autre chose que ce que sa documentation annonce.
//!
//! [`to_depth`]: super::fixed::to_depth

use core::f32::consts::PI;

use crate::error::{Argument, Error, Result};

use super::Vec3;
use super::fixed::{to_depth, to_subpixel};

/// La demi-largeur de la bande de garde, en pixels.
///
/// Borne **absolue** sur la coordonnée écran, et non marge autour de l'image :
/// c'est elle qui garantit que le résultat de `to_subpixel` tient dans ±2¹⁶, et
/// avec lui toutes les bornes entières du rasteriseur.
pub const GUARD_PIXELS: f32 = 4096.0;

/// Au-delà, une coordonnée de clip est rejetée avant le découpage.
///
/// **Ce que la borne doit couvrir, ce sont les produits de
/// [`Frustum::intersect`]**, et non l'évaluation des plans. Une distance est
/// linéaire en la coordonnée — au plus `5121 · L`, les coefficients de plan
/// valant `GUARD_PIXELS + center` avec un centre sous 1024 —, mais
/// `da · b.x - db · a.x` est **quadratique** : `2 · 5121 · L²`. À `L = 2⁵⁶`, ce
/// numérateur reste sous 2¹²⁵·³², un facteur six sous l'infini d'un `f32`.
///
/// **La borne porte sur la coordonnée de clip, et non sur celle de vue.** `x`
/// vaut `scale_x · x_vue`, et `scale_x` croît sans borne quand le champ de
/// vision se ferme : au pas le plus fin de l'angle binaire, la cotangente de la
/// demi-ouverture dépasse cinq millions. Testée en amont de cette
/// multiplication, la borne laissait passer des coordonnées de clip infinies,
/// dont `intersect` faisait un `NaN` puis un triangle de bruit — sans erreur et
/// sans panique, ce qu'elle existe précisément pour empêcher.
const COORDINATE_LIMIT: f32 = 7.205_759_4e16;

/// Un sommet projeté, avant la division par `w`.
///
/// `z` n'y figure pas : le plan lointain étant à l'infini, la coordonnée de
/// profondeur homogène vaut `near` pour tous les sommets, et n'apprend rien.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipVertex {
    /// L'abscisse homogène, `sx · x_vue`.
    pub x: f32,
    /// L'ordonnée homogène, `sy · y_vue`.
    pub y: f32,
    /// La profondeur de vue, qui est aussi le `w` homogène.
    pub w: f32,
}

/// Les cinq plans contre lesquels un triangle se découpe.
///
/// Le plan proche, puis les quatre de la bande de garde. Pas les six du tronc :
/// les côtés de l'image se traitent par découpe du rectangle de parcours, les
/// fonctions de bord s'évaluant en coordonnées globales. Clipper les côtés
/// produirait des sommets pour un résultat identique au bit près.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frustum {
    near: f32,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

/// Le nombre de plans, et donc l'ordre du découpage.
pub const PLANE_COUNT: usize = 5;

impl Frustum {
    /// La distance signée du sommet au plan d'indice `plane`, positive à
    /// l'intérieur.
    ///
    /// Les cinq expressions sont linéaires en `(x, y, w)`, ce dont dépend tout
    /// le reste : un sommet engendré étant une combinaison convexe de deux
    /// sommets, il ne peut violer un plan que ses deux parents satisfont.
    pub fn distance(&self, v: ClipVertex, plane: usize) -> f32 {
        match plane {
            0 => v.w - self.near,
            1 => v.x + self.left * v.w,
            2 => self.right * v.w - v.x,
            3 => v.y + self.top * v.w,
            _ => self.bottom * v.w - v.y,
        }
    }

    /// Le point où le segment `a → b` traverse un plan, `da` et `db` étant les
    /// distances de ses extrémités à ce plan.
    ///
    /// **La forme est symétrique au bit près, et c'est tout son objet.** Deux
    /// triangles qui partagent une arête la parcourent en sens opposés : si le
    /// point d'intersection différait d'un seul bit entre les deux, une fissure
    /// s'ouvrirait le long de l'arête — invisible à l'arrêt, visible en
    /// mouvement, exactement ce que la règle top-left élimine ailleurs.
    ///
    /// `a + t · (b - a)` n'a pas cette propriété : les deux sens calculent deux
    /// quotients arrondis séparément, dont la somme exacte vaut un sans que
    /// leurs arrondis la vaillent. Ici, échanger les extrémités nie exactement
    /// le numérateur et le dénominateur — l'arrondi au plus proche pair est une
    /// fonction impaire, donc `fl(v - u) = -fl(u - v)` sans exception —, et le
    /// quotient correctement arrondi de deux valeurs niées est le même
    /// flottant. Quatre opérations, toutes définies au bit près par IEEE 754 :
    /// la propriété survivra telle quelle aux chemins SIMD, tant qu'aucune
    /// intrinsèque fusionnée n'y entre.
    ///
    /// Le court-circuit sur une distance nulle n'est pas une optimisation : il
    /// rend un sommet posé exactement sur le plan identique à lui-même, là où
    /// la forme générale en produirait une copie voisine et un triangle en
    /// aiguille.
    pub fn intersect(a: ClipVertex, b: ClipVertex, da: f32, db: f32) -> ClipVertex {
        if da == 0.0 {
            return a;
        }
        if db == 0.0 {
            return b;
        }
        let inverse = 1.0 / (da - db);
        ClipVertex {
            x: (da * b.x - db * a.x) * inverse,
            y: (da * b.y - db * a.y) * inverse,
            w: (da * b.w - db * a.w) * inverse,
        }
    }

    /// Les bits des plans que le sommet viole, un par plan.
    ///
    /// Leur `ET` sur les trois sommets désigne un demi-espace qui les exclut
    /// tous : le triangle est rejeté sans être découpé. Leur `OU` nul dit
    /// qu'aucun plan ne coupe, et le triangle passe sans découpe — le cas de
    /// l'écrasante majorité.
    pub fn outcode(&self, v: ClipVertex) -> u8 {
        let mut code = 0;
        for plane in 0..PLANE_COUNT {
            // NaN nommément, et non par une comparaison niée : c'est ce que
            // veut `rust.md`, et le sens est le même — un sommet dont une
            // coordonnée n'est pas un nombre est dehors, de tous les plans qui
            // la lisent.
            let d = self.distance(v, plane);
            if d.is_nan() || d < 0.0 {
                code |= 1 << plane;
            }
        }
        code
    }
}

/// La projection perspective d'une résolution et d'un champ de vision donnés.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    scale_x: f32,
    scale_y: f32,
    center_x: f32,
    center_y: f32,
    near: f32,
    frustum: Frustum,
}

impl Projection {
    /// La projection d'une image de `width × height`, de champ de vision
    /// vertical `fov_y` radians et de plan proche `near`.
    ///
    /// `fov_y` est refusé hors de `]0, π[` **sur les radians, avant toute
    /// conversion** : [`Angle::from_radians`] replie à un tour, si bien qu'un
    /// champ de vision de trois demi-tours y deviendrait un demi-tour, accepté
    /// sans que rien ne le signale.
    ///
    /// [`Angle::from_radians`]: super::Angle::from_radians
    pub fn new(width: u32, height: u32, fov_y: f32, near: f32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidArgument(Argument::Resolution));
        }
        if fov_y.is_nan() || fov_y <= 0.0 || fov_y >= PI {
            return Err(Error::InvalidArgument(Argument::Projection));
        }
        if near.is_nan() || near <= 0.0 || near > COORDINATE_LIMIT {
            return Err(Error::InvalidArgument(Argument::Projection));
        }

        let half = super::Angle::from_radians(fov_y).half();
        let (sin, cos) = (half.sin(), half.cos());
        // L'angle binaire a un pas fini : sous quelques milliardièmes de radian,
        // la demi-ouverture s'arrondit à zéro et la cotangente serait infinie.
        if sin <= 0.0 {
            return Err(Error::InvalidArgument(Argument::Projection));
        }

        // Pixels carrés : le rapport d'aspect est porté par la largeur seule, et
        // les deux facteurs sont égaux. Ils restent distincts pour qu'un pixel
        // non carré n'exige pas une seconde API.
        let scale = (height as f32 * 0.5) * (cos / sin);
        // Le centre du pixel `i` est en `i + 0,5`, ce que le rasteriseur code
        // par son demi-pas : `(width - 1) / 2` décalerait l'image entière d'un
        // demi-pixel sans que rien ne le montre.
        let center_x = width as f32 * 0.5;
        let center_y = height as f32 * 0.5;

        Ok(Self {
            scale_x: scale,
            scale_y: scale,
            center_x,
            center_y,
            near,
            frustum: Frustum {
                near,
                left: GUARD_PIXELS + center_x,
                right: GUARD_PIXELS - center_x,
                top: GUARD_PIXELS + center_y,
                bottom: GUARD_PIXELS - center_y,
            },
        })
    }

    /// Les plans de découpe.
    pub fn frustum(&self) -> &Frustum {
        &self.frustum
    }

    /// Porte un point de l'espace de vue en espace de clip.
    ///
    /// Rend `None` sur une coordonnée non finie ou démesurée : c'est le seul
    /// contrôle du chemin, et il est en amont du découpage pour qu'aucune
    /// distance de plan ne puisse déborder. Un `NaN` tomberait du côté extérieur
    /// de chaque plan, donc disparaîtrait de lui-même — mais un seul sommet
    /// suffirait à empoisonner les deux autres par le calcul d'intersection.
    pub fn to_clip(self, view: Vec3) -> Option<ClipVertex> {
        let over = |v: f32| v.is_nan() || v.abs() > COORDINATE_LIMIT;
        // Après la multiplication, jamais avant : c'est la coordonnée de clip
        // qui entre dans le découpage. Les facteurs d'échelle étant finis et
        // strictement positifs, un `x_vue` non fini ressort non fini et tombe
        // dans le même test — il n'y a donc rien à vérifier en amont.
        let x = self.scale_x * view.x;
        let y = self.scale_y * view.y;
        if over(x) || over(y) || over(view.z) {
            return None;
        }
        Some(ClipVertex { x, y, w: view.z })
    }

    /// Divise par `w` et passe en virgule fixe : la dernière opération flottante
    /// du moteur.
    ///
    /// Un seul inverse pour les trois valeurs, dans un ordre écrit une fois.
    /// La profondeur se **recalcule** ici et ne s'interpole jamais au
    /// découpage : `near/w` n'est pas affine le long d'une arête de l'espace,
    /// et une profondeur interpolée serait incohérente avec l'équation de plan
    /// du rasteriseur, qui la suppose affine en espace écran. C'est aussi ce qui
    /// garde [`to_depth`] seul maître du bornage.
    ///
    /// [`to_depth`]: super::fixed::to_depth
    pub fn to_vertex(self, v: ClipVertex) -> ProjectedVertex {
        let inverse = 1.0 / v.w;
        ProjectedVertex {
            x: to_subpixel(self.center_x + v.x * inverse),
            y: to_subpixel(self.center_y + v.y * inverse),
            z: to_depth(self.near * inverse),
        }
    }
}

/// Un sommet après division, prêt pour le rasteriseur.
///
/// Type de passage : il évite que ce module connaisse les types du rasteriseur,
/// et donc que `math` dépende de `raster`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectedVertex {
    /// L'abscisse écran, en sous-pixels.
    pub x: i32,
    /// L'ordonnée écran, en sous-pixels.
    pub y: i32,
    /// La profondeur, `near/w` en 0.32.
    pub z: u32,
}

#[cfg(test)]
mod tests;
