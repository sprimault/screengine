// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage d'un triangle par fonctions de bord.
//!
//! Deux triangles qui partagent une arête doivent se partager ses pixels sans
//! trou ni recouvrement. C'est ce que la règle top-left obtient, et c'est le
//! défaut le plus coûteux du projet : invisible à l'arrêt, visible en mouvement
//! comme un scintillement de la couture, et découvert trop tard il est déjà sous
//! tout le reste du moteur.

use crate::light;
use crate::math::fixed::{PIXEL_CENTER, SUBPIXEL_SCALE, UV_BITS};
use crate::math::projection::ProjectedVertex;

use super::plane::{GRADIENT_BITS, Plane};
use super::{Rect, Target};
use crate::texture::{Filter, MAX_TEXTURE_SIZE, Texture};

/// Une texture et la façon de la lire.
///
/// Les deux voyagent ensemble plutôt qu'en deux paramètres : le filtre ne sert
/// que là où il y a une texture, et le remplissage en porte déjà six.
#[derive(Debug, Clone, Copy)]
pub struct Sampling<'a> {
    /// La texture du triangle.
    pub texture: &'a Texture,
    /// Le mode d'échantillonnage du contexte.
    pub filter: Filter,
}

/// L'éclairage d'un triangle : sa lightmap, ses plans, et le réglage du
/// contexte.
///
/// **Pas de mode de filtrage ici** : une lightmap se lit toujours en
/// bilinéaire. Elle est presque toujours agrandie — un de ses texels couvre
/// une poignée de pixels —, et le plus proche voisin y dessinerait des blocs
/// que le tramage ne masque pas : celui-ci ne déplace la coordonnée que d'un
/// demi-texel, ce qui ne fait rien contre une marche de plusieurs pixels de
/// large.
#[derive(Debug, Clone, Copy)]
pub struct Lit<'a> {
    /// La lightmap du triangle, **absente quand seules des lumières
    /// dynamiques l'éclairent**.
    ///
    /// Les deux sources sont indépendantes : un décor n'aura de lightmaps
    /// qu'à l'étape 5, et une torche doit pouvoir éclairer un mur uni d'ici
    /// là.
    pub lightmap: Option<&'a Texture>,
    /// Les plans de ses coordonnées, pris dans le tableau annexe de l'image.
    pub planes: &'a Lighting,
    /// Le décalage de sur-éclairement du contexte.
    pub overbright: u32,
}

/// Une position projetée, en sous-pixels.
///
/// Deux dimensions seulement : c'est sur elle que portent les fonctions de bord
/// et la règle top-left, et leurs tests ne dépendent ainsi de rien de ce que
/// [`Vertex`] porte en plus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    /// Abscisse en sous-pixels, dans la bande de garde.
    pub x: i32,
    /// Ordonnée en sous-pixels, Y vers le bas.
    pub y: i32,
}

/// Un sommet projeté : sa position et ses attributs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vertex {
    /// La position, en sous-pixels.
    pub position: Point,
    /// La profondeur, `near/w` en 0.32, plus grand est plus proche, bornée par
    /// `to_depth`.
    pub z: u32,
    /// L'abscisse de texture multipliée par la profondeur, en 14.12.
    pub s: i32,
    /// L'ordonnée de texture multipliée par la profondeur, en 14.12.
    pub t: i32,
    /// L'abscisse de lightmap multipliée par la profondeur, en 14.12.
    ///
    /// Nulle sur un lot qui n'en porte pas : les plans correspondants ne sont
    /// alors pas construits, et rien ne la lit.
    pub s2: i32,
    /// L'ordonnée de lightmap multipliée par la profondeur, en 14.12.
    pub t2: i32,
    /// Ce que les lumières dynamiques ajoutent ici, par canal, en 16.16.
    ///
    /// **Non multiplié par la profondeur**, contrairement aux coordonnées :
    /// une couleur s'interpole affinement en espace écran, comme la
    /// profondeur, et c'est ce que faisaient les moteurs de cette famille. La
    /// correction de perspective coûterait une seconde marche par segment pour
    /// un écart que personne ne voit sur un dégradé qui varie lentement — et
    /// une lumière ponctuelle en produit un par construction.
    pub light: [i32; 3],
}

/// Un triangle prêt à être parcouru dans n'importe quelle fenêtre.
///
/// Ce qui ne dépend pas de la fenêtre se calcule une fois, à la soumission ;
/// ce qui en dépend — le point de départ du parcours — se recalcule par la
/// forme close à chaque fenêtre. C'est ce partage qui rend l'image
/// indépendante du découpage : une tuile ne reprend jamais une valeur
/// accumulée par sa voisine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prepared {
    v: [Point; 3],
    /// Pixels extrêmes dont le centre peut être couvert, bornes comprises, sans
    /// limitation par l'image.
    x0: i32,
    x1: i32,
    y0: i32,
    y1: i32,
    /// Le point de référence des trois plans, en sous-pixels.
    ///
    /// Commun, et c'est structurel : les plans d'un même triangle s'évaluent
    /// aux mêmes écarts, qui ne se calculent donc qu'une fois par pixel.
    ref_x: i32,
    ref_y: i32,
    depth: Plane,
    /// Les coordonnées de texture multipliées par la profondeur, `u·d` puis
    /// `v·d`, qui sont affines en espace écran là où `u` et `v` ne le sont pas.
    uv: [Plane; 2],
    color: u32,
    /// L'index de la texture dans la table du contexte, ou [`NO_TEXTURE`].
    texture: u16,
    /// La place de ses plans d'éclairage dans le tableau annexe de l'image, ou
    /// [`NO_LIGHTING`].
    lighting: u16,
}

/// Cent vingt-huit octets, deux lignes de cache pleines. La répartition par
/// tuile parcourt ce tableau deux fois par image : sa taille compte.
const _: () = assert!(size_of::<Prepared>() == 128);

/// L'index que porte un triangle sans texture.
///
/// Une sentinelle plutôt qu'un `Option<u16>` : celui-ci ferait quatre octets
/// là où deux suffisent, et le test se fait une fois par triangle, hors de la
/// boucle de pixels.
pub const NO_TEXTURE: u16 = u16::MAX;

/// La place que porte un triangle sans éclairage.
pub const NO_LIGHTING: u16 = u16::MAX;

/// Ce qu'un triangle éclairé porte en plus, rangé à part.
///
/// **Ce n'est pas un choix d'esthétique mais de taille** : [`Prepared`] tient
/// en deux lignes de cache, que la répartition par tuile parcourt deux fois
/// par image. Deux plans de plus l'étendraient à trois lignes pour tous les
/// triangles, éclairés ou non, alors qu'un index de deux octets suffit — et
/// ces deux octets étaient déjà là, en bourrage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lighting {
    /// Les coordonnées de lightmap multipliées par la profondeur, `s·d` puis
    /// `t·d`, affines en espace écran par la même raison que celles de texture.
    st: [Plane; 2],
    /// L'index de la lightmap dans la table de l'image.
    ///
    /// Ici plutôt que dans [`Prepared`] : un triangle éclairé porte déjà la
    /// place de ses plans, et un second index l'aurait fait déborder de ses
    /// deux lignes de cache. Celui-ci voyage avec ce qu'il désigne.
    ///
    /// Vaut [`NO_TEXTURE`] quand seules des lumières dynamiques éclairent le
    /// triangle : les deux sources sont indépendantes, et les lightmaps ne
    /// viendront des cartes qu'à l'étape 5.
    lightmap: u16,
    /// Ce que les lumières dynamiques ajoutent, un plan par canal, en 16.16.
    ///
    /// **Interpolés affinement, sans division** : voir [`Vertex::light`].
    dynamic: [Plane; 3],
    /// Faux quand aucune lumière n'éclaire ce triangle.
    ///
    /// Les plans sont alors nuls et leur évaluation rendrait zéro, mais la
    /// faire coûterait trois évaluations par pixel pour un résultat connu
    /// d'avance : ce drapeau choisit un chemin de remplissage, il ne se teste
    /// pas dans la boucle.
    lit: bool,
}

