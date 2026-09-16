// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Les fronts d'entrée, pas par pas.

use super::*;

/// Un appui se voit au pas qui suit, et à celui-là seulement : sans le
/// vidage de fin de pas, un saut se déclencherait à chaque pas tant que la
/// touche reste tenue.
#[test]
fn un_front_ne_vaut_que_pour_un_pas() {
    let mut input = Input::default();
    input.key(KeyCode::Space, true);

    assert!(input.pressed(KeyCode::Space));
    assert!(input.down(KeyCode::Space));
    input.end_step();

    assert!(!input.pressed(KeyCode::Space));
    assert!(input.down(KeyCode::Space));
}

/// La répétition automatique du système renvoie des appuis sur une touche
/// déjà tenue : elle ne doit pas refaire un front.
#[test]
fn un_appui_repete_ne_refait_pas_de_front() {
    let mut input = Input::default();
    input.key(KeyCode::KeyW, true);
    input.end_step();
    input.key(KeyCode::KeyW, true);
    assert!(!input.pressed(KeyCode::KeyW));
}

/// Un appui et un relâchement entre deux pas : la touche n'est plus tenue,
/// mais les deux fronts restent visibles. Sinon un tapotement rapide serait
/// perdu à faible fréquence de mise à jour.
#[test]
fn un_tapotement_entre_deux_pas_n_est_pas_perdu() {
    let mut input = Input::default();
    input.key(KeyCode::KeyE, true);
    input.key(KeyCode::KeyE, false);

    assert!(!input.down(KeyCode::KeyE));
    assert!(input.pressed(KeyCode::KeyE));
    assert!(input.released(KeyCode::KeyE));
}

/// Le déplacement se cumule entre deux pas et repart de zéro après : une
/// caméra qui le lit à chaque pas ne compte chaque mouvement qu'une fois.
#[test]
fn le_deplacement_de_la_souris_se_cumule_puis_se_vide() {
    let mut input = Input::default();
    input.motion(3.0, -1.0);
    input.motion(2.0, 4.0);
    assert_eq!(input.mouse_delta(), (5.0, 3.0));

    input.end_step();
    assert_eq!(input.mouse_delta(), (0.0, 0.0));
}

/// Une touche relâchée hors focus n'envoie rien : la perte du focus la
/// relâche, avec son front, pour qu'une mise à jour qui attend le
/// relâchement le voie.
#[test]
fn la_perte_du_focus_relache_tout() {
    let mut input = Input::default();
    input.key(KeyCode::KeyD, true);
    input.button(MouseButton::Left, true);
    input.end_step();

    input.release_all();
    assert!(!input.down(KeyCode::KeyD));
    assert!(input.released(KeyCode::KeyD));
    assert!(!input.button_down(MouseButton::Left));
    assert!(input.button_released(MouseButton::Left));
}
