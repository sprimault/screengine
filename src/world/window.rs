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
use crate::math::projection::{ClipVertex, NEAR_PLANE};
use crate::math::{Affine3, Projection, Vec3};
use crate::raster::{Rect, clip};

/// Le nombre maximum de sommets qu'un triangle découpé par la fenêtre peut
/// porter.
///
/// Trois sommets et **huit** bords : chaque bord ajoute au plus une arête, donc
/// au plus un sommet. La borne est prouvée et non majorée, comme celle du
/// découpage homogène — un tampon dimensionné au jugé serait soit du gaspillage,
/// soit un dépassement qu'aucun test ne rejoue.
///
/// Elle valait sept quand la fenêtre était un rectangle. Les quatre bords
/// obliques en ajoutent quatre, et c'est le seul coût en mémoire de l'octogone :
/// quatre couples de plus sur la pile, par triangle de l'éventail.
const MAX_WINDOW_VERTICES: usize = 11;

/// Les quatre directions sur lesquelles une fenêtre se borne.
///
/// **Axes et diagonales, et rien d'autre.** Les normales sont figées, donc
/// l'intersection de deux fenêtres de cette famille en est une — huit minimums
/// et maximums entiers, sans division —, et leur nombre reste **fermé à huit
/// quelle que soit la profondeur de la chaîne** de portails. C'est ce qu'aucune
/// autre forme ne donne : un polygone à normales libres gagnerait un côté par
/// portail franchi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    /// `x`, le bord vertical.
    X,
    /// `y`, le bord horizontal.
    Y,
    /// `x + y`, la diagonale qui descend vers la droite.
    Sum,
    /// `x − y`, la diagonale qui monte vers la droite.
    Diff,
}

impl Axis {
    /// Les quatre, dans l'ordre où les bords se découpent.
    const ALL: [Self; 4] = [Self::X, Self::Y, Self::Sum, Self::Diff];

    /// La coordonnée d'un point sur cet axe.
    ///
    /// En `i64` : une somme de deux coordonnées 28.4 sort du `i32` sur les
    /// valeurs extrêmes de la bande de garde, et c'est précisément là qu'un
    /// débordement serait silencieux.
    fn of(self, x: i32, y: i32) -> i64 {
        let (x, y) = (i64::from(x), i64::from(y));
        match self {
            Self::X => x,
            Self::Y => y,
            Self::Sum => x + y,
            Self::Diff => x - y,
        }
    }
}

/// Une fenêtre de propagation : un octogone en sous-pixels, bornes inclusives.
///
/// **La boîte axiale plus la même tournée de quarante-cinq degrés.** Un
/// rectangle suffit à borner un parcours, mais pas à propager : il a un mauvais
/// cas, une ouverture allongée vue avec du roulis, où il laisse passer jusqu'à
/// cinq fois ce que l'ouverture montre — et ce surplus est le liseré où naissent
/// les portails faussement visibles, donc les cellules ramenées pour rien.
///
/// Inclusives parce que c'est ainsi que le remplissage compte ses colonnes et
/// ses lignes, et qu'une fenêtre vide se reconnaît alors à `min > max` sans
/// qu'aucune soustraction ne déborde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Window {
    /// Les minimums, dans l'ordre d'[`Axis::ALL`].
    min: [i64; 4],
    /// Les maximums, dans le même ordre.
    max: [i64; 4],
}

impl Window {
    /// La fenêtre vide, prête à accueillir un premier point.
    ///
    /// Volontairement croisée : tout point la corrige, et aucune borne n'est une
    /// valeur plausible qu'on oublierait de remplacer.
    pub(crate) const EMPTY: Self = Self {
        min: [i64::MAX; 4],
        max: [i64::MIN; 4],
    };

    /// Vrai si aucun point n'y est entré.
    pub(crate) fn is_empty(self) -> bool {
        let mut axis = 0;
        while axis < 4 {
            if self.min[axis] > self.max[axis] {
                return true;
            }
            axis += 1;
        }
        false
    }

    /// La fenêtre d'un rectangle de pixels, ramenée en sous-pixels.
    ///
    /// Le pixel `x` occupe les sous-pixels `[x·16, x·16 + 15]` : la borne haute
    /// prend donc le dernier sous-pixel du dernier pixel, et non le premier du
    /// suivant.
    ///
    /// **Les bornes obliques se déduisent des quatre coins**, et c'est ce qui
    /// fait d'un rectangle un octogone de cette famille sans rien élargir : la
    /// diagonale d'un rectangle est extrémale sur un coin, jamais sur un bord.
    pub(crate) fn of(rect: Rect) -> Self {
        let scale = i64::from(SUBPIXEL_SCALE);
        let min_x = i64::from(rect.x) * scale;
        let max_x = i64::from(rect.x + rect.width) * scale - 1;
        let min_y = i64::from(rect.y) * scale;
        let max_y = i64::from(rect.y + rect.height) * scale - 1;
        Self {
            min: [min_x, min_y, min_x + min_y, min_x - max_y],
            max: [max_x, max_y, max_x + max_y, max_x - min_y],
        }
    }

