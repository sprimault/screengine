// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la scène des rotations doit éprouver, et qu'une empreinte ne dit pas.
//!
//! **Une empreinte dit qu'un résultat a changé, jamais qu'il est juste.** Une
//! scène dont tous les quaternions seraient l'identité se figerait aussi bien
//! qu'une autre, et c'est le mode de panne d'une liste d'axes mal choisie.

use super::*;

/// Les rotations tournent vraiment, et aucune n'est l'identité par accident.
///
/// Sauf la dernière, qui doit l'être : l'axe nul rend l'identité par contrat.
/// Les deux clauses vivent dans le même cas parce qu'elles se gardent l'une
/// l'autre — une scène dont tout serait l'identité passerait la première seule.
#[test]
fn les_axes_tournent_sauf_celui_qui_est_nul() {
    let identite = |q: Quat| q.x == 0.0 && q.y == 0.0 && q.z == 0.0;

    for (index, (axis, angle)) in AXES.iter().zip(ANGLES).enumerate() {
        let q = Quat::from_axis_angle(*axis, Angle(angle));
        let nul = *axis == Vec3::ZERO;
        assert_eq!(
            identite(q),
            nul,
            "axe {index} : identité attendue seulement pour l'axe nul"
        );
    }

    assert!(
        AXES.contains(&Vec3::ZERO),
        "aucun axe nul : la clause du contrat n'est pas éprouvée"
    );
}

/// Le produit n'est pas commutatif sur cette liste, donc l'ordre de l'ABI est
/// éprouvé.
///
/// **Sans cette garde, inverser les arguments du produit laisserait l'empreinte
/// inchangée** : deux rotations autour d'axes colinéaires commutent, et une
/// liste qui ne porterait que celles-là ferait d'une erreur d'ordre un défaut
/// invisible — jusqu'à ce qu'un hôte en compose deux quelconques.
#[test]
fn l_ordre_du_produit_se_voit_sur_cette_liste() {
    let rotations: Vec<Quat> = AXES
        .iter()
        .zip(ANGLES)
        .map(|(axis, angle)| Quat::from_axis_angle(*axis, Angle(angle)))
        .collect();

    let differents = rotations
        .iter()
        .flat_map(|a| rotations.iter().map(move |b| (a, b)))
        .filter(|(a, b)| product(**a, **b) != product(**b, **a))
        .count();

    assert!(
        differents > 0,
        "tous les produits commutent : l'ordre des arguments n'est pas gardé"
    );
}

/// Les angles ne tombent pas tous sur une entrée de la table, donc
/// l'interpolation entre dans l'empreinte.
///
/// La table du noyau porte ses valeurs sur les bits de poids fort de l'angle ;
/// un angle dont les bits de poids faible sont nuls tombe sur une entrée et
/// n'éprouve que la lecture.
#[test]
fn un_angle_au_moins_tombe_entre_deux_entrees() {
    let interpole = ANGLES.iter().filter(|angle| **angle & 0xFFFF != 0).count();
    assert!(
        interpole > 0,
        "tous les angles tombent sur une entrée de table"
    );
}

/// La matrice porte la translation dans sa dernière colonne, et la rotation
/// dans les trois premières.
///
/// Le cas qui attrape une transposition : une matrice transposée garde les
/// mêmes seize coefficients, donc la même empreinte si l'absorbeur l'était
/// aussi, et ne se voit que sur une composante nommée.
#[test]
fn la_translation_occupe_la_derniere_colonne() {
    let q = Quat::from_axis_angle(AXES[0], Angle(ANGLES[0]));
    let m = Affine3::from_rotation_translation(q, TRANSLATION);
    assert_eq!(
        [m.m[9], m.m[10], m.m[11]],
        [TRANSLATION.x, TRANSLATION.y, TRANSLATION.z],
        "la translation n'est pas dans la dernière colonne"
    );
}
