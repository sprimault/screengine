// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le balayage d'une boîte axiale contre les cellules d'une carte.
//!
//! **Utilisable sans contexte de rendu** : un serveur de jeu charge une carte,
//! balaie, et n'alloue jamais un tampon d'image. Rien ici ne touche au
//! rasteriseur, et l'édition de liens l'écarte d'un binaire qui ne dessine pas.
//!
//! Le contrat vu de l'hôte est dans `docs/abi.md`, section « Étape 7 » ; ce que
//! deux cibles doivent produire au bit près est dans `docs/rust.md`, section
//! « Collision ». Trois clauses commandent tout ce qui suit :
//!
//! - **le balayage est continu**, jamais des pas discrets avec dégagement : la
//!   profondeur de pénétration n'est définie que contre un convexe, et nos
//!   surfaces ne le sont pas ;
//! - **il teste le polygone d'une surface, jamais sa triangulation.** Chaque
//!   arête interne de la découpe d'oreilles deviendrait un prisme, donc une
//!   normale de contact qui n'existe pas sur la surface, et une boîte qui glisse
//!   sur un sol plan y accrocherait ;
//! - **une arête rentrante ou coplanaire ne porte aucun volume**, ce que le
//!   chargement a déjà classé : c'est la règle de l'arête partagée, et elle tient
//!   la justesse de l'étape comme la règle top-left tient celle du rasteriseur.
//!
//! Tout y est en `f64`, et le calcul reste scalaire : sur armv7, le SIMD avancé
//! n'a que la sémantique du zéro forcé là où le VFP scalaire traite les
//! sous-normaux.

mod brute;
mod overlap;
mod shape;

use crate::format::World;
use crate::format::world::{Cell, Surface};
use crate::math::{Vec3, Vec3d};

use shape::Touch;

pub(crate) use brute::sweep_brute;

/// Le plus grand nombre de cellules qu'un balayage visite.
///
/// **Une borne et non deux**, à la différence de la traversée de rendu :
/// l'étendue balayée borne déjà la région, et le nombre de cellules visitées est
/// la seule chose qui puisse enfler. Elle ne se configure pas — un résultat qui
/// dépendrait d'un champ de configuration échapperait à la conformance.
///
/// **Sa valeur se mesure avant d'être publiée**, sur le décor de validation : une
/// constante d'ABI ne change jamais de sens une fois publiée, et celle-ci attend
/// donc le lot qui écrit la scène de conformance.
pub const SWEEP_CELLS: usize = 64;

/// De combien la boîte est dilatée, en fraction de sa plus grande demi-étendue.
///
/// **On dilate la boîte, on ne recule pas le temps d'impact.** Reculer `t`
/// laisserait la boîte pénétrante sur les axes perpendiculaires au mouvement, si
/// bien que le problème reviendrait au balayage suivant, ailleurs : un recul est
/// proportionnel à la vitesse, une dilatation ne l'est pas.
///
/// Une puissance de deux, donc le produit est exact et n'introduit aucun
/// arrondi ; relative à la boîte, donc sans échelle de monde à inventer, et sans
/// sous-normal quelle que soit la taille du décor.
///
/// **Sa valeur se juge sur un décor réel**, à la marche d'escalier et au
/// chambranle de porte, et se fige avec la scène de conformance — avant que sa
/// référence soit écrite. Celle-ci est l'ordre de grandeur de départ, et les
/// tests de ce module portent tous sur des propriétés vraies pour **tout un
/// intervalle** de valeurs, jamais sur celle-ci.
const SKIN: f64 = 1.0 / 1024.0;