impl Lighting {
    /// Les plans d'éclairage du triangle, sur sa base.
    ///
    /// `lit` dit si les lumières dynamiques ont éclairé au moins un de ses
    /// sommets. Faux, les trois plans restent nuls et le remplissage prendra
    /// un chemin qui ne les lit pas.
    fn new(basis: &Basis, vertices: &[Vertex; 3], lightmap: u16, lit: bool) -> Self {
        let plane = |value: fn(&Vertex) -> i32| {
            Plane::new(
                basis.v,
                [
                    i64::from(value(&vertices[0])),
                    i64::from(value(&vertices[1])),
                    i64::from(value(&vertices[2])),
                ],
                basis.area,
                basis.reference,
            )
        };
        let channel = |index: usize| {
            Plane::new(
                basis.v,
                [
                    i64::from(vertices[0].light[index]),
                    i64::from(vertices[1].light[index]),
                    i64::from(vertices[2].light[index]),
                ],
                basis.area,
                basis.reference,
            )
        };
        Self {
            st: [plane(|v| v.s2), plane(|v| v.t2)],
            lightmap,
            dynamic: [channel(0), channel(1), channel(2)],
            lit,
        }
    }

    /// Ses deux plans de lightmap, dans l'ordre `s` puis `t`.
    pub fn planes(&self) -> &[Plane; 2] {
        &self.st
    }

    /// L'index de sa lightmap dans la table de l'image, ou [`NO_TEXTURE`].
    pub fn lightmap(&self) -> u16 {
        self.lightmap
    }

    /// Ses trois plans de lumière dynamique.
    pub fn dynamic(&self) -> &[Plane; 3] {
        &self.dynamic
    }

    /// Vrai si une lumière dynamique éclaire ce triangle.
    pub fn is_lit(&self) -> bool {
        self.lit
    }
}

impl From<ProjectedVertex> for Vertex {
    /// Le sommet du rasteriseur, depuis celui que la projection rend.
    ///
    /// **Un seul endroit reprend les attributs un à un.** Ils étaient recopiés
    /// champ par champ à quatre endroits — la soumission et trois helpers de
    /// test —, si bien que chaque attribut ajouté au sommet les cassait tous
    /// les quatre. Le type de passage existe précisément pour que `math`
    /// ignore le rasteriseur ; la conversion, elle, n'a pas à être écrite
    /// quatre fois.
    fn from(v: ProjectedVertex) -> Self {
        Self {
            position: Point { x: v.x, y: v.y },
            z: v.z,
            s: v.s,
            t: v.t,
            s2: v.s2,
            t2: v.t2,
            light: v.light,
        }
    }
}

#[cfg(test)]
impl Vertex {
    /// Un sommet nu : sa position en sous-pixels, sa profondeur, et aucun
    /// attribut.
    ///
    /// **Réservé aux tests.** Le pipeline construit ses sommets depuis la
    /// projection, qui les remplit tous ; un test, lui, n'a presque jamais
    /// besoin de plus de deux champs. Sans ce constructeur, chaque attribut
    /// ajouté au sommet fait écrire des zéros dans une vingtaine de sites qui
    /// ne s'en servent pas, et ce bruit finit par cacher les rares endroits où
    /// la valeur compte.
    pub(crate) fn plain(x: i32, y: i32, z: u32) -> Self {
        Self {
            position: Point { x, y },
            z,
            s: 0,
            t: 0,
            s2: 0,
            t2: 0,
            light: [0; 3],
        }
    }

    /// Le même, avec ses coordonnées de texture.
    pub(crate) fn uv(self, s: i32, t: i32) -> Self {
        Self { s, t, ..self }
    }

    /// Le même, avec ses coordonnées de lightmap.
    pub(crate) fn uv2(self, s2: i32, t2: i32) -> Self {
        Self { s2, t2, ..self }
    }

    /// Le même, avec l'apport des lumières dynamiques à ce sommet.
    ///
    /// C'est lui, et lui seul, qui décide du `glowing` d'un triangle : un
    /// sommet laissé à zéro partout range des plans nuls, et le remplissage
    /// prend alors le bras sans lumière.
    pub(crate) fn light(self, light: [i32; 3]) -> Self {
        Self { light, ..self }
    }
}

impl Prepared {
    /// Les pixels extrêmes que le triangle peut couvrir : `(x0, y0, x1, y1)`,
    /// bornes comprises.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        (self.x0, self.y0, self.x1, self.y1)
    }

    /// L'index de sa texture dans la table de l'image, ou [`NO_TEXTURE`].
    pub fn texture(&self) -> u16 {
        self.texture
    }

    /// La place de ses plans d'éclairage dans le tableau annexe de l'image, ou
    /// [`NO_LIGHTING`].
    pub fn lighting(&self) -> u16 {
        self.lighting
    }
}

/// Produit vectoriel en deux dimensions, positif du côté intérieur de `a → b`.
///
/// `i64` et non `i32` : une coordonnée tient dans ±2¹⁶ en sous-pixels, un écart
/// dans ±2¹⁷, un produit dans ±2³⁴ et la différence dans ±2³⁵. Un `i32`
/// déborderait dès que la bande de garde sert, c'est-à-dire dès qu'un sommet
/// sort de l'écran.
fn edge(ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32) -> i64 {
    let (ax, ay) = (ax as i64, ay as i64);
    (bx as i64 - ax) * (py as i64 - ay) - (by as i64 - ay) * (px as i64 - ax)
}

/// Vrai si l'arête de vecteur `(dx, dy)` est haute ou gauche.
///
/// En parcours horaire avec Y vers le bas, on va vers les x croissants en haut,
/// on descend à droite et on remonte à gauche : d'où le signe de `dy`, puis
/// celui de `dx` pour départager les arêtes horizontales. Le triangle étant
/// parcouru dans l'autre sens ici, c'est `-d` qu'on lui passe : voir `fill`.
///
/// La propriété qui rend l'arête partagée étanche est que pour un vecteur et son
/// opposé, **exactement un des deux** est haut-ou-gauche — sauf si les deux
/// composantes sont nulles, cas qui n'atteint jamais cette fonction puisqu'un
/// triangle d'aire nulle est écarté avant.
///
/// Ne jamais raccourcir en `dy <= 0` : les deux triangles d'une arête
/// horizontale la revendiqueraient alors tous les deux, et la ligne serait
/// rendue deux fois.
fn is_top_left(dx: i32, dy: i32) -> bool {
    dy < 0 || (dy == 0 && dx > 0)
}

/// Le biais que porte une arête : nul si elle est haute ou gauche, `-1` sinon.
///
/// Ajouté à la constante du setup, il transforme le test `E >= 0` en `E > 0`
/// pour les arêtes qui ne s'appartiennent pas — les deux écritures sont
/// équivalentes puisque `E` est entier. Le biais est retenu parce qu'il
/// disparaît dans la valeur initiale : la boucle garde un test unique sur le
/// signe, transposable en SIMD sans branche par arête.
fn bias(dx: i32, dy: i32) -> i64 {
    if is_top_left(dx, dy) { 0 } else { -1 }
}

