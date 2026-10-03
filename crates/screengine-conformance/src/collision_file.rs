// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le décor de validation du balayage : trois cellules, neuf cas.
//!
//! **Une carte à part plutôt que des cas ajoutés à `salles.world`.** Celle-ci a
//! pour métier de figer une image, et chaque cas de collision qu'on y ajouterait
//! traînerait derrière lui une mise à jour de référence — alors que les cas
//! voulus ici sont délibérément dégénérés : un passage exactement à la largeur
//! d'une boîte, un portail qui ne mène nulle part, une surface qui n'arrête rien.
//! Les deux points de gel restent ainsi découplés.
//!
//! Les onze cas, et ce que chacun éprouve que les autres n'éprouvent pas :
//!
//! - **la cage d'escalier**, douze marches empilées : **douze concavités** et un
//!   creux qui court sur deux étages, là où la cellule en U n'en referme qu'une.
//!   C'est la couverture d'une classe de décors qu'un intégrateur emploiera — un
//!   immeuble, un garage, des salles reliées par des escaliers — et son plafond
//!   suit la pente, ce qui lui donne une **normale de contact non axiale** que
//!   seule la rampe portait. Un départ par marche, puisque le treillis tire ses
//!   cotes entre le sol et le plafond des cellules basses et ne monterait pas ;
//!
//! - **la distance à l'origine du monde**, et c'est le seul cas qui soit une
//!   propriété du **placement** du décor plutôt que de sa forme : il est posé à
//!   [`ORIGIN_X`] et non en zéro. Le signe du volume d'une cellule se calculait
//!   par une somme dont le résidu croît avec la distance, si bien qu'une cellule
//!   éloignée retournait toutes ses normales et n'arrêtait plus rien — en zéro,
//!   ce décor restait vert, et c'est un intégrateur qui l'a trouvé sur
//!   soixante-quatre unités de côté. Vérifié en rétablissant le défaut : les deux
//!   empreintes divergent désormais, là où elles ne bougeaient pas ;
//!
//! - **les coins rentrants** de la salle en L — cinq angles droits vus de
//!   l'intérieur : leur arête partagée est éteinte au chargement, et une boîte
//!   qui glisse le long d'un mur ne doit pas y accrocher ;
//! - **le coin saillant**, l'angle de 270° que le L dessine en `(4, 4)` : son
//!   arête garde son prisme, sans quoi une boîte passerait au voisinage de
//!   l'angle ;
//! - **le portail apparié** entre les deux cellules, franchi d'un seul balayage :
//!   c'est lui qui met la traversée à l'épreuve, et l'égalité avec le chemin de
//!   force brute avec elle ;
//! - **le portail non apparié**, sur l'arête `(4, 8)`–`(0, 8)` : il est **solide**,
//!   c'est le mot du format, et c'est ce qui garde la cellule fermée ;
//! - **la surface non solide**, le plafond du couloir : le premier lecteur du
//!   drapeau, qui l'exclut du balayage et de rien d'autre ;
//! - **le passage exactement à la largeur d'une boîte** : le couloir fait une
//!   unité de large, donc exactement le côté de la boîte d'épreuve. C'est le cas
//!   qui rend la constante de dilatation **mesurable** au lieu d'arbitraire — une
//!   boîte qui n'y passe plus dit que la marge est trop grande ;
//! - **le passage plus étroit que la boîte**, qui est le même couloir balayé avec
//!   une boîte plus grande : il doit refuser franchement, jamais laisser passer ;
//! - **le départ derrière le plan d'une face de sa propre cellule**, que la
//!   cellule en U porte et que ni un convexe ni un L ne peuvent porter : le
//!   contact immédiat d'un instant d'impact négatif doit s'arrêter à la dalle
//!   dilatée, et au-delà il n'y a pas de contact. Voir [`BRANCHES`] ;
//! - **une cellule que la traversée ne visite pas**, la même : sans portail et
//!   loin des deux autres, elle est la seule part du décor où le chemin de force
//!   brute regarde une géométrie que la traversée ignore. C'est là qu'un faux
//!   contact les fait diverger plutôt que de les tromper ensemble.
//!
//! **Les deux portails s'apparient au bit près**, ce qui décide de la géométrie :
//! la salle porte deux sommets colinéaires sur son mur de droite, en `y = 2` et
//! `y = 3`, pour que l'arête entre eux soit exactement celle du couloir. Sans
//! eux, les deux portails n'auraient aucun sommet commun et le chargement en
//! ferait deux murs — la traversée ne franchirait rien, et la scène éprouverait
//! le contraire de ce qu'elle annonce.
//!
//! **Toutes les arêtes ont une longueur puissance de deux**, et ce n'est pas un
//! choix esthétique : le repère de lightmap d'un mur prend l'arête pour axe
//! horizontal, et le chargement exige que le carré de cet axe soit une puissance
//! de deux — faute de quoi la reconstruction d'un luxel demanderait une division.
//! Un décor qui ne se dessine pas en a besoin quand même : le format ne connaît
//! pas la différence, et une arête de 1,5 unité fait refuser la carte entière.

