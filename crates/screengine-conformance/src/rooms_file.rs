// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le décor de validation de la traversée : le second fichier de carte.
//!
//! **Il existe pour les trois propriétés que la feuille de route exige**, et
//! chacune attrape un défaut que les autres laisseraient passer :
//!
//! - **une cellule non convexe** — la salle en L — éprouve la traversée
//!   conservatrice : une branche cache l'autre, ce qui coûte du remplissage et ne
//!   doit rien retirer à l'image ;
//! - **un portail oblique** éprouve la fenêtre là où sa boîte englobante est la
//!   plus lâche. C'est le seul cas où le rectangle sur-estime franchement le
//!   polygone, et c'est pour lui que la réduction découpe le portail avant de le
//!   borner ;
//! - **deux cellules superposées** éprouvent la localisation : le même point au
//!   sol appartient à deux cellules que seule la hauteur sépare, et c'est la
//!   première dans l'ordre du fichier qui gagne.
//!
//! `couloir.world` n'est pas touché : son empreinte reste, et c'est ce qui permet
//! de distinguer une régression du rendu d'une évolution de ce décor-ci.
//!
//! **Les empreintes du rez-de-chaussée gardent des arêtes dont le carré est une
//! puissance de deux**, mais ce n'est plus une obligation : le chargement l'a
//! exigé — le repère de lightmap d'un mur a pour axes l'arête et la verticale —
//! et la clause est tombée, parce qu'elle interdisait toute pente autre que 45°.
//! Ces arêtes-là restent ce qu'elles sont, pour ne pas déplacer des empreintes
//! sans raison.
//!
//! **Le tunnel voûté, lui, est là pour le cas contraire** : ses facettes ont des
//! cordes dont le carré n'est pas une puissance de deux, et leurs normales ne sont
//! ni axiales ni à 45°. C'est la seule part du décor qui éprouve une surface
//! oblique au chargement, à la cuisson et à la traversée.

/// La hauteur du sol des cellules du rez-de-chaussée.
const FLOOR_Z: f32 = 0.0;

/// Celle de leur plafond.
const CEILING_Z: f32 = 4.0;

/// Le sol de la cellule superposée, assez haut pour qu'un plancher les sépare.
const UPPER_FLOOR_Z: f32 = 8.0;

/// Son plafond.
const UPPER_CEILING_Z: f32 = 12.0;

/// Les rangs de matériaux, les densités de plaquage et l'écriture des surfaces
/// sont communs aux trois cartes : voir [`crate::map_bytes`].
use crate::map_bytes::{FLOOR, WALLS, floats, surface, words};

// **Un troisième matériau pour les murs obliques a été essayé, puis retiré.** À la
// densité des autres murs, une surface lointaine voit les cases de son damier
// tomber sous le pixel et le mipmap les moyenne en un aplat : l'idée était de lui
// donner un plaquage quatre fois plus lâche. L'essai a montré que l'aplat vient de
// la **distance** et non de l'obliquité — il persistait au fond des mêmes vues —,
// et que les murs obliques proches devenaient de larges bandes qui ne
// ressemblaient plus au reste du décor. Un matériau de plus pour rien.

