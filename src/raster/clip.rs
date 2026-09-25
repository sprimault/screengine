// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le découpage d'un triangle contre le plan proche et la bande de garde.
//!
//! En espace homogène, avant la division par `w` : tant que le signe de `w`
//! n'est pas connu, une borne sur la coordonnée écran ne se lit pas comme une
//! inégalité sur les coordonnées de clip, multiplier par un `w` négatif en
//! retournant le sens. C'est pourquoi le plan proche passe en premier, et c'est
//! ce qu'il établit.
//!
//! L'ordre des plans est écrit en dur et ne dépend jamais des données : deux
//! ordres différents engendrent des sommets différents, et c'est la constance
//! de cet ordre d'un triangle à l'autre qui garde cohérentes les arêtes
//! partagées.

use crate::math::projection::{ClipVertex, Frustum, PLANE_COUNT};

/// Le nombre maximal de sommets après découpe.
///
/// Le résultat est l'intersection d'un triangle avec cinq demi-espaces : chaque
/// arête du polygone porte soit une des trois arêtes du triangle, soit un des
/// cinq plans. Huit au plus, donc, quel que soit l'ordre des plans. Le pire cas
/// est ainsi **prouvé** et non majoré — une capacité au jugé serait soit du
/// gaspillage sur chaque triangle, soit un dépassement qu'aucun test ne rejoue.
pub const MAX_CLIP_VERTICES: usize = 3 + PLANE_COUNT;

/// Le nombre maximal de triangles qu'un triangle soumis peut produire.
pub const MAX_CLIP_TRIANGLES: usize = MAX_CLIP_VERTICES - 2;

/// Le polygone convexe issu de la découpe.
///
/// Tient sur la pile de l'appel : huit sommets de quarante octets — position
/// homogène, deux jeux de coordonnées et trois canaux de lumière —, deux tampons
/// en alternance. Aucun tampon de contexte n'est nécessaire, et il n'y a donc
/// rien à dimensionner à la création.
#[derive(Debug, Clone, Copy)]
pub struct Polygon {
    v: [ClipVertex; MAX_CLIP_VERTICES],
    len: usize,
}

impl Polygon {
    /// Le polygone vide, qui sert aussi de valeur initiale aux deux tampons.
    const EMPTY: Self = Self {
        v: [ClipVertex::ZERO; MAX_CLIP_VERTICES],
        len: 0,
    };

    /// Le nombre de triangles de l'éventail, nul si le polygone est vide.
    pub fn triangle_count(&self) -> usize {
        self.len.saturating_sub(2)
    }

    /// Le triangle d'indice `i` de l'éventail, pris depuis le premier sommet.
    ///
    /// Les diagonales de l'éventail ne sont partagées qu'entre deux triangles du
    /// même polygone, dont les sommets sont rigoureusement les mêmes objets :
    /// la règle top-left les rend étanches sans rien de plus.
    pub fn triangle(&self, i: usize) -> [ClipVertex; 3] {
        debug_assert!(i < self.triangle_count());
        [self.v[0], self.v[i + 1], self.v[i + 2]]
    }

    /// Ajoute un sommet, en ignorant ce qui dépasserait la borne prouvée.
    ///
    /// La borne étant démontrée, le dépassement est un défaut du moteur et non
    /// une donnée : l'assertion le signale en débogage, et en release on préfère
    /// un polygone tronqué à une écriture hors tableau.
    fn push(&mut self, v: ClipVertex) {
        debug_assert!(self.len < MAX_CLIP_VERTICES);
        if self.len < MAX_CLIP_VERTICES {
            self.v[self.len] = v;
            self.len += 1;
        }
    }
}

/// Découpe un triangle contre les cinq plans.
///
/// Le chemin rapide — aucun plan violé par aucun sommet — recopie les trois
/// sommets **bit pour bit**, et il est exact et non conservateur : les cinq
/// fonctions de plan sont linéaires, un sommet engendré est une combinaison
/// convexe de deux sommets d'origine, donc aucun plan satisfait par les trois
/// sommets ne peut être violé par un sommet engendré. Les deux chemins rendent
/// rigoureusement la même image, ce qu'un test compare.
pub fn clip(triangle: [ClipVertex; 3], frustum: &Frustum) -> Polygon {
    let codes = [
        frustum.outcode(triangle[0]),
        frustum.outcode(triangle[1]),
        frustum.outcode(triangle[2]),
    ];
    if codes[0] & codes[1] & codes[2] != 0 {
        return Polygon::EMPTY;
    }

    let mut current = Polygon::EMPTY;
    for v in triangle {
        current.push(v);
    }

    let union = codes[0] | codes[1] | codes[2];
    if union == 0 {
        return current;
    }

    let mut next = Polygon::EMPTY;
    for plane in 0..PLANE_COUNT {
        if union & (1 << plane) == 0 {
            continue;
        }
        next.len = 0;
        for i in 0..current.len {
            let a = current.v[i];
            let b = current.v[if i + 1 == current.len { 0 } else { i + 1 }];
            let da = frustum.distance(a, plane);
            let db = frustum.distance(b, plane);
            // Un sommet posé sur le plan est intérieur : c'est ce qui fait
            // tomber un NaN du côté extérieur, toute comparaison avec lui étant
            // fausse.
            let (a_inside, b_inside) = (da >= 0.0, db >= 0.0);
            if a_inside {
                next.push(a);
            }
            // Le croisement n'engendre un sommet que si aucune extrémité n'est
            // déjà sur le plan : sinon l'intersection vaut cette extrémité, que
            // ce tour ou le suivant ajoute de toute façon, et le polygone
            // porterait deux fois le même point — une arête de longueur nulle
            // dont l'éventail ferait un triangle dégénéré.
            if a_inside != b_inside && da != 0.0 && db != 0.0 {
                next.push(Frustum::intersect(a, b, da, db));
            }
        }
        core::mem::swap(&mut current, &mut next);
        if current.len < 3 {
            return Polygon::EMPTY;
        }
    }
    current
}

#[cfg(test)]
mod tests;