use crate::map_bytes::{FLOOR, WALLS, flagged, floats, surface, words};

/// Le sol des deux cellules.
pub const FLOOR_Z: f32 = 0.0;

/// Leur plafond.
pub const CEILING_Z: f32 = 4.0;

/// Le bit qui dit qu'une surface n'arrête aucun volume qui la balaie.
const NON_SOLID: u32 = 0b100;

/// De combien le décor est posé loin de l'origine du monde, sur `X`.
///
/// **Un décor à l'origine ne voit pas ce que la distance révèle**, et ce décor
/// l'a montré deux fois : le signe du volume d'une cellule se calculait par une
/// somme dont le résidu croît avec la distance, si bien qu'une cellule éloignée
/// retournait toutes ses normales et n'arrêtait plus rien. Posé en zéro, ce décor
/// restait vert — c'est l'intégrateur, sur soixante-quatre unités de côté, qui
/// l'a trouvé. La distance est donc une propriété du décor, pas un détail de
/// placement.
///
/// **Sur `X` seulement.** La rampe calcule la cote de son sol sur sa coordonnée
/// `Y`, donc un décalage en `Y` ferait monter son sol d'autant. `X` porte la
/// distance sans toucher la pente, et la somme des normales du décor y a une
/// composante non nulle — c'est elle que le résidu amplifie.
///
/// **Une puissance de deux, entière**, pour deux raisons : la translation est
/// alors exacte, donc les portails continuent de s'apparier au bit près ; et
/// l'origine des repères de lightmap, qui est le zéro du monde, reste un nœud de
/// leur grille — une translation fractionnaire ferait refuser la carte.
pub const ORIGIN_X: f32 = 512.0;

/// Une empreinte posée à [`ORIGIN_X`].
///
/// Les empreintes s'écrivent en coordonnées locales, qui se lisent, et la
/// translation s'applique ici : elles sont lues par le générateur du décor **et**
/// par le treillis de départs, et deux décalages à tenir accordés auraient fini
/// par diverger.
const fn placed<const N: usize>(mut points: [[f32; 2]; N]) -> [[f32; 2]; N] {
    let mut i = 0;
    while i < N {
        points[i][0] += ORIGIN_X;
        i += 1;
    }
    points
}

/// L'empreinte de la salle en L.
///
/// Les sommets 2 et 3 sont colinéaires avec 1 et 4 : ils ne dessinent aucun coin
/// et n'existent que pour porter l'arête du portail. Un polygone a le droit
/// d'avoir des sommets alignés, et la triangulation par découpe d'oreilles les
/// traverse sans rien en faire.
pub const ROOM: [[f32; 2]; 8] = placed([
    [0.0, 0.0],
    [8.0, 0.0],
    [8.0, 2.0],
    [8.0, 3.0],
    [8.0, 4.0],
    [4.0, 4.0],
    [4.0, 8.0],
    [0.0, 8.0],
]);