    /// La même fenêtre, bornes obliques ouvertes : un rectangle.
    ///
    /// **Réservée aux tests, et c'est sa seule raison d'être** : elle redonne
    /// exactement le comportement d'avant l'octogone, donc elle permet de
    /// mesurer ce que les quatre bords obliques retirent, sur le même code et le
    /// même décor. Sans elle, la comparaison demanderait de reconstruire
    /// l'ancienne version.
    /// Les bornes obliques y prennent une valeur **largement au-delà de la bande
    /// de garde, et non l'infini d'un `i64`** : une borne extrême déborderait à
    /// la dilatation, et ferait rougir le code de production pour une valeur
    /// qu'aucun décor ne produit.
    #[cfg(test)]
    pub(crate) fn axial_only(self) -> Self {
        const OPEN: i64 = 1 << 40;
        Self {
            min: [self.min[0], self.min[1], -OPEN, -OPEN],
            max: [self.max[0], self.max[1], OPEN, OPEN],
        }
    }

    /// Étend la fenêtre jusqu'à contenir le point, sur les quatre axes.
    fn add(&mut self, x: i32, y: i32) {
        // Par comparaisons écrites, comme partout dans ce projet : `min` et
        // `max` de la bibliothèque ne traitent pas NaN et −0 comme les chemins
        // SIMD, et la règle vaut même là où aucun flottant n'entre.
        for (index, axis) in Axis::ALL.into_iter().enumerate() {
            let value = axis.of(x, y);
            if value < self.min[index] {
                self.min[index] = value;
            }
            if value > self.max[index] {
                self.max[index] = value;
            }
        }
    }

    /// L'intersection de deux fenêtres, qui en est une.
    ///
    /// **C'est la propriété qui ferme la famille à huit côtés** : les normales
    /// étant figées, le plus grand des minimums et le plus petit des maximums
    /// suffisent, sans division ni tri de plans. Un polygone à normales libres
    /// gagnerait ici un côté par portail franchi, et la pile de traversée
    /// n'aurait plus de taille bornée.
    pub(crate) fn intersect(self, other: Self) -> Self {
        let mut out = self;
        for axis in 0..4 {
            if other.min[axis] > out.min[axis] {
                out.min[axis] = other.min[axis];
            }
            if other.max[axis] < out.max[axis] {
                out.max[axis] = other.max[axis];
            }
        }
        out
    }

    /// La fenêtre élargie de ce qu'il faut pour absorber les arrondis.
    ///
    /// **Deux unités sur les diagonales, une sur les axes.** Un sous-pixel
    /// d'écart géométrique vaut un sur un axe, mais √2 sur une diagonale, dont
    /// la forme `x ± y` n'est pas normalisée : arrondir à deux est la plus
    /// petite valeur entière qui majore, et l'élargissement d'une fenêtre ne
    /// coûte que du sur-dessin là où l'étrécir troue.
    fn dilated(self) -> Self {
        if self.is_empty() {
            return self;
        }
        let mut out = self;
        for axis in 0..4 {
            let margin = if axis < 2 { 1 } else { 2 };
            out.min[axis] -= margin;
            out.max[axis] += margin;
        }
        out
    }

