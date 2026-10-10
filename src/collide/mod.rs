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
use crate::format::world::{Cell, Portal, Surface};
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

/// De combien le balayage dilate une boîte de ces demi-étendues.
///
/// **Le jeu résiduel qu'un hôte ne peut pas deviner.** Le temps rendu place la
/// boîte juste avant le contact, jamais dessus, et un hôte qui écrit une sonde
/// « suis-je posé » a besoin de l'ordre de grandeur de ce jeu pour choisir la
/// longueur de sa sonde et son seuil. Sans cette fonction, il n'avait que deux
/// voies : recopier la constante, ou la mesurer.
///
/// **Une fonction plutôt que la constante**, parce que ce qui se recopie de
/// travers n'est pas sa valeur mais sa **clause** : le facteur porte sur la plus
/// grande demi-étendue et non sur chacune, de sorte qu'une boîte plate — un
/// disque, une lame — ne garde pas une épaisseur nulle que rien ne séparerait du
/// sol. Un hôte qui recopierait le nombre sans cette règle se tromperait sur
/// exactement les boîtes où elle compte.
///
/// Ce qu'elle rend est la marge **le long de chaque axe**. Le jeu réel le long
/// d'une normale vaut cette marge multipliée par la somme des valeurs absolues de
/// cette normale : une marge sur un sol axial, √2 sur une rampe à 45°, jusqu'à √3
/// sur un coin — c'est pourquoi un mobile décolle un peu plus d'une pente que
/// d'un plancher.
///
/// Des demi-étendues négatives ou non finies n'ont pas de marge ; la frontière
/// les refuse avant d'appeler, et le chemin Rust rend alors zéro.
pub fn sweep_skin(half_extents: Vec3) -> f32 {
    let half = Vec3d::from(half_extents);
    let largest = max(max(half.x, half.y), half.z);
    // `is_nan` d'abord : toute comparaison avec lui est fausse, et un refus écrit
    // `largest <= 0.0` le laisserait passer jusqu'à la multiplication.
    if largest.is_nan() || largest <= 0.0 {
        return 0.0;
    }
    (largest * SKIN) as f32
}

/// Le pas d'un `f32` relativement à sa valeur : sa mantisse porte vingt-quatre
/// bits.
const F32_STEP: f64 = 1.0 / 16_777_216.0;

/// La boîte est-elle trop petite, là où elle se déplace, pour garder un jeu ?
///
/// **Le jeu disponible et le pas de la position se comparent, et rien d'autre.**
/// Le premier vaut une demi-marge, celle que le bord d'un panneau laisse ; le
/// second est l'écart entre deux `f32` voisins à cette coordonnée. Quand le jeu
/// n'excède plus le pas, la pose rendue à l'hôte retombe dans le solide et le
/// balayage suivant part pénétrant — ce que la dilatation existe pour empêcher.
///
/// **Dérivé de [`SKIN`], jamais écrit en `2⁻¹³`.** Ce chiffre est le quotient des
/// deux, et le figer ferait mentir le drapeau le jour où la constante bouge —
/// or c'est précisément elle qu'un décor plus vaste ferait reconsidérer. Les deux
/// facteurs étant des puissances de deux, les produits sont exacts.
///
/// **La plus grande coordonnée du segment entier**, et non celle d'une extrémité :
/// la repose tombe n'importe où entre les deux, et le pas suit la coordonnée.
fn no_gap(half: Vec3d, from: Vec3d, to: Vec3d) -> bool {
    let largest = max(max(half.x, half.y), half.z);
    // Même forme que `sweep_skin`, et pour la même raison : `is_nan` d'abord, un
    // refus écrit `largest <= 0.0` le laisserait passer. Un rayon sort ici — sa
    // dilatation est nulle par construction, donc il n'a aucun jeu à perdre.
    if largest.is_nan() || largest <= 0.0 {
        return false;
    }
    let reach = max(
        max(max(abs(from.x), abs(from.y)), abs(from.z)),
        max(max(abs(to.x), abs(to.y)), abs(to.z)),
    );
    largest * (SKIN * 0.5) <= reach * F32_STEP
}

