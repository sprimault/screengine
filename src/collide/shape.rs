// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le balayage d'un segment contre le volume dilaté d'une surface.
//!
//! **La boîte se réduit à un point contre la somme de Minkowski** de la surface
//! et de la boîte réfléchie : le balayage devient un segment contre un convexe,
//! donc une découpe d'intervalle, purement linéaire. Le volume se décompose en
//! trois familles, et chacune a sa fonction ici — la **face**, le **prisme**
//! d'une arête exposée, la **boîte** d'un sommet.
//!
//! **Aucune normalisation nulle part.** La distance signée à un plan de normale
//! non unitaire et le support de la boîte sur cette même normale portent tous
//! deux le facteur `|N|`, qui se simplifie dans le quotient : le temps d'impact
//! est donc exact sans racine, et la normale rendue ne se normalise qu'une fois,
//! tout à la fin, à la sortie du balayage.
//!
//! Les ordres d'opérations sont ceux que `docs/rust.md` fige, section
//! « Collision ».

use crate::math::Vec3d;

/// Ce qu'un balayage retient d'un contact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Touch {
    /// La fraction du déplacement à laquelle le contact a lieu.
    pub(crate) fraction: f64,
    /// La normale du contact, pas encore unitaire.
    pub(crate) normal: Vec3d,
    /// Le rang de la famille : face, arête puis sommet.
    ///
    /// C'est le premier des trois critères qui départagent deux contacts au même
    /// instant, et il est contractuel — voir la clause (c) de la règle de l'arête
    /// partagée.
    pub(crate) rank: u8,
}

/// Le rang d'un contact de face.
pub(crate) const RANK_FACE: u8 = 0;
/// Le rang d'un contact d'arête.
pub(crate) const RANK_EDGE: u8 = 1;
/// Le rang d'un contact de sommet.
pub(crate) const RANK_VERTEX: u8 = 2;

/// Le support de la boîte sur une normale, dans l'ordre x, y, z.
///
/// L'ordre est figé : c'est une valeur dérivée qui entre dans le résultat, donc
/// son ordre d'opérations est contractuel comme celui des transformations.
pub(crate) fn support(normal: Vec3d, half: Vec3d) -> f64 {
    (abs(normal.x) * half.x + abs(normal.y) * half.y) + abs(normal.z) * half.z
}

/// La valeur absolue, écrite plutôt qu'empruntée à la bibliothèque du système.
fn abs(value: f64) -> f64 {
    if value < 0.0 { -value } else { value }
}

/// Le coin de la boîte qui touche la facette d'une face, selon sa normale.
///
/// C'est le point de la boîte qui maximise le produit scalaire avec `normal`,
/// donc celui dont ce produit vaut exactement [`support`] : la facette du volume
/// dilaté est le polygone translaté de ce coin, et le contact d'un centre sur la
/// facette a lieu en ce centre moins ce décalage.
///
/// **Par comparaisons écrites, et une composante nulle de la normale ne décale
/// rien** : `signum` rend `-1` sur le zéro négatif, ce qui décalerait une boîte le
/// long d'un axe que la face ignore. Le résultat ne dépend pas de la norme de
/// `normal`, qui n'est pas unitaire ici.
fn facet_offset(normal: Vec3d, half: Vec3d) -> Vec3d {
    let pick = |n: f64, h: f64| {
        if n > 0.0 {
            h
        } else if n < 0.0 {
            -h
        } else {
            0.0
        }
    };
    Vec3d::new(
        pick(normal.x, half.x),
        pick(normal.y, half.y),
        pick(normal.z, half.z),
    )
}

