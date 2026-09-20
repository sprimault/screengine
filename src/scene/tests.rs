// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la caméra désigne, et ce que la couleur range.
//!
//! Les égalités y sont exactes partout où l'orientation est neutre : la base de
//! vue est une permutation signée, et une approximation y signalerait que la
//! normalisation ou le produit ont cessé d'être neutres. Dès qu'une rotation
//! quelconque entre en jeu, la comparaison devient approchée — la table
//! trigonométrique interpole un quart de cercle en 1024 intervalles, et rien
//! dans le projet ne prétend qu'un quart de tour rende des zéros exacts.

use super::*;
use crate::math::Angle;

/// L'écart admis quand une rotation passe par la table trigonométrique.
///
/// Trois ordres de grandeur au-dessus de l'erreur d'interpolation, et trois
/// sous ce qu'une erreur de signe ou d'axe produirait : la borne sépare les
/// deux sans arbitrer sur la précision des tables, qui se teste ailleurs.
const TOLERANCE: f32 = 1e-6;

/// Compare deux vecteurs composante par composante, à [`TOLERANCE`] près.
fn assert_close(actual: Vec3, expected: Vec3) {
    let off = |a: f32, b: f32| (a - b).abs() > TOLERANCE;
    assert!(
        !(off(actual.x, expected.x) || off(actual.y, expected.y) || off(actual.z, expected.z)),
        "{actual:?} au lieu de {expected:?}"
    );
}

/// Les trois axes du monde, pour lire les tests d'orientation.
const EAST: Vec3 = Vec3::new(1.0, 0.0, 0.0);
/// Le nord, qui est le +Y du monde.
const NORTH: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// Le zénith, qui est le +Z du monde.
const UP: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// La convention que tout le reste suppose : une caméra d'orientation neutre
/// regarde le +X du monde, le zénith vers le haut de l'écran.
///
/// Trois assertions et non une : « regarde le +X » laisse le roulis libre, et
/// c'est justement lui qu'une erreur de signe dans la base ferait basculer sans
/// changer la direction du regard.
#[test]
fn l_orientation_neutre_regarde_l_est() {
    let view = Camera::DEFAULT.view();
    // Z de vue est l'avant, Y de vue est vers le bas, X de vue est la droite.
    assert_eq!(view.transform_vector(EAST), Vec3::new(0.0, 0.0, 1.0));
    assert_eq!(view.transform_vector(UP), Vec3::new(0.0, -1.0, 0.0));
    assert_eq!(view.transform_vector(NORTH), Vec3::new(-1.0, 0.0, 0.0));
}

/// Le repère de vue reste direct, comme le monde.
///
/// S'il devenait indirect, `inverse_rigid` rendrait un faux inverse sans rien
/// signaler : sa précondition est l'absence d'échelle, et un miroir y passerait
/// pour une rotation.
#[test]
fn le_repere_de_vue_est_direct() {
    let view = Camera::DEFAULT.view();
    let x = view.transform_vector(EAST);
    let y = view.transform_vector(NORTH);
    assert_eq!(x.cross(y), view.transform_vector(EAST.cross(NORTH)));
}

/// La position de la caméra devient l'origine de l'espace de vue.
#[test]
fn la_vue_ramene_la_camera_a_l_origine() {
    let position = Vec3::new(3.0, -4.0, 5.0);
    let camera = Camera {
        position,
        ..Camera::DEFAULT
    };
    assert_eq!(camera.view().transform_point(position), Vec3::ZERO);
}

/// Un quart de tour autour du zénith fait regarder le nord.
///
/// C'est le sens de rotation que le test attrape : pris à l'envers, la caméra
/// regarderait le sud, et rien d'autre dans la suite ne le dirait.
#[test]
fn un_quart_de_tour_autour_du_zenith_fait_regarder_le_nord() {
    let camera = Camera {
        orientation: Quat::from_axis_angle(UP, Angle::QUARTER),
        ..Camera::DEFAULT
    };
    // Le nord est devant, donc sur le +Z de vue ; le zénith reste en haut.
    let view = camera.view();
    assert_close(view.transform_vector(NORTH), Vec3::new(0.0, 0.0, 1.0));
    assert_close(view.transform_vector(UP), Vec3::new(0.0, -1.0, 0.0));
    assert_close(view.transform_vector(EAST), Vec3::new(1.0, 0.0, 0.0));
}

/// Un quaternion non unitaire est normalisé à la réception, et un quaternion
/// déjà unitaire traverse sans être altéré.
///
/// Le second cas est le vrai sujet : si la normalisation déplaçait d'un bit une
/// orientation déjà bonne, chaque changement de caméra dériverait un peu, et
/// l'image cesserait de dépendre seulement de ce que l'hôte a décrit.
#[test]
fn la_normalisation_est_neutre_sur_un_quaternion_unitaire() {
    let doubled = Camera {
        orientation: Quat::new(0.0, 0.0, 0.0, 2.0),
        ..Camera::DEFAULT
    };
    assert_eq!(doubled.view(), Camera::DEFAULT.view());
}

/// L'entier que le rasteriseur porte est `0xAABBGGRR`, et non `0xAARRGGBB`.
///
/// C'est la confusion que les quatre champs nommés existent pour fermer : elle
/// échange le rouge et le bleu, et se voit dans l'image sans qu'aucun contrôle
/// ne la nomme.
#[test]
fn la_couleur_se_range_en_octets_rgba() {
    let color = Color::new(0xE0, 0xA0, 0x30, 0xFF);
    assert_eq!(color.packed(), 0xFF30_A0E0);
    assert_eq!(color.packed().to_le_bytes(), [0xE0, 0xA0, 0x30, 0xFF]);
}