/// Une cellule prismatique : une empreinte au sol, deux hauteurs.
///
/// Les sommets sont rangés en deux étages — l'empreinte au sol, puis la même en
/// haut —, ce qui donne à un mur d'arête `i` les quatre indices
/// `i, i+n, (i+1)+n, i+1`.
///
/// `portals` désigne les arêtes qui sont des ouvertures plutôt que des murs. Deux
/// cellules qui se rejoignent y décrivent la même arête avec les mêmes littéraux,
/// donc les mêmes octets : c'est ce que l'appariement exige, et c'est une clause du
/// format qu'un éditeur devrait tenir.
///
/// **Le sol et le plafond restent des surfaces**, même sur une empreinte concave :
/// la découpe d'oreilles du chargement les triangule, et c'est justement ce qu'un
/// décor convexe ne mettrait jamais à l'épreuve.
fn prism(
    id: u32,
    first_id: u32,
    footprint: &[[f32; 2]],
    low: f32,
    high: f32,
    portals: &[usize],
    out: &mut Vec<u8>,
) {
    let n = footprint.len();
    // **Toutes les empreintes tournent dans le même sens**, et c'est ce qui décide
    // de la face visible de chaque surface. Une empreinte à l'envers rend sol,
    // plafond et murs invisibles de l'intérieur : la cellule devient une coquille
    // vue de dehors, et l'image montre un trou noir là où elle devrait être. Vu
    // en écrivant ce décor — le couloir tournait à l'envers, et la vue depuis la
    // salle ne montrait de lui qu'un mur oblique flottant dans le vide.
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
    // Deux surfaces horizontales, plus un mur par arête qui n'est pas un portail.
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

    for z in [low, high] {
        for point in footprint {
            floats(&[point[0], point[1], z], &mut body);
        }
    }

    // **Le sol se parcourt dans l'ordre de l'empreinte, le plafond à l'envers.**
    // Une face retournée disparaît au découpage, et la cellule rend alors une
    // image où son sol manque — vu en écrivant ce décor : la vue depuis le
    // couloir ne peignait que 21 % de l'écran, et le contrôle d'égalité avec le
    // chemin brut divergeait sur trois vues.
    let bottom: Vec<u32> = (0..n as u32).collect();
    surface(
        first_id,
        FLOOR,
        &bottom,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        &mut body,
    );
    let top: Vec<u32> = (n as u32..(n * 2) as u32).rev().collect();
    surface(
        first_id + 1,
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
        // L'axe horizontal du repère est l'arête elle-même : son carré est une
        // puissance de deux par construction de l'empreinte, et il est orthogonal
        // à la verticale comme au plan du mur.
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

/// La demi-largeur du tunnel, en unités de monde : cinq mètres de voie.
const VAULT_HALF: f32 = 2.5;

/// La hauteur des piédroits, où la voûte commence.
const VAULT_SPRING: f32 = 2.0;

/// Où le tunnel passe, à l'écart du reste du décor.
const VAULT_Y: f32 = -12.0;

/// La section du tunnel, dans le plan `XZ` translaté en `VAULT_Y`.
///
/// **Un plein cintre à quatre facettes, et le compte n'est pas indifférent.** Six
/// en placeraient une exactement à 45°, un nombre impair en placerait une à plat
/// au sommet : dans les deux cas le décor retomberait sur les orientations que les
/// autres cellules portent déjà. À quatre, les normales tombent à 22,5°, 67,5°,
/// 112,5° et 157,5° — aucune axiale, aucune à 45° —, et la corde d'une facette a
/// pour carré `2·R²·(1 − cos 45°)`, qui n'est pas une puissance de deux. C'est ce
/// que le chargement refusait avant que la clause tombe.
///
/// Les cotes du cintre portent donc des irrationnels, ce qui est le cas réel d'un
/// décor courbe : `R/√2` pour les deux naissances obliques.
fn section() -> [[f32; 2]; 7] {
    // `2.5 / √2`, écrit en littéral : le noyau n'appelle aucune racine, et un
    // générateur n'a pas à être la première exception.
    const DIAGONAL: f32 = 1.767_767;
    let (w, s) = (VAULT_HALF, VAULT_SPRING);
    [
        [-w, 0.0],
        [w, 0.0],
        [w, s],
        [DIAGONAL, s + DIAGONAL],
        [0.0, s + w],
        [-DIAGONAL, s + DIAGONAL],
        [-w, s],
    ]
}

/// Un tronçon de tunnel voûté, extrudé le long de `X`.
///
/// **Le pendant de [`prism`] pour une section verticale.** Celui-ci extrude une
/// empreinte au sol et ne sait faire que des murs verticaux ; une voûte demande
/// l'inverse — une section dans le plan transversal, tirée le long de l'axe de la
/// voie —, et c'est tout ce qui les distingue.
///
/// `portals` désigne les extrémités ouvertes : `0` pour celle de `x0`, `1` pour
/// celle de `x1`. Une extrémité qui n'y figure pas est un mur, et une qui y figure
/// sans vis-à-vis en est un aussi — c'est le mot du format.
///
/// **L'origine de chaque repère de lightmap est le zéro du monde**, et c'est une
/// obligation et non un choix : le contrôle du chargement exige que le produit
/// scalaire de l'origine par chaque axe soit un entier **exact**, et un coin de
/// facette oblique ne le donne pas — il vaudrait 3,99999976 au lieu de 4 sur des
/// composantes en simple précision. Voir `docs/cartes.md`.
fn vault(id: u32, first_id: u32, x0: f32, x1: f32, portals: &[usize], out: &mut Vec<u8>) {
    let section = section();
    let n = section.len();

    let mut body = Vec::new();
    // Une facette par arête de la section, plus celles des deux extrémités qui ne
    // sont pas des portails.
    let surfaces = n + (2 - portals.len());
    let header = [id, 0, (n * 2) as u32, surfaces as u32, portals.len() as u32];
    words(&header, &mut body);

    // Les sommets : la section à `x0`, puis la même à `x1`.
    for x in [x0, x1] {
        for point in &section {
            floats(&[x, VAULT_Y + point[0], point[1]], &mut body);
        }
    }

    // Les deux extrémités, celles qui ne sont pas des portails. La section tourne
    // dans le sens direct vue depuis `+X`, donc l'extrémité de `x0` se parcourt à
    // l'envers pour que sa normale sorte de la cellule.
    let mut next_id = first_id;
    if !portals.contains(&0) {
        let face: Vec<u32> = (0..n as u32).collect();
        surface(
            next_id,
            WALLS,
            &face,
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            &mut body,
        );
        next_id += 1;
    }
    if !portals.contains(&1) {
        let face: Vec<u32> = (n as u32..(n * 2) as u32).rev().collect();
        surface(
            next_id,
            WALLS,
            &face,
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            &mut body,
        );
        next_id += 1;
    }

    // Une facette par arête de la section. Son repère prend l'axe de la voie et
    // la corde : les deux sont dans le plan de la facette et orthogonaux entre
    // eux, l'axe de la voie étant perpendiculaire au plan de section.
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b) = (section[i], section[j]);
        let chord = [0.0, b[0] - a[0], b[1] - a[1]];
        let material = if i == 0 { FLOOR } else { WALLS };
        // **L'ordre suit celui d'un mur de [`prism`]** : le coin de section à
        // `x0`, le même à `x1`, puis le coin suivant en revenant. L'ordre miroir
        // retourne la facette, et une cellule entière ainsi écrite rend une image
        // **noire** — vu en écrivant ce tunnel, dont la première vue ne peignait
        // pas un pixel.
        surface(
            next_id,
            material,
            &[i as u32, (i + n) as u32, (j + n) as u32, j as u32],
            [1.0, 0.0, 0.0],
            chord,
            &mut body,
        );
        next_id += 1;
    }

    for end in portals {
        let face: Vec<u32> = if *end == 0 {
            (0..n as u32).collect()
        } else {
            (n as u32..(n * 2) as u32).rev().collect()
        };
        words(&[next_id, n as u32], &mut body);
        words(&face, &mut body);
        next_id += 1;
    }

    words(&[body.len() as u32], out);
    out.extend_from_slice(&body);
}