/// Le contact du segment avec la **face** d'une surface.
///
/// Le plan de la surface décalé du support, puis l'appartenance du point de
/// contact au polygone **non dilaté** : c'est cette seconde moitié qui empêche la
/// face de revendiquer ce qui appartient à une arête ou à un sommet.
///
/// `normal` est la normale intérieure brute, `anchor` un point du plan.
pub(crate) fn face(
    points: &[Vec3d],
    normal: Vec3d,
    anchor: Vec3d,
    half: Vec3d,
    from: Vec3d,
    to: Vec3d,
) -> Option<Touch> {
    let s = support(normal, half);
    let d0 = normal.dot(from - anchor);
    let d1 = normal.dot(to - anchor);

    // Mouvement parallèle au plan : aucun quotient à prendre, et le cas est écrit
    // avant la division plutôt que rattrapé par un epsilon.
    if d0 == d1 {
        return None;
    }
    // L'élément n'est retenu que s'il est franchi **en entrant** : deux murs dos
    // à dos de part et d'autre d'une frontière se départagent ainsi seuls.
    if d1 >= d0 {
        return None;
    }
    let t = (d0 - s) / (d0 - d1);
    if t > 1.0 {
        return None;
    }
    // **Un instant négatif est un contact immédiat, pas une absence de
    // contact.** Il dit que la boîte dilatée franchit déjà ce plan au départ, et
    // `d1 < d0` dit qu'elle continue de s'y enfoncer : l'écarter laissait une
    // bande d'une épaisseur de dilatation où le balayage ne voyait rien, tandis
    // que le départ dans le solide, mesuré sur la **vraie** boîte, restait faux.
    // Une boîte arrêtée au contact par un balayage y retombait au suivant, et
    // traversait le mur un pas après l'autre — ce que seul un enchaînement de
    // deux balayages révèle.
    //
    // **Mais le volume dilaté d'une face est une dalle, jamais un demi-espace**,
    // et c'est ce que le contact immédiat doit borner : `|d| ≤ s`. Un départ
    // au-delà de la dalle est derrière la face, et aucun contact n'y a lieu —
    // ni en s'éloignant, qui est ce cas-ci, ni en revenant, que le rejet
    // d'entrée ci-dessus a déjà écarté. Une face est à sens unique : un contact
    // par l'arrière n'en est pas un.
    //
    // Sans cette borne, un départ situé derrière une face en rendait un contact
    // à l'instant zéro depuis **n'importe quelle distance**, dès que sa
    // projection tombait dans le polygone. Deux formes, et la seconde est celle
    // qui se voit : la face est celle de la cellule du départ, ce qu'une cellule
    // non convexe suffit à produire et ce qui trompe les deux chemins de la même
    // façon ; ou elle est celle d'une **autre** cellule, ce que deux cellules
    // convexes suffisent à produire et ce qui les fait diverger, le chemin brut
    // examinant ce que la traversée n'atteint pas.
    let t = if t < 0.0 {
        if d0 < -s {
            return None;
        }
        0.0
    } else {
        t
    };
    let centre = from + (to - from) * t;
    // **Le point à tester est celui de la surface, pas le centre de la boîte**, et
    // les confondre a été un défaut silencieux sur toute face non axiale. La
    // facette du volume dilaté est le polygone translaté du coin de la boîte le
    // plus avancé selon la normale : quand le centre la touche, le contact réel a
    // lieu en `centre − facet_offset`, qui est dans le plan du polygone. Pour une
    // normale axiale les deux se projettent au même endroit — l'écart ne porte que
    // sur l'axe que `plane_axes` laisse justement tomber —, d'où un test qui
    // paraissait juste partout. Sur une rampe à 45°, le domaine de la face se
    // décalait d'une demi-extension : trou de contact le long de l'arête amont,
    // face fantôme au-delà de l'aval, et les deux chemins de balayage faux de la
    // même façon.
    if !inside(points, normal, centre - facet_offset(normal, half)) {
        return None;
    }
    Some(Touch {
        fraction: t,
        normal,
        rank: RANK_FACE,
    })
}

/// Le contact du segment avec le **prisme** d'une arête.
///
/// L'enveloppe convexe des deux boîtes posées aux extrémités de l'arête : six
/// plans axiaux, et les biseaux que l'arête forme avec chaque axe. Le segment s'y
/// découpe en intervalle.
pub(crate) fn edge(a: Vec3d, b: Vec3d, half: Vec3d, from: Vec3d, to: Vec3d) -> Option<Touch> {
    let mut slab = Interval::FULL;
    let direction = b - a;

    // Les trois paires de plans axiaux, qui bornent le prisme dans le sens de
    // chaque axe. Un axe où l'arête ne s'étend pas donne la seule épaisseur de la
    // boîte, ce que le minimum et le maximum couvrent sans cas particulier.
    for i in 0..3 {
        let low = min(a.axis(i), b.axis(i)) - half.axis(i);
        let high = max(a.axis(i), b.axis(i)) + half.axis(i);
        let mut normal = Vec3d::ZERO;
        set_axis(&mut normal, i, 1.0);
        slab.cut(normal, high, from, to)?;
        let mut normal = Vec3d::ZERO;
        set_axis(&mut normal, i, -1.0);
        slab.cut(normal, -low, from, to)?;
    }

    // Les biseaux : pour chaque axe, la normale `arête × axe` et son opposée.
    // Ce sont eux qui donnent au prisme sa forme d'enveloppe plutôt que celle
    // d'une boîte, et sans eux une arête oblique arrêterait bien trop tôt.
    for i in 0..3 {
        let mut axis = Vec3d::ZERO;
        set_axis(&mut axis, i, 1.0);
        let normal = direction.cross(axis);
        if normal == Vec3d::ZERO {
            continue;
        }
        let s = support(normal, half);
        slab.cut(normal, normal.dot(a) + s, from, to)?;
        slab.cut(-normal, -normal.dot(a) + s, from, to)?;
    }

    let (t, normal) = slab.immediate(from, to)?;
    Some(Touch {
        fraction: t,
        normal,
        rank: RANK_EDGE,
    })
}

