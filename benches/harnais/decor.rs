// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Des cartes engendrées, paramétrées, et légales.
//!
//! **Partagé entre deux bancs plutôt que recopié.** [`chargement`] mesure une
//! pente de décodage, [`cuisson`] une pente de calcul d'éclairage, et les deux
//! ont besoin de la même chose : une carte dont **un seul** paramètre varie.
//! Aucun décor écrit à la main ne donne cela, et deux générateurs divergeraient
//! sur ce qui est délicat ici — la légalité. Les contraintes qu'il respecte sont
//! celles de `docs/cartes.md`, et une erreur du générateur arrête le banc au
//! lieu de rendre un chiffre.
//!
//! [`chargement`]: ../chargement/index.html
//! [`cuisson`]: ../cuisson/index.html
//!
//! # La forme des cartes
//!
//! Une enfilade de cellules prismatiques, chacune reliée à la suivante par un
//! portail — la forme du couloir du dépôt, à trois paramètres près :
//!
//! - **le nombre de cellules**, qui fait varier ce qui est linéaire et ce qui
//!   est global : décodage, triangulation, et l'appariement des portails, qui
//!   compare les portails de la carte entière ;
//! - **le nombre de surfaces par cellule**, obtenu en subdivisant les deux longs
//!   côtés de l'empreinte. C'est lui qui fait travailler les termes quadratiques
//!   — le classement des arêtes au chargement, le rangement de l'atlas à la
//!   cuisson —, et celui qu'aucun décor du dépôt ne pousse : une cellule
//!   d'éditeur en porte cinquante là où les leurs en ont six ;
//! - **les lampes par cellule**, qui ne changent rien au décodage et décident de
//!   tout à la cuisson. À zéro, la carte n'a pas de section de lumières et sort
//!   **octet pour octet** ce que ce générateur rendait avant d'en savoir poser —
//!   c'est ce qui a permis de vérifier que l'extraction ne déplaçait aucune
//!   mesure.
//!
//! Les arêtes sont axiales et longues de huit unités, pour que le carré de
//! chaque axe de repère de lightmap soit une puissance de deux.

/// Le côté d'un segment de mur, en unités de monde.
///
/// Huit, et non une valeur quelconque : le repère de lightmap d'un mur a pour
/// axe horizontal l'arête elle-même, dont le carré doit être une puissance de
/// deux. Huit donne soixante-quatre.
pub const PAS: f32 = 8.0;

/// La hauteur d'une cellule, même contrainte que [`PAS`].
pub const HAUTEUR: f32 = 8.0;

/// La largeur d'une cellule, et donc la largeur d'un portail.
pub const LARGEUR: f32 = 8.0;

/// Le rang du matériau des murs.
const MUR: u32 = 1;

/// Celui du sol et du plafond.
const SOL: u32 = 2;

/// Luxels par unité de monde.
///
/// Un par unité, comme les cartes du dépôt. C'est ce qui garde l'étendue d'un
/// sol sous le plafond de 256 luxels par côté tant que la cellule reste courte.
const LUXEL: f32 = 1.0;

/// Écrit des flottants, octet de poids faible en tête.
fn floats(values: &[f32], out: &mut Vec<u8>) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// Écrit des entiers, octet de poids faible en tête.
fn words(values: &[u32], out: &mut Vec<u8>) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// Écrit une surface : son en-tête, ses indices, puis ses deux repères.
///
/// L'origine des deux repères est celle du monde : elle tombe alors sur un nœud
/// de sa propre grille sans aucun calcul, ce que le chargement exige.
fn surface(id: u32, material: u32, indices: &[u32], u: [f32; 3], v: [f32; 3], out: &mut Vec<u8>) {
    words(&[id, 0, material, indices.len() as u32], out);
    words(indices, out);
    // Le repère de texture prend la même densité que celui de lightmap : ces
    // bancs ne rendent aucune image, et seule la lightmap porte des contraintes.
    for echelle in [LUXEL, LUXEL] {
        floats(&[0.0, 0.0, 0.0], out);
        floats(&[u[0] * echelle, u[1] * echelle, u[2] * echelle], out);
        floats(&[v[0] * echelle, v[1] * echelle, v[2] * echelle], out);
    }
}

/// L'empreinte au sol d'une cellule, en tournant dans le sens direct.
///
/// Un rectangle dont les deux longs côtés sont découpés en `segments` arêtes de
/// [`PAS`] unités, les deux petits restant d'un seul tenant — ce sont eux qui
/// deviennent des portails. L'aire signée est positive, ce qui oriente toutes
/// les surfaces de la cellule d'un coup.
fn empreinte(debut: f32, segments: u32) -> Vec<[f32; 2]> {
    let mut points = Vec::new();
    for i in 0..segments {
        points.push([debut + PAS * i as f32, 0.0]);
    }
    let fin = debut + PAS * segments as f32;
    for i in 0..segments {
        points.push([fin - PAS * i as f32, LARGEUR]);
    }
    points
}