/// L'empreinte du couloir étroit, large d'une unité.
pub const CORRIDOR: [[f32; 2]; 4] = placed([[8.0, 2.0], [16.0, 2.0], [16.0, 3.0], [8.0, 3.0]]);

/// L'empreinte de la cellule en U : une base et deux branches.
///
/// **Ce qu'un L ne peut pas porter.** Le volume dilaté d'une face est une dalle
/// autour de son plan, et le balayage doit refuser un contact immédiat au départ
/// qui tombe au-delà d'elle. Encore faut-il un départ qui y tombe : il lui faut
/// être derrière le plan d'une face de sa propre cellule **et** s'y projeter
/// dedans. Aucun point intérieur d'un convexe ne l'est, et aucun point de la
/// salle en L non plus — la région derrière le plan d'une de ses faces est
/// précisément son quart manquant. Un U l'a : un point de la branche gauche est
/// à quatre unités derrière le plan de la face intérieure de la branche droite,
/// et se projette en plein milieu d'elle.
///
/// **Elle est loin des deux autres et n'a aucun portail**, et les deux tiennent
/// à la même raison : le chemin de force brute la voit, la traversée ne la
/// visite jamais depuis la salle. C'est la seule configuration du décor où les
/// deux chemins ne regardent pas la même géométrie, et c'est celle où un faux
/// contact les fait **diverger** au lieu de les tromper ensemble — le reste du
/// décor étant d'un seul tenant, une formule fausse y restait invisible.
///
/// Les huit arêtes mesurent 8, 8, 2, 4, 4, 4, 2 et 8 unités : le carré de
/// chacune est une puissance de deux, ce que le repère de lightmap exige.
pub const BRANCHES: [[f32; 2]; 8] = placed([
    [0.0, 16.0],
    [8.0, 16.0],
    [8.0, 24.0],
    [6.0, 24.0],
    [6.0, 20.0],
    [2.0, 20.0],
    [2.0, 24.0],
    [0.0, 24.0],
]);

/// Le rang de l'arête de la salle qui porte le portail vers le couloir.
const ROOM_PORTAL: usize = 2;

/// Celui de l'arête qui porte un portail **sans vis-à-vis**.
const ROOM_DEAD_PORTAL: usize = 6;

/// Celui de l'arête du couloir qui rejoint la salle.
const CORRIDOR_PORTAL: usize = 3;

/// L'empreinte au sol de la rampe, à l'écart des trois autres cellules.
///
/// **Une face oblique, que rien d'autre dans ce dépôt ne balayait.** Le test
/// d'appartenance d'une face se faisait au centre de la boîte au lieu du point de
/// la facette : exact tant que la normale est axiale — ce que toutes les autres
/// surfaces du décor sont —, faux dès qu'elle ne l'est pas. Le défaut étant dans
/// la formule que les deux chemins partagent, l'égalité d'oracle ne le voyait pas ;
/// seule une empreinte peut le dire, et seulement si un départ tombe dans la bande
/// fautive, d'où les départs choisis de la scène.
pub const RAMP: [[f32; 2]; 4] = placed([[24.0, 0.0], [32.0, 0.0], [32.0, 8.0], [24.0, 8.0]]);

/// La pente de la rampe : son sol monte d'une unité par unité le long de `Y`.
///
/// **45° n'est pas un choix esthétique, c'est la seule pente chargeable** : l'axe
/// de pente du repère de lightmap vaut alors `(0, 1, 1)`, de carré 2, et le
/// chargement exige une puissance de deux. Une pente 1:2 ferait refuser le fichier.
pub const RAMP_RISE: f32 = 1.0;

/// Les octets du décor.
pub fn bytes() -> Vec<u8> {
    let mut cells = Vec::new();
    prism(
        7,
        100,
        &ROOM,
        &[ROOM_PORTAL, ROOM_DEAD_PORTAL],
        false,
        &mut cells,
    );
    prism(8, 200, &CORRIDOR, &[CORRIDOR_PORTAL], true, &mut cells);
    prism(9, 300, &BRANCHES, &[], false, &mut cells);
    ramp(10, 400, &mut cells);
    stairwell(11, 500, &mut cells);

    let mut materials = Vec::new();
    for (id, name) in [(1u32, "mur"), (2, "sol")] {
        words(&[id], &mut materials);
        materials.extend_from_slice(&(name.len() as u16).to_le_bytes());
        materials.extend_from_slice(name.as_bytes());
    }

    file(&cells, &materials)
}

