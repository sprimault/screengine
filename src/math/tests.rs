// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Vecteurs, quaternions et transformations : ce qui est exact se vérifie en
//! bits, le reste à une tolérance près, jamais l'inverse.

use super::*;
use crate::testing::{Rng, fnv1a};

/// Un vecteur tiré au hasard dans [-100, 100)³.
fn vector(rng: &mut Rng) -> Vec3 {
    let mut c = || rng.unit_f32() * 200.0 - 100.0;
    Vec3::new(c(), c(), c())
}

/// Une transformation rigide tirée au hasard.
fn rigid(rng: &mut Rng) -> Affine3 {
    let axis = vector(rng);
    let q = Quat::from_axis_angle(axis, Angle(rng.next() as u32));
    Affine3::from_rotation_translation(q, vector(rng))
}

/// Les bits d'un vecteur, pour une empreinte.
fn bits(v: Vec3) -> [u8; 12] {
    let mut out = [0; 12];
    for (slot, c) in out.chunks_exact_mut(4).zip([v.x, v.y, v.z]) {
        slot.copy_from_slice(&c.to_bits().to_le_bytes());
    }
    out
}

/// Le quaternion (½, ½, ½, ½) est la rotation d'un tiers de tour autour de
/// (1, 1, 1) : elle permute les axes, et tous ses coefficients sont des
/// dyadiques. La matrice doit donc être exacte, et la permutation aussi.
#[test]
fn un_tiers_de_tour_permute_les_axes_exactement() {
    let q = Quat::new(0.5, 0.5, 0.5, 0.5);
    let m = Affine3::from_rotation_translation(q, Vec3::ZERO);
    let (x, y, z) = (
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    assert_eq!(bits(m.transform_vector(x)), bits(y));
    assert_eq!(bits(m.transform_vector(y)), bits(z));
    assert_eq!(bits(m.transform_vector(z)), bits(x));
}

/// Un quart de tour autour de Z, main droite : X va sur Y. C'est le sens que
/// l'éditeur voit de dessus, et un signe inversé dans la conversion le
/// retournerait sans rien casser d'autre.
#[test]
fn un_quart_de_tour_autour_de_z_envoie_x_sur_y() {
    let q = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), Angle::QUARTER);
    let m = Affine3::from_rotation_translation(q, Vec3::ZERO);
    let d = m.transform_vector(Vec3::new(1.0, 0.0, 0.0)) - Vec3::new(0.0, 1.0, 0.0);
    assert!(d.dot(d) < 1.0e-12, "{d:?}");
}

/// `transform_point` rend les bits de l'expression écrite de gauche à droite,
/// colonne par colonne. Ce test protège l'ordre des opérations contre une
/// réécriture « équivalente » qui ne l'est pas en flottant.
#[test]
fn la_transformation_suit_l_ordre_ecrit() {
    let mut rng = Rng::new(11);
    for _ in 0..10_000 {
        let a = rigid(&mut rng);
        let p = vector(&mut rng);
        let m = a.m;
        let expected = Vec3::new(
            ((m[0] * p.x + m[3] * p.y) + m[6] * p.z) + m[9],
            ((m[1] * p.x + m[4] * p.y) + m[7] * p.z) + m[10],
            ((m[2] * p.x + m[5] * p.y) + m[8] * p.z) + m[11],
        );
        assert_eq!(bits(a.transform_point(p)), bits(expected));
    }
}

/// Le produit de transformations applique d'abord la droite : `(a · b)(p)`
/// vaut `a(b(p))`, à l'arrondi près.
#[test]
fn le_produit_applique_d_abord_la_droite() {
    let mut rng = Rng::new(13);
    for _ in 0..10_000 {
        let (a, b, p) = (rigid(&mut rng), rigid(&mut rng), vector(&mut rng));
        let composed = a.product(b).transform_point(p);
        let nested = a.transform_point(b.transform_point(p));
        let d = composed - nested;
        assert!(d.dot(d) < 1.0e-6, "{composed:?} {nested:?}");
    }
}

