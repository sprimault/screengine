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
//! La forme des cartes et ce que chacun de ses paramètres fait travailler sont
//! dans [`harnais::decor`](../harnais/decor/index.html), qui les engendre pour ce
//! banc et pour [`cuisson`](../cuisson/index.html). Celui-ci n'en pose **aucune
//! lampe** : une lumière ne traverse aucun chemin du décodage, et la carte sort
//! alors octet pour octet celle sur laquelle les chiffres ci-dessous ont été
//! pris.
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

use screengine::World;

#[path = "harnais/mod.rs"]
mod harnais;

use harnais::decor::{carte, surfaces};
use harnais::mesure;

/// Répétitions par cas.
///
/// Moins que les soixante des bancs d'image : un chargement de plusieurs
/// centaines de cellules se compte en millisecondes, et le minimum se stabilise
/// bien avant.
const TOURS: u32 = 12;

/// Le nombre de lampes posées par cellule : aucune.
///
/// Le décodage ne lit pas une lumière autrement qu'il ne lit un sommet, et la
/// section absente garde la carte identique à celle des chiffres ci-dessus.
const LAMPES: u32 = 0;

/// Mesure un cas et écrit sa ligne, durée totale puis coût par cellule.
///
/// **Le coût par cellule est la colonne qui informe**, pas la durée : c'est elle
/// qui dit si le coût est linéaire — elle reste plate — ou non.
fn cas(cellules: u32, segments: u32) {
    let bytes = carte(cellules, segments, LAMPES);
    // Le générateur est hors du chronomètre, et son résultat est vérifié avant :
    // une carte refusée rendrait une durée sans objet.
    let monde = World::load(&bytes).expect("la carte engendrée est légale");
    assert_eq!(
        monde.cell_count(),
        cellules,
        "le générateur n'a pas écrit le nombre de cellules demandé"
    );
    let surfaces = surfaces(segments);

    let duree = mesure(TOURS, || {
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
