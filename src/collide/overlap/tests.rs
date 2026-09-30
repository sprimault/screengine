// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests du prédicat de recouvrement.
//!
//! Ils portent sur le test boîte contre triangle, qui est ce que le prédicat a
//! de propre : le reste — retrouver les triangles d'une surface, mesurer la
//! profondeur sur la normale — se lit dans les tests du balayage, où une carte
//! existe.

use super::*;

/// Un triangle rectangle de quatre unités dans le plan `z = 0`.
fn triangle() -> [Vec3d; 3] {
    [
        Vec3d::new(0.0, 0.0, 0.0),
        Vec3d::new(4.0, 0.0, 0.0),
        Vec3d::new(0.0, 4.0, 0.0),
    ]
}

/// Une boîte posée sur le triangle le recouvre.
#[test]
fn une_boite_posee_sur_le_triangle_le_recouvre() {
    let half = Vec3d::new(1.0, 1.0, 1.0);
    assert!(overlaps_triangle(
        triangle(),
        half,
        Vec3d::new(1.0, 1.0, 0.5)
    ));
}

/// Une boîte au-dessus du plan, hors de portée, ne le recouvre pas.
///
/// C'est la normale du triangle qui sépare, et ce test la fait travailler seule :
/// les trois axes de la boîte, eux, ne concluent pas ici.
#[test]
fn une_boite_trop_haute_ne_recouvre_pas() {
    let half = Vec3d::new(1.0, 1.0, 1.0);
    assert!(!overlaps_triangle(
        triangle(),
        half,
        Vec3d::new(1.0, 1.0, 5.0)
    ));
}

/// Une boîte dans le plan mais à côté du triangle ne le recouvre pas.
///
/// Elle est à portée sur les trois axes de la boîte **et** dans le plan : seuls
/// les produits croisés la séparent, et sans eux le prédicat rendrait vrai. C'est
/// le test qui justifie les neuf axes.
#[test]
fn une_boite_a_cote_dans_le_plan_ne_recouvre_pas() {
    let half = Vec3d::new(0.5, 0.5, 0.5);
    assert!(!overlaps_triangle(
        triangle(),
        half,
        Vec3d::new(3.0, 3.0, 0.0)
    ));
}

/// Une boîte qui touche le triangle par son coin le recouvre.
#[test]
fn une_boite_qui_touche_par_le_coin_recouvre() {
    let half = Vec3d::new(1.0, 1.0, 1.0);
    assert!(overlaps_triangle(
        triangle(),
        half,
        Vec3d::new(-0.5, -0.5, 0.0)
    ));
}

/// Un triangle dégénéré ne fait pas échouer le prédicat.
///
/// Sa normale est nulle et l'axe correspondant est sauté : le chargement ne
/// vérifie pas la cohérence géométrique, par clause, donc c'est ici que le cas se
/// traite — sans panique et sans division.
#[test]
fn un_triangle_degenere_ne_fait_pas_echouer_le_predicat() {
    let plat = [
        Vec3d::new(0.0, 0.0, 0.0),
        Vec3d::new(4.0, 0.0, 0.0),
        Vec3d::new(8.0, 0.0, 0.0),
    ];
    let half = Vec3d::new(1.0, 1.0, 1.0);
    assert!(overlaps_triangle(plat, half, Vec3d::new(4.0, 0.0, 0.0)));
    assert!(!overlaps_triangle(plat, half, Vec3d::new(4.0, 50.0, 0.0)));
}

/// La longueur retrouve la racine d'un carré, sur plusieurs ordres de grandeur.
///
/// À moins de quelques ulp : elle ne sert qu'à convertir une profondeur en unités
/// de monde, jamais à décider d'un contact. Le contrôle passe par le carré du
/// résultat, ce qui évite de comparer à une racine calculée autrement — et donc
/// d'introduire ici la bibliothèque que le module existe pour éviter.
#[test]
fn la_longueur_retrouve_la_racine() {
    for square in [1.0, 4.0, 1024.0, 0.25, 1e-12, 1e12, 2.0, 3.0] {
        let length: f64 = length_of(square);
        let relative = (length * length - square) / square;
        assert!(
            relative > -1e-12 && relative < 1e-12,
            "carré {square}, longueur {length}"
        );
    }
}

/// Un carré nul ou négatif rend une longueur neutre plutôt que de diverger.
///
/// Le cas ne devrait pas se présenter — une normale nulle est écartée avant —,
/// mais l'itération de Newton diviserait par zéro s'il arrivait, et une panique
/// dans un chemin sans code de retour n'a nulle part où aller.
#[test]
fn un_carre_nul_rend_une_longueur_neutre() {
    assert_eq!(length_of(0.0), 1.0);
    assert_eq!(length_of(-1.0), 1.0);
}