/// Quelles surfaces une interrogation voit.
///
/// **Le balayage et la sélection ne regardent pas le même décor**, et c'est la
/// seule chose qui les distingue. Le premier ne voit que ce qui arrête un
/// volume ; la seconde doit attraper une grille, une vitre, un volume de
/// déclenchement — des surfaces que la carte marque « non solides » et que la
/// collision ignore par construction.
///
/// Le drapeau décrit la géométrie, jamais l'appelant : une surface non solide
/// compte toujours dans la parité qui localise un point, et occulte toujours la
/// cuisson. Ce filtre ne change que ce qu'une requête retient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surfaces {
    /// Les seules surfaces solides : ce que le balayage arrête.
    Solid,
    /// Toutes, non solides comprises : ce qu'une sélection d'éditeur attrape.
    All,
}

impl Surfaces {
    /// Vrai si cette surface entre dans la requête.
    fn keeps(self, surface: &Surface) -> bool {
        match self {
            Self::Solid => surface.is_solid(),
            Self::All => true,
        }
    }
}

/// Ce qu'un balayage rend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// La fraction du déplacement parcourue avant le contact, dans `[0, 1]`.
    pub fraction: f32,
    /// La normale du contact, unitaire, opposée au mouvement.
    ///
    /// Nulle quand rien n'est touché : le vecteur nul est ce que la table de
    /// racine inverse rend d'une longueur négligeable, et l'hôte n'a de toute
    /// façon rien à en faire quand `surface` vaut zéro.
    pub normal: Vec3,
    /// Le point de contact, sur le plan de la surface touchée.
    pub point: Vec3,
    /// L'identifiant de la surface touchée, ou zéro.
    pub surface: u32,
    /// L'identifiant de la cellule où le contact a lieu, ou zéro.
    pub cell: u32,
    /// La boîte était-elle déjà en intersection au départ ?
    pub start_solid: bool,
    /// La région examinée a-t-elle été tronquée par [`SWEEP_CELLS`] ?
    pub incomplete: bool,
}

/// Le meilleur contact connu, et ce qu'il faut pour le départager du suivant.
///
/// Les trois valeurs de départage voyagent ensemble parce qu'elles ne veulent
/// rien dire séparément : une fraction sans son rang ne tranche pas une égalité,
/// et la pénétration la moins profonde n'a de sens qu'avec la surface qui la
/// porte.
pub(super) struct Best {
    /// Ce que le balayage rendra.
    hit: Hit,
    /// L'instant du meilleur contact.
    fraction: f64,
    /// La famille du meilleur contact : face, arête ou sommet.
    rank: u8,
    /// La moindre pénétration trouvée au départ.
    deepest: f64,
}

impl Best {
    /// L'état d'un balayage qui n'a encore rien rencontré.
    fn new(to: Vec3d) -> Self {
        Self {
            hit: Hit::free(to),
            fraction: 1.0,
            rank: u8::MAX,
            deepest: f64::MAX,
        }
    }

    /// Ramène le résultat à `reached`, reculé de la marge, s'il allait plus loin.
    ///
    /// **Le recul est celui du dégagement de sécurité, et c'est la même
    /// constante** : s'arrêter pile sur la frontière poserait le mobile **sur un
    /// portail**, au seuil d'une cellule que le moteur n'a pas regardée, et un
    /// balayage repris de là repartirait sans plus de garantie. Le dégagement
    /// évite le contact exact avec ce qui arrête, celui-ci le contact exact avec
    /// ce qui n'a pas été examiné : c'est le même invariant, **le moteur ne rend
    /// jamais une position pile sur une limite**.
    ///
    /// Aucune surface n'est nommée : le moteur n'en a touché aucune, et prétendre
    /// le contraire serait inventer de la géométrie. C'est le statut qui dit à
    /// l'hôte pourquoi il s'arrête là.
    fn truncate(&mut self, reached: f64) {
        // Jamais en deçà du départ : sur un balayage assez court pour que sa
        // première cellule soit déjà la dernière examinable, le recul mordrait
        // sur l'origine. Zéro est alors la réponse honnête — l'hôte apprend
        // qu'il n'avance pas, ce qui est vrai — là où une valeur négative serait
        // un déplacement à rebours qu'il n'a pas demandé.
        let stopped = if reached - SKIN < 0.0 {
            0.0
        } else {
            reached - SKIN
        };
        if stopped < self.fraction {
            self.fraction = stopped;
            self.hit.fraction = stopped as f32;
            self.hit.surface = 0;
            self.hit.normal = Vec3::ZERO;
        }
    }
}