/// L'inverse rigide ramène un point à sa place, à l'arrondi près.
#[test]
fn l_inverse_rigide_ramene_le_point() {
    let mut rng = Rng::new(17);
    for _ in 0..10_000 {
        let (a, p) = (rigid(&mut rng), vector(&mut rng));
        let back = a.inverse_rigid().transform_point(a.transform_point(p));
        assert!((back - p).dot(back - p) < 1.0e-6, "{back:?} {p:?}");
    }
}

/// Le produit de quaternions et le produit de matrices décrivent la même
/// rotation : une erreur de signe dans l'un des deux se voit ici.
#[test]
fn le_produit_de_quaternions_est_celui_des_rotations() {
    let mut rng = Rng::new(19);
    for _ in 0..10_000 {
        let qa = Quat::from_axis_angle(vector(&mut rng), Angle(rng.next() as u32));
        let qb = Quat::from_axis_angle(vector(&mut rng), Angle(rng.next() as u32));
        let p = vector(&mut rng);
        let rotation = |q| Affine3::from_rotation_translation(q, Vec3::ZERO);
        let via_quat = rotation(qa.product(qb)).transform_point(p);
        let via_matrix = rotation(qa).product(rotation(qb)).transform_point(p);
        let d = via_quat - via_matrix;
        assert!(d.dot(d) < 1.0e-6, "{via_quat:?} {via_matrix:?}");
    }
}

/// La normalisation rend une longueur un, et le vecteur nul pour une longueur
/// négligeable plutôt qu'un NaN.
#[test]
fn la_normalisation_rend_une_longueur_un_ou_le_nul() {
    let mut rng = Rng::new(23);
    for _ in 0..10_000 {
        let v = vector(&mut rng).normalize();
        assert!((v.dot(v) - 1.0).abs() < 1.0e-5, "{v:?}");
    }
    assert_eq!(Vec3::ZERO.normalize(), Vec3::ZERO);
    assert_eq!(Vec3::new(1.0e-16, 0.0, 0.0).normalize(), Vec3::ZERO);
}

/// L'interpolation rend ses extrémités, et prend le plus court chemin : entre
/// `q` et `-q`, qui sont la même orientation, elle ne tourne pas.
#[test]
fn l_interpolation_rend_ses_extremites_par_le_plus_court_chemin() {
    let q = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), Angle(123_456_789));
    let r = Quat::from_axis_angle(Vec3::new(-2.0, 0.5, 1.0), Angle(987_654_321));
    let close = |a: Quat, b: Quat| (a.dot(b).abs() - 1.0).abs() < 1.0e-6;
    assert!(close(q.nlerp(r, 0.0), q));
    assert!(close(q.nlerp(r, 1.0), r));
    let opposite = Quat::new(-q.x, -q.y, -q.z, -q.w);
    assert!(close(q.nlerp(opposite, 0.5), q));
}

/// L'empreinte de transformations tirées à graine fixe, en bits. C'est elle que
/// la conformance croisée compare d'une cible à l'autre : un écart de
/// plateforme dans la trigonométrie, la racine inverse ou l'ordre des sommes
/// la fait changer.
#[test]
fn l_empreinte_des_transformations_est_figee() {
    let mut rng = Rng::new(29);
    let mut bytes = alloc::vec::Vec::new();
    for _ in 0..1_000 {
        let (a, b, p) = (rigid(&mut rng), rigid(&mut rng), vector(&mut rng));
        bytes.extend_from_slice(&bits(a.product(b).inverse_rigid().transform_point(p)));
    }
    assert_eq!(fnv1a(bytes), TRANSFORM_FINGERPRINT);
}

/// L'empreinte attendue des transformations.
const TRANSFORM_FINGERPRINT: u64 = 14_201_116_867_928_617_966;
