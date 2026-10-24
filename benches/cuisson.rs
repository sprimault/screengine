// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que coûte la cuisson des lightmaps d'une carte, et sur quoi ce coût porte.
//!
//! **La question est celle d'un changement de carte.** `docs/abi.md` arrête la
//! méthode — charger la nouvelle à côté, basculer, détruire l'ancienne — et
//! publie les deux axes du **chargement**, linéaire en cellules et super-linéaire
//! en surfaces d'une même cellule. De la cuisson, qui précède la première image
//! de la carte nouvelle, il ne disait rien : ni complexité, ni axe, ni ordre de
//! grandeur, alors qu'un intégrateur doit décider s'il la masque, l'étale sur
//! plusieurs images ou la prend d'un bloc. Ce banc existe pour que cette clause
//! puisse s'écrire sur une mesure et non sur une impression.
//!
//! **Ce n'est pas le chargement qui domine, et l'écart n'est pas d'un facteur
//! deux.** [`chargement`](../chargement/index.html) mesure la même carte : les
//! deux colonnes se comparent ligne à ligne, et c'est le rapport entre elles qui
//! informe plus que la durée seule.
//!
//! La forme des cartes est dans
//! [`harnais::decor`](../harnais/decor/index.html), et ce banc en est le second
//! appelant. Il pose en plus des **lampes**, sans lesquelles il ne mesurerait
//! rien : la sélection géométrique rejette une cellule qu'aucune sphère ne
//! touche, et le coût s'effondre sur un décor éteint — ce qui est exactement le
//! piège d'une scène mal cadrée que `rust.md` décrit pour les bancs d'image.
//!
//! Le harnais et ce que ces chiffres valent sont dans
//! [`remplissage`](../remplissage/index.html) : aucun seuil, jamais d'échec sur
//! une durée, le minimum plutôt que la moyenne.
//!
//! # Ce que le luxel apporte comme unité
//!
//! Une cellule de ce générateur porte autant de luxels que sa surface en unités
//! de monde, et `scg_world_cell_luxel_count` les rend — c'est le seul chiffre
//! que l'ABI publiait sur la cuisson, annoncé comme un **poids relatif** que rien
//! ne traduisait en temps. Ce banc le traduit : s'il est proportionnel à la
//! durée, un hôte peut pondérer une barre de progression avec, et c'est tout ce
//! qu'on lui demande.
//!
//! # La mesure, prise le 2026-10-24
//!
//! ```text
//!    8 cellules ×   6 surfaces × 1 lampe       11808 luxels      4.015 ms    0.502 ms/cellule   0.340 µs/luxel
//!   16 cellules ×   6 surfaces × 1 lampe       23616 luxels      8.444 ms    0.528 ms/cellule   0.358 µs/luxel
//!   32 cellules ×   6 surfaces × 1 lampe       47232 luxels     16.150 ms    0.505 ms/cellule   0.342 µs/luxel
//!   64 cellules ×   6 surfaces × 1 lampe       94464 luxels     32.144 ms    0.502 ms/cellule   0.340 µs/luxel
//!
//!   16 cellules ×   6 surfaces × 1 lampe       23616 luxels      8.049 ms    0.503 ms/cellule   0.341 µs/luxel
//!   16 cellules ×  10 surfaces × 1 lampe       65664 luxels     29.861 ms    1.866 ms/cellule   0.455 µs/luxel
//!   16 cellules ×  18 surfaces × 1 lampe      149760 luxels    117.873 ms    7.367 ms/cellule   0.787 µs/luxel
//!
//!    8 cellules ×   6 surfaces × 1 lampe       11808 luxels      3.790 ms    0.474 ms/cellule   0.321 µs/luxel
//!    8 cellules ×   6 surfaces × 2 lampes      11808 luxels      6.373 ms    0.797 ms/cellule   0.540 µs/luxel
//!    8 cellules ×   6 surfaces × 4 lampes      11808 luxels     11.773 ms    1.472 ms/cellule   0.997 µs/luxel
//! ```
//!
//! Poste Windows au repos, rien d'autre en cours ; deux exécutions successives
//! concordent à moins d'un dixième, et c'est la seule garantie qu'on a sur une
//! machine de bureau.
//!
//! **Le coût est linéaire en nombre de cellules**, et il l'est à toutes les
//! tailles mesurées : le coût par cellule reste à un demi-millième de seconde de
//! 8 à 64 cellules. La cuisson d'une cellule ne regarde que la cellule et ses
//! voisines immédiates, et cette borne se voit dans la mesure.
//!
//! **Il est super-linéaire en surfaces d'une même cellule, et pour deux raisons
//! qui se composent.** Trois fois plus de surfaces coûtent **quatorze fois**
//! plus : six fois plus de luxels à calculer, et chaque rayon qui en part teste
//! toutes les surfaces de l'ensemble d'occulteurs, qui a grossi d'autant. C'est
//! ce que dit la troisième colonne — le coût **par luxel** double lui aussi,
//! de 0,34 à 0,79 µs —, et c'est la raison pour laquelle les deux colonnes
//! valent d'être lues ensemble.
//!
//! **Il est linéaire en lampes retenues** : quatre lampes dans la même géométrie
//! coûtent trois fois une seule, l'écart au facteur quatre venant du rejet par la
//! distance, qui écarte une lampe avant de lancer son rayon.
//!
//! **Le chiffre qui décide, et c'est celui qu'`abi.md` porte désormais : la
//! cuisson coûte deux à trois ordres de grandeur le chargement de la même
//! carte.** 64 cellules simples se chargent en 0,13 ms et se cuisent en 32 ; 16
//! cellules de dix-huit surfaces se chargent en 0,17 ms et se cuisent en 118. Un
//! changement de carte est donc dominé par la cuisson, et le rechargement dont
//! le contrat publiait les axes n'en est pas la part qui compte.
//!
//! **Ce que le compte de luxels vaut comme pondération**, puisque c'est ce que
//! l'ABI offre à un hôte qui veut une barre de progression : il est juste **à
//! géométrie et éclairage comparables**, où le coût par luxel est plat à 3 %
//! près ; il sous-estime une cellule chargée en surfaces ou en lampes d'un
//! facteur qui va jusqu'à trois sur les cas ci-dessus. Une barre pondérée par
//! lui avance donc trop vite au début d'un décor hétérogène — ce qui est
//! utilisable, et qu'il faut savoir.