/// Jusqu'à quelle distance de l'origine une boîte de ces demi-étendues garde un
/// jeu.
///
/// **La question que le drapeau ne peut pas porter.** Un statut décrit l'appel qui
/// vient de rendre, et celui du jeu perdu est le moins actionnable des quatre :
/// classé dernier, il est masqué par le départ solide exactement dans le cas qui
/// compte, puisqu'une boîte sans jeu se repose dans le solide et que le balayage
/// suivant rend ce départ. Le garde-fou se tairait dans son propre symptôme. Ce
/// qu'un hôte a besoin de savoir est donc un **état** et non un événement, et le
/// canal du code de retour ne transporte que des événements, un à la fois.
///
/// **Une valeur, et les demi-étendues en entrée** — non un prédicat sur une pose.
/// Ce qu'un hôte compare à cette distance est l'étendue de son décor, qui est une
/// donnée de sa génération : il pose la question une fois, avant même qu'une carte
/// existe. Un prédicat sur une position l'obligerait à rappeler par mobile et par
/// image pour apprendre la même chose, et retomberait dans le canal par événement
/// qu'on vient d'écarter.
///
/// Le jeu tient **en deçà** de cette distance, strictement : à cette coordonnée
/// exactement, il est déjà perdu. C'est le seuil, pas la dernière valeur sûre.
///
/// **Dérivée de `SKIN` comme `no_gap`, et par l'expression inverse** : deux
/// calculs écrits séparément finiraient par ne plus se répondre, et c'est l'hôte
/// qui en paierait l'écart — la fonction lui dit sûr ce que le drapeau lui dit
/// perdu.
///
/// Un rayon rend zéro, comme il rend zéro de marge : n'ayant aucun jeu, il n'a
/// aucune portée au-delà de laquelle le perdre.
pub fn sweep_reach(half_extents: Vec3) -> f32 {
    let half = Vec3d::from(half_extents);
    let largest = max(max(half.x, half.y), half.z);
    // Même forme et même raison que `sweep_skin` : `is_nan` d'abord, un refus
    // écrit `largest <= 0.0` le laisserait passer jusqu'à la multiplication.
    if largest.is_nan() || largest <= 0.0 {
        return 0.0;
    }
    (largest * (SKIN * 0.5) / F32_STEP) as f32
}

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
    /// racine inverse rend d'une longueur négligeable. Le critère est bien « rien
    /// de rencontré » et non `surface == 0`, qu'un portail non apparié laisse nul
    /// tout en rendant une normale utile.
    pub normal: Vec3,
    /// Le point de contact, sur le plan de ce qui arrête.
    ///
    /// **Nul quand rien n'est rencontré**, et c'est le déplacement libre autant
    /// que le résultat tronqué : celui-ci s'arrête au bord de la région
    /// examinée, où le moteur n'a rien regardé. Ce n'est donc jamais la position
    /// d'arrêt du mobile, que l'hôte obtient de la fraction — et ce n'est pas
    /// non plus lié à `surface`, qu'un portail non apparié laisse nulle en
    /// portant pourtant un contact.
    pub point: Vec3,
    /// L'identifiant de la surface touchée, ou zéro.
    pub surface: u32,
    /// L'identifiant de la cellule où le contact a lieu, ou zéro.
    ///
    /// Nulle avec `point` et pour la même raison : une cellule n'est nommée que
    /// si un contact y a lieu. Un portail non apparié en nomme une sans nommer
    /// de surface, n'en étant pas une.
    pub cell: u32,
    /// La boîte était-elle déjà en intersection au départ ?
    pub start_solid: bool,
    /// La région examinée a-t-elle été tronquée par [`SWEEP_CELLS`] ?
    pub incomplete: bool,
    /// La boîte est-elle trop petite, là où elle se déplace, pour garder un jeu ?
    ///
    /// Le jeu que le balayage laisse vaut une demi-marge, donc
    /// `half_max · 2⁻¹¹` ; reposer le mobile à la fraction rendue passe par des
    /// positions en `f32`, dont le pas vaut `|p|·2⁻²⁴`. Quand le premier n'excède
    /// plus le second, la pose retombe **dans** le solide et le balayage suivant
    /// part d'un départ pénétrant. Le contact rendu reste juste : c'est la repose
    /// qui perd son jeu, et c'est pourquoi ceci est un drapeau et non une erreur.
    ///
    /// **Un rayon ne le lève jamais.** Sa dilatation est nulle par construction,
    /// donc il n'a aucun jeu à perdre — le signaler ferait du cas normal une
    /// anomalie permanente.
    pub no_gap: bool,
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
    ///
    /// **Et cet argument porte sur les cinq champs du contact, pas sur trois.**
    /// Le point et la cellule décrivent un contact autant que la surface et la
    /// normale : laissés en place, ils désignaient soit la destination que
    /// [`Hit::free`] y avait posée — une arrivée que le mouvement n'atteint
    /// pas —, soit la surface d'un contact que cette troncature vient
    /// précisément d'écarter. Un hôte lisait donc une géométrie dans le même
    /// résultat qui déclare n'en avoir rencontré aucune.
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
            self.hit.point = Vec3::ZERO;
            self.hit.cell = 0;
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
            no_gap: false,
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

    let bounds = moving_bounds(grown_half.face, from, to);
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
                if let Some(reached) = portal_fraction(&portal.points, grown_half.face, from, to) {
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
    best.hit.no_gap = no_gap(half, from, to);
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

/// Les deux boîtes dilatées d'un balayage.
///
/// **Deux et non une, et l'écart entre elles est ce qui permet de longer une
/// paroi.** Voir [`grown`].
#[derive(Debug, Clone, Copy)]
pub(super) struct Grown {
    /// Celle que les faces reçoivent, dilatée de [`SKIN`].
    face: Vec3d,
    /// Celle que les arêtes et les sommets reçoivent, dilatée de la moitié.
    rim: Vec3d,
}

/// Les boîtes dilatées de [`SKIN`], et de sa moitié pour le bord.
///
/// Le facteur porte sur la plus grande demi-étendue et non sur chacune : une
/// boîte plate — un disque, une lame — verrait sinon son épaisseur nulle rester
/// nulle, et rien ne la séparerait jamais du sol qu'elle touche.
///
/// **Le bord reçoit la moitié, et c'est ce qui rend une paroi longeable.** Une
/// face et le prisme de l'arête qui la termine se construisent sur la même
/// demi-étendue : dilatés pareil, leurs plans de support sont **confondus**. Or
/// un balayage pose le mobile exactement là, à `support` de la face — c'est la
/// définition du contact —, si bien qu'il repart tangent au prisme du bout du
/// panneau. Le pas suivant, parallèle à la face, y entre alors par un plan
/// perpendiculaire et s'arrête net, avec une normale orthogonale à celle du mur
/// qu'il longe. C'est le régime de tout personnage qui suit une paroi, à chaque
/// image, et un couloir étroit n'en sort jamais.
///
/// En retranchant la moitié de la marge au bord, la pose du contact se retrouve
/// à une demi-marge **hors** du prisme, ce qui absorbe ce que l'hôte perd en
/// reposant son mobile. Deux termes s'y ajoutent, et le dominant n'est pas celui
/// qu'on croit : la **quantification de la position**, `|p|·2⁻²⁴`, qui ne dépend
/// que de l'éloignement de l'origine, devant l'arrondi de la fraction, qui suit
/// la composante du pas le long de la normale et reste négligeable.
///
/// D'où la borne, mesurée : le jeu tient tant que `half_max > |p| · 2⁻¹³`. **La
/// plus petite boîte utilisable croît donc avec l'éloignement de l'origine**, et
/// c'est une limite de la bibliothèque, pas de ce calcul — à 512 unités, une
/// demi-étendue sous un seizième d'unité n'a plus de jeu garanti.
///
/// **Ce que cela ne coûte pas : la couverture.** Le bord arrête une demi-marge
/// plus tard, donc la **vraie** boîte reste à une demi-marge du solide, jamais
/// dedans. Rien ne fuit, et la marge d'une face — celle que `scg_sweep_skin`
/// rend — ne bouge pas.
///
/// Écartée : la même marge des deux côtés avec une comparaison stricte sur un
/// plan parallèle au mouvement. Elle ne traite que la tangence **exacte**, que
/// l'arrondi de la fraction manque, et la scène d'interrogation l'a refusée —
/// cent trente-deux rayons y traversaient la surface qu'ils touchaient.
pub(super) fn grown(half: Vec3d) -> Grown {
    let largest = max(max(half.x, half.y), half.z);
    let margin = largest * SKIN;
    let rim = margin * 0.5;
    Grown {
        face: Vec3d::new(half.x + margin, half.y + margin, half.z + margin),
        rim: Vec3d::new(half.x + rim, half.y + rim, half.z + rim),
    }
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
        // **Le départ dans le solide absorbe le départage**, et ces deux lignes
        // sont tout ce qui l'en empêchait. Sans elles, seule la fraction du
        // résultat tombait à zéro : les trois critères restaient à leur
        // initialisation, si bien qu'un contact trouvé dans une cellule atteinte
        // ensuite par portail passait pour meilleur et remplaçait la normale de
        // la surface qui pénètre par celle d'un mur plus loin — un hôte qui lit
        // la normale pour se dégager recevait une direction qui ne dégage rien.
        //
        // Zéro et [`RANK_FACE`] sont le plus petit couple des deux premiers
        // critères : plus aucun contact ne les bat, et [`Best::truncate`] ne peut
        // plus reculer en deçà puisqu'elle ne descend jamais sous zéro.
        best.fraction = 0.0;
        best.rank = shape::RANK_FACE;
        best.hit.start_solid = true;
        best.hit.fraction = 0.0;
        best.hit.normal = reported(to_f32(Vec3d::from(cell.inward(surface))).normalize());
        best.hit.point = reported(to_f32(project_on_plane(surface_plane(cell, surface), from)));
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
    half: Grown,
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
        let plane = surface_plane(cell, surface);
        offer(best, touch, cell.id(), surface.id(), plane, from, to);
    }

    // **Un portail non apparié est un mur**, et c'est ce qui garde la cellule
    // fermée : son volume l'est par ses surfaces *et* ses portails, ce que la
    // parité de la localisation compte déjà. Sans eux, un mobile sort du volume
    // par une ouverture inachevée, se retrouve hors de toute cellule, et le
    // balayage suivant le déclare libre faute de cellule de départ — plus rien
    // ne le retient. Conséquence assumée, celle que le format annonce : un mur
    // invisible là où l'éditeur n'a pas fini. L'hôte qui veut laisser tomber
    // marque la surface « non solide », drapeau prévu pour cela.
    //
    // Le filtre ne s'applique pas : il porte sur le drapeau d'une surface, et un
    // portail n'en a pas. Un portail apparié, lui, n'arrête rien — on le franchit,
    // et la cellule d'en face est empilée.
    for portal in &cell.portals {
        if portal.link.is_some() {
            continue;
        }
        // La marge pleine : un portail n'a que sa face, jamais de prisme, donc
        // rien ici ne peut mordre un mobile qui le longe.
        let Some((touch, plane)) = sweep_portal(portal, half.face, from, to) else {
            continue;
        };
        // **Aucune surface n'est nommée**, et c'est le patron de la troncature :
        // le contact est réel, mais rien dans l'espace d'identifiants des
        // surfaces ne le porte. Y mettre l'identifiant du portail ferait trouver
        // une *autre* surface à `scg_world_surface_material`, les deux espaces
        // étant séparés par famille mais pas disjoints en valeur.
        offer(best, touch, cell.id(), 0, plane, from, to);
    }
}