/// Les trois fonctions de bord au centre du pixel `(x0, y0)`, et leurs pas
/// d'un pixel vers la droite puis vers le bas.
///
/// Extraite du remplissage parce que le test qui compare le span au balayage
/// naïf doit partir exactement des mêmes valeurs : recopiée, elle finirait par
/// diverger, et les deux se tromperaient ensemble sans que rien ne le dise.
fn setup(triangle: &Prepared, x0: i32, y0: i32) -> ([i64; 3], [i64; 3], [i64; 3]) {
    let v = triangle.v;
    // Les trois arêtes, dans le sens du parcours : un point intérieur est du bon
    // côté des trois à la fois.
    let e = [(0usize, 1usize), (1, 2), (2, 0)];

    let px = x0 * SUBPIXEL_SCALE + PIXEL_CENTER;
    let py = py_of(y0);

    let mut row = [0i64; 3];
    let mut step_x = [0i64; 3];
    let mut step_y = [0i64; 3];

    for (i, &(a, b)) in e.iter().enumerate() {
        let (dx, dy) = (v[b].x - v[a].x, v[b].y - v[a].y);
        // La fonction de bord est niée, et le biais se prend sur `-d` : c'est ce
        // qui classe l'arête sur le sens réellement parcouru. Pris sur `d`, il
        // le serait sur le sens inverse, et les deux triangles d'une arête la
        // revendiqueraient ensemble.
        row[i] = -edge(v[a].x, v[a].y, v[b].x, v[b].y, px, py) + bias(-dx, -dy);
        // Dérivées de la forme close niée, multipliées par le pas d'un pixel.
        // Les additions entières qui suivent sont exactes : parcourir vaut le
        // recalcul complet, bit pour bit, quel que soit le nombre de pas.
        step_x[i] = (dy as i64) * SUBPIXEL_SCALE as i64;
        step_y[i] = -(dx as i64) * SUBPIXEL_SCALE as i64;
    }
    (row, step_x, step_y)
}

/// Vrai si le pixel d'indice `k` depuis le début de la ligne est couvert.
///
/// La forme close, évaluée sans parcourir : les additions entières du parcours
/// donnent les mêmes bits, mais celle-ci se calcule en un point quelconque.
fn covered(row: &[i64; 3], step_x: &[i64; 3], k: i64) -> bool {
    let at = |i: usize| row[i] + step_x[i] * k;
    (at(0) | at(1) | at(2)) >= 0
}

/// Les abscisses extrêmes couvertes sur une ligne, bornes comprises, ou `None`
/// si la ligne ne porte aucun pixel.
///
/// **Exact, et pas par approximation.** Sur une ligne, chaque arête vaut
/// `C + k·S` avec `C` sa valeur à `x0` — biais top-left compris, qui ne fait
/// que translater le demi-plan — et le test est `C + k·S ≥ 0`. Résoudre en
/// entiers donne le plancher exact par `div_euclid`, donc chaque borne est
/// **la** solution, pas une majoration : l'intersection des trois est
/// rigoureusement ce que le test pixel par pixel retiendrait. Un span plus
/// large ferait diviser la perspective là où la profondeur n'a pas de sens ;
/// un span plus étroit trouerait le triangle.
///
/// Une arête horizontale — `S` nul — pose une condition constante sur toute la
/// ligne, et ne demande aucune division.
fn span(row: &[i64; 3], step_x: &[i64; 3], x0: i32, x1: i32) -> Option<(i32, i32)> {
    let (mut lo, mut hi) = (0i64, (x1 - x0) as i64);
    for i in 0..3 {
        let (c, s) = (row[i], step_x[i]);
        if s > 0 {
            // `k ≥ −C/S`, donc le plafond du quotient, qui est l'opposé du
            // plancher de son opposé.
            lo = lo.max(-c.div_euclid(s));
        } else if s < 0 {
            hi = hi.min(c.div_euclid(-s));
        } else if c < 0 {
            return None;
        }
        if lo > hi {
            return None;
        }
    }
    // `lo` et `hi` sont encadrés par `0` et `x1 − x0`, qui tiennent tous deux
    // dans un `i32` : la conversion ne peut pas déborder.
    Some((x0 + lo as i32, x0 + hi as i32))
}

/// Le plus petit pixel entier dont le centre atteint `subpixel`.
///
/// Par décalage arithmétique et non par division : `/ 16` tronque vers zéro et
/// perdrait la colonne d'abscisse `-1`.
fn first_pixel(subpixel: i32) -> i32 {
    (subpixel + (SUBPIXEL_SCALE - PIXEL_CENTER - 1)) >> 4
}

/// Le plus grand pixel entier dont le centre reste sous `subpixel`.
fn last_pixel(subpixel: i32) -> i32 {
    (subpixel - PIXEL_CENTER) >> 4
}

/// Prépare un triangle, sommets donnés en sous-pixels.
///
/// **La face avant est antihoraire dans les données**, donc horaire à l'écran
/// une fois l'axe Y retourné vers le bas : son aire signée est négative. Le
/// moteur la rend en niant les fonctions de bord plutôt qu'en permutant deux
/// sommets — l'ordre reçu reste l'ordre parcouru, et l'appelant n'a rien à
/// réarranger.
///
/// Nier et transposer sont la même règle, pas deux conventions voisines :
/// `edge` est exactement antisymétrique sur les entiers, et `is_top_left(d)` est
/// le complémentaire de `is_top_left(-d)`. Ce qui rend l'arête partagée étanche
/// n'est donc pas modifié, **à une condition qui ne se voit pas ici** : la
/// négation vaut pour tous les triangles, toujours. Deux triangles adjacents
/// dont l'un serait nié et l'autre non auraient des tests identiques au lieu de
/// complémentaires sur leur arête commune, qui serait alors revendiquée deux
/// fois ou pas du tout. C'est pourquoi une surface à deux faces se soumet par
/// son triangle miroir, et jamais en levant le test de signe ci-dessous.
///
/// Rend `None` pour un triangle qui ne peut couvrir aucun centre de pixel.
pub fn prepare(vertices: [Vertex; 3], color: u32, texture: u16) -> Option<Prepared> {
    let basis = Basis::new(&vertices)?;
    assemble(&basis, &vertices, color, texture, NO_LIGHTING)
}

/// Prépare un triangle éclairé, `slot` étant la place que ses plans
/// d'éclairage occuperont dans le tableau annexe de l'image.
///
/// L'appelant range le [`Lighting`] rendu à cette place exacte : c'est lui qui
/// tient le tableau, et le décalage d'un cran ferait lire les coordonnées du
/// voisin sans que rien n'échoue.
pub fn prepare_lit(
    vertices: [Vertex; 3],
    color: u32,
    texture: u16,
    lightmap: u16,
    lit: bool,
    slot: u16,
) -> Option<(Prepared, Lighting)> {
    debug_assert!(slot != NO_LIGHTING);
    let basis = Basis::new(&vertices)?;
    let prepared = assemble(&basis, &vertices, color, texture, slot)?;
    Some((prepared, Lighting::new(&basis, &vertices, lightmap, lit)))
}

/// Ce que tous les plans d'un triangle partagent : ses positions, son aire
/// signée et le sommet d'où partent les évaluations.
///
/// Calculé une fois pour les deux familles de plans. Recalculé de part et
/// d'autre, un sommet de référence choisi différemment ferait diverger
/// l'arrondi des coordonnées de lightmap de celui des coordonnées de texture,
/// sur le même triangle.
struct Basis {
    v: [Point; 3],
    area: i64,
    reference: usize,
}

impl Basis {
    /// Rend `None` pour un triangle vu de dos ou d'aire nulle.
    fn new(vertices: &[Vertex; 3]) -> Option<Self> {
        let v = vertices.map(|vertex| vertex.position);
        let area = edge(v[0].x, v[0].y, v[1].x, v[1].y, v[2].x, v[2].y);
        // Un seul test pour le dos et pour le dégénéré. Obligatoire et non
        // défensif : les équations de plan des attributs diviseront par cette
        // aire, et un triangle plat verrait ses trois fonctions de bord
        // s'annuler le long d'un segment, que les biais pourraient toutes
        // satisfaire.
        if area >= 0 {
            return None;
        }
        // Le plus petit sommet dans l'ordre (y, x), partagé par les plans :
        // pris sur `v[0]`, une permutation circulaire changerait l'arrondi en
        // chaque pixel sans changer le triangle.
        let reference = (0..3).min_by_key(|&i| (v[i].y, v[i].x)).unwrap_or(0);
        Some(Self { v, area, reference })
    }
}