impl Hit {
    /// Le déplacement libre, que rend un balayage qui ne rencontre rien.
    pub(crate) fn free(to: Vec3d) -> Self {
        Self {
            fraction: 1.0,
            normal: Vec3::ZERO,
            point: to_f32(to),
            surface: 0,
            cell: 0,
            start_solid: false,
            incomplete: false,
        }
    }
}

/// Balaie une boîte axiale de `from` à `to`, depuis la cellule `from_cell`.
///
/// Rend `None` quand la cellule de départ n'existe pas — l'appelant en fait une
/// erreur de ressource inconnue. `from_cell` à zéro n'arrive pas ici : c'est
/// l'absence de cellule, et la frontière la traite avant d'appeler.
///
/// Que la cellule contienne la boîte n'est pas vérifié : ce serait un test
/// d'appartenance par requête pour un appelant qui le sait déjà.
pub(crate) fn sweep(
    world: &World,
    from_cell: u32,
    half: Vec3d,
    from: Vec3d,
    to: Vec3d,
    surfaces: Surfaces,
) -> Option<Hit> {
    let start = world.cell_of(from_cell)?;
    // **La boîte dilatée pour le balayage, la vraie pour le départ solide.** Un
    // test de recouvrement est non strict : avec la boîte dilatée, une boîte qui
    // vient de s'arrêter au contact d'un mur repartirait en départ solide à
    // l'image suivante, c'est-à-dire exactement ce que la dilatation existe pour
    // empêcher.
    let grown_half = grown(half);

    // La pile de travail et l'ensemble des visitées vivent ici, en tableaux de
    // taille fixe : aucune allocation, et la borne est celle de l'ABI.
    let mut stack = [0u32; SWEEP_CELLS];
    let mut seen = [0u32; SWEEP_CELLS];
    let mut stacked = 1;
    let mut count = 1;
    stack[0] = start;
    seen[0] = start;

    let bounds = moving_bounds(grown_half, from, to);
    let mut best = Best::new(to);

    while stacked > 0 {
        stacked -= 1;
        let index = stack[stacked];
        let Some(cell) = world.cells().get(index as usize) else {
            continue;
        };

        sweep_cell(cell, grown_half, from, to, surfaces, &mut best);
        start_solid(cell, half, from, surfaces, &mut best);

        for portal in &cell.portals {
            let Some((linked, _)) = portal.link else {
                continue;
            };
            if !crosses_bounds(&portal.points, bounds) {
                continue;
            }
            if seen[..count].contains(&linked) {
                continue;
            }
            if count == SWEEP_CELLS {
                // **La région examinée s'arrête à ce portail**, et le mouvement
                // avec elle : au-delà, le moteur n'a rien regardé. Rendre le
                // déplacement entier ferait passer une entité à travers un mur
                // qu'il n'a pas eu le temps de voir, et le statut ne servirait
                // qu'à s'en excuser.
                if let Some(reached) = portal_fraction(&portal.points, grown_half, from, to) {
                    best.truncate(reached);
                }
                best.hit.incomplete = true;
                // **Et les portails suivants se regardent quand même**, là où un
                // arrêt net serait une **sur-déclaration de validité** : la borne
                // retenue est un minimum, donc en abandonner des candidats ne peut
                // que la laisser trop grande — le moteur déclarerait valide une
                // portion qu'il n'a pas examinée, et un contact manqué ne se
                // signalerait par rien. Le minimum sur tous est conservateur.
                //
                // Cela retire aussi au résultat sa dépendance à l'ordre des
                // portails dans le fichier, qu'aucune clause n'annonçait et qu'un
                // hôte ne pourrait pas exploiter : prédire sa troncature lui
                // demanderait de raisonner sur l'ordre d'écriture de sa propre
                // carte.
                continue;
            }
            seen[count] = linked;
            count += 1;
            stack[stacked] = linked;
            stacked += 1;
        }
    }

    // **La fraction est bornée à la sortie, et non au seul endroit du calcul où
    // on la croirait nécessaire.** Le contrat annonce `[0, 1]` : un hôte n'a
    // donc aucune raison de tester une valeur négative, elle traverserait tous
    // ses garde-fous, et un déplacement à rebours se manifesterait comme un
    // défaut de son code de glissade — très loin de sa cause.
    //
    // Par comparaisons écrites et non par `clamp`, que `clippy.toml` refuse :
    // son traitement de `NaN` n'est pas celui des chemins vectoriels. L'ordre
    // des deux tests le borne aussi — un `NaN` ne satisfait ni l'un ni l'autre
    // et ressortirait tel quel, d'où le cas nommé qui le ramène à zéro.
    best.hit.fraction = in_unit(best.hit.fraction);
    Some(best.hit)
}

