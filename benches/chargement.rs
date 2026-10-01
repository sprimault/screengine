// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comment le coût de chargement d'une carte croît, et ce que cette pente
//! décide.
//!
//! **La question n'est pas « combien coûte une carte », c'est « que coûte la
//! suivante ».** [`carte`](../carte/index.html) a mesuré les deux décors du
//! dépôt — trois et sept microsecondes — et cette mesure ne se prolonge pas :
//! leurs cellules ont six surfaces, alors que le classement des arêtes
//! partagées est **quadratique en surfaces d'une même cellule** et que
//! l'appariement des portails est global à la carte. Le terme qui domine à une
//! échelle n'est pas celui qui domine à l'autre, et il n'existe aucune carte
//! dans le dépôt qui le montre.
//!
//! **Les cartes de ce banc sont donc engendrées**, et c'est la seule façon
//! d'obtenir une pente : deux points de mesure doivent différer d'un **seul**
//! paramètre, ce qu'aucune paire de décors écrits à la main ne donne.
//! L'objection de [`carte`](../carte/index.html) — « un banc qui construirait sa
//! carte mesurerait un encodeur écrit pour lui » — ne porte pas ici :
//! l'encodeur tourne **hors du chronomètre**, qui ne voit que [`World::load`].
//! Ce que la carte engendrée doit être, en revanche, c'est **légale** : elle
//! passe les mêmes contrôles que n'importe quelle autre, et une erreur du
//! générateur arrête le banc au lieu de rendre un chiffre.
//!
//! Le harnais et ce que ces chiffres valent sont dans
//! [`remplissage`](../remplissage/index.html) : aucun seuil, jamais d'échec sur
//! une durée, le minimum plutôt que la moyenne.
//!
//! # La forme des cartes engendrées
//!
//! Une enfilade de cellules prismatiques, chacune reliée à la suivante par un
//! portail — la forme du couloir du dépôt, à deux paramètres près :
//!
//! - **le nombre de cellules**, qui fait varier ce qui est linéaire et ce qui
//!   est global : décodage, triangulation, et l'appariement des portails, qui
//!   compare les portails de la carte entière ;
//! - **le nombre de surfaces par cellule**, obtenu en subdivisant les deux longs
//!   côtés de l'empreinte. C'est lui qui fait travailler le terme quadratique,
//!   et c'est celui qu'aucun décor du dépôt ne pousse — une cellule d'éditeur en
//!   porte cinquante là où les leurs en ont six.
//!
//! Les arêtes sont axiales et longues de huit unités, pour que le carré de
//! chaque axe de repère de lightmap soit une puissance de deux : sans cela le
//! chargement refuse la carte, et le banc ne mesurerait rien. Les contraintes
//! que ce générateur respecte sont celles de `docs/cartes.md`.
//!
//! # La mesure, prise le 2026-10-01
//!
//! ```text
//!    25 cellules ×   6 surfaces      0.037 ms     1.476 µs/cellule
//!    50 cellules ×   6 surfaces      0.073 ms     1.462 µs/cellule
//!   100 cellules ×   6 surfaces      0.179 ms     1.791 µs/cellule
//!   200 cellules ×   6 surfaces      0.395 ms     1.974 µs/cellule
//!   400 cellules ×   6 surfaces      0.728 ms     1.819 µs/cellule
//!
//!   100 cellules ×  10 surfaces      0.401 ms     4.012 µs/cellule
//!   100 cellules ×  18 surfaces      0.853 ms     8.527 µs/cellule
//!   100 cellules ×  26 surfaces      1.692 ms    16.924 µs/cellule
//!   100 cellules ×  50 surfaces      5.442 ms    54.420 µs/cellule
//!
//!   200 cellules ×  50 surfaces     10.913 ms    54.567 µs/cellule
//!   400 cellules ×  50 surfaces     22.252 ms    55.630 µs/cellule
//! ```
//!
//! Poste Windows au repos, rien d'autre en cours ; deux exécutions successives
//! concordent à un dixième près, et c'est la seule garantie qu'on a sur une
//! machine de bureau.
//!
//! **Le coût est linéaire en nombre de cellules, et il l'est à toutes les
//! tailles.** Le coût par cellule reste plat de 25 à 400 cellules — autour de
//! deux microsecondes pour une cellule simple, de cinquante-cinq pour une grosse.
//! L'appariement des portails, qui compare la carte entière, ne domine donc
//! jamais : c'était la crainte, elle est levée.
//!
//! **Il est super-linéaire en surfaces d'une même cellule.** Huit fois plus de
//! surfaces coûtent **vingt-sept fois** plus, soit un exposant moyen de 1,55 qui
//! monte à 1,8 sur le dernier intervalle : le terme linéaire du décodage domine
//! en dessous d'une dizaine de surfaces, le terme quadratique du classement des
//! arêtes prend le dessus au-delà. C'est exactement ce que la lecture du code
//! annonçait, et les décors du dépôt — six surfaces par cellule — vivent tous du
//! mauvais côté pour l'observer.
//!
//! **D'où vient ce terme**, écrit ici pour qu'on n'ait pas à le redécouvrir en
//! lisant la courbe : le classement des arêtes partagées balaie, **pour chaque
//! arête de chaque surface, toutes les autres surfaces de la même cellule**. Il
//! est donc quadratique en surfaces d'une cellule, et indifférent au nombre de
//! cellules — ce que les deux séries montrent séparément.
//!
//! **Ce n'est pas une invitation à l'optimiser.** Le classement exact est ce qui
//! remplace un marquage d'arêtes stocké dans les données, et la raison en est
//! écrite dans `rust.md` : un lien stocké est une occasion d'incohérence que
//! l'éditeur devrait maintenir. Aucun décor du dépôt n'approche l'échelle où le
//! terme pèse, et une structure d'accélération ici serait un index spatial de
//! plus à tenir juste. Ce qui est mesuré est mesuré ; ce qui le rendrait
//! nécessaire n'existe pas.
//!
//! **Le chiffre qui décide : une carte de quatre cents cellules grosses se
//! recharge en 22 ms.** C'est plus qu'une image à soixante par seconde. Un
//! éditeur peut donc recharger à chaque opération **validée**, et ne peut pas le
//! faire pendant un glissé de souris — ce qui est une contrainte sur l'éditeur,
//! pas sur le moteur.

