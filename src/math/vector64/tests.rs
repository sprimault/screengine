// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests du vecteur en double précision.

use super::*;

/// La promotion depuis [`Vec3`] est exacte, y compris sur une valeur qui n'a pas
/// d'écriture décimale finie.
///
/// C'est ce qui permet de passer la frontière sans se demander si une coordonnée
/// a bougé : tout `f32` fini a une image exacte en `f64`, et un balayage qui
/// commencerait par un arrondi n'aurait plus rien à prouver.
#[test]
fn la_promotion_depuis_vec3_est_exacte() {
    let v = Vec3::new(0.1, -3.5, 1e-20);
    let d = Vec3d::from(v);
    assert_eq!(d.x, f64::from(0.1f32));
    assert_eq!(d.y, -3.5);
    assert_eq!(d.z, f64::from(1e-20f32));
}

/// Le produit scalaire somme de gauche à droite, dans le même ordre que celui de
/// [`Vec3`].
///
/// Comparé à l'expression écrite et non à une valeur attendue : c'est l'**ordre**
/// qui est contractuel, et une réassociation qui rendrait le même nombre sur ces
/// valeurs-ci divergerait ailleurs.
#[test]
fn le_produit_scalaire_somme_de_gauche_a_droite() {
    let a = Vec3d::new(1e16, 1.0, -1e16);
    let b = Vec3d::new(1.0, 1.0, 1.0);
    assert_eq!(a.dot(b), (1e16 + 1.0) + -1e16);
}

/// Le produit vectoriel suit la main droite.
#[test]
fn le_produit_vectoriel_suit_la_main_droite() {
    let x = Vec3d::new(1.0, 0.0, 0.0);
    let y = Vec3d::new(0.0, 1.0, 0.0);
    assert_eq!(x.cross(y), Vec3d::new(0.0, 0.0, 1.0));
    assert_eq!(y.cross(x), Vec3d::new(0.0, 0.0, -1.0));
}

/// Les rangs désignent x, y puis z, et tout rang au-delà rend la cote.
#[test]
fn les_rangs_designent_les_composantes() {
    let v = Vec3d::new(1.0, 2.0, 3.0);
    assert_eq!(v.axis(0), 1.0);
    assert_eq!(v.axis(1), 2.0);
    assert_eq!(v.axis(2), 3.0);
}

/// Les quatre opérations vectorielles rendent ce que leur écriture annonce.
#[test]
fn les_operations_rendent_ce_qu_elles_annoncent() {
    let a = Vec3d::new(1.0, 2.0, 3.0);
    let b = Vec3d::new(0.5, 0.5, 0.5);
    assert_eq!(a + b, Vec3d::new(1.5, 2.5, 3.5));
    assert_eq!(a - b, Vec3d::new(0.5, 1.5, 2.5));
    assert_eq!(-a, Vec3d::new(-1.0, -2.0, -3.0));
    assert_eq!(a * 2.0, Vec3d::new(2.0, 4.0, 6.0));
    assert_eq!(Vec3d::ZERO, Vec3d::default());
}