/// Retient un contact s'il précède le meilleur connu, et le décrit.
///
/// **Les trois critères de départage vivent ici, en un seul endroit** : l'instant,
/// puis la famille — face, arête, sommet —, puis l'ordre du fichier, que le
/// parcours donne gratuitement en ne remplaçant jamais à égalité stricte.
/// Recopiés pour les portails, ils auraient fini par diverger de ceux des
/// surfaces, et un départage divergent ne rend pas une erreur : il rend une autre
/// normale.
///
/// `plane` est le couple (normale, point d'ancrage) du **plan** de l'élément
/// touché, qui n'est pas la normale du contact : celle d'une arête ou d'un sommet
/// pointe ailleurs, et c'est bien sur le plan que le point se projette.
fn offer(
    best: &mut Best,
    touch: Touch,
    cell_id: u32,
    surface_id: u32,
    plane: (Vec3d, Vec3d),
    from: Vec3d,
    to: Vec3d,
) {
    let later = touch.fraction > best.fraction;
    let equal_but_coarser = touch.fraction == best.fraction && touch.rank >= best.rank;
    if later || equal_but_coarser {
        return;
    }
    best.fraction = touch.fraction;
    best.rank = touch.rank;
    let centre = from + (to - from) * touch.fraction;
    best.hit.fraction = touch.fraction as f32;
    best.hit.normal = reported(to_f32(touch.normal).normalize());
    best.hit.point = reported(to_f32(project_on_plane(plane, centre)));
    best.hit.surface = surface_id;
    best.hit.cell = cell_id;
}

