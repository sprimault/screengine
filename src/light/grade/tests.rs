// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que les courbes rendent, et surtout ce qu'elles ne changent pas.

use super::*;

/// Un pixel dont les trois canaux diffèrent, alpha compris : une permutation de
/// canaux ou un alpha emporté s'y voient, là où un gris les cacherait.
const PIXEL: u32 = 0xC0_40_80_20;

/// Des gains neutres, que la plupart des cas n'ont pas à faire varier.
const UNIT: [f32; CHANNELS] = [1.0; CHANNELS];

/// Des décalages neutres.
const ZERO: [f32; CHANNELS] = [0.0; CHANNELS];

/// Un post-traitement neutre par vacuité laisse le pixel intact.
///
/// C'est l'invariant du lot : un contexte qu'on ne configure pas rend ce qu'il
/// rendait avant que ce module existe, et les douze empreintes de conformance
/// en dépendent.
#[test]
fn un_post_traitement_neutre_ne_change_rien() {
    let grade = Grade::new().expect("réservation");
    assert!(!grade.is_set());
}

/// Réglé à l'identité, il rend le pixel intact lui aussi — au bit près.
///
/// C'est le test qui éprouve vraiment `powf` : `x^1` doit retomber sur `x` à
/// moins d'un demi-niveau sur les deux cent cinquante-six valeurs, sans quoi la
/// table identité ne serait pas l'identité. « Neutre par vacuité » et « neutre
/// par paramètres » doivent coïncider, faute de quoi l'un des deux ment.
#[test]
fn une_identite_reglee_ne_change_rien() {
    let mut grade = Grade::new().expect("réservation");
    grade.set(1.0, UNIT, ZERO).expect("réglage valide");
    assert!(grade.is_set());

    for level in 0..LEVELS {
        for channel in 0..CHANNELS {
            assert_eq!(
                grade.tables[channel * LEVELS + level],
                level as u8,
                "canal {channel}, niveau {level}"
            );
        }
    }
    assert_eq!(grade.apply(PIXEL), PIXEL & 0x00FF_FFFF);
}

/// `clear` ramène à l'état neutre, et un contexte qui n'a jamais rien réglé ne
/// se distingue pas d'un contexte qu'on a éteint.
#[test]
fn eteindre_ramene_a_l_etat_neutre() {
    let mut grade = Grade::new().expect("réservation");
    grade.set(2.2, UNIT, ZERO).expect("réglage valide");
    grade.clear();
    assert!(!grade.is_set());
}

/// Le gamma va dans le sens qu'un intégrateur attend : 2,2 éclaircit.
///
/// Le sens inverse marcherait aussi bien et surprendrait tout le monde une
/// fois ; c'est exactement le genre de convention qu'un test doit river.
#[test]
fn un_gamma_de_deux_deux_eclaircit() {
    let mut grade = Grade::new().expect("réservation");
    grade.set(2.2, UNIT, ZERO).expect("réglage valide");

    // Les extrêmes restent des extrêmes : une courbe de transfert ne déplace
    // ni le noir ni le blanc.
    assert_eq!(grade.tables[0], 0);
    assert_eq!(grade.tables[LEVELS - 1], 255);

    for level in 1..LEVELS - 1 {
        assert!(
            grade.tables[level] > level as u8,
            "niveau {level} : {} devrait être plus clair",
            grade.tables[level]
        );
    }
}

/// Chaque gain va sur son canal, et les trois tables ne sont pas permutées.
///
/// **Les trois gains doivent être distincts, et le pixel gris.** Avec deux
/// gains égaux, une permutation des deux canaux correspondants passerait
/// inaperçue — c'est le cas que ce test laissait filer dans sa première
/// écriture, et seule la conformance l'avait vu. Un gris rend les trois
/// sorties comparables entre elles : elles doivent décroître dans l'ordre des
/// gains, ce qu'aucune permutation ne préserve.
#[test]
fn chaque_gain_va_sur_son_canal() {
    let mut grade = Grade::new().expect("réservation");
    grade
        .set(1.0, [1.0, 0.75, 0.5], ZERO)
        .expect("réglage valide");

    let sortie = grade.apply(0x00_80_80_80);
    let (rouge, vert, bleu) = (sortie & 0xFF, sortie >> 8 & 0xFF, sortie >> 16 & 0xFF);

    assert_eq!(rouge, 0x80, "un gain de un laisse le canal intact");
    assert!(
        rouge > vert && vert > bleu,
        "les trois canaux doivent décroître comme leurs gains : {rouge}, {vert}, {bleu}"
    );
}