/// Le contact du segment avec la **boîte** d'un sommet.
pub(crate) fn vertex(point: Vec3d, half: Vec3d, from: Vec3d, to: Vec3d) -> Option<Touch> {
    let mut slab = Interval::FULL;
    for i in 0..3 {
        let mut normal = Vec3d::ZERO;
        set_axis(&mut normal, i, 1.0);
        slab.cut(normal, point.axis(i) + half.axis(i), from, to)?;
        let mut normal = Vec3d::ZERO;
        set_axis(&mut normal, i, -1.0);
        slab.cut(normal, -point.axis(i) + half.axis(i), from, to)?;
    }
    let (t, normal) = slab.immediate(from, to)?;
    Some(Touch {
        fraction: t,
        normal,
        rank: RANK_VERTEX,
    })
}

/// L'intervalle du segment resté dans un convexe au fil de ses demi-espaces.
///
/// `enter` reste `None` tant qu'aucun plan n'a été franchi en entrant : un
/// segment qui part déjà dans le convexe et n'en sort pas n'a pas d'instant
/// d'entrée, et c'est au balayage de le traiter comme un départ dans le solide.
struct Interval {
    /// Le plus tard des instants d'entrée, s'il y en a un.
    enter: Option<f64>,
    /// Le plus tôt des instants de sortie.
    leave: f64,
    /// La normale du plan qui a posé `enter`.
    normal: Vec3d,
    /// La normale du plan dont la sortie est la plus proche au départ.
    ///
    /// Elle ne sert qu'au cas où le segment part **dans** le convexe : c'est
    /// alors la direction qui demande le moins de recul, le même choix que la
    /// détection de départ dans le solide fait sur un jeu de surfaces.
    shallow: Vec3d,
    /// Le carré de la profondeur sous ce plan, et le carré de la norme de sa
    /// normale.
    ///
    /// Deux nombres plutôt qu'un quotient : la distance vaut `profondeur / |n|`,
    /// et les comparer par produit croisé évite une racine. Les biseaux d'un
    /// prisme n'ont pas de normale unitaire, si bien que les profondeurs brutes
    /// ne sont pas comparables entre elles.
    shallow_depth: f64,
    /// Voir [`Interval::shallow_depth`].
    shallow_norm: f64,
}

impl Interval {
    /// L'intervalle entier, avant toute découpe.
    ///
    /// `shallow_norm` nul fait gagner le premier plan rencontré, quelle que soit
    /// sa profondeur : c'est la forme d'un « plus rien de comparable » qui ne
    /// demande pas d'infini.
    const FULL: Self = Self {
        enter: None,
        leave: 1.0,
        normal: Vec3d::ZERO,
        shallow: Vec3d::ZERO,
        shallow_depth: 1.0,
        shallow_norm: 0.0,
    };