/// Assemble le triangle préparé, bornes comprises.
fn assemble(
    basis: &Basis,
    vertices: &[Vertex; 3],
    color: u32,
    texture: u16,
    lighting: u16,
) -> Option<Prepared> {
    let (v, area, reference) = (basis.v, basis.area, basis.reference);

    let min_x = v[0].x.min(v[1].x).min(v[2].x);
    let max_x = v[0].x.max(v[1].x).max(v[2].x);
    let min_y = v[0].y.min(v[1].y).min(v[2].y);
    let max_y = v[0].y.max(v[1].y).max(v[2].y);

    let prepared = Prepared {
        v,
        x0: first_pixel(min_x),
        x1: last_pixel(max_x),
        y0: first_pixel(min_y),
        y1: last_pixel(max_y),
        ref_x: v[reference].x,
        ref_y: v[reference].y,
        depth: Plane::new(
            v,
            vertices.map(|vertex| i64::from(vertex.z)),
            area,
            reference,
        ),
        uv: [
            Plane::new(
                v,
                vertices.map(|vertex| i64::from(vertex.s)),
                area,
                reference,
            ),
            Plane::new(
                v,
                vertices.map(|vertex| i64::from(vertex.t)),
                area,
                reference,
            ),
        ],
        color,
        texture,
        lighting,
    };
    (prepared.x0 <= prepared.x1 && prepared.y0 <= prepared.y1).then_some(prepared)
}

/// Remplit la partie d'un triangle préparé qui tombe dans `window`.
///
/// `window` est en pixels de l'image, et tient dans la bande de garde.
pub fn fill<T: Target>(
    target: &mut T,
    window: Rect,
    triangle: &Prepared,
    sampling: Option<Sampling<'_>>,
    lit: Option<Lit<'_>>,
) {
    let color = triangle.color;

    // La fenêtre borne la boucle, jamais les valeurs : une fonction de bord
    // évaluée en un pixel ne dépend pas du rectangle dans lequel on la parcourt.
    let wx0 = triangle.x0.max(window.x as i32);
    let wx1 = triangle.x1.min((window.x + window.width) as i32 - 1);
    let y0 = triangle.y0.max(window.y as i32);
    let y1 = triangle.y1.min((window.y + window.height) as i32 - 1);
    if wx0 > wx1 || y0 > y1 {
        return;
    }

    // **Les fonctions de bord partent du span du triangle, pas de celui de la
    // fenêtre.** Le span sert aux segments de perspective, dont les extrémités
    // doivent être les mêmes quelle que soit la tuile : bornés par la fenêtre,
    // ils diviseraient à d'autres abscisses et la même surface se texturerait
    // autrement selon le découpage.
    let (mut row, step_x, step_y) = setup(triangle, triangle.x0, y0);
    let plane = &triangle.depth;
    let depth_x = plane.step_x(SUBPIXEL_SCALE);

    for y in y0..=y1 {
        if let Some((gl, gr)) = span(&row, &step_x, triangle.x0, triangle.x1) {
            // Les deux extrémités sont couvertes, et le pixel qui précède le
            // span ne l'est pas : c'est la coïncidence du span avec le test par
            // pixel, vérifiée en débogage plutôt que relue.
            debug_assert!(covered(&row, &step_x, (gl - triangle.x0) as i64));
            debug_assert!(covered(&row, &step_x, (gr - triangle.x0) as i64));
            debug_assert!(
                gl == triangle.x0 || !covered(&row, &step_x, (gl - triangle.x0 - 1) as i64)
            );

            // Le parcours, lui, s'arrête au bord de la fenêtre.
            let (lo, hi) = (gl.max(wx0), gr.min(wx1));
            if lo > hi {
                for i in 0..3 {
                    row[i] += step_y[i];
                }
                continue;
            }

            // La profondeur au centre du premier pixel du span, par la forme
            // close : elle ne se propage pas d'une ligne à l'autre, les spans
            // ne commençant pas à la même abscisse. Les pas entiers qui suivent
            // donnent les mêmes bits que l'évaluation directe en chaque pixel.
            let ey = (py_of(y) - triangle.ref_y) as i64;
            let ex = |x: i32| (x * SUBPIXEL_SCALE + PIXEL_CENTER - triangle.ref_x) as i64;
            // **Le seul chemin qui se passe de segments est la couleur unie
            // sans éclairage.** Dès qu'un attribut se lit dans une image — le
            // texel, l'éclairage, ou les deux —, il faut la division de
            // perspective, donc le découpage en segments de seize pixels. Un
            // triangle uni éclairé passe donc par le même chemin qu'un triangle
            // texturé : ses coordonnées de lightmap sont interpolées de la même
            // façon, et rien ne justifierait une seconde boucle pour cela.
            if sampling.is_none() && lit.is_none() {
                let mut depth = plane.at(ex(lo), ey);
                for x in lo..=hi {
                    // En un pixel couvert, la valeur tient dans [0, 2³²) : les
                    // sommets sont bornés par `to_depth` avec une marge qui
                    // couvre l'arrondi des gradients.
                    let z = (depth >> GRADIENT_BITS) as u32;
                    if target.test(x, y, z) {
                        target.write(x, y, z, color);
                    }
                    depth = depth.wrapping_add(depth_x);
                }
            } else {
                let mut x = lo;
                while x <= hi {
                    // Le segment court d'un multiple de seize de l'image au
                    // suivant, **rabattu sur le span global** : au-delà, la
                    // profondeur prolongée hors du triangle peut s'annuler,
                    // et il n'y aurait aucun quotient à prendre. Ses bornes
                    // ne dépendent donc que du triangle et de la grille de
                    // l'image, jamais de la fenêtre où l'on parcourt.
                    let base = x - x.rem_euclid(SEGMENT);
                    let segment = (base.max(gl), (base + SEGMENT - 1).min(gr));
                    let last = segment.1.min(hi);
                    fill_segment(target, triangle, sampling, lit, y, segment, gr, (x, last));
                    x = last + 1;
                }
            }
        }
        for i in 0..3 {
            row[i] += step_y[i];
        }
    }
}

/// L'ordonnée du centre du pixel `y`, en sous-pixels.
fn py_of(y: i32) -> i32 {
    y * SUBPIXEL_SCALE + PIXEL_CENTER
}

/// Pixels entre deux divisions de perspective.
///
/// Seize, la valeur d'époque : la division coûte alors un seizième de pixel, et
/// l'écart à la perspective exacte reste sous le texel sur un segment de cette
/// longueur. Les points de division sont les **multiples de seize de l'abscisse
/// dans l'image**, jamais un décompte reparti du bord de la tuile ou du
/// triangle — sinon la même surface se texturerait autrement selon le
/// découpage.
const SEGMENT: i32 = 16;

/// Décalage qui ramène `S · W` en 16.16.
///
/// `S` est en 14.12 et vaut `u · d · 2¹²` ; `W` vaut `2⁵⁶ / D` avec `D = d ·
/// 2³²`. Leur produit vaut donc `u · 2³⁶`, et `u` en 16.16 s'en tire par un
/// décalage de vingt bits.
///
/// **Le produit ne déborde pas, et ce n'est pas par la borne des facteurs** :
/// pris séparément, `S` atteint 2²⁶ et `W` 2⁵⁰, dont le produit serait hors de
/// l'`i64`. Mais les deux sont **anticorrélés** — la profondeur qui fait
/// grandir `W` fait rétrécir `S` dans la même proportion —, si bien que leur
/// produit vaut exactement `u · 2³⁶` et reste sous 2⁵⁰ pour une coordonnée
/// bornée à 2¹⁴ texels.
const UV_SHIFT: u32 = 20;