/// Un vecteur rendu à l'hôte, débarrassé de ses zéros négatifs.
///
/// **Une empreinte hache des motifs de bits**, donc deux résultats
/// mathématiquement égaux dont l'un porte un `-0,0` en rendent deux. La clause
/// est dans `docs/rust.md` et elle tenait par chance : rien n'en produisait,
/// jusqu'à ce qu'un chemin ait à **retourner** un vecteur — nier une composante
/// nulle est précisément d'où un zéro négatif sort.
///
/// `x + 0,0` le ramène à `+0,0` et ne touche aucun autre motif de bits : c'est
/// exact pour toute valeur finie, et le refus des non-finis au chargement les
/// garantit telles. Écrit ici plutôt qu'à chaque appelant, parce que c'est le
/// point par lequel tout vecteur sort du balayage.
fn reported(v: Vec3) -> Vec3 {
    Vec3::new(v.x + 0.0, v.y + 0.0, v.z + 0.0)
}

/// Balaie un portail non apparié : **sa face, et rien d'autre.**
///
/// Ni prisme d'arête ni boîte de sommet, à la différence d'une surface, et ce
/// n'est pas une économie. Les arêtes d'un portail ne passent pas par le
/// classement du chargement, qui n'examine que les surfaces : elles seraient donc
/// toutes tenues pour exposées, et le portail revendiquerait avec le mur voisin le
/// volume le long de leur arête commune. Une boîte qui glisse accrocherait à cette
/// couture — exactement ce que la règle de l'arête partagée existe pour empêcher.
/// La face suffit, les surfaces qui entourent le portail couvrant déjà son bord.
///
/// **Sa normale est celle qui s'oppose au mouvement**, et elle ne peut pas venir
/// d'ailleurs. Le format ne dit pas lequel des deux côtés d'un portail est
/// l'avant : il fixe seulement que les deux portails d'une paire ont des
/// enroulements inverses, si bien qu'un portail et le mur voisin de sa propre
/// cellule peuvent tourner en sens contraires — le décor de validation le fait.
/// Le signe du volume de la cellule n'y change rien, puisqu'il suppose justement
/// l'enroulement cohérent qui manque ici, et une normale prise à l'endroit une
/// fois sur deux laisse le portail ne rien arrêter dans l'autre moitié des cas.
///
/// C'est le traitement que le projet réserve déjà au drapeau « deux faces »,
/// pour la même raison et dans les mêmes mots. Le portail est donc une paroi à
/// double face : on ne le franchit ni dans un sens ni dans l'autre, ce qui est
/// la réponse conservatrice — la seule qui ne perde jamais le mobile.
///
/// Rend le contact et le plan du portail, celui sur lequel le point se projette.
fn sweep_portal(
    portal: &Portal,
    half: Vec3d,
    from: Vec3d,
    to: Vec3d,
) -> Option<(Touch, (Vec3d, Vec3d))> {
    let mut points = heapless::Points::new();
    for point in &portal.points {
        points.push(Vec3d::from(*point));
    }
    // Par comparaison écrite, jamais par `signum` : celui-ci rend −1 sur le zéro
    // négatif, et un mouvement parallèle au plan n'a pas de côté à choisir — il
    // ne touche rien, ce que le cas `d0 == d1` du découpage écarte déjà.
    let raw = Vec3d::from(crate::math::polygon::newell(&portal.points));
    let normal = if raw.dot(to - from) > 0.0 { -raw } else { raw };
    let anchor = *points.first()?;
    let touch = shape::face(&points, normal, anchor, half, from, to)?;
    Some((touch, (normal, anchor)))
}