/// Combien de marches la cage d'escalier empile.
///
/// **Douze, parce que ce sont douze concavités** : chaque nez de marche en
/// referme une, et c'est le nombre qui distingue cette cellule de la cellule en U,
/// qui n'en a qu'une. Douze marches d'une unité montent de douze unités, soit deux
/// étages d'un décor de cette échelle — le creux court donc sur deux niveaux, ce
/// qu'aucune autre cellule du décor ne fait.
pub const STEPS: usize = 12;

/// La hauteur libre au-dessus des marches, en unités de monde.
const HEADROOM: f32 = 4.0;

/// Où la cage commence, en `Y`, et sa largeur.
const STAIR_Y: [f32; 2] = [32.0, 36.0];

/// Le milieu de la cage en `Y`, où ses départs se posent.
///
/// Lu ici et non recopié par la liste des balayages : les deux dériveraient, et
/// un départ hors de la cage ne rougirait pas — il rendrait simplement « aucune
/// cellule », ce qui est une réponse valide.
pub fn stair_middle() -> f32 {
    (STAIR_Y[0] + STAIR_Y[1]) * 0.5
}

/// Le profil de la cage d'escalier, dans le plan `XZ`.
///
/// **Un profil vertical extrudé, là où les autres cellules sont des empreintes
/// horizontales extrudées** : c'est l'inverse de [`prism`], et c'est la seule
/// façon d'obtenir des marches. Le profil monte en marches de `(0, 0)` à
/// `(12, 12)`, remonte le long du mur du fond, puis **redescend par un plafond
/// oblique** parallèle à l'escalier — ce qui donne à la cellule sa surface non
/// axiale, le cas que la rampe d'un garage réclame.
///
/// Vingt-sept points, donc vingt-sept faces latérales plus les deux flancs :
/// **vingt-neuf surfaces**.
fn profile() -> Vec<[f32; 2]> {
    let mut points = Vec::new();
    points.push([0.0, 0.0]);
    for step in 0..STEPS {
        let x = step as f32;
        points.push([x + 1.0, x]);
        points.push([x + 1.0, x + 1.0]);
    }
    let top = STEPS as f32;
    points.push([top, top + HEADROOM]);
    points.push([0.0, HEADROOM]);
    points
}

