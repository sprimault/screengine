// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests des primitives de polygone.

use super::*;

/// Newell rend la normale d'un carré, et sa longueur vaut le double de l'aire.
#[test]
fn newell_rend_la_normale_et_le_double_de_l_aire() {
    let carre = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(0.0, 2.0, 0.0),
    ];
    let normal = newell(&carre);
    assert_eq!(normal, Vec3::new(0.0, 0.0, 8.0));
}

/// Newell tient quand les trois premiers sommets sont alignés, ce qu'un produit
/// vectoriel sur eux seuls ne ferait pas.
///
/// C'est la raison d'être de cette formule, et elle serait perdue en silence si
/// quelqu'un la remplaçait par le produit des trois premiers.
#[test]
fn newell_tient_sur_trois_premiers_sommets_alignes() {
    let polygone = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(0.0, 2.0, 0.0),
    ];
    assert_eq!(newell(&polygone), Vec3::new(0.0, 0.0, 8.0));
}

/// Un polygone parcouru à l'envers rend la normale opposée.
#[test]
fn newell_suit_l_enroulement() {
    let direct = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ];
    let inverse = [direct[0], direct[2], direct[1]];
    assert_eq!(newell(&direct), -newell(&inverse));
}

/// Un polygone sans sommet rend le vecteur nul plutôt que de paniquer.
#[test]
fn newell_sans_sommet_rend_le_vecteur_nul() {
    assert_eq!(newell(&[]), Vec3::ZERO);
}

/// Les rangs désignent bien x, y puis z, et tout rang au-delà rend la cote.
#[test]
fn axis_designe_les_composantes_par_leur_rang() {
    let v = Vec3::new(1.0, 2.0, 3.0);
    assert_eq!(axis(v, 0), 1.0);
    assert_eq!(axis(v, 1), 2.0);
    assert_eq!(axis(v, 2), 3.0);
}

/// La valeur absolue laisse passer le zéro négatif, et cela ne gêne personne.
///
/// `-0,0 < 0,0` est faux, donc la négation ne s'applique pas et le zéro ressort
/// signé comme il est entré. Écrit parce que la tentation de « corriger » est
/// réelle : ses deux appelants comparent des valeurs absolues entre elles, et
/// `-0,0` et `0,0` se comparent égaux, si bien qu'aucun choix d'axe ne change.
/// Normaliser le signe ici n'apporterait donc rien et réécrirait une expression
/// que deux chemins d'empreinte traversent.
#[test]
fn abs_laisse_passer_le_zero_negatif() {
    assert!(abs(-0.0).is_sign_negative());
    assert!(abs(-0.0) >= 0.0 && abs(-0.0) <= 0.0);
    assert_eq!(abs(-2.5), 2.5);
    assert_eq!(abs(2.5), 2.5);
}
