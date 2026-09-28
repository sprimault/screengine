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

/// Une échelle non uniforme, à coefficients bien séparés de zéro.
fn scale(rng: &mut Rng) -> Affine3 {
    let mut s = || rng.unit_f32() * 4.8 + 0.2;
    let (sx, sy, sz) = (s(), s(), s());
    Affine3 {
        m: [sx, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, sz, 0.0, 0.0, 0.0],
    }
}

/// La propriété qui définit une normale : elle reste perpendiculaire à la
/// surface une fois celle-ci transformée.
///
/// Le cas qui compte est l'échelle non uniforme, la seule où porter la normale
/// comme une direction ordinaire donne un autre vecteur. Deux tangentes portées
/// par la transformation, leur normale portée par les cofacteurs, et l'angle se
/// mesure sur les vecteurs unitaires : à ces amplitudes, un écart absolu ne
/// dirait rien.
#[test]
fn les_cofacteurs_gardent_la_normale_perpendiculaire() {
    let mut rng = Rng::new(31);
    for _ in 0..10_000 {
        let m = rigid(&mut rng).product(scale(&mut rng));
        let (t1, t2) = (vector(&mut rng), vector(&mut rng));
        let normal = t1.cross(t2);
        if normal.dot(normal) < 1.0 {
            continue;
        }
        let carried = m.cofactors().0.transform_vector(normal).normalize();
        for tangent in [t1, t2] {
            let moved = m.transform_vector(tangent).normalize();
            assert!(carried.dot(moved).abs() < 1.0e-4, "{carried:?} {moved:?}");
        }
    }
}

/// Une face à 45° étirée du double en x : le cas concret qui sépare les deux
/// écritures, et qui a motivé le correctif.
#[test]
fn une_echelle_incline_la_normale_dans_le_bon_sens() {
    let m = Affine3 {
        m: [2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    };
    let normal = Vec3::new(1.0, 1.0, 0.0);
    // La tangente de la face, portée par la transformation.
    let tangent = m.transform_vector(Vec3::new(1.0, -1.0, 0.0));
    let carried = m.cofactors().0.transform_vector(normal);
    assert!(carried.normalize().dot(tangent.normalize()).abs() < 1.0e-6);
    // Portée comme une direction, elle penche de l'autre côté : c'est le défaut
    // que ce test retiendrait s'il revenait.
    assert!(
        m.transform_vector(normal)
            .normalize()
            .dot(tangent.normalize())
            .abs()
            > 0.5
    );
}

/// Sur une rotation, les cofacteurs redonnent la rotation : c'est ce qui fait
/// qu'aucune scène sans échelle ne change d'image.
///
/// À une tolérance et non en bits : trois produits vectoriels ne rendent pas
/// les coefficients d'origine au dernier bit.
#[test]
fn les_cofacteurs_d_une_rotation_la_redonnent() {
    let mut rng = Rng::new(37);
    for _ in 0..10_000 {
        let m = rigid(&mut rng);
        let v = vector(&mut rng).normalize();
        let direct = m.transform_vector(v);
        let carried = m.cofactors().0.transform_vector(v);
        let d = direct - carried;
        assert!(d.dot(d) < 1.0e-6, "{direct:?} {carried:?}");
    }
}

/// Un miroir suit le reflet plutôt que de retourner la normale.
///
/// Sans le signe du déterminant, une face qui regarde le `+x` verrait sa
/// normale pointer vers le `+x` de son reflet, donc vers l'intérieur, et
/// tournerait le dos à toutes les lumières.
#[test]
fn un_miroir_porte_la_normale_avec_le_reflet() {
    let m = Affine3 {
        m: [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    };
    let (carried, determinant) = m.cofactors();
    assert_eq!(
        carried.transform_vector(Vec3::new(1.0, 0.0, 0.0)),
        Vec3::new(-1.0, 0.0, 0.0)
    );
    assert!(determinant < 0.0);
}

/// Un modèle aplati rend des normales nulles, jamais des NaN.
///
/// Aplaties, pas toutes : celle qui reste perpendiculaire au plan écrasé garde
/// sa direction, et c'est la bonne. Ce sont les normales *dans* ce plan qui
/// s'annulent, et une normale nulle est une normale absente pour l'éclairage
/// dynamique, qui retombe alors sur l'atténuation par la distance.
#[test]
fn un_determinant_nul_aplatit_les_normales_du_plan_ecrase() {
    let flat = Affine3 {
        m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    let (carried, determinant) = flat.cofactors();
    assert_eq!(determinant, 0.0);
    assert_eq!(
        carried.transform_vector(Vec3::new(0.0, 0.0, 1.0)),
        Vec3::new(0.0, 0.0, 1.0)
    );
    assert_eq!(
        carried.transform_vector(Vec3::new(1.0, 0.0, 0.0)),
        Vec3::ZERO
    );
}

/// Le déterminant vaut un sur une transformation rigide directe : c'est ce qui
/// rend les cofacteurs de la composée égaux à ceux de la seule matrice modèle.
#[test]
fn une_transformation_rigide_a_pour_determinant_un() {
    let mut rng = Rng::new(41);
    for _ in 0..10_000 {
        assert!((rigid(&mut rng).determinant() - 1.0).abs() < 1.0e-5);
    }
}

/// Le signe des normales et le déterminant rendu s'accordent toujours.
///
/// Les deux décident ensemble — l'un du sens des normales, l'autre du sens de
/// parcours attendu des faces —, et un lot presque plat est précisément là où
/// deux calculs séparés pourraient les séparer. Le test le vérifie sur des
/// matrices dont le déterminant traverse zéro.
#[test]
fn le_determinant_rendu_est_celui_qui_a_signe_les_normales() {
    let mut rng = Rng::new(43);
    for _ in 0..10_000 {
        let mut m = rigid(&mut rng).product(scale(&mut rng));
        // Une colonne ramenée vers zéro puis niée une fois sur deux : le
        // déterminant passe d'un signe à l'autre en frôlant le zéro.
        let squeeze = rng.unit_f32() * 2.0 - 1.0;
        for slot in &mut m.m[6..9] {
            *slot *= squeeze;
        }
        let (carried, determinant) = m.cofactors();
        // `Cof` sans signe est ce que rend le produit vectoriel des deux
        // premières colonnes : la troisième colonne de la matrice rendue doit
        // lui être égale au signe du déterminant près, et à lui seul.
        let c0 = Vec3::new(m.m[0], m.m[1], m.m[2]);
        let c1 = Vec3::new(m.m[3], m.m[4], m.m[5]);
        let expected = c0.cross(c1);
        let got = Vec3::new(carried.m[6], carried.m[7], carried.m[8]);
        let sign = if determinant < 0.0 { -1.0 } else { 1.0 };
        assert_eq!(got, expected * sign, "det {determinant}");
    }
}