/// Le décalage relève le noir — ce que ni le gain ni le gamma ne savent faire.
///
/// C'est la raison d'être du champ, et elle tient en une phrase : le gamma fixe
/// les deux extrêmes, et un gain multiplie donc laisse le zéro à zéro. Seule
/// une affine complète atteint cette famille d'images, et c'est pour elle que
/// la structure publiée porte trois décalages plutôt que de les renvoyer à
/// plus tard, où ils n'auraient plus tenu dans ses champs réservés.
#[test]
fn un_decalage_releve_le_noir() {
    let mut grade = Grade::new().expect("réservation");

    // Ni l'un ni l'autre ne bouge le noir.
    grade.set(2.2, [4.0; CHANNELS], ZERO).expect("valide");
    assert_eq!(grade.tables[0], 0, "gain et gamma laissent le noir à zéro");

    grade.set(1.0, UNIT, [0.25; CHANNELS]).expect("valide");
    assert!(grade.tables[0] > 0, "le décalage relève le noir");
    assert_eq!(grade.tables[LEVELS - 1], 255, "le blanc reste plein");

    // Et dans l'autre sens, il abaisse le blanc.
    grade.set(1.0, [0.5; CHANNELS], ZERO).expect("valide");
    assert!(grade.tables[LEVELS - 1] < 255, "le gain abaisse le blanc");
    assert_eq!(grade.tables[0], 0, "sans toucher au noir");
}

/// Le gain sature avant le gamma, et la table s'arrête à l'octet plein.
#[test]
fn un_gain_fort_sature_sans_deborder() {
    let mut grade = Grade::new().expect("réservation");
    grade
        .set(1.0, [MAX_GAIN; CHANNELS], ZERO)
        .expect("réglage valide");

    // Le plein est atteint dès le quart de l'échelle — exactement au niveau
    // qui suit 255/4, et pas avant : c'est ce « pas avant » qui dit que le
    // bornage porte sur la bonne valeur.
    assert_eq!(grade.tables[LEVELS / 4], 255);
    assert!(grade.tables[LEVELS / 4 - 1] < 255);
    assert_eq!(grade.tables[LEVELS - 1], 255);
    assert_eq!(grade.tables[0], 0, "le noir reste noir");
}

/// Ce que le réglage refuse, et la preuve qu'un refus ne laisse rien derrière.
#[test]
fn les_reglages_hors_bornes_sont_refuses() {
    let mut grade = Grade::new().expect("réservation");
    let refus = Err(Error::InvalidArgument(Argument::Grade));

    assert_eq!(grade.set(0.0, UNIT, ZERO), refus, "gamma nul");
    assert_eq!(grade.set(-1.0, UNIT, ZERO), refus, "gamma négatif");
    assert_eq!(grade.set(f32::NAN, UNIT, ZERO), refus, "gamma NaN");
    let haut = grade.set(MAX_GAMMA + 0.1, UNIT, ZERO);
    assert_eq!(haut, refus, "gamma au-delà de la borne");
    let negatif = grade.set(1.0, [-0.1, 1.0, 1.0], ZERO);
    assert_eq!(negatif, refus, "gain négatif");
    let gain = grade.set(1.0, [1.0, MAX_GAIN + 0.1, 1.0], ZERO);
    assert_eq!(gain, refus, "gain au-delà de la borne");
    let infini = grade.set(1.0, [1.0, 1.0, f32::INFINITY], ZERO);
    assert_eq!(infini, refus, "gain infini");
    let bas = grade.set(1.0, UNIT, [-MAX_OFFSET - 0.1, 0.0, 0.0]);
    assert_eq!(bas, refus, "décalage sous la borne");
    let sup = grade.set(1.0, UNIT, [0.0, MAX_OFFSET + 0.1, 0.0]);
    assert_eq!(sup, refus, "décalage au-delà de la borne");
    let nan = grade.set(1.0, UNIT, [0.0, 0.0, f32::NAN]);
    assert_eq!(nan, refus, "décalage NaN");

    assert!(!grade.is_set(), "un refus ne doit rien laisser derrière");
}

/// Régler ne réalloue pas : la capacité est prise une fois pour toutes à la
/// création, et un hôte qui ajuste sa courbe entre deux images n'alloue pas
/// davantage qu'un hôte qui n'y touche jamais.
#[test]
fn le_reglage_n_alloue_pas() {
    let mut grade = Grade::new().expect("réservation");
    let capacity = grade.tables.capacity();

    for gamma in 1..=20 {
        grade
            .set(gamma as f32 / 10.0, UNIT, ZERO)
            .expect("réglage valide");
        grade.clear();
    }
    assert_eq!(grade.tables.capacity(), capacity);
}