/// La cage d'escalier : le profil en marches, extrudé le long de `Y`.
///
/// **Ce qu'elle apporte et qu'aucune autre cellule du décor n'apporte** : douze
/// concavités empilées, un creux qui court sur deux étages, et vingt-neuf
/// surfaces. La cellule en U referme une seule concavité, et la rampe n'a qu'une
/// pente sans marche. Ce n'est la réponse à aucun défaut connu : c'est la
/// couverture d'une classe de décors — un immeuble, un garage, des salles reliées
/// par des escaliers — qu'un intégrateur emploiera et que rien n'éprouvait.
///
/// **Sans portail et à l'écart**, comme la cellule en U : le chemin de force
/// brute la voit, la traversée ne l'atteint jamais. C'est la configuration où un
/// faux contact fait **diverger** les deux chemins au lieu de les tromper
/// ensemble.
///
/// Les repères suivent chaque face : l'axe d'une face latérale est la direction
/// de son arête ramenée à sa plus grande composante — exacte, donc `(1, 0, 1)`
/// pour le plafond oblique, de carré 2, ce que le chargement accepte depuis qu'il
/// n'exige plus de puissance de deux.
fn stairwell(id: u32, first_id: u32, out: &mut Vec<u8>) {
    let profile = profile();
    let n = profile.len();
    let mut body = Vec::new();
    words(&[id, 0, (n * 2) as u32, (n + 2) as u32, 0], &mut body);

    // Les points du profil à chaque flanc : `Y` est la direction d'extrusion,
    // donc ce que la hauteur est à un prisme.
    for y in STAIR_Y {
        for point in &profile {
            floats(&[ORIGIN_X + point[0], y, FLOOR_Z + point[1]], &mut body);
        }
    }

    // Les deux flancs, dans le plan `XZ` : le premier parcourt le profil dans
    // l'ordre, le second à l'envers, de sorte que leurs normales sortent toutes
    // deux de la cellule — le chargement les retourne ensemble s'il le faut.
    let near: Vec<u32> = (0..n as u32).collect();
    let far: Vec<u32> = (0..n as u32).rev().map(|i| i + n as u32).collect();
    surface(
        first_id,
        WALLS,
        &near,
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        &mut body,
    );
    surface(
        first_id + 1,
        WALLS,
        &far,
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        &mut body,
    );

    // Une face par arête du profil : plat de marche, contremarche, mur du fond,
    // plafond oblique, mur de départ.
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b) = (profile[i], profile[j]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        // Ramenée à sa plus grande composante : exacte, et elle rend `(1, 0, 1)`
        // sur le plafond oblique plutôt qu'un axe de douze unités de long.
        let span = if dx.abs() > dz.abs() {
            dx.abs()
        } else {
            dz.abs()
        };
        let along = [dx / span, 0.0, dz / span];
        // Le plat d'une marche porte le matériau du sol, tout le reste celui des
        // murs : c'est ce qui donne à la cage deux densités de plaquage, comme le
        // reste du décor.
        let material = if dz == 0.0 { FLOOR } else { WALLS };
        surface(
            first_id + 2 + i as u32,
            material,
            &[i as u32, j as u32, (j + n) as u32, (i + n) as u32],
            along,
            [0.0, 1.0, 0.0],
            &mut body,
        );
    }

    words(&[body.len() as u32], out);
    out.extend_from_slice(&body);
}

/// La cellule à sol oblique : [`RAMP`] extrudée le long d'une pente.
///
/// **Pas un prisme, et c'est pour cela qu'elle a sa fonction** : son sol et son
/// plafond montent avec `Y`, donc leurs normales ne sont alignées sur aucun axe.
/// Les deux flancs gardent leurs repères dans leur plan en prenant `(0, 1, 1)` et
/// `(0, −1, 1)` — orthogonaux, de carré 2 chacun, ce que le chargement exige.
///
/// La hauteur est celle des autres cellules, de sorte que le treillis de départs
/// les traite de la même façon.
fn ramp(id: u32, first_id: u32, out: &mut Vec<u8>) {
    let height = CEILING_Z - FLOOR_Z;
    let mut body = Vec::new();
    words(&[id, 0, 8, 6, 0], &mut body);

    // Les quatre coins au sol, puis les quatre au plafond : la cote monte de
    // `RAMP_RISE` par unité de `Y`, et le plafond suit à hauteur constante.
    for lift in [0.0, height] {
        for point in &RAMP {
            floats(
                &[point[0], point[1], FLOOR_Z + point[1] * RAMP_RISE + lift],
                &mut body,
            );
        }
    }

    // Le sol suit l'empreinte, le plafond la parcourt à l'envers : les deux
    // normales sortent de la cellule, et le chargement les retourne ensemble.
    let slope = [0.0, 1.0, 1.0];
    let across = [1.0, 0.0, 0.0];
    surface(first_id, FLOOR, &[0, 1, 2, 3], across, slope, &mut body);
    surface(
        first_id + 1,
        FLOOR,
        &[7, 6, 5, 4],
        [1.0, 0.0, 0.0],
        slope,
        &mut body,
    );

    // Les deux bouts, perpendiculaires à `Y`, puis les deux flancs obliques.
    let up = [0.0, 0.0, 1.0];
    surface(
        first_id + 2,
        WALLS,
        &[0, 4, 5, 1],
        [1.0, 0.0, 0.0],
        up,
        &mut body,
    );
    surface(
        first_id + 3,
        WALLS,
        &[2, 6, 7, 3],
        [1.0, 0.0, 0.0],
        up,
        &mut body,
    );
    let down_slope = [0.0, -1.0, 1.0];
    surface(
        first_id + 4,
        WALLS,
        &[3, 7, 4, 0],
        slope,
        down_slope,
        &mut body,
    );
    surface(
        first_id + 5,
        WALLS,
        &[1, 5, 6, 2],
        slope,
        down_slope,
        &mut body,
    );

    words(&[body.len() as u32], out);
    out.extend_from_slice(&body);
}