/// Balaie une surface : sa face, puis ses arêtes exposées, puis ses sommets.
fn sweep_surface(
    cell: &Cell,
    surface: &Surface,
    half: Grown,
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

    keep(shape::face(&points, normal, anchor, half.face, from, to));
    for i in 0..points.len() {
        if !surface.edge_is_exposed(i) {
            continue;
        }
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        keep(shape::edge(a, b, half.rim, from, to));
        keep(shape::vertex(a, half.rim, from, to));
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

/// Le plan d'une surface : sa normale intérieure et un point qui lui appartient.
fn surface_plane(cell: &Cell, surface: &Surface) -> (Vec3d, Vec3d) {
    let normal = Vec3d::from(cell.inward(surface));
    let anchor = Vec3d::from(cell.vertices[surface.first_vertex as usize].position);
    (normal, anchor)
}

/// Le point de contact : la projection du centre de la boîte sur le plan.
///
/// Un contact face contre face est un rectangle et non un point ; le moteur en
/// choisit un et le fige, sans quoi la conformance n'aurait rien à comparer. Un
/// produit scalaire, sans racine.
fn project_on_plane((normal, anchor): (Vec3d, Vec3d), centre: Vec3d) -> Vec3d {
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

/// La valeur absolue, écrite plutôt qu'empruntée à la bibliothèque du système.
///
/// Une seule pour le module : elle vivait en trois exemplaires identiques, ce qui
/// est un de trop pour une expression dont chaque copie est une occasion de la
/// relâcher.
pub(super) fn abs(value: f64) -> f64 {
    if value < 0.0 { -value } else { value }
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
