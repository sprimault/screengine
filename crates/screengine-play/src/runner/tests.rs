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

/// La cadence est rapportée à la durée réelle de la fenêtre, qui dépasse la
/// seconde : le compte brut annonçait soixante-deux images par seconde sur un
/// écran qui en montre soixante.
#[test]
fn la_cadence_se_rapporte_a_la_duree_reelle() {
    let mut stats = Stats::default();
    let start = Instant::now();
    for _ in 0..3 {
        stats.record(ms(1), ms(1), ms(1), start);
    }
    let line = stats.take(start + ms(1500));
    assert_eq!(line.as_deref().and_then(|l| l.split(' ').next()), Some("2"));
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

/// L'ordre des canaux, qui est tout ce qui peut être faux sans se voir ailleurs
/// que dans un navigateur : `softbuffer` range `0x00RRGGBB`, le canvas attend
/// R, G, B, A. Un décalage d'un canal échange les rouges et les bleus, ce que
/// l'ABI décrit nommément comme le défaut classique de cette frontière.
#[test]
fn la_conversion_pour_le_canvas_garde_l_ordre_des_canaux() {
    let mut frame = Vec::new();
    to_rgba(&[0x00_11_22_33, 0x00_FF_00_00], &mut frame);
    assert_eq!(frame, [0x11, 0x22, 0x33, 255, 0xFF, 0x00, 0x00, 255]);
}

/// L'alpha est écrit à 255 et non recopié : `softbuffer` laisse son octet de
/// poids fort à zéro, et un canvas composite réellement ce canal — l'image
/// serait entièrement transparente.
#[test]
fn la_conversion_force_l_alpha() {
    let mut frame = Vec::new();
    to_rgba(&[0x00_40_50_60], &mut frame);
    assert_eq!(frame[3], 255);
}

/// Le tampon se réemploie d'une image à l'autre : il est dimensionné sur ce
/// qu'on lui donne, sans traîner ce qu'une fenêtre plus grande y avait laissé.
#[test]
fn la_conversion_redimensionne_son_tampon() {
    let mut frame = vec![7; 64];
    to_rgba(&[0x00_01_02_03], &mut frame);
    assert_eq!(frame.len(), 4);
}

/// Une fenêtre sans image ne rend rien plutôt que de diviser par zéro. C'est le
/// cas d'une fenêtre masquée, qui ne rend pas et dont le réveil s'arrête.
#[test]
fn sans_image_il_n_y_a_pas_de_releve() {
    let mut stats = Stats::default();
    assert_eq!(stats.take(Instant::now() + STATS_WINDOW * 10), None);
}

/// Le plafond tiré des écrans, dans les quatre régimes qui décident.
///
/// **C'est tout ce qui peut être faux sans se voir** : la lecture des écrans
/// demande une boucle active, et le plafond ne déplace aucun pixel — un calcul
/// faux ne se verrait donc ni à l'image, ni dans une empreinte, mais seulement
/// en mémoire, ou par une hausse refusée plus tard sur l'autre écran.
///
/// Le deuxième cas est celui qui porte la décision : **le plus grand des
/// écrans**, axe par axe. Avec le plus petit, une fenêtre portée sur le grand
/// ne pourrait plus monter, et un plafond ne se relève pas.
#[test]
fn le_plafond_suit_l_enveloppe_des_ecrans() {
    let opening = (640, 360);

    // Un seul écran, sous la borne du moteur : c'est lui qui décide.
    assert_eq!(screen_ceiling(&[(1920, 1080)], opening), (1920, 1080));

    // Trois écrans, dont deux formats : chaque axe prend son maximum, même venu
    // d'écrans différents — la hauteur de 1440 vient du premier, et elle serait
    // perdue si l'on prenait le plus petit.
    //
    // **La borne du moteur mord sur un axe seulement** : 2560 passe à 2048, la
    // hauteur reste entière. C'est le cas d'un poste réel en 2560 × 1600, dont le
    // plafond informé vaut 2048 × 1600.
    assert_eq!(
        screen_ceiling(&[(2560, 1440), (1920, 1200), (1280, 720)], opening),
        (MAX_RESOLUTION, 1440)
    );

    // Au-delà de la borne du moteur, c'est elle qui plafonne — un écran 4K ne
    // fait pas rendre une image de 3840 pixels de large.
    assert_eq!(
        screen_ceiling(&[(3840, 2160)], opening),
        (MAX_RESOLUTION, MAX_RESOLUTION)
    );

    // Aucun écran nommé — Wayland, le web : la borne du moteur, parce qu'une
    // hausse refusée coûte plus qu'un tampon trop grand.
    assert_eq!(
        screen_ceiling(&[], opening),
        (MAX_RESOLUTION, MAX_RESOLUTION)
    );

    // Un écran plus petit que la résolution d'ouverture : celle-ci gagne, un
    // contexte devant pouvoir rendre ce qu'on lui a demandé d'ouvrir.
    assert_eq!(screen_ceiling(&[(320, 200)], opening), opening);
}