use core::hint::black_box;

use screengine::{Lightmaps, World};

#[path = "harnais/mod.rs"]
mod harnais;

use harnais::decor::{carte, surfaces};
use harnais::mesure;

/// Répétitions par cas.
///
/// Trois, là où le chargement en prend douze : une cuisson se compte en dizaines
/// de millisecondes par cellule, et cinq exécutions — deux à blanc, trois
/// mesurées — suffisent à stabiliser un minimum sans que le banc entier dépasse
/// la minute.
const TOURS: u32 = 3;

/// Les lampes posées dans chaque cellule.
///
/// Une seule, qui est le régime d'un couloir éclairé. Le nombre de lampes
/// **retenues** par cellule est ce qui multiplie le travail par luxel, et la
/// troisième série le fait varier.
const LAMPE: u32 = 1;

/// Cuit toutes les cellules d'une carte, une par une comme l'hôte le fait.
///
/// **Cellule par cellule et non en bloc**, parce que c'est la seule forme que
/// l'ABI offre : le grain est la cellule, il n'y a pas de reprise à curseur, et
/// ce que mesure ce banc doit être ce qu'un hôte paie.
fn cuire(monde: &World, lightmaps: &mut Lightmaps) {
    for index in 0..monde.cell_count() {
        let id = monde.cell_id(index).expect("rang sous le compte");
        lightmaps.build(monde, id).expect("cuisson possible");
    }
}

/// Mesure un cas et écrit sa ligne.
///
/// **Trois colonnes dérivées, et la troisième est celle qui sert à un hôte** : la
/// durée totale dit ce que coûte une bascule, le coût par cellule dit si l'axe
/// est linéaire, le coût par luxel dit si le compte que l'ABI publie est une
/// pondération utilisable.
fn cas(cellules: u32, segments: u32, par_cellule: u32) {
    let bytes = carte(cellules, segments, par_cellule);
    // Hors du chronomètre, et vérifié avant : une carte refusée rendrait une
    // durée sans objet, et un décor éteint un chiffre qui ne mesure rien.
    let monde = World::load(&bytes).expect("la carte engendrée est légale");
    let luxels: u64 = (0..monde.cell_count())
        .map(|index| {
            let id = monde.cell_id(index).expect("rang sous le compte");
            u64::from(
                monde
                    .cell_luxel_count(id)
                    .expect("la cellule porte son compte"),
            )
        })
        .sum();
    assert!(
        luxels > 0,
        "une carte sans luxel ne mesurerait pas la cuisson"
    );

    let duree = mesure(TOURS, || {
        let mut lightmaps = Lightmaps::new(black_box(&monde)).expect("porteur");
        cuire(&monde, &mut lightmaps);
    });
    let ms = duree.as_secs_f64() * 1e3;
    println!(
        "{cellules:>4} cellules × {:>3} surfaces × {par_cellule} lampe(s)   {luxels:>9} luxels   {ms:>9.3} ms   {:>8.3} ms/cellule   {:>7.3} µs/luxel",
        surfaces(segments),
        ms / f64::from(cellules),
        ms * 1e3 / luxels as f64,
    );
}

/// Joue les trois séries et écrit leur tableau.
///
/// Trois séries et non une grille : chacune ne fait varier qu'un paramètre, ce
/// qui est la condition pour qu'une pente soit attribuable — la règle que
/// [`chargement`](../chargement/index.html) a posée.
fn main() {
    println!("screengine — cuisson des lightmaps, minimum sur {TOURS} tours\n");

    println!("— le nombre de cellules croît, les cellules restent simples");
    for cellules in [8, 16, 32, 64] {
        cas(cellules, 2, LAMPE);
    }

    println!("\n— le nombre de cellules est fixe, les cellules grossissent");
    for segments in [2, 4, 8] {
        cas(16, segments, LAMPE);
    }

    println!("\n— une seule cellule, et les lampes se multiplient");
    for par_cellule in [1, 2, 4] {
        cas(8, 2, par_cellule);
    }
}