/// Le pixel d'appui de la pente d'un segment, et le nombre de pas qui l'en
/// sépare du premier.
///
/// La pente d'un segment se prend entre son premier pixel et un point d'appui,
/// divisée par le nombre de pas entre les deux. L'appui est normalement le
/// pixel **suivant** le segment, pour que la pente soit celle d'un pas de pixel
/// et non d'un pas de segment.
///
/// **Sauf au bout du span**, où ce pixel n'est plus couvert : `near/w` y est
/// prolongé hors du triangle, où il peut s'annuler ou changer de signe, et il
/// n'existe alors aucun quotient à prendre. La borne est donc `span_end`, la
/// dernière abscisse couverte sur la ligne, et jamais celle de la boîte
/// englobante — qui déborde le span sur toute ligne d'un triangle non
/// rectangle.
///
/// **Le nombre de pas suit l'appui, et c'est ce qui garde la pente exacte** :
/// les deux sont les moitiés d'une même division. Reculer l'appui sans reculer
/// le diviseur sous-estimerait la pente d'un seizième sur le dernier segment de
/// chaque ligne. Un segment d'un seul pixel n'a aucun pas, d'où le plancher à
/// un, qui rend une pente nulle plutôt qu'une division par zéro.
fn segment_slope_ends(from: i32, to: i32, span_end: i32) -> (i32, i64) {
    if to < span_end {
        (to + 1, i64::from(to - from + 1))
    } else {
        (to, i64::from((to - from).max(1)))
    }
}

/// Remplit la part de `draw` qui tombe dans le segment `segment`, bornes
/// comprises.
///
/// Les coordonnées de texture se divisent aux **deux extrémités du segment** et
/// s'interpolent affinement entre elles : c'est le compromis d'époque, une
/// division pour seize pixels au lieu d'une par pixel, et l'écart à la
/// perspective exacte reste sous le texel sur une longueur pareille.
///
/// `segment` ne dépend que du triangle et de la grille de l'image ; `draw` en
/// est la part que la fenêtre laisse voir. Les séparer est ce qui rend la
/// texture indépendante du découpage : tout part de la forme close, rien ne
/// s'accumule d'une tuile à l'autre. `span_end` est la dernière abscisse
/// couverte sur cette ligne, et vient du span pour la même raison.
#[allow(clippy::too_many_arguments)]
fn fill_segment<T: Target>(
    target: &mut T,
    triangle: &Prepared,
    sampling: Option<Sampling<'_>>,
    lit: Option<Lit<'_>>,
    y: i32,
    segment: (i32, i32),
    span_end: i32,
    draw: (i32, i32),
) {
    // Les écarts se recalculent ici plutôt que de traverser la signature :
    // deux soustractions par segment, contre deux paramètres de plus dans une
    // liste qui en compte déjà huit.
    let ey = (py_of(y) - triangle.ref_y) as i64;
    let ex = |x: i32| (x * SUBPIXEL_SCALE + PIXEL_CENTER - triangle.ref_x) as i64;
    let depth_at = |x: i32| triangle.depth.at(ex(x), ey);

    let (from, to) = segment;
    let (anchor, steps) = segment_slope_ends(from, to, span_end);
    // **Les réciproques se calculent une fois pour les deux jeux de
    // coordonnées.** Elles ne dépendent que de la profondeur, et c'est la
    // seule division du remplissage : recalculées par jeu, un triangle à la
    // fois texturé et éclairé en paierait le double pour des valeurs
    // identiques.
    let ends = Ends {
        from: Reciprocal::new(from, depth_at(from)),
        to: Reciprocal::new(to, depth_at(to)),
        anchor: Reciprocal::new(anchor, depth_at(anchor)),
        steps,
        start: draw.0,
    };

    // **L'ombrage se choisit ici, une fois, et non à chaque pixel.** Chacun de
    // ces quatre appels instancie la boucle sur un type concret, si bien que
    // les tests qui distinguent les chemins disparaissent à la compilation.
    // Écrits dans la boucle — deux `Option` examinées par pixel —, ils
    // coûtaient un dixième du remplissage **à toute scène**, éclairée ou non.
    let texels = sampling.map(|s| (s, Crawl::new(triangle, &triangle.uv, ey, &ends)));
    let walk = Walk {
        target,
        triangle,
        y,
        draw,
    };
    match texels {
        Some((sampling, texel)) => match sampling.filter {
            Filter::Dither => textured::<T, false>(walk, sampling.texture, texel, lit, ey, &ends),
            Filter::Bilinear => textured::<T, true>(walk, sampling.texture, texel, lit, ey, &ends),
        },
        None => match lit {
            Some(lit) => plain(walk, lit, ey, &ends),
            None => walk.run(Flat(triangle.color)),
        },
    }
}

/// Construit l'éclairage d'un segment, les deux sources portées par le type.
///
/// Les trois bras couvrent les trois combinaisons possibles : la quatrième —
/// aucune source — n'existe pas, un triangle sans lightmap ni lumière ne
/// portant pas de plans d'éclairage.
macro_rules! with_glow {
    ($walk:expr, $triangle:expr, $start:expr, $lit:expr, $ey:expr, $ends:expr, $build:expr) => {{
        let lit = $lit;
        let planes = lit.planes;
        // Les deux marches se construisent dans le bras qui les lit, jamais
        // avant : bâtir celle d'une source absente coûterait un choix de
        // niveau de mipmap ou trois évaluations de plan pour rien.
        match (lit.lightmap.is_some(), planes.is_lit()) {
            (true, true) => $build(
                $walk,
                Glow::<true, true> {
                    map: Crawl::new($triangle, planes.planes(), $ey, $ends),
                    lightmap: lit.lightmap,
                    dynamic: Ramp::new($triangle, planes.dynamic(), $start, $ey),
                },
                lit.overbright,
            ),
            (true, false) => $build(
                $walk,
                Glow::<true, false> {
                    map: Crawl::new($triangle, planes.planes(), $ey, $ends),
                    lightmap: lit.lightmap,
                    dynamic: Ramp::EMPTY,
                },
                lit.overbright,
            ),
            (false, true) => $build(
                $walk,
                Glow::<false, true> {
                    map: Crawl::EMPTY,
                    lightmap: None,
                    dynamic: Ramp::new($triangle, planes.dynamic(), $start, $ey),
                },
                lit.overbright,
            ),
            // **Ni lightmap, ni lumière qui l'atteigne** : le triangle est
            // dans une scène éclairée mais hors de portée. Il s'éteint, comme
            // ses voisins là où la lumière ne porte pas — et sans qu'on
            // évalue trois plans par pixel pour arriver à zéro.
            (false, false) => $build(
                $walk,
                Glow::<false, false> {
                    map: Crawl::EMPTY,
                    lightmap: None,
                    dynamic: Ramp::EMPTY,
                },
                lit.overbright,
            ),
        }
    }};
}

/// Parcourt un segment texturé, éclairé ou non.
///
/// Séparée pour que le filtrage se choisisse une fois et l'éclairage ensuite :
/// écrits ensemble, les deux donneraient huit bras au lieu de deux plus trois,
/// pour les mêmes instances.
fn textured<T: Target, const BILINEAR: bool>(
    walk: Walk<'_, T>,
    texture: &Texture,
    texel: Crawl,
    lit: Option<Lit<'_>>,
    ey: i64,
    ends: &Ends,
) {
    let (triangle, start) = (walk.triangle, walk.draw.0);
    match lit {
        None => walk.run(Texel::<BILINEAR> { texel, texture }),
        Some(lit) => with_glow!(
            walk,
            triangle,
            start,
            lit,
            ey,
            ends,
            |walk: Walk<'_, T>, glow, overbright| {
                walk.run(TexelLight::<BILINEAR, _> {
                    texel,
                    texture,
                    glow,
                    overbright,
                })
            }
        ),
    }
}

