// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! L'équation de plan d'un attribut sur un triangle, en virgule fixe.
//!
//! Un attribut affine en espace écran — la profondeur `near/w` aujourd'hui —
//! s'évalue en un pixel par une forme close : valeur au point de référence,
//! plus deux gradients multipliés par l'écart. Les gradients se calculent une
//! fois, par une division entière arrondie vers le bas ; l'évaluation n'a plus
//! aucun arrondi, donc elle ne dépend ni de la tuile, ni du point de départ du
//! parcours.

use super::triangle::Point;

/// Bits fractionnaires des gradients, par sous-pixel.
///
/// Une valeur de sommet tient dans 2³², un écart de coordonnées dans 2¹⁷ : le
/// numérateur d'un gradient, écrit en différences, reste sous 2⁵⁰, et décalé de
/// douze bits sous 2⁶², dans un `i64`. L'arrondi de chaque gradient coûte moins
/// d'une unité de 2⁻¹² par sous-pixel ; sur les 2¹⁷ sous-pixels de la bande de
/// garde et deux gradients, l'erreur en un pixel reste sous 64 unités de
/// l'attribut.
pub const GRADIENT_BITS: u32 = 12;

/// Un attribut sur un triangle.
///
/// Le point de référence n'y figure pas : il est commun aux plans d'un même
/// triangle, qui le tient une fois pour les trois. Le dupliquer coûterait seize
/// octets par triangle et ferait recalculer trois fois les mêmes écarts dans la
/// boucle de pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plane {
    /// La valeur au point de référence, décalée de [`GRADIENT_BITS`].
    base: i64,
    /// Les gradients par sous-pixel, en [`GRADIENT_BITS`] bits fractionnaires.
    dx: i64,
    dy: i64,
}

impl Plane {
    /// L'équation de plan des valeurs `z` aux sommets `v`, dont l'aire signée
    /// `area` est celle que rend `edge` sur ces trois sommets, dans cet ordre.
    ///
    /// **L'aire arrive signée, et c'est ici qu'elle se normalise.** Le
    /// dénominateur des gradients est cette aire : la nier sans nier les
    /// numérateurs inverserait le sens de la profondeur, silencieusement, sur un
    /// moteur qui rend désormais les faces avant par négation des fonctions de
    /// bord. Le triplet change donc de signe d'un bloc, et le diviseur reste
    /// positif pour que `div_euclid` garde l'arrondi vers le bas que sa
    /// documentation promet. Un appelant qui normaliserait de son côté pourrait
    /// désaccorder l'aire de ses sommets ; un seul endroit le peut, celui-ci.
    ///
    /// `reference` désigne le sommet d'où part l'évaluation, et il doit être le
    /// plus petit dans l'ordre (y, x), jamais `v[0]`. Les gradients ne
    /// dépendent pas de l'ordre des sommets, mais la valeur de départ, si :
    /// prise sur `v[0]`, une permutation circulaire des sommets — qui ne change
    /// ni le triangle ni son orientation — changerait l'arrondi en chaque
    /// pixel, et deux soumissions du même triangle ne rendraient pas la même
    /// profondeur. Seul un test de permutation le voit. Il arrive en paramètre
    /// parce que les plans d'un même triangle doivent le partager.
    pub fn new(v: [Point; 3], z: [i64; 3], area: i64, reference: usize) -> Self {
        debug_assert!(area != 0);
        // **La précondition n'est pas « les valeurs tiennent dans 2³² » mais
        // « leur étendue y tient ».** C'est elle qui borne le numérateur à 2⁵⁰
        // et donc le gradient décalé à 2⁶², un bit sous l'`i64`. Une profondeur
        // `u32` la vérifie parce qu'elle est positive, un attribut `i32` parce
        // qu'il est sur 32 bits ; un attribut vraiment étalé sur ±2³² ne la
        // vérifierait pas, et il n'y a pas de place pour l'accueillir.
        debug_assert!(
            z.iter().max().unwrap_or(&0) - z.iter().min().unwrap_or(&0) < 1 << 32,
            "étendue des valeurs au-delà de 2³²"
        );
        let (x, y) = (v.map(|p| p.x as i64), v.map(|p| p.y as i64));

        // En différences à partir du troisième sommet : c'est ce qui borne le
        // numérateur à 2⁵⁰ au lieu de trois termes de 2⁴⁹.
        let numerator_x = (z[0] - z[2]) * (y[1] - y[2]) + (z[1] - z[2]) * (y[2] - y[0]);
        let numerator_y = (z[0] - z[2]) * (x[2] - x[1]) + (z[1] - z[2]) * (x[0] - x[2]);

        let (numerator_x, numerator_y, area) = if area < 0 {
            (-numerator_x, -numerator_y, -area)
        } else {
            (numerator_x, numerator_y, area)
        };

        Self {
            base: z[reference] << GRADIENT_BITS,
            // `div_euclid` avec un diviseur positif arrondit vers le bas, de
            // part et d'autre de zéro. La division `/` tronquerait vers zéro, et
            // un gradient qui change de signe ferait un pas double autour de 0.
            dx: (numerator_x << GRADIENT_BITS).div_euclid(area),
            dy: (numerator_y << GRADIENT_BITS).div_euclid(area),
        }
    }

    /// La valeur, décalée de [`GRADIENT_BITS`], au point d'écarts `(ex, ey)`
    /// sous-pixels depuis le point de référence.
    ///
    /// En arithmétique enveloppante, et c'est exact **partout dans l'enveloppe
    /// convexe du triangle**, pas seulement sur les pixels que le parcours
    /// retient. La valeur exacte y est une combinaison convexe des trois
    /// valeurs de sommet, donc bornée par leur étendue — 2⁴⁴ une fois décalée
    /// —, et le biais top-left ne l'élargit que d'un facteur trois au pire :
    /// dix-sept bits de marge sous l'`i64`. Sur un triangle très fin un
    /// gradient approche 2⁶² et un produit intermédiaire déborde, mais
    /// l'addition et la multiplication étant des morphismes modulo 2⁶⁴, le
    /// résultat reste l'entier exact dès que cet entier tient dans un `i64`.
    ///
    /// Cette garantie porte tout le schéma des segments de perspective, dont
    /// les extrémités sont le span et non des pixels du parcours.
    pub fn at(&self, ex: i64, ey: i64) -> i64 {
        self.base
            .wrapping_add(self.dx.wrapping_mul(ex))
            .wrapping_add(self.dy.wrapping_mul(ey))
    }

    /// Le pas d'un pixel vers la droite, décalé de [`GRADIENT_BITS`].
    ///
    /// Il n'y a pas de pas vertical : le parcours reprend la forme close au
    /// premier pixel de chaque ligne, les spans ne commençant pas tous à la
    /// même abscisse.
    pub fn step_x(&self, pixel: i32) -> i64 {
        self.dx.wrapping_mul(pixel as i64)
    }
}

#[cfg(test)]
mod tests;