use core::hint::black_box;
use core::time::Duration;
use std::time::Instant;

use screengine::World;

/// Répétitions par cas.
///
/// Moins que les soixante des bancs d'image : un chargement de plusieurs
/// centaines de cellules se compte en millisecondes, et le minimum se stabilise
/// bien avant.
const TOURS: u32 = 12;

/// Le côté d'un segment de mur, en unités de monde.
///
/// Huit, et non une valeur quelconque : le repère de lightmap d'un mur a pour
/// axe horizontal l'arête elle-même, dont le carré doit être une puissance de
/// deux. Huit donne soixante-quatre.
const PAS: f32 = 8.0;

/// La hauteur d'une cellule, même contrainte que [`PAS`].
const HAUTEUR: f32 = 8.0;

/// La largeur d'une cellule, et donc la largeur d'un portail.
const LARGEUR: f32 = 8.0;

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
    // Le repère de texture prend une densité quelconque : ce banc ne rend aucune
    // image, et seule la lightmap porte des contraintes de chargement.
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

/// Les octets d'une carte de `cellules` cellules à `segments` segments par long
/// côté.
///
/// Les cellules se suivent le long de l'axe X, et deux voisines décrivent leur
/// arête commune avec **les mêmes littéraux** : c'est ce que l'appariement
/// exige, puisqu'il compare les positions au bit près.
fn carte(cellules: u32, segments: u32) -> Vec<u8> {
    let mut cells = Vec::new();
    for i in 0..cellules {
        cellule(
            i + 1,
            1000 * (i + 1),
            PAS * segments as f32 * i as f32,
            segments,
            &mut cells,
        );
    }

    let mut materiaux = Vec::new();
    for (id, nom) in [(MUR, "mur"), (SOL, "sol")] {
        words(&[id], &mut materiaux);
        materiaux.extend_from_slice(&(nom.len() as u16).to_le_bytes());
        materiaux.extend_from_slice(nom.as_bytes());
    }

    // Deux sections seulement : ni lumières, ni entités. Ce banc mesure le
    // décodage de la géométrie, et une lumière n'en traverse aucun chemin.
    let sections = [(*b"CELL", &cells), (*b"MATS", &materiaux)];
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

/// Le plus court temps observé sur [`TOURS`] répétitions.
fn mesure(mut tour: impl FnMut()) -> Duration {
    tour();
    tour();

    let mut court = Duration::MAX;
    for _ in 0..TOURS {
        let debut = Instant::now();
        tour();
        court = court.min(debut.elapsed());
    }
    court
}

/// Mesure un cas et écrit sa ligne, durée totale puis coût par cellule.
///
/// **Le coût par cellule est la colonne qui informe**, pas la durée : c'est elle
/// qui dit si le coût est linéaire — elle reste plate — ou non.
fn cas(cellules: u32, segments: u32) {
    let bytes = carte(cellules, segments);
    // Le générateur est hors du chronomètre, et son résultat est vérifié avant :
    // une carte refusée rendrait une durée sans objet.
    let monde = World::load(&bytes).expect("la carte engendrée est légale");
    assert_eq!(
        monde.cell_count(),
        cellules,
        "le générateur n'a pas écrit le nombre de cellules demandé"
    );
    // Deux longs côtés subdivisés, plus le sol et le plafond ; les deux petits
    // côtés sont des portails et ne comptent pas comme surfaces.
    let surfaces = 2 * segments + 2;

    let duree = mesure(|| {
        World::load(black_box(&bytes)).expect("carte valide");
    });
    let ms = duree.as_secs_f64() * 1e3;
    println!(
        "{cellules:>5} cellules × {surfaces:>3} surfaces   {:>10} octets   {ms:>8.3} ms   {:>7.3} µs/cellule",
        bytes.len(),
        ms * 1e3 / f64::from(cellules),
    );
}

/// Joue les deux séries et écrit leur tableau.
///
/// Deux séries et non une grille complète : chacune ne fait varier **qu'un**
/// paramètre, ce qui est la condition pour qu'une pente soit attribuable.
fn main() {
    println!("screengine — chargement d'une carte, minimum sur {TOURS} tours\n");

    println!("— le nombre de cellules croît, les cellules restent simples");
    for cellules in [25, 50, 100, 200, 400] {
        cas(cellules, 2);
    }

    println!("\n— le nombre de cellules est fixe, les cellules grossissent");
    for segments in [2, 4, 8, 12, 24] {
        cas(100, segments);
    }

    // **Le cas qui décide, et il est mesuré plutôt qu'extrapolé.** Les deux
    // séries donnent chacune une pente ; les composer de tête supposerait que les
    // deux termes n'interagissent pas, ce que rien ne garantit — et c'est
    // précisément ce chiffre-là qu'on citera.
    println!("\n— une carte d'éditeur : des centaines de cellules, et grosses");
    for cellules in [200, 400] {
        cas(cellules, 24);
    }
}