/// Parcourt un segment uni qu'un éclairage couvre.
fn plain<T: Target>(walk: Walk<'_, T>, lit: Lit<'_>, ey: i64, ends: &Ends) {
    let (triangle, start) = (walk.triangle, walk.draw.0);
    let color = triangle.color;
    with_glow!(
        walk,
        triangle,
        start,
        lit,
        ey,
        ends,
        |walk: Walk<'_, T>, glow, overbright| {
            walk.run(Light {
                color,
                glow,
                overbright,
            })
        }
    )
}

/// Ce que le parcours d'un segment a de commun à tous ses ombrages.
struct Walk<'a, T: Target> {
    target: &'a mut T,
    triangle: &'a Prepared,
    y: i32,
    draw: (i32, i32),
}

impl<T: Target> Walk<'_, T> {
    /// Parcourt le segment, en laissant `shade` décider de chaque pixel.
    ///
    /// La profondeur, son test et son écriture sont ici : ils ne dépendent
    /// d'aucun ombrage, et les recopier dans chacun aurait rendu quatre fois
    /// le contrat du tampon de profondeur.
    fn run<S: Shade>(self, mut shade: S) {
        let ey = (py_of(self.y) - self.triangle.ref_y) as i64;
        let ex = |x: i32| (x * SUBPIXEL_SCALE + PIXEL_CENTER - self.triangle.ref_x) as i64;
        let mut depth = self.triangle.depth.at(ex(self.draw.0), ey);
        let depth_x = self.triangle.depth.step_x(SUBPIXEL_SCALE);

        for x in self.draw.0..=self.draw.1 {
            let z = (depth >> GRADIENT_BITS) as u32;
            if self.target.test(x, self.y, z) {
                self.target.write(x, self.y, z, shade.pixel(x, self.y));
            }
            depth = depth.wrapping_add(depth_x);
            shade.step();
        }
    }
}

/// Ce qui décide de la couleur d'un pixel, une fois sa profondeur acceptée.
///
/// Un trait plutôt qu'un test par pixel : chaque implémentation instancie la
/// boucle de parcours sur elle-même, et ce qui distingue les chemins est résolu
/// à la compilation. Un drapeau examiné à chaque pixel coûtait dix pour cent du
/// remplissage — y compris à une scène qui n'a ni texture ni lightmap, donc
/// pour un choix toujours identique.
trait Shade {
    /// La couleur du pixel `(x, y)`.
    fn pixel(&self, x: i32, y: i32) -> u32;
    /// Avance d'un pixel vers la droite.
    fn step(&mut self);
}

/// Une surface unie.
///
/// Elle ne passe pas par ici en pratique — `fill` peint une couleur unie sans
/// découper en segments —, mais l'écrire ferme la combinaison au lieu de
/// laisser un cas que rien ne traite.
struct Flat(u32);

impl Shade for Flat {
    fn pixel(&self, _x: i32, _y: i32) -> u32 {
        self.0
    }

    fn step(&mut self) {}
}

/// Une surface texturée, sans éclairage.
///
/// `BILINEAR` porte le filtrage dans le type : il vaut pour l'image entière, et
/// le choisir une fois par segment plutôt qu'à chaque pixel est ce qui permet
/// aux deux lectures d'être compilées séparément.
struct Texel<'a, const BILINEAR: bool> {
    texel: Crawl,
    texture: &'a Texture,
}

impl<const BILINEAR: bool> Shade for Texel<'_, BILINEAR> {
    #[inline]
    fn pixel(&self, x: i32, y: i32) -> u32 {
        self.texel.sample::<BILINEAR>(self.texture, x, y)
    }

    #[inline]
    fn step(&mut self) {
        self.texel.step();
    }
}

/// L'éclairage d'un pixel : ce qu'une lightmap y pose, ce que les lumières y
/// ajoutent, ou les deux.
///
/// `MAPPED` et `DYNAMIC` disent lesquelles des deux sources existent. Portés
/// par le type, comme le filtrage : une source absente ne se teste pas à
/// chaque pixel, elle n'est simplement pas compilée. Les deux faux n'arrive
/// jamais — un triangle sans aucune source ne porte pas de plans d'éclairage.
struct Glow<'a, const MAPPED: bool, const DYNAMIC: bool> {
    /// Les coordonnées de lightmap, inutilisées quand `MAPPED` est faux.
    map: Crawl,
    /// La lightmap elle-même, absente pour la même raison.
    lightmap: Option<&'a Texture>,
    /// Ce que les lumières ajoutent, en 16.16, inutilisé quand `DYNAMIC` est
    /// faux.
    dynamic: Ramp,
}

/// Ce qui éclaire une surface, quelles que soient ses sources.
///
/// **Un trait plutôt que deux paramètres constants portés par les ombrages** :
/// ceux-ci s'infèrent à l'appel, et l'inférence d'un paramètre constant
/// n'existe pas dans la version minimale de Rust que le projet vise. Le trait
/// ramène le choix à un paramètre de type, qui s'infère partout.
trait Glowing {
    /// L'éclairage au pixel `(x, y)`, trois canaux de huit bits saturés.
    fn at(&self, x: i32, y: i32) -> u32;
    /// Avance d'un pixel vers la droite.
    fn step(&mut self);
}

impl<const MAPPED: bool, const DYNAMIC: bool> Glowing for Glow<'_, MAPPED, DYNAMIC> {
    /// **La somme sature après l'addition**, comme celle des lumières entre
    /// elles : une lightmap presque pleine ne doit pas éteindre la torche qui
    /// passe, elle doit porter le total au blanc.
    #[inline]
    fn at(&self, x: i32, y: i32) -> u32 {
        // La lightmap se lit toujours en bilinéaire : voir [`Lit`].
        let mapped = match (MAPPED, self.lightmap) {
            (true, Some(texture)) => self.map.sample::<true>(texture, x, y),
            _ => 0,
        };
        if !DYNAMIC {
            return mapped;
        }
        let added = self.dynamic.channels();
        let channel = |index: u32| {
            let base = (mapped >> index) & 0xFF;
            (base + added[(index / 8) as usize]).min(0xFF) << index
        };
        channel(0) | channel(8) | channel(16)
    }

    /// Avance d'un pixel vers la droite.
    #[inline]
    fn step(&mut self) {
        if MAPPED {
            self.map.step();
        }
        if DYNAMIC {
            self.dynamic.step();
        }
    }
}

/// Une surface unie qu'un éclairage couvre : la couleur du triangle y tient
/// lieu de texel.
struct Light<G: Glowing> {
    color: u32,
    glow: G,
    overbright: u32,
}

impl<G: Glowing> Shade for Light<G> {
    #[inline]
    fn pixel(&self, x: i32, y: i32) -> u32 {
        light::modulate(self.color, self.glow.at(x, y), self.overbright)
    }

    #[inline]
    fn step(&mut self) {
        self.glow.step();
    }
}

/// Une surface texturée qu'un éclairage couvre.
struct TexelLight<'a, const BILINEAR: bool, G: Glowing> {
    texel: Crawl,
    texture: &'a Texture,
    glow: G,
    overbright: u32,
}

impl<const BILINEAR: bool, G: Glowing> Shade for TexelLight<'_, BILINEAR, G> {
    #[inline]
    fn pixel(&self, x: i32, y: i32) -> u32 {
        let texel = self.texel.sample::<BILINEAR>(self.texture, x, y);
        light::modulate(texel, self.glow.at(x, y), self.overbright)
    }

    #[inline]
    fn step(&mut self) {
        self.texel.step();
        self.glow.step();
    }
}