/// Une cellule prismatique, écrite dans `out`.
///
/// Les sommets sont rangés en deux étages, l'empreinte au sol puis la même en
/// haut, si bien qu'un mur d'arête `i` porte les indices `i, i+n, (i+1)+n, i+1`.
/// Le sol se parcourt dans l'ordre de l'empreinte et le plafond à l'envers :
/// une face retournée disparaîtrait au découpage.
fn cellule(id: u32, premier_id: u32, debut: f32, segments: u32, out: &mut Vec<u8>) {
    let sol = empreinte(debut, segments);
    let n = sol.len();
    // Les deux arêtes qui ferment le rectangle sont les portails : celle qui
    // joint le dernier point du bord bas au premier du bord haut, et l'inverse.
    let portails = [segments as usize - 1, 2 * segments as usize - 1];

    let mut corps = Vec::new();
    let murs = n - portails.len();
    words(
        &[
            id,
            0,
            (n * 2) as u32,
            (murs + 2) as u32,
            portails.len() as u32,
        ],
        &mut corps,
    );

    for z in [0.0, HAUTEUR] {
        for point in &sol {
            floats(&[point[0], point[1], z], &mut corps);
        }
    }

    let bas: Vec<u32> = (0..n as u32).collect();
    surface(
        premier_id,
        SOL,
        &bas,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        &mut corps,
    );
    let haut: Vec<u32> = (n as u32..(n * 2) as u32).rev().collect();
    surface(
        premier_id + 1,
        SOL,
        &haut,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        &mut corps,
    );

    let mut suivant = premier_id + 2;
    for i in 0..n {
        if portails.contains(&i) {
            continue;
        }
        let j = (i + 1) % n;
        let (a, b) = (sol[i], sol[j]);
        let le_long = [b[0] - a[0], b[1] - a[1], 0.0];
        surface(
            suivant,
            MUR,
            &[i as u32, (i + n) as u32, (j + n) as u32, j as u32],
            le_long,
            [0.0, 0.0, 1.0],
            &mut corps,
        );
        suivant += 1;
    }

    for i in portails {
        let j = (i + 1) % n;
        words(&[suivant, 4], &mut corps);
        words(
            &[i as u32, j as u32, (j + n) as u32, (i + n) as u32],
            &mut corps,
        );
        suivant += 1;
    }

    words(&[corps.len() as u32], out);
    out.extend_from_slice(&corps);
}

/// Les lampes d'une cellule, réparties sur sa longueur.
///
/// **Le rayon couvre la cellule et pas la carte**, et c'est ce qui rend la pente
/// lisible : la cuisson retient une lumière dont la sphère coupe la boîte de la
/// cellule, si bien qu'un rayon généreux ferait croître le nombre de lampes
/// retenues avec la taille du décor — deux paramètres variant d'un coup, ce que
/// ces bancs existent pour éviter. Il déborde sur les deux voisines immédiates,
/// qui sont justement l'ensemble d'occulteurs du calcul.
fn lampes(premier_id: u32, debut: f32, segments: u32, par_cellule: u32, out: &mut Vec<u8>) {
    let longueur = PAS * segments as f32;
    for i in 0..par_cellule {
        // Réparties à intervalles réguliers, jamais toutes au même point : des
        // lampes confondues donneraient le même luxel pour chacune, et le rejet
        // par la distance les traiterait toutes de la même façon.
        let x = debut + longueur * (i as f32 + 0.5) / par_cellule as f32;
        words(&[premier_id + i], out);
        floats(&[x, LARGEUR / 2.0, HAUTEUR / 2.0, longueur], out);
        out.extend_from_slice(&[0xFF, 0xC0, 0x90, 0x00]);
    }
}

/// Les octets d'une carte de `cellules` cellules à `segments` segments par long
/// côté, avec `par_cellule` lampes dans chacune.
///
/// Les cellules se suivent le long de l'axe X, et deux voisines décrivent leur
/// arête commune avec **les mêmes littéraux** : c'est ce que l'appariement
/// exige, puisqu'il compare les positions au bit près.
pub fn carte(cellules: u32, segments: u32, par_cellule: u32) -> Vec<u8> {
    let mut cells = Vec::new();
    // Pas de compte en tête : le décodeur dérive celui des lumières de la
    // longueur de leur section, comme pour tout compte du format. Un mot de
    // compte écrit ici décalerait chaque entrée d'un champ, et le refus tombe
    // alors sur l'octet réservé d'une lumière — sans rien dire du décalage.
    let mut lgts = Vec::new();
    for i in 0..cellules {
        let debut = PAS * segments as f32 * i as f32;
        cellule(i + 1, 1000 * (i + 1), debut, segments, &mut cells);
        lampes(1000 * (i + 1), debut, segments, par_cellule, &mut lgts);
    }

    let mut materiaux = Vec::new();
    for (id, nom) in [(MUR, "mur"), (SOL, "sol")] {
        words(&[id], &mut materiaux);
        materiaux.extend_from_slice(&(nom.len() as u16).to_le_bytes());
        materiaux.extend_from_slice(nom.as_bytes());
    }

    // Jamais d'entités : aucun des deux bancs ne traverse leur chemin, et une
    // section vide est refusée par le format.
    let toutes: [([u8; 4], &Vec<u8>); 3] = [
        (*b"CELL", &cells),
        (*b"LGTS", &lgts),
        (*b"MATS", &materiaux),
    ];
    let sections: Vec<([u8; 4], &Vec<u8>)> = toutes
        .into_iter()
        .filter(|(_, corps)| !corps.is_empty())
        .collect();
    let premier = 20 + 12 * sections.len();
    let total = premier + sections.iter().map(|(_, c)| c.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"WRLD");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(sections.len() as u32).to_le_bytes());

    let mut decalage = premier;
    for (tag, corps) in &sections {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(decalage as u32).to_le_bytes());
        bytes.extend_from_slice(&(corps.len() as u32).to_le_bytes());
        decalage += corps.len();
    }
    for (_, corps) in &sections {
        bytes.extend_from_slice(corps);
    }
    bytes
}

/// Combien de surfaces porte une cellule de `segments` segments.
///
/// Deux longs côtés subdivisés, plus le sol et le plafond ; les deux petits
/// côtés sont des portails et ne comptent pas comme surfaces. Dérivé ici plutôt
/// que recalculé par chaque banc, qui s'en servent tous deux pour étiqueter
/// leurs lignes.
pub fn surfaces(segments: u32) -> u32 {
    2 * segments + 2
}