/// La fraction ramenée dans `[0, 1]`, `NaN` compris.
///
/// **Par comparaisons écrites et non par `clamp`**, que `clippy.toml` refuse :
/// son traitement de `NaN` n'est pas celui des chemins vectoriels. L'ordre des
/// cas fait le reste — ce qui n'est ni au-dessus de un ni strictement au-dessus
/// de zéro tombe dans le dernier, et c'est là que `NaN` atterrit sans avoir à
/// être nommé, toute comparaison avec lui étant fausse.
pub(super) fn in_unit(value: f32) -> f32 {
    if value > 1.0 {
        1.0
    } else if value > 0.0 {
        value
    } else {
        0.0
    }
}

/// La fraction à laquelle la boîte atteint le plan d'un portail.
///
/// Sans test d'appartenance au polygone : ce qui est cherché est le moment où le
/// mouvement quitte la région examinée, et **tronquer trop tôt est sûr quand
/// tronquer trop tard ne l'est pas**. Un portail atteint par le plan mais manqué
/// par le polygone ne fait donc qu'arrêter le mobile un peu avant.
fn portal_fraction(points: &[Vec3], half: Vec3d, from: Vec3d, to: Vec3d) -> Option<f64> {
    let corners: heapless::Points = {
        let mut list = heapless::Points::new();
        for point in points {
            list.push(Vec3d::from(*point));
        }
        list
    };
    let normal = newell(&corners);
    let anchor = *corners.first()?;
    if normal == Vec3d::ZERO {
        return None;
    }

    let reach = shape::support(normal, half);
    let d0 = normal.dot(from - anchor);
    let d1 = normal.dot(to - anchor);
    if d0 == d1 {
        return None;
    }
    // Des deux côtés : un portail se franchit dans le sens que le mouvement lui
    // donne, et le sien n'est pas orienté.
    let t = if d0 > d1 {
        (d0 - reach) / (d0 - d1)
    } else {
        (d0 + reach) / (d0 - d1)
    };
    if (0.0..=1.0).contains(&t) {
        Some(t)
    } else {
        None
    }
}

/// La normale de Newell d'un polygone en double précision.
fn newell(points: &[Vec3d]) -> Vec3d {
    let mut normal = Vec3d::ZERO;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        normal.x += (a.y - b.y) * (a.z + b.z);
        normal.y += (a.z - b.z) * (a.x + b.x);
        normal.z += (a.x - b.x) * (a.y + b.y);
    }
    normal
}

/// La boîte dilatée de [`SKIN`].
///
/// Le facteur porte sur la plus grande demi-étendue et non sur chacune : une
/// boîte plate — un disque, une lame — verrait sinon son épaisseur nulle rester
/// nulle, et rien ne la séparerait jamais du sol qu'elle touche.
pub(super) fn grown(half: Vec3d) -> Vec3d {
    let largest = max(max(half.x, half.y), half.z);
    let margin = largest * SKIN;
    Vec3d::new(half.x + margin, half.y + margin, half.z + margin)
}

