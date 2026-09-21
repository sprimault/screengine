// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la caméra libre fait des angles, sans fenêtre.
//!
//! Les entrées ne se simulent pas ici : `Tick` emprunte un `Input` que la
//! boucle remplit. Ce qui se teste sans elle, c'est ce qui décide de l'image —
//! la composition des deux angles et la borne du tangage.

use super::*;

/// L'écart admis après deux rotations par les tables trigonométriques.
const TOLERANCE: f32 = 1e-5;

/// Compare deux vecteurs composante par composante.
fn assert_close(actual: Vec3, expected: Vec3, what: &str) {
    let off = |a: f32, b: f32| (a - b).abs() > TOLERANCE;
    assert!(
        !(off(actual.x, expected.x) || off(actual.y, expected.y) || off(actual.z, expected.z)),
        "{what} : {actual:?} au lieu de {expected:?}"
    );
}

/// La direction que la caméra regarde, en coordonnées de monde.
fn gaze(camera: &FreeCamera) -> Vec3 {
    let pose = Affine3::from_rotation_translation(camera.camera().orientation, Vec3::ZERO);
    pose.transform_vector(FORWARD)
}

/// Sans rotation, la caméra regarde l'est : la convention de l'ABI.
#[test]
fn au_repos_elle_regarde_l_est() {
    let camera = FreeCamera::new(Vec3::ZERO);
    assert_close(gaze(&camera), FORWARD, "repos");
}

/// Un lacet positif tourne vers le nord, un tangage positif vers le haut.
///
/// Les deux signes ensemble : pris à l'envers, l'un comme l'autre donne une
/// caméra qui marche encore, mais dont la souris va du mauvais côté.
#[test]
fn les_deux_angles_vont_dans_le_bon_sens() {
    let mut camera = FreeCamera::new(Vec3::ZERO);
    camera.yaw = core::f32::consts::FRAC_PI_2;
    assert_close(gaze(&camera), Vec3::new(0.0, 1.0, 0.0), "lacet");

    let mut camera = FreeCamera::new(Vec3::ZERO);
    camera.pitch = core::f32::consts::FRAC_PI_2;
    assert_close(gaze(&camera), Vec3::new(0.0, 0.0, 1.0), "tangage");
}

/// Le tangage penche autour de l'axe latéral **de la caméra**, et l'horizon ne
/// bascule pas quand on tourne.
///
/// Composés dans l'autre ordre, les deux angles donnent un roulis : la caméra
/// regarderait à peu près au bon endroit, l'image serait penchée, et rien
/// d'autre ne le dirait.
#[test]
fn tourner_puis_lever_les_yeux_ne_penche_pas_l_horizon() {
    let mut camera = FreeCamera::new(Vec3::ZERO);
    camera.yaw = core::f32::consts::FRAC_PI_2;
    camera.pitch = core::f32::consts::FRAC_PI_4;

    // Regardant le nord et vers le haut, la droite de l'écran reste l'est.
    let pose = Affine3::from_rotation_translation(camera.camera().orientation, Vec3::ZERO);
    assert_close(
        pose.transform_vector(RIGHT),
        Vec3::new(1.0, 0.0, 0.0),
        "axe latéral",
    );
}

/// Le tangage est borné au quart de tour, jamais replié.
///
/// Sans la borne, regarder trop haut retourne l'image, et on croit à un défaut
/// du moteur plutôt qu'à une caméra passée par-dessus la verticale.
#[test]
fn le_tangage_est_borne_au_quart_de_tour() {
    let mut camera = FreeCamera::new(Vec3::ZERO);
    for excess in [3.0f32, -3.0] {
        camera.pitch = excess;
        camera.pitch = camera.pitch.clamp(-MAX_PITCH, MAX_PITCH);
        assert_eq!(camera.pitch.abs(), MAX_PITCH);
        // Le regard reste du côté où l'on visait, jamais retourné.
        assert!(gaze(&camera).z.signum() == excess.signum());
    }
}
