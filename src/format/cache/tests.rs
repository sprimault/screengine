// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que le décodeur d'un cache doit refuser.
//!
//! **Les octets viennent d'un fichier que l'hôte a rangé à côté de sa carte**, donc
//! d'un endroit où n'importe quoi peut arriver : une copie interrompue, une archive
//! d'une autre version, un bloc forgé. Le décodeur ne panique sur aucun d'eux, et
//! aucune capacité d'allocation ne vient d'un nombre qu'il aurait lu.
//!
//! Les blocs se construisent ici à la main, champ par champ : passer par l'écriture
//! ferait tester le décodeur contre l'encodeur, et deux défauts symétriques
//! s'annuleraient sans que rien ne le montre.

use alloc::vec::Vec;

use super::*;
use crate::testing::Rng;

/// Les octets d'une suite d'entiers.
fn words(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Un bloc valide : une entrée, un atlas de deux luxels de côté, une surface.
fn block() -> Vec<u8> {
    let record = {
        let mut body = words(&[
            7,           // identifiant de cellule
            0x1234_5678, // empreinte, mot bas
            0x9abc_def0, // empreinte, mot haut
            2,           // côté de l'atlas
            0,           // position des luxels
            16,          // leur longueur : 2 × 2 × 4
            1,           // un rectangle
        ]);
        body.extend_from_slice(&words(&[11, 0, 0, 2, 2]));
        let mut out = words(&[body.len() as u32]);
        out.extend_from_slice(&body);
        out
    };
    let texels = words(&[0xFF00_0000, 0xFF00_0000, 0xFF00_0000, 0xFF00_0000]);

    let first = HEADER_LEN + 2 * ENTRY_LEN;
    let total = first + record.len() + texels.len();

    let mut out = Vec::new();
    out.extend_from_slice(&super::super::SIGNATURE);
    out.extend_from_slice(&KIND);
    out.extend_from_slice(&words(&[VERSION, total as u32, 2]));
    out.extend_from_slice(&RECORDS);
    out.extend_from_slice(&words(&[first as u32, record.len() as u32]));
    out.extend_from_slice(&LUXELS);
    out.extend_from_slice(&words(&[
        (first + record.len()) as u32,
        texels.len() as u32,
    ]));
    out.extend_from_slice(&record);
    out.extend_from_slice(&texels);
    out
}

/// Un bloc bien formé rend son entrée.
#[test]
fn un_bloc_bien_forme_rend_son_entree() {
    let bytes = block();
    let entries = read(&bytes).expect("bloc valide");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].cell_id, 7);
    assert_eq!(entries[0].fingerprint, 0x9abc_def0_1234_5678);
    assert_eq!(entries[0].atlas.side, 2);
    assert_eq!(entries[0].surfaces, [11]);
    assert_eq!(entries[0].texels.len(), 16);
}

/// Toute troncature est refusée, et aucune ne panique.
#[test]
fn toute_troncature_est_refusee() {
    let bytes = block();
    for len in 0..bytes.len() {
        match read(&bytes[..len]) {
            Ok(_) => panic!("tronqué à {len} octets, et accepté"),
            Err(error) => assert!(
                matches!(
                    error,
                    Error::InvalidFormat(_) | Error::UnsupportedFormatVersion
                ),
                "à {len} octets : {error:?}"
            ),
        }
    }
}

/// Une mutation quelconque rend un succès ou une erreur de données, jamais autre
/// chose et jamais une panique.
///
/// Le générateur est celui du noyau, à graine écrite : un échec qui ne se rejoue
/// pas n'a pas été trouvé.
#[test]
fn une_mutation_quelconque_ne_panique_pas() {
    let valid = block();
    let mut rng = Rng::new(0x5ca1_ab1e_d0d0_1234);
    for _ in 0..4096 {
        let mut bytes = valid.clone();
        let at = (rng.next() as usize) % bytes.len();
        bytes[at] ^= (rng.next() as u8) | 1;
        match read(&bytes) {
            Ok(_) => {}
            Err(error) => assert!(
                matches!(
                    error,
                    Error::InvalidFormat(_) | Error::UnsupportedFormatVersion | Error::OutOfMemory
                ),
                "octet {at} : {error:?}"
            ),
        }
    }
}

/// Un côté d'atlas qui ne répond pas à la longueur annoncée est refusé.
///
/// **C'est le contrôle qui empêche de lire les luxels d'une autre entrée.** Sans
/// lui, un bloc forgé annonce un petit côté et une grande longueur, et la tranche
/// rendue déborde sur ce qui suit ; l'atlas serait alors construit sur des octets
/// qui appartiennent à une autre cellule.
#[test]
fn un_cote_qui_ne_repond_pas_a_la_longueur_est_refuse() {
    let mut bytes = block();
    // Le côté est le quatrième mot du corps de l'enregistrement.
    let side = HEADER_LEN + 2 * ENTRY_LEN + 4 + 4 * 3;
    bytes[side..side + 4].copy_from_slice(&4u32.to_le_bytes());
    assert!(matches!(read(&bytes), Err(Error::InvalidFormat(_))));
}

/// Deux entrées hors d'ordre sont refusées.
///
/// Le moteur est le seul écrivain et les trie ; un bloc qui ne l'est pas vient donc
/// d'ailleurs, et rien ne dit ce que porte le reste.
#[test]
fn deux_entrees_hors_d_ordre_sont_refusees() {
    let one = block();
    let first = HEADER_LEN + 2 * ENTRY_LEN;
    let record = &one[first..one.len() - 16];
    let texels = &one[one.len() - 16..];

    // Deux fois la même entrée : le second identifiant n'est donc pas strictement
    // supérieur au premier, ce que l'unicité interdit autant que l'ordre.
    let body = record.len() * 2;
    let total = first + body + 32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&super::super::SIGNATURE);
    bytes.extend_from_slice(&KIND);
    bytes.extend_from_slice(&words(&[VERSION, total as u32, 2]));
    bytes.extend_from_slice(&RECORDS);
    bytes.extend_from_slice(&words(&[first as u32, body as u32]));
    bytes.extend_from_slice(&LUXELS);
    bytes.extend_from_slice(&words(&[(first + body) as u32, 32]));
    bytes.extend_from_slice(record);
    bytes.extend_from_slice(record);
    bytes.extend_from_slice(texels);
    bytes.extend_from_slice(texels);

    assert!(matches!(
        read(&bytes),
        Err(Error::InvalidFormat(Malformation::SectionOrder))
    ));
}