/// Marque le départ dans le solide, et retient la surface la moins pénétrée.
///
/// **Le moteur ne dégage pas** : il n'existe aucun vecteur de dégagement défini
/// contre un jeu de surfaces non convexes. Il signale, et la normale rendue est
/// celle qui demande le moins de recul — à égalité, l'ordre du fichier, que le
/// parcours donne en ne remplaçant jamais un égal.
///
/// Le test passe par le prédicat de recouvrement, qui ne partage aucune algèbre
/// avec le balayage : un segment ne peut pas rendre cet instant-là, le sien
/// commençant précisément où la boîte est déjà là.
pub(super) fn start_solid(
    cell: &Cell,
    half: Vec3d,
    from: Vec3d,
    surfaces: Surfaces,
    best: &mut Best,
) {
    for surface in &cell.surfaces {
        if !surfaces.keeps(surface) {
            continue;
        }
        let Some(depth) = overlap::penetration(cell, surface, half, from) else {
            continue;
        };
        if depth >= best.deepest {
            continue;
        }
        best.deepest = depth;
        best.hit.start_solid = true;
        best.hit.fraction = 0.0;
        best.hit.normal = to_f32(Vec3d::from(cell.inward(surface))).normalize();
        best.hit.point = to_f32(plane_point(cell, surface, from));
        best.hit.surface = surface.id();
        best.hit.cell = cell.id();
    }
}

/// Balaie une cellule et retient le contact s'il précède le meilleur connu.
///
/// **Les trois critères de départage sont ici**, et leur ordre est contractuel :
/// l'instant, puis la famille — face, arête, sommet —, puis l'ordre du fichier,
/// que le parcours donne gratuitement en ne remplaçant jamais à égalité stricte.
fn sweep_cell(
    cell: &Cell,
    half: Vec3d,
    from: Vec3d,
    to: Vec3d,
    surfaces: Surfaces,
    best: &mut Best,
) {
    for surface in &cell.surfaces {
        if !surfaces.keeps(surface) {
            continue;
        }
        let Some(touch) = sweep_surface(cell, surface, half, from, to) else {
            continue;
        };
        let later = touch.fraction > best.fraction;
        let equal_but_coarser = touch.fraction == best.fraction && touch.rank >= best.rank;
        if later || equal_but_coarser {
            continue;
        }
        best.fraction = touch.fraction;
        best.rank = touch.rank;
        let centre = from + (to - from) * touch.fraction;
        best.hit.fraction = touch.fraction as f32;
        best.hit.normal = to_f32(touch.normal).normalize();
        best.hit.point = to_f32(plane_point(cell, surface, centre));
        best.hit.surface = surface.id();
        best.hit.cell = cell.id();
    }
}

/// Balaie une surface : sa face, puis ses arêtes exposées, puis ses sommets.
fn sweep_surface(
    cell: &Cell,
    surface: &Surface,
    half: Vec3d,
    from: Vec3d,
    to: Vec3d,
) -> Option<Touch> {
    let points = corners(cell, surface);
    let normal = Vec3d::from(cell.inward(surface));
    let anchor = *points.first()?;

    let mut best: Option<Touch> = None;
    let mut keep = |touch: Option<Touch>| {
        let Some(touch) = touch else {
            return;
        };
        let better = best.is_none_or(|current| {
            touch.fraction < current.fraction
                || (touch.fraction == current.fraction && touch.rank < current.rank)
        });
        if better {
            best = Some(touch);
        }
    };

    keep(shape::face(&points, normal, anchor, half, from, to));
    for i in 0..points.len() {
        if !surface.edge_is_exposed(i) {
            continue;
        }
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        keep(shape::edge(a, b, half, from, to));
        keep(shape::vertex(a, half, from, to));
    }
    best
}

