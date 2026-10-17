// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le relevé des durées, sans fenêtre : l'horloge est fournie par le test.
//!
//! Ce que la boucle fait du relevé — le poser dans le titre — demande une
//! fenêtre et n'est donc éprouvé par aucun test ; ce qui se vérifie ici est le
//! calcul, qui est tout ce qui peut être faux sans se voir.

use super::*;

/// Une durée en millisecondes, pour que les cas se lisent.
fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// La virgule décimale et le dixième, qui sont la forme affichée.
#[test]
fn les_millisecondes_se_lisent_au_dixieme() {
    assert_eq!(millis(ms(20) + Duration::from_micros(200)), "20,2");
    assert_eq!(millis(Duration::ZERO), "0,0");
    assert_eq!(millis(ms(1)), "1,0");
}

/// L'arrondi est au plus proche, et c'est le demi-dixième qui le dit : tronquer
/// rendrait `1,8` des deux côtés, et personne ne le verrait sur un chiffre qui
/// bouge chaque seconde.
#[test]
fn le_dixieme_s_arrondit_au_plus_proche() {
    assert_eq!(millis(Duration::from_micros(1849)), "1,8");
    assert_eq!(millis(Duration::from_micros(1850)), "1,9");
}

/// Le titre du jeu survit au relevé : c'est lui qui porte les commandes.
#[test]
fn le_titre_du_jeu_garde_sa_place() {
    assert_eq!(
        window_title("Couloir — F : tramage", None),
        "Couloir — F : tramage"
    );
    assert_eq!(
        window_title("Couloir", Some("16 img/s")),
        "Couloir · 16 img/s"
    );
}

/// Rien ne sort avant la période écoulée, et la moyenne est par image et non le
/// total — c'est la confusion qui ferait lire une image à soixante fois son
/// coût.
#[test]
fn la_moyenne_sort_a_la_seconde_et_divise_par_les_images() {
    let mut stats = Stats::default();
    let start = Instant::now();
    stats.record(ms(6), ms(20), ms(2), start);
    assert_eq!(stats.take(start + ms(999)), None);

    stats.record(ms(6), ms(20), ms(2), start + ms(500));
    assert_eq!(
        stats.take(start + STATS_WINDOW),
        Some(String::from(
            "2 img/s · soumission 6,0 · remplissage 20,0 · écran 2,0 ms"
        ))
    );
}

/// Le relevé rendu remet le compteur à zéro : sans cela, la seconde suivante
/// moyennerait sur toutes les précédentes et le chiffre se figerait peu à peu.
#[test]
fn le_releve_repart_de_zero() {
    let mut stats = Stats::default();
    let start = Instant::now();
    stats.record(ms(10), ms(10), ms(10), start);
    assert!(stats.take(start + STATS_WINDOW).is_some());
    assert_eq!(stats.take(start + STATS_WINDOW * 4), None);
}

/// Une fenêtre sans image ne rend rien plutôt que de diviser par zéro. C'est le
/// cas d'une fenêtre masquée, qui ne rend pas et dont le réveil s'arrête.
#[test]
fn sans_image_il_n_y_a_pas_de_releve() {
    let mut stats = Stats::default();
    assert_eq!(stats.take(Instant::now() + STATS_WINDOW * 10), None);
}