/// Un attribut à trois canaux qui avance affinement le long d'une ligne.
///
/// Comme la profondeur, et non comme les coordonnées de texture : aucune
/// division, aucun segment. Une couleur qui varie lentement n'a pas besoin de
/// correction de perspective, et une lumière ponctuelle n'en produit pas
/// d'autre.
struct Ramp {
    value: [i64; 3],
    step: [i64; 3],
}

impl Ramp {
    /// La marche d'un triangle qu'aucune lumière n'éclaire.
    ///
    /// Elle n'est jamais lue — le type qui la porte a `DYNAMIC` à faux —, mais
    /// il faut bien remplir le champ. Nommée plutôt que réécrite à chaque
    /// usage, pour qu'un lecteur voie que c'est un remplissage et non une
    /// valeur qui compte.
    const EMPTY: Self = Self {
        value: [0; 3],
        step: [0; 3],
    };

    /// La marche des trois plans depuis le pixel `start`.
    fn new(triangle: &Prepared, planes: &[Plane; 3], start: i32, ey: i64) -> Self {
        let ex = (start * SUBPIXEL_SCALE + PIXEL_CENTER - triangle.ref_x) as i64;
        let mut value = [0i64; 3];
        let mut step = [0i64; 3];
        for i in 0..3 {
            value[i] = planes[i].at(ex, ey);
            step[i] = planes[i].step_x(SUBPIXEL_SCALE);
        }
        Self { value, step }
    }

    /// Les trois canaux, ramenés de 16.16 à huit bits et bornés.
    ///
    /// Le bornage est là parce que l'évaluation d'un plan déborde de quelques
    /// unités sur les bords d'un triangle — c'est l'arrondi des gradients, que
    /// la marge de profondeur absorbe ailleurs et qu'une couleur n'a pas.
    #[inline]
    fn channels(&self) -> [u32; 3] {
        let read = |index: usize| {
            let raw = self.value[index] >> (GRADIENT_BITS + 8);
            raw.clamp(0, 0xFF) as u32
        };
        [read(0), read(1), read(2)]
    }

    /// Avance d'un pixel vers la droite.
    #[inline]
    fn step(&mut self) {
        for i in 0..3 {
            self.value[i] = self.value[i].wrapping_add(self.step[i]);
        }
    }
}

/// Une profondeur de segment et sa réciproque, gardées ensemble.
///
/// La réciproque sert aux coordonnées, l'abscisse au choix du niveau : les
/// séparer ferait repasser l'une ou l'autre dans une signature déjà longue.
#[derive(Clone, Copy)]
struct Reciprocal {
    x: i32,
    w: u64,
}

impl Reciprocal {
    /// La réciproque de la profondeur en `x`, telle que le plan la rend.
    fn new(x: i32, depth: i64) -> Self {
        Self {
            x,
            w: reciprocal((depth >> GRADIENT_BITS) as u32),
        }
    }
}

/// Ce qu'un segment a de commun à tous ses attributs.
struct Ends {
    from: Reciprocal,
    to: Reciprocal,
    anchor: Reciprocal,
    steps: i64,
    /// La première abscisse réellement peinte, que la fenêtre décide.
    start: i32,
}

/// Un attribut à deux composantes qui avance le long d'un segment.
///
/// Les coordonnées de texture et celles de lightmap suivent exactement la même
/// marche : division aux extrémités du segment, interpolation affine entre
/// elles, niveau de mipmap pris au plus fin des deux bouts. Une seule
/// implémentation, donc, et la lightmap ne peut pas dériver de la texture au fil
/// des lots.
struct Crawl {
    value: [i64; 2],
    slope: [i64; 2],
    level: u32,
}

impl Crawl {
    /// La marche d'un triangle qui ne lit aucune image.
    ///
    /// Jamais lue — le type qui la porte a sa source à faux —, mais il faut
    /// remplir le champ. Voir [`Ramp::EMPTY`].
    const EMPTY: Self = Self {
        value: [0; 2],
        slope: [0; 2],
        level: 0,
    };

    /// La marche d'`planes` sur le segment que `ends` décrit.
    fn new(triangle: &Prepared, planes: &[Plane; 2], ey: i64, ends: &Ends) -> Self {
        let ex = |x: i32| (x * SUBPIXEL_SCALE + PIXEL_CENTER - triangle.ref_x) as i64;
        let at = |end: &Reciprocal| {
            let read = |plane: &Plane| {
                i64::from(texel_coord(plane.at(ex(end.x), ey) >> GRADIENT_BITS, end.w))
            };
            [read(&planes[0]), read(&planes[1])]
        };

        let first = at(&ends.from);
        let last = at(&ends.to);
        // **Le niveau se prend au maximum des deux extrémités**, et non au seul
        // point de division gauche : quand la densité double sur seize pixels,
        // un niveau pris à gauche sous-sélectionne toute la moitié droite du
        // segment. Les deux réciproques étant déjà là, ce maximum coûte
        // quelques multiplications et aucune division.
        let level = {
            let of = |end: &Reciprocal, uv: [i64; 2]| {
                mip_level(triangle, planes, uv[0] as i32, uv[1] as i32, end.w)
            };
            of(&ends.from, first).max(of(&ends.to, last))
        };

        let after = if ends.anchor.x == ends.to.x {
            last
        } else {
            at(&ends.anchor)
        };
        // `div_euclid` : la pente s'arrondit vers le bas des deux côtés de
        // zéro, là où `/` ferait un pas double autour de l'origine.
        let slope = [
            (after[0] - first[0]).div_euclid(ends.steps),
            (after[1] - first[1]).div_euclid(ends.steps),
        ];

        let skipped = (ends.start - ends.from.x) as i64;
        Self {
            value: [first[0] + slope[0] * skipped, first[1] + slope[1] * skipped],
            slope,
            level,
        }
    }

    /// Le texel sous le pixel `(x, y)`, lu dans `texture`.
    ///
    /// **Le filtrage est un paramètre de compilation et non une valeur** : il
    /// vaut pour l'image entière, et l'examiner à chaque pixel coûtait plus
    /// cher que les deux lectures qu'il départage.
    #[inline]
    fn sample<const BILINEAR: bool>(&self, texture: &Texture, x: i32, y: i32) -> u32 {
        // Le décalage de niveau porte sur la coordonnée **interpolée**, et non
        // sur les extrémités du segment : appliqué à celles-ci, il quantifierait
        // la pente par 2ⁿ, soit cinq bits perdus au niveau 5.
        let scaled = [self.value[0] >> self.level, self.value[1] >> self.level];
        if BILINEAR {
            // Les bits fractionnaires servent de poids au lieu d'être jetés :
            // c'est la seule différence entre les deux modes, et c'est pourquoi
            // le tramage n'a plus rien à masquer ici.
            texture.bilinear(self.level as usize, scaled[0] as i32, scaled[1] as i32)
        } else {
            // Le tramage s'ajoute **après** le décalage de niveau : ajouté
            // avant, il serait divisé par 2ⁿ et s'éteindrait dès le niveau 2.
            let dither = dither_offsets(x, y);
            let coord = |c: i64, shift: i32| ((c + i64::from(shift)) >> UV_BITS) as i32;
            texture.texel(
                self.level as usize,
                coord(scaled[0], dither[0]),
                coord(scaled[1], dither[1]),
            )
        }
    }

    /// Avance d'un pixel vers la droite.
    #[inline]
    fn step(&mut self) {
        self.value[0] += self.slope[0];
        self.value[1] += self.slope[1];
    }
}