/// Une cellule prismatique : une empreinte au sol, deux hauteurs.
///
/// `portals` désigne les arêtes qui sont des ouvertures plutôt que des murs, et
/// `open_ceiling` pose le drapeau « non solide » sur le plafond.
fn prism(
    id: u32,
    first_id: u32,
    footprint: &[[f32; 2]],
    portals: &[usize],
    open_ceiling: bool,
    out: &mut Vec<u8>,
) {
    let n = footprint.len();
    // **Toutes les empreintes tournent dans le même sens.** Une empreinte à
    // l'envers rend les normales sortantes du mauvais côté, et le balayage
    // n'arrêterait plus rien de l'intérieur — c'est l'erreur qui ne se voit pas
    // sur une carte qui ne se dessine pas.
    let twice_area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (footprint[i], footprint[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    assert!(
        twice_area > 0.0,
        "l'empreinte tourne à l'envers : aire signée {twice_area}"
    );

    let mut body = Vec::new();
    let walls = n - portals.len();
    words(
        &[
            id,
            0,
            (n * 2) as u32,
            (walls + 2) as u32,
            portals.len() as u32,
        ],
        &mut body,
    );

    for z in [FLOOR_Z, CEILING_Z] {
        for point in footprint {
            floats(&[point[0], point[1], z], &mut body);
        }
    }

    // Le sol suit l'empreinte, le plafond la parcourt à l'envers : les deux
    // normales sortent alors de la cellule, et le chargement les retourne
    // ensemble vers l'intérieur.
    let bottom: Vec<u32> = (0..n as u32).collect();
    surface(
        first_id,
        FLOOR,
        &bottom,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        &mut body,
    );
    let top: Vec<u32> = (0..n as u32).rev().map(|i| i + n as u32).collect();
    flagged(
        first_id + 1,
        if open_ceiling { NON_SOLID } else { 0 },
        FLOOR,
        &top,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        &mut body,
    );

    let mut next_id = first_id + 2;
    for i in 0..n {
        if portals.contains(&i) {
            continue;
        }
        let j = (i + 1) % n;
        let (a, b) = (footprint[i], footprint[j]);
        let along = [b[0] - a[0], b[1] - a[1], 0.0];
        surface(
            next_id,
            WALLS,
            &[i as u32, (i + n) as u32, (j + n) as u32, j as u32],
            along,
            [0.0, 0.0, 1.0],
            &mut body,
        );
        next_id += 1;
    }

    for i in portals {
        let j = (i + 1) % n;
        words(&[next_id, 4], &mut body);
        words(
            &[*i as u32, j as u32, (j + n) as u32, (i + n) as u32],
            &mut body,
        );
        next_id += 1;
    }

    words(&[body.len() as u32], out);
    out.extend_from_slice(&body);
}

/// Le conteneur : en-tête, table de deux sections, puis les sections.
fn file(cells: &[u8], materials: &[u8]) -> Vec<u8> {
    let header = 20 + 2 * 12;
    let total = header + cells.len() + materials.len();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"WRLD");
    words(&[1, total as u32, 2], &mut bytes);

    // Par genre croissant, ce que le décodeur vérifie avant de lire un champ.
    bytes.extend_from_slice(b"CELL");
    words(&[header as u32, cells.len() as u32], &mut bytes);
    bytes.extend_from_slice(b"MATS");
    words(
        &[(header + cells.len()) as u32, materials.len() as u32],
        &mut bytes,
    );

    bytes.extend_from_slice(cells);
    bytes.extend_from_slice(materials);
    bytes
}