/// Les sommets d'une surface, en double précision.
///
/// Un tableau de taille fixe sur la pile : une surface est plafonnée à
/// [`MAX_POLYGON`] coins par la triangulation, et une allocation par requête
/// serait le défaut que « aucune allocation » existe pour interdire.
///
/// [`MAX_POLYGON`]: crate::format::ears::MAX_POLYGON
fn corners(cell: &Cell, surface: &Surface) -> heapless::Points {
    let mut points = heapless::Points::new();
    let first = surface.first_vertex as usize;
    for i in 0..surface.corners.len() {
        points.push(Vec3d::from(cell.vertices[first + i].position));
    }
    points
}

/// Le point de contact : la projection du centre de la boîte sur le plan.
///
/// Un contact face contre face est un rectangle et non un point ; le moteur en
/// choisit un et le fige, sans quoi la conformance n'aurait rien à comparer. Un
/// produit scalaire, sans racine.
fn plane_point(cell: &Cell, surface: &Surface, centre: Vec3d) -> Vec3d {
    let normal = Vec3d::from(cell.inward(surface));
    let anchor = Vec3d::from(cell.vertices[surface.first_vertex as usize].position);
    let square = normal.dot(normal);
    if square <= 0.0 {
        return centre;
    }
    centre - normal * (normal.dot(centre - anchor) / square)
}

/// La boîte englobante du mouvement entier, boîte balayée comprise.
fn moving_bounds(half: Vec3d, from: Vec3d, to: Vec3d) -> [Vec3d; 2] {
    let mut low = Vec3d::ZERO;
    let mut high = Vec3d::ZERO;
    for i in 0..3 {
        let a = from.axis(i);
        let b = to.axis(i);
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        set_axis(&mut low, i, lo - half.axis(i));
        set_axis(&mut high, i, hi + half.axis(i));
    }
    [low, high]
}

/// Le portail coupe-t-il la boîte du mouvement ?
///
/// Pré-rejet conservateur, et rien d'autre : un portail retenu à tort ne coûte
/// qu'une cellule visitée pour rien, un portail écarté à tort troue le résultat.
fn crosses_bounds(points: &[Vec3], bounds: [Vec3d; 2]) -> bool {
    for i in 0..3 {
        let mut low = f64::MAX;
        let mut high = f64::MIN;
        for point in points {
            let value = Vec3d::from(*point).axis(i);
            if value < low {
                low = value;
            }
            if value > high {
                high = value;
            }
        }
        if high < bounds[0].axis(i) || low > bounds[1].axis(i) {
            return false;
        }
    }
    true
}

/// La conversion vers le vecteur du pipeline, à la sortie du balayage.
fn to_f32(v: Vec3d) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
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

/// Un tableau de sommets de taille fixe, sur la pile.
mod heapless {
    use crate::format::ears::MAX_POLYGON;
    use crate::math::Vec3d;

    /// Les sommets d'une surface, au plus [`MAX_POLYGON`].
    pub(super) struct Points {
        /// Le tableau, dont seuls les `len` premiers ont un sens.
        items: [Vec3d; MAX_POLYGON],
        /// Combien de sommets il porte.
        len: usize,
    }

    impl Points {
        /// Un tableau vide.
        pub(super) fn new() -> Self {
            Self {
                items: [Vec3d::ZERO; MAX_POLYGON],
                len: 0,
            }
        }

        /// Ajoute un sommet, en ignorant ce qui dépasse le plafond.
        ///
        /// Le dépassement ne peut pas arriver — la triangulation refuse au
        /// chargement un polygone de plus de [`MAX_POLYGON`] coins —, et le
        /// silence vaut mieux qu'une panique sur un chemin qui ne rend pas
        /// d'erreur.
        pub(super) fn push(&mut self, point: Vec3d) {
            if self.len < MAX_POLYGON {
                self.items[self.len] = point;
                self.len += 1;
            }
        }
    }

    impl core::ops::Deref for Points {
        type Target = [Vec3d];

        fn deref(&self) -> &[Vec3d] {
            &self.items[..self.len]
        }
    }
}

#[cfg(test)]
mod tests;