    /// L'aire de l'octogone, en sous-pixels carrés.
    ///
    /// **Réservée aux tests**, qui sont les seuls à comparer deux fenêtres par
    /// leur taille : le moteur ne décide jamais rien d'une aire, il découpe et
    /// regarde si le résultat est vide.
    ///
    /// La boîte moins ses quatre coins coupés. Chaque coin est un triangle
    /// rectangle isocèle dont la jambe est l'écart entre la borne oblique et le
    /// coin de la boîte ; une jambe négative veut dire que la diagonale ne mord
    /// pas ce coin, et vaut alors zéro.
    #[cfg(test)]
    fn area(self) -> i64 {
        if self.is_empty() {
            return 0;
        }
        let (min_x, max_x) = (self.min[0], self.max[0]);
        let (min_y, max_y) = (self.min[1], self.max[1]);
        let box_area = (max_x - min_x + 1) * (max_y - min_y + 1);
        // Les quatre coins, chacun avec la diagonale qui le coupe.
        let legs = [
            self.min[2] - (min_x + min_y),
            (max_x + max_y) - self.max[2],
            self.min[3] - (min_x - max_y),
            (max_x - min_y) - self.max[3],
        ];
        let mut cut = 0;
        for leg in legs {
            if leg > 0 {
                cut += leg * leg / 2;
            }
        }
        box_area - cut
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
    /// **Seules les bornes axiales y entrent**, et c'est ce qui laisse le
    /// remplissage et les empreintes hors de ce changement : un parcours de
    /// pixels se borne par un rectangle, et les coins que l'octogone coupe en
    /// plus ne feraient qu'économiser des tests de profondeur déjà bornés par la
    /// boîte du triangle. Ce que l'octogone gagne est en amont, dans les
    /// cellules qu'il ne ramène pas.
    pub(crate) fn to_rect(self) -> Rect {
        if self.is_empty() {
            return Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            };
        }
        let scale = i64::from(SUBPIXEL_SCALE);
        // **Sans dilatation ici**, et c'est un changement : elle y vivait quand
        // cette conversion était la sortie de `reduce`, qui dilate désormais sa
        // fenêtre elle-même — sur les quatre axes, dont les obliques que celle-ci
        // ne voit pas. L'appliquer deux fois rendait 641 pixels pour une image de
        // 640, et faisait déborder une fenêtre convertie puis reconvertie.
        //
        // `div_euclid` et non la division de Rust, qui tronque vers zéro : un
        // sous-pixel négatif tomberait alors du mauvais côté et la fenêtre
        // mordrait dans l'image.
        let x0 = self.min[0].div_euclid(scale);
        let x1 = self.max[0].div_euclid(scale);
        let y0 = self.min[1].div_euclid(scale);
        let y1 = self.max[1].div_euclid(scale);

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
        // Les bornes viennent de la bande de garde, dont les valeurs tiennent
        // dans un `u32` après division par l'échelle sous-pixel : la conversion
        // ne tronque rien qu'un décor puisse produire.
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
    window: Window,
    points: &[Vec3],
    view: Affine3,
    projection: &Projection,
) -> Window {
    if points.len() < 3 || window.is_empty() {
        return Window::EMPTY;
    }

    let limits = window;
    let mut bounds = Window::EMPTY;
    // **Deux drapeaux sur le portail entier, et non par sommet ni par triangle** :
    // la clause de fin les croise, et deux triangles de l'éventail peuvent porter
    // chacun une moitié de la condition sans qu'aucun ne porte les deux.
    let mut ahead = false;
    let mut near_side = false;

    for i in 1..points.len() - 1 {
        let corners = [points[0], points[i], points[i + 1]];
        let mut homogeneous = [ClipVertex::ZERO; 3];
        let mut projected = true;
        for (slot, corner) in homogeneous.iter_mut().zip(corners) {
            // Les attributs ne servent pas : une fenêtre ne porte ni texture ni
            // lumière, et seuls `x`, `y` et `w` décident de sa géométrie.
            match projection.to_clip(view.transform_point(corner), 0.0, 0.0, 0.0, 0.0, [0.0; 3]) {
                Some(vertex) => *slot = vertex,
                // Un sommet hors des limites de coordonnées abandonne son
                // triangle, ce qui **rétrécit** la fenêtre et n'est donc pas
                // conservateur : `bounds` part de vide et accumule l'union des
                // morceaux, si bien qu'un morceau perdu est un morceau de moins.
                // Rien ne l'atteint — `to_clip` ne refuse que sur une coordonnée
                // de clip non finie ou démesurée, qu'une carte chargée ne porte
                // pas, ses coordonnées étant vérifiées finies. Le jour où quelque
                // chose l'atteindrait, la réponse sûre est celle du plan proche
                // plus bas : rendre la fenêtre reçue.
                None => {
                    projected = false;
                    break;
                }
            }
        }
        if !projected {
            continue;
        }

        for vertex in &homogeneous {
            ahead |= vertex.w > 0.0;
            near_side |= projection.frustum().distance(*vertex, NEAR_PLANE) < 0.0;
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

    // **Un portail qu'on est en train de franchir ne réduit rien**, et c'est la
    // clause que ce calcul n'avait pas. Ce qui est en deçà du plan proche est en
    // deçà du plan de projection : sa boîte écran n'existe pas, et ce qu'il cache
    // occupe l'image sans borne. Le découpage, lui, l'emporte — en entier si le
    // portail est tout près, par un bord s'il est vu de biais —, et la boîte des
    // morceaux restants est alors **trop étroite**. La cellule d'en face est perdue
    // ou amputée, ce qui se voit comme une bande verticale vide pendant une image
    // ou deux, à chaque embrasure traversée.
    //
    // La géométrie dit l'inverse de ce que le découpage donne : plus l'œil est
    // près du plan d'un portail, **plus large** est ce qu'on voit à travers,
    // jusqu'à l'écran entier au moment de le passer. La réponse sûre est donc la
    // fenêtre reçue, inchangée.
    //
    // **Les deux drapeaux se croisent, et aucun ne suffit seul.** `near_side` dit
    // qu'un sommet est en deçà du plan proche, ce qui est vrai aussi d'un portail
    // entièrement derrière l'œil — celui-là doit bien continuer de vider la
    // fenêtre, on l'a passé, on ne le franchit pas. `ahead` dit qu'un sommet est
    // devant l'œil. Ensemble ils retiennent les trois cas qui comptent : le
    // portail tout entier entre l'œil et le plan proche, celui que le plan coupe,
    // et celui dont l'œil est presque dans le plan — un bord devant, l'autre
    // derrière.
    //
    // **Ce dernier est celui qu'une première écriture a manqué**, en exigeant d'un
    // même sommet qu'il soit devant l'œil *et* en deçà du plan : le bord derrière
    // l'œil a une profondeur négative, donc la clause ne voyait rien et le
    // découpage amputait la fenêtre comme avant. Mesuré chez un intégrateur,
    // inchangé au pixel près sur cinq écarts au plan.
    //
    // La monotonie de la réduction, dont la traversée a besoin pour terminer, est
    // préservée : on rend la fenêtre reçue, jamais plus large.
    if ahead && near_side {
        return window;
    }

    // La dilatation peut pousser un bord juste au-delà de la fenêtre reçue :
    // l'intersection la ramène. C'est aussi ce qui rend la réduction monotone,
    // propriété dont la traversée a besoin pour terminer.
    bounds.dilated().intersect(window)
}

/// Accumule dans `bounds` la boîte de l'intersection d'un triangle avec
/// `limits`.
///
/// Découpage de Sutherland-Hodgman contre les quatre bords, en sous-pixels : les
/// écarts entre sommets atteignent 2¹⁷ et les produits 2³⁴, d'où les `i64` —
/// mêmes bornes que les fonctions de bord, et la même marge.
fn accumulate(triangle: &[(i32, i32); 3], limits: Window, bounds: &mut Window) {
    let mut current = [(0i32, 0i32); MAX_WINDOW_VERTICES];
    let mut next = [(0i32, 0i32); MAX_WINDOW_VERTICES];
    let mut len = 3;
    current[..3].copy_from_slice(triangle);

    // **Huit demi-plans au lieu de quatre**, et la boucle ne distingue plus les
    // obliques des axiales : un bord est une forme linéaire et une borne, donc
    // le côté se lit par une soustraction et le croisement par une division,
    // comme avant. C'est ce qui rend l'octogone gratuit en structure — seul le
    // nombre de tours change.
    for (index, axis) in Axis::ALL.into_iter().enumerate() {
        for (bound, keep_greater) in [(limits.min[index], true), (limits.max[index], false)] {
            let inside = |p: (i32, i32)| {
                let value = axis.of(p.0, p.1);
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
                    next[count] = cross(a, b, bound, axis);
                    count += 1;
                }
            }

            len = count;
            if len == 0 {
                return;
            }
            current[..len].copy_from_slice(&next[..len]);
        }
    }

    for point in &current[..len] {
        bounds.add(point.0, point.1);
    }
}

/// Le croisement du segment `a → b` avec la droite `axis(p) = bound`.
///
/// **Les deux coordonnées s'interpolent, et non une seule.** Sur un bord axial,
/// l'une des deux est la borne elle-même, et la forme précédente en profitait ;
/// sur une diagonale, aucune ne l'est. Interpoler les deux depuis le paramètre
/// du segment couvre les quatre axes d'une seule écriture, et rend exactement la
/// même valeur sur un bord axial — où le paramètre vaut ce que valait la
/// division d'alors.
///
/// La division tronque vers zéro, et ce n'est pas corrigé ici : la fenêtre
/// obtenue est élargie d'un sous-pixel à la conversion en pixels, ce qui absorbe
/// l'arrondi dans le sens conservateur quel qu'il soit. Corriger chaque
/// croisement selon son bord coûterait huit cas pour un résultat que la fenêtre
/// n'utilise pas plus finement.
fn cross(a: (i32, i32), b: (i32, i32), bound: i64, axis: Axis) -> (i32, i32) {
    let from = axis.of(a.0, a.1);
    let to = axis.of(b.0, b.1);
    let span = to - from;
    // Un segment parallèle au bord n'a pas de croisement à donner ; il ne peut
    // pas en avoir été demandé, ses deux extrémités étant du même côté.
    if span == 0 {
        return a;
    }
    let numerator = bound - from;
    let dx = i64::from(b.0) - i64::from(a.0);
    let dy = i64::from(b.1) - i64::from(a.1);
    // Les deux coordonnées restent dans la bande de garde, dont les bornes sont
    // celles du format 28.4 : la conversion ne peut pas saturer.
    let x = i64::from(a.0) + numerator * dx / span;
    let y = i64::from(a.1) + numerator * dy / span;
    (x as i32, y as i32)
}

#[cfg(test)]
mod tests;