/// Le fichier du décor de validation : quatre cellules.
///
/// La salle en L s'ouvre sur un couloir dont le bout est coupé en biais ; ce biais
/// est le portail oblique, et il donne sur une salle carrée. Une quatrième cellule
/// est posée **au-dessus** de la salle en L, sans lien avec elle : elle n'est jamais
/// visible depuis le rez-de-chaussée — un plancher les sépare, et le tampon de
/// profondeur s'en charge de toute façon —, mais elle occupe la même empreinte au
/// sol, ce qui est tout ce qu'il faut pour éprouver la localisation.
pub fn bytes() -> Vec<u8> {
    let mut cells = Vec::new();

    // La salle en L. Son arête 1 — de (8,0) à (8,4) — est l'ouverture vers le
    // couloir. Les cinq autres sont des murs, dont deux forment le coin rentrant
    // qui rend l'empreinte concave.
    let hall = [
        [0.0f32, 0.0],
        [8.0, 0.0],
        [8.0, 4.0],
        [4.0, 4.0],
        [4.0, 8.0],
        [0.0, 8.0],
    ];
    prism(1, 100, &hall, FLOOR_Z, CEILING_Z, &[1], &mut cells);

    // Le couloir. Son arête 0 reprend l'ouverture de la salle, écrite avec les
    // mêmes littéraux ; son arête 2 est la coupe en biais, de (12,4) à (16,0) —
    // un écart de (4, −4), donc un carré de 32, la diagonale qui rend le portail
    // oblique et son mur éclairable.
    let corridor = [[8.0f32, 0.0], [16.0, 0.0], [12.0, 4.0], [8.0, 4.0]];
    prism(2, 200, &corridor, FLOOR_Z, CEILING_Z, &[1, 3], &mut cells);

    // La salle du bout, un losange dont les quatre arêtes sont obliques. Son arête
    // 3 est celle du couloir, parcourue en sens inverse : mêmes sommets, mêmes
    // octets, enroulement contraire — ce que l'appariement attend.
    let far = [[16.0f32, 0.0], [20.0, 4.0], [16.0, 8.0], [12.0, 4.0]];
    prism(3, 300, &far, FLOOR_Z, CEILING_Z, &[3], &mut cells);

    // L'étage : la même empreinte que la salle en L, huit unités plus haut, et
    // aucun portail. Un portail non apparié serait un mur ; ici il n'y en a aucun,
    // donc la cellule est close et la traversée ne l'atteint jamais.
    prism(
        4,
        400,
        &hall,
        UPPER_FLOOR_Z,
        UPPER_CEILING_Z,
        &[],
        &mut cells,
    );

    // **Deux tronçons de tunnel voûté**, à l'écart et reliés l'un à l'autre par
    // une arche. Ce qu'ils portent et que les quatre autres cellules n'ont pas :
    // des facettes dont aucune n'est axiale ni à 45°, donc des repères de lightmap
    // dont le carré n'est pas une puissance de deux — ce que le chargement
    // refusait. Leur jointure éprouve en plus le classement des arêtes au travers
    // d'un portail, sur un sol que les deux tronçons partagent.
    //
    // **Autonomes, et c'est délibéré** : les relier au décor existant demanderait
    // d'y percer une arche, donc de remanier la salle du bout qui porte le portail
    // oblique et le losange que la conformance éprouve déjà. Un cas nouveau ne
    // paie pas la remise en jeu de trois acquis.
    vault(5, 500, 0.0, 16.0, &[1], &mut cells);
    vault(6, 600, 16.0, 32.0, &[0], &mut cells);

    // **Une lampe devant l'angle rentrant de la salle en L**, et du bon côté des
    // deux murs qui le forment : c'est le seul endroit du décor où l'on voit si
    // l'orientation d'une face entre dans l'éclairage, et sans le terme de Lambert
    // les deux murs recevraient des valeurs que la seule distance explique.
    //
    // Le sommet rentrant est en `(4, 4)` et le creux du L est **dehors** : une
    // lampe posée en `(5, 5)` — ce qu'elle était — a les deux murs dans son dos,
    // Lambert les rejette, et ils restent noirs. Le décor annonçait alors un angle
    // éclairé qu'aucune vue ne montrait.
    let mut lights = Vec::new();
    words(&[1], &mut lights);
    floats(&[3.0, 3.0, 2.0, 20.0], &mut lights);
    lights.extend_from_slice(&[0xFF, 0xE8, 0xC0, 0x00]);
    // Une seconde, plus loin dans le couloir, pour que la cellule voisine ne soit
    // pas noire quand on la regarde par le portail.
    words(&[2], &mut lights);
    floats(&[11.0, 2.0, 2.0, 16.0], &mut lights);
    lights.extend_from_slice(&[0xC0, 0xD0, 0xFF, 0x00]);
    // **Une troisième à l'étage, parce que la cellule superposée est close.** Aucun
    // portail ne la relie au rez-de-chaussée et son plancher est opaque : les deux
    // lampes du bas ne l'atteignent pas, et sans celle-ci la vue qui l'éprouve est
    // une image noire — dont l'empreinte ne distingue plus rien.
    words(&[3], &mut lights);
    floats(&[5.0, 2.0, 10.0, 20.0], &mut lights);
    lights.extend_from_slice(&[0xE0, 0xFF, 0xD0, 0x00]);
    // **Une quatrième au milieu du premier tronçon de tunnel**, sous la voûte :
    // sans elle la cuisson d'une facette oblique ne rendrait que du noir, et
    // l'empreinte ne distinguerait plus un éclairage juste d'un éclairage absent.
    // Posée assez bas pour que les piédroits et les naissances de voûte reçoivent
    // des valeurs franchement différentes — c'est l'écart entre facettes qui dit
    // si leur orientation entre dans le calcul.
    words(&[4], &mut lights);
    floats(&[8.0, VAULT_Y, 1.5, 18.0], &mut lights);
    lights.extend_from_slice(&[0xFF, 0xD8, 0xA8, 0x00]);
    // **Une cinquième dans le second tronçon**, et elle n'est pas décorative : sans
    // elle le fond du tunnel reste noir, donc l'arche qui relie les deux tronçons
    // ne se voit sur aucune image — or c'est elle que la vue existe pour montrer.
    // Vu en regardant la première capture, pas en la figeant.
    words(&[5], &mut lights);
    floats(&[24.0, VAULT_Y, 1.5, 18.0], &mut lights);
    lights.extend_from_slice(&[0xA8, 0xC0, 0xFF, 0x00]);

    let mut materials = Vec::new();
    for (id, name) in [(WALLS, "mur"), (FLOOR, "sol")] {
        words(&[id], &mut materials);
        materials.extend_from_slice(&(name.len() as u16).to_le_bytes());
        materials.extend_from_slice(name.as_bytes());
    }

    file(&cells, &lights, &materials)
}

/// Assemble un fichier de carte à partir de ses deux sections.
fn file(cells: &[u8], lights: &[u8], materials: &[u8]) -> Vec<u8> {
    // Les sections se rangent par genre croissant, ce que l'en-tête exige :
    // `CELL`, `LGTS`, `MATS`.
    let sections = [(*b"CELL", cells), (*b"LGTS", lights), (*b"MATS", materials)];
    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"WRLD");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(sections.len() as u32).to_le_bytes());

    let mut offset = first;
    for (tag, body) in &sections {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        offset += body.len();
    }
    for (_, body) in &sections {
        bytes.extend_from_slice(body);
    }
    bytes
}