/// La réciproque de la profondeur, `2⁵⁶ / D`.
///
/// La seule division du remplissage, et elle a lieu une fois par segment de
/// seize pixels. `D` est strictement positif en un pixel couvert : les sommets
/// sont bornés par `to_depth` à distance des bornes, et une combinaison convexe
/// reste dans l'intervalle.
fn reciprocal(depth: u32) -> u64 {
    debug_assert!(depth > 0, "profondeur nulle en un pixel couvert");
    (1u64 << 56) / depth.max(1) as u64
}

/// La matrice de Bayer 4×4, en décalages de coordonnée 16.16.
///
/// **Le filtrage par défaut du moteur**, et une indirection de table pour tout
/// coût. Ce qu'il masque est la troncature vers le texel entier, qui commet une
/// erreur uniforme dans `[0, 1)` texel et **de signe constant** : sur une
/// surface agrandie, la frontière entre deux texels tombe alors sur une ligne
/// franche, l'escalier caractéristique. Un décalage nul en moyenne et réparti
/// sur ±½ texel remplace cette ligne par une bande d'un texel où les deux
/// voisins alternent selon la position du pixel.
///
/// Les entrées valent `(2·M − 15) << 11`, soit les multiples impairs de 2048
/// entre ±30720 : une amplitude de ±0,469 texel. Moins ne déplacerait rien ;
/// plus ferait lire un texel à deux de distance, ce qui n'est plus du tramage
/// mais du bruit, et détruirait en minification ce que le mipmap vient de
/// moyenner.
///
/// **La table est une permutation des seize niveaux, et somme à zéro.** Un
/// biais constant serait un décalage sous-texel permanent — et comme il
/// s'ajoute après le décalage de niveau, il vaudrait `biais · 2ⁿ` texels du
/// niveau 0 : la texture glisserait au passage d'un niveau à l'autre, soit
/// exactement le scintillement que le mipmap vient de supprimer.
pub(crate) const DITHER: [i32; 16] = [
    -30720, 2048, -22528, 10240, //
    18432, -14336, 26624, -6144, //
    -18432, 14336, -26624, 6144, //
    30720, -2048, 22528, -10240,
];

/// Les décalages de tramage d'un pixel, `u` puis `v`.
///
/// **L'index se prend sur la position dans l'image**, jamais sur une position
/// locale à la tuile : le motif se décalerait d'une tuile à l'autre, et la
/// conformance ne le verrait même pas, ses deux tailles de tuile étant toutes
/// deux des multiples de quatre. Seul un test dont les fenêtres commencent à
/// des abscisses non multiples de quatre l'attrape.
///
/// **`v` prend la transposée du même index.** Avec le même, les deux décalages
/// seraient égaux en tout pixel et le déplacement sous-texel toujours porté par
/// la diagonale : sur une surface où `u` vaut `v`, la texture ne montrerait que
/// sa diagonale. La transposée étant une permutation, elle garde à `v` la somme
/// nulle et les seize niveaux.
///
/// `& 3` et non `% 4` : le masque replie aussi une coordonnée négative du bon
/// côté, pour la même raison que le repli des texels.
fn dither_offsets(x: i32, y: i32) -> [i32; 2] {
    let (cx, cy) = ((x & 3) as usize, (y & 3) as usize);
    [DITHER[cy * 4 + cx], DITHER[cx * 4 + cy]]
}

/// Le niveau de mipmap d'un segment, depuis ses dérivées et sa réciproque.
///
/// **Les quatre dérivées se prennent en forme close, sans division nouvelle.**
/// `u = S/D` n'est pas affine en espace écran, mais `S` et `D` le sont, et
/// `∂u/∂x = (S_x − u·D_x)/D`. Le facteur `1/D` étant commun aux quatre et le
/// critère étant un **maximum** — distance de Chebyshev, sans racine carrée —,
/// on le sort du maximum : quatre numérateurs entiers, un seul `max`, et une
/// seule multiplication par la réciproque déjà calculée.
///
/// **La dérivée verticale est indispensable.** Sur un sol, `∂u/∂x` reste
/// modérée le long d'une ligne alors que `∂u/∂y` explose vers l'horizon : un
/// niveau choisi sur la seule horizontale sous-sélectionne, et le sol
/// scintille. C'est le critère de franchissement de l'étape, manqué exactement
/// là.
///
/// Écartée : la différence finie verticale, qui exigerait une réciproque un
/// pixel plus bas — donc hors du triangle dès la dernière ligne, où la
/// profondeur prolongée peut s'annuler. Le quotient y enveloppe en silence, et
/// le scintillement reviendrait précisément à l'horizon.
fn mip_level(triangle: &Prepared, planes: &[Plane; 2], u: i32, v: i32, reciprocal: u64) -> u32 {
    let pixel = SUBPIXEL_SCALE;
    let (dx, dy) = (
        triangle.depth.step_x(pixel) >> GRADIENT_BITS,
        triangle.depth.step_y(pixel) >> GRADIENT_BITS,
    );
    // `S` est en 14.12 et la coordonnée en 16.16 : le décalage met les deux
    // termes à la même échelle avant la soustraction. `step_x` n'est pas encore
    // réduit de `GRADIENT_BITS`, ce qui laisse les quatre bits de marge dont ce
    // décalage a besoin.
    let numerator = |plane: &Plane, coord: i32, depth_step: i64| {
        let slope = plane.step_x(pixel) << (UV_BITS - GRADIENT_BITS);
        slope - ((i64::from(coord).saturating_mul(depth_step)) >> UV_SHIFT)
    };
    let worst = [
        numerator(&planes[0], u, dx),
        numerator(&planes[1], v, dx),
        {
            let slope = planes[0].step_y(pixel) << (UV_BITS - GRADIENT_BITS);
            slope - ((i64::from(u).saturating_mul(dy)) >> UV_SHIFT)
        },
        {
            let slope = planes[1].step_y(pixel) << (UV_BITS - GRADIENT_BITS);
            slope - ((i64::from(v).saturating_mul(dy)) >> UV_SHIFT)
        },
    ]
    .into_iter()
    .map(|n| n.unsigned_abs())
    .max()
    .unwrap_or(0);

    // `ρ·2¹⁶ = M·W/2³⁶`, donc le niveau vaut `bit_length(ρ) − 1`, soit
    // `11 − leading_zeros(M·W)`. Une dérivée nulle donne `leading_zeros = 64`
    // et le niveau 0, sans cas particulier ; la saturation du produit donne le
    // niveau 11, ce qui est inoffensif : elle demanderait plus de 2¹² texels
    // par pixel, donc un niveau qu'aucune chaîne ne porte.
    LEVEL_BIAS.saturating_sub(worst.saturating_mul(reciprocal).leading_zeros())
}

/// Le biais du logarithme de `mip_level`.
///
/// **Ce onze n'est pas un réglage** : il sort de la largeur de l'`u64` et des
/// échelles de `S`, de `W` et des coordonnées. Qu'il vaille aussi le dernier
/// niveau d'une texture de [`MAX_TEXTURE_SIZE`] est une coïncidence — et c'est
/// l'assertion ci-dessous qui la surveille, faute de quoi porter cette borne à
/// 4096 plafonnerait le niveau sans que rien ne le signale.
const LEVEL_BIAS: u32 = 11;

const _: () = assert!(MAX_TEXTURE_SIZE.trailing_zeros() == LEVEL_BIAS);

/// La coordonnée de texture en 16.16 d'un attribut `S` à la profondeur `D`.
///
/// **Par décalage et non par division** : `/` tronque vers zéro, donc
/// changerait de sens d'arrondi de part et d'autre de l'origine de la texture,
/// et une couture y apparaîtrait sur une ligne que rien d'autre ne distingue.
/// Le décalage arithmétique, lui, arrondit vers le bas des deux côtés.
fn texel_coord(s: i64, reciprocal: u64) -> i32 {
    ((s.wrapping_mul(reciprocal as i64)) >> UV_SHIFT) as i32
}

#[cfg(test)]
mod tests;