    /// Découpe par le demi-espace `normal · p ≤ offset`.
    ///
    /// Rend `None` quand l'intervalle devient vide, ce qui termine le test : le
    /// segment ne rencontre pas ce convexe.
    fn cut(&mut self, normal: Vec3d, offset: f64, from: Vec3d, to: Vec3d) -> Option<()> {
        let d0 = normal.dot(from) - offset;
        let d1 = normal.dot(to) - offset;

        // Le départ est-il déjà du bon côté de ce plan, et de combien ? C'est ce
        // qui répondra si aucun plan n'est franchi en entrant — le segment part
        // alors dans le convexe, et le contact est immédiat plutôt qu'absent.
        if d0 <= 0.0 {
            let depth = d0 * d0;
            let norm = normal.dot(normal);
            if depth * self.shallow_norm < self.shallow_depth * norm {
                self.shallow_depth = depth;
                self.shallow_norm = norm;
                self.shallow = normal;
            }
        }

        if d0 == d1 {
            // Parallèle au plan : dedans pour toujours, ou dehors pour toujours.
            return if d0 <= 0.0 { Some(()) } else { None };
        }
        let t = d0 / (d0 - d1);
        if d1 < d0 {
            // Le segment entre par ce plan — **pendant le mouvement, et pas
            // avant**. Un instant négatif dit que le segment était déjà du bon
            // côté au départ : ce n'est pas une entrée, et le retenir ferait
            // remonter un contact à une fraction antérieure à l'origine, que
            // l'hôte n'a aucune raison de tester puisque le contrat annonce
            // `[0, 1]`. Le cas se produit dès qu'une boîte part entre deux plans
            // d'un prisme d'arête, ce qui n'a rien d'exceptionnel.
            if t >= 0.0 && self.enter.is_none_or(|entered| t > entered) {
                self.enter = Some(t);
                self.normal = normal;
            }
        } else if t < self.leave {
            self.leave = t;
        }
        let entered = self.enter.unwrap_or(0.0);
        if entered > self.leave || entered > 1.0 {
            return None;
        }
        Some(())
    }

    /// L'instant du contact et sa normale, une fois toutes les découpes faites.
    ///
    /// **Sans instant d'entrée, le segment part dans le convexe**, et le contact
    /// est alors immédiat : c'est ce que la boîte dilatée décrit d'une position
    /// qu'un balayage précédent a posée au contact, et l'écarter laissait une
    /// bande où le mobile entrait librement dans le décor.
    ///
    /// **Mais seulement s'il s'y enfonce**, au sens strict. Un mobile qui
    /// ressort n'est pas arrêté : le bloquer là le collerait au décor sans rien
    /// pour l'en tirer, ce que la dilatation existe précisément pour éviter — et
    /// un hôte n'a aucun moyen de distinguer ce blocage-là d'un mur. La
    /// comparaison stricte écarte du même geste les deux cas où il n'y a rien à
    /// signaler : un déplacement nul, et un déplacement tangent, qui glisse le
    /// long sans entrer.
    fn immediate(&self, from: Vec3d, to: Vec3d) -> Option<(f64, Vec3d)> {
        match self.enter {
            Some(t) => Some((t, self.normal)),
            None if (to - from).dot(self.shallow) < 0.0 => Some((0.0, self.shallow)),
            None => None,
        }
    }
}

/// Le point est-il dans le polygone, dans le plan de celui-ci ?
///
/// Parité de traversées sur le plan de projection le moins incliné, comme la
/// cuisson. La règle de bord n'a pas besoin d'être écrite ici : un point qui
/// tombe exactement sur une arête est de toute façon revendiqué par le prisme de
/// cette arête ou par la face voisine, et la clause (c) de la règle de l'arête
/// partagée tranche le reste.
fn inside(points: &[Vec3d], normal: Vec3d, point: Vec3d) -> bool {
    let (i0, i1) = plane_axes(normal);
    let n = points.len();
    let (hu, hv) = (point.axis(i0), point.axis(i1));

    let mut inside = false;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        let (au, av) = (a.axis(i0), a.axis(i1));
        let (bu, bv) = (b.axis(i0), b.axis(i1));
        if (av > hv) == (bv > hv) {
            continue;
        }
        let dv = bv - av;
        let left = (hu - au) * dv;
        let right = (hv - av) * (bu - au);
        if (dv > 0.0 && left < right) || (dv < 0.0 && left > right) {
            inside = !inside;
        }
    }
    inside
}

/// Les deux axes sur lesquels projeter pour que le polygone ne dégénère pas.
///
/// La paire n'est pas circulaire, et c'est sans conséquence : une parité de
/// traversées est invariante par échange des deux axes. C'est le même choix que
/// `world::bake::plane_axes`, et pour la même raison.
fn plane_axes(normal: Vec3d) -> (usize, usize) {
    let (x, y, z) = (abs(normal.x), abs(normal.y), abs(normal.z));
    if x >= y && x >= z {
        (1, 2)
    } else if y >= z {
        (0, 2)
    } else {
        (0, 1)
    }
}

/// Le plus petit de deux nombres, par comparaison écrite.
fn min(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

/// Le plus grand de deux nombres, par comparaison écrite.
fn max(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

/// Pose une composante d'un vecteur, par son rang.
fn set_axis(v: &mut Vec3d, index: usize, value: f64) {
    match index {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
}

#[cfg(test)]
mod tests;
