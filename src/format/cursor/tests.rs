// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests du curseur borné.

use super::*;

/// L'erreur qu'une lecture hors borne doit rendre, écrite une fois.
const TRUNCATED: Error = Error::InvalidFormat(Malformation::Truncated);

/// Les entiers se lisent en petit-boutiste, quelle que soit la cible.
///
/// Le test porte des octets distincts par position : une lecture en
/// gros-boutiste rendrait les mêmes valeurs sur une suite palindrome, et
/// passerait.
#[test]
fn les_entiers_se_lisent_en_petit_boutiste() {
    let bytes = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
    let mut cursor = Cursor::new(&bytes);
    assert_eq!(cursor.u8().unwrap(), 0x01);
    assert_eq!(cursor.u16().unwrap(), 0x0302);
    assert_eq!(cursor.u32().unwrap(), 0x0706_0504);
    assert_eq!(cursor.remaining(), 0);
}

/// Chaque lecture avance du nombre d'octets qu'elle consomme, et d'aucun autre.
///
/// Un curseur qui avancerait de trop rendrait des champs décalés sans jamais
/// sortir du bloc : un défaut qui ne se voit pas en borne mais en contenu.
#[test]
fn chaque_lecture_avance_de_sa_largeur() {
    let bytes = [0; 16];
    let mut cursor = Cursor::new(&bytes);
    cursor.u8().unwrap();
    assert_eq!(cursor.offset(), 1);
    cursor.u16().unwrap();
    assert_eq!(cursor.offset(), 3);
    cursor.u32().unwrap();
    assert_eq!(cursor.offset(), 7);
    cursor.tag().unwrap();
    assert_eq!(cursor.offset(), 11);
    cursor.f32().unwrap();
    assert_eq!(cursor.offset(), 15);
    cursor.take(1).unwrap();
    assert_eq!(cursor.offset(), 16);
}

/// Une lecture qui dépasse la fin est refusée, y compris d'un seul octet.
#[test]
fn une_lecture_hors_borne_est_refusee() {
    let bytes = [0; 3];
    assert_eq!(Cursor::new(&bytes).u32().unwrap_err(), TRUNCATED);
    assert_eq!(Cursor::new(&bytes).tag().unwrap_err(), TRUNCATED);
    assert_eq!(Cursor::new(&bytes).take(4).unwrap_err(), TRUNCATED);
    assert_eq!(Cursor::new(&[]).u8().unwrap_err(), TRUNCATED);
}

/// Une lecture refusée laisse le curseur où il était.
///
/// Sans quoi un décodeur qui rattraperait une erreur — ou un test qui enchaîne
/// deux tentatives — lirait la suite depuis une position avancée à moitié.
#[test]
fn une_lecture_refusee_n_avance_pas() {
    let bytes = [0; 2];
    let mut cursor = Cursor::new(&bytes);
    assert_eq!(cursor.u32().unwrap_err(), TRUNCATED);
    assert_eq!(cursor.offset(), 0);
    assert_eq!(cursor.u16().unwrap(), 0);
}

/// Une longueur démesurée est refusée sans déborder le calcul de borne.
///
/// C'est le cas que le fichier déclare : une section de quatre milliards
/// d'octets dans un bloc de quarante. La comparaison se fait sur ce qui reste,
/// jamais sur une somme, donc rien ne déborde même en 32 bits.
#[test]
fn une_longueur_demesuree_est_refusee() {
    let bytes = [0; 40];
    let mut cursor = Cursor::new(&bytes);
    assert_eq!(cursor.take(usize::MAX).unwrap_err(), TRUNCATED);
    assert_eq!(cursor.offset(), 0);
}

/// Une lecture de longueur nulle réussit, même sur un curseur épuisé.
///
/// C'est ce qui rend une section vide légitime sans cas particulier dans le
/// décodeur.
#[test]
fn une_lecture_vide_reussit_sur_un_curseur_epuise() {
    let bytes = [0; 1];
    let mut cursor = Cursor::new(&bytes);
    cursor.u8().unwrap();
    assert!(cursor.take(0).unwrap().is_empty());
    assert_eq!(cursor.remaining(), 0);
}

/// Les flottants non finis sont refusés à la lecture, avant toute arithmétique.
///
/// Les deux formes de `NaN` y passent, silencieux et signalant : c'est la charge
/// utile du second qu'un passage en registre normaliserait, et la divergence
/// entre cibles serait silencieuse.
#[test]
fn un_flottant_non_fini_est_refuse() {
    let refused = Error::InvalidFormat(Malformation::NonFinite);
    for bits in [
        0x7f80_0000, // +inf
        0xff80_0000, // -inf
        0x7fc0_0000, // NaN silencieux
        0x7f80_0001, // NaN signalant
        0xffff_ffff, // NaN négatif, tous bits à un
    ] {
        let bytes = u32::to_le_bytes(bits);
        assert_eq!(Cursor::new(&bytes).f32().unwrap_err(), refused, "{bits:#x}");
    }
}

/// Les flottants finis passent, subnormaux et zéro négatif compris.
///
/// Le zéro négatif importe : l'appariement des portails se fait au bit près, et
/// un décodeur qui le normaliserait apparierait deux portails que l'éditeur a
/// écrits différents.
#[test]
fn un_flottant_fini_passe_sans_etre_normalise() {
    for value in [0.0f32, -0.0, 1.0, -1.5, f32::MIN_POSITIVE, f32::MAX] {
        let bytes = value.to_bits().to_le_bytes();
        let read = Cursor::new(&bytes).f32().unwrap();
        assert_eq!(read.to_bits(), value.to_bits(), "{value}");
    }
    let subnormal = f32::from_bits(1);
    let bytes = subnormal.to_bits().to_le_bytes();
    assert_eq!(Cursor::new(&bytes).f32().unwrap().to_bits(), 1);
}

/// Une étiquette rend ses quatre octets dans l'ordre du fichier.
#[test]
fn une_etiquette_garde_l_ordre_des_octets() {
    let bytes = *b"SURFTEXN";
    let mut cursor = Cursor::new(&bytes);
    assert_eq!(cursor.tag().unwrap(), *b"SURF");
    assert_eq!(cursor.tag().unwrap(), *b"TEXN");
}
