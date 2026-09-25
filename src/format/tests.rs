// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests du conteneur commun aux deux formats.
//!
//! Aucun fichier binaire n'est versionné pour les nourrir : ils s'écrivent
//! octet par octet ici. Un binaire ne se relit pas en revue, et un fichier
//! produit par l'écrivain du projet rendrait le test tautologique — il
//! prouverait que l'écrivain et le lecteur s'accordent, pas que l'un des deux
//! est juste. Le constructeur ci-dessous est le seul endroit où la disposition
//! est écrite une seconde fois, et c'est ce qui fait rougir un désaccord.

use alloc::vec::Vec;

use super::*;
use crate::testing::Rng;

/// Le genre que les tests attendent, et celui qu'ils opposent.
const MESH: [u8; 4] = *b"MESH";
/// L'autre genre, pour éprouver le refus d'un maillage là où une carte est
/// attendue.
const WORLD: [u8; 4] = *b"WRLD";
/// La version que le décodeur des tests accepte.
const VERSION: u32 = 1;
/// Les genres de sections du format de maillage, croissants.
const TAGS: [[u8; 4]; 4] = [*b"SURF", *b"TEXN", *b"TRIS", *b"VTXS"];

/// Un fichier bien formé : en-tête de vingt octets, table de sections, puis les
/// sections dans l'ordre reçu.
///
/// Les décalages sont posés ici à la main — vingt octets d'en-tête, douze par
/// entrée — plutôt que demandés au code testé.
fn file(kind: [u8; 4], version: u32, sections: &[([u8; 4], &[u8])]) -> Vec<u8> {
    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(&kind);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(sections.len() as u32).to_le_bytes());

    let mut offset = first;
    for (tag, body) in sections {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        offset += body.len();
    }
    for (_, body) in sections {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// Un maillage bien formé à deux sections, celui que les tests abîment.
fn valid() -> Vec<u8> {
    file(
        MESH,
        VERSION,
        &[(*b"SURF", &[1, 2, 3, 4]), (*b"TRIS", &[5, 6])],
    )
}

/// Le décodage que les tests éprouvent : un maillage, sa version, ses genres.
fn read(bytes: &[u8]) -> Result<[&[u8]; 4]> {
    decode(bytes, MESH, VERSION, TAGS)
}

/// Écrase un `u32` du fichier, pour abîmer un champ précis d'un fichier valide.
fn patch(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// Relit un `u32` du fichier, pour abîmer un champ à partir de sa valeur.
fn field(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// Le décalage du champ de longueur de la `n`-ième entrée de la table.
fn entry_length(n: usize) -> usize {
    20 + 12 * n + 8
}

/// Le décalage du champ de décalage de la `n`-ième entrée de la table.
fn entry_offset(n: usize) -> usize {
    20 + 12 * n + 4
}

/// Un refus qui vient du contenu du bloc, et jamais d'ailleurs.
///
/// C'est l'invariant que les épreuves exhaustives vérifient : le décodeur rend
/// une erreur de données ou rien, et il n'invente ni manque de mémoire ni appel
/// hors séquence.
fn is_data_error(error: Error) -> bool {
    matches!(
        error,
        Error::InvalidFormat(_) | Error::UnsupportedFormatVersion
    )
}

/// Un fichier bien formé rend ses sections, à leur rang et à leur contenu.
#[test]
fn un_fichier_bien_forme_rend_ses_sections() {
    let bytes = valid();
    let sections = read(&bytes).unwrap();
    assert_eq!(sections[0], &[1, 2, 3, 4], "SURF");
    assert_eq!(sections[1], &[] as &[u8], "TEXN, absente");
    assert_eq!(sections[2], &[5, 6], "TRIS");
    assert_eq!(sections[3], &[] as &[u8], "VTXS, absente");
}

/// Une section absente et une section vide rendent la même tranche.
///
/// Le décodeur qui suit n'a donc aucun cas particulier à écrire : un maillage
/// sans triangle se lit comme un maillage dont la section de triangles manque.
#[test]
fn une_section_vide_et_une_section_absente_se_lisent_pareil() {
    let empty = file(MESH, VERSION, &[(*b"TRIS", &[])]);
    let absent = file(MESH, VERSION, &[]);
    assert!(read(&empty).unwrap()[2].is_empty());
    assert!(read(&absent).unwrap()[2].is_empty());
}

/// Un fichier réduit à son en-tête est légitime : la ressource est vide.
#[test]
fn un_fichier_sans_aucune_section_est_legitime() {
    let bytes = file(MESH, VERSION, &[]);
    assert_eq!(bytes.len(), 20, "l'en-tête fait vingt octets");
    assert!(read(&bytes).unwrap().iter().all(|s| s.is_empty()));
}

/// Chacun des quatre octets de signature est vérifié.
///
/// Un contrôle écrit sur trois octets laisserait passer un fichier dont le
/// marqueur de fin de fichier a été mangé par un transfert en mode texte, qui
/// est exactement ce que ce quatrième octet attrape.
#[test]
fn une_signature_fausse_est_refusee() {
    for byte in 0..4 {
        let mut bytes = valid();
        bytes[byte] ^= 0xff;
        assert_eq!(
            read(&bytes).unwrap_err(),
            Error::InvalidFormat(Malformation::Signature),
            "octet {byte} de la signature"
        );
    }
}

/// Une carte là où un maillage est attendu est refusée par le genre.
///
/// Et non par la version : c'est toute la raison de couper la signature en deux.
#[test]
fn un_genre_inattendu_est_refuse() {
    let bytes = file(WORLD, VERSION, &[]);
    assert_eq!(
        read(&bytes).unwrap_err(),
        Error::InvalidFormat(Malformation::Kind)
    );
}

/// Une version que cette construction ne lit pas est refusée par son propre
/// code, plus récente comme plus ancienne.
#[test]
fn une_version_inconnue_est_refusee() {
    for version in [0, VERSION + 1, u32::MAX] {
        let bytes = file(MESH, version, &[]);
        assert_eq!(
            read(&bytes).unwrap_err(),
            Error::UnsupportedFormatVersion,
            "version {version}"
        );
    }
}

/// La version se contrôle avant toute structure.
///
/// Le fichier de ce test est faux deux fois : sa version est future, et sa table
/// de sections ne tient pas dans le bloc. C'est la version qui doit sortir —
/// sinon l'hôte d'une version plus récente lit « format invalide » et cherche
/// une corruption qui n'existe pas.
#[test]
fn la_version_se_controle_avant_la_structure() {
    let mut bytes = valid();
    patch(&mut bytes, 8, VERSION + 1);
    patch(&mut bytes, 16, u32::MAX);
    assert_eq!(read(&bytes).unwrap_err(), Error::UnsupportedFormatVersion);
}

/// Un fichier tronqué à n'importe laquelle de ses longueurs est refusé, sans
/// une seule panique.
///
/// C'est l'épreuve qui trouve : la borne oubliée ne se manifeste que sur la
/// longueur qui la franchit, et aucune inspection ne dit laquelle.
#[test]
fn toute_troncature_est_refusee() {
    let bytes = valid();
    for len in 0..bytes.len() {
        match read(&bytes[..len]) {
            Ok(_) => panic!("tronqué à {len} octets, et accepté"),
            Err(error) => assert!(is_data_error(error), "à {len} octets : {error:?}"),
        }
    }
    assert!(read(&bytes).is_ok(), "la longueur entière passe");
}

/// Des octets de queue sont refusés, y compris un seul.
///
/// Tolérés, deux fichiers d'octets différents rendraient la même image, et
/// l'empreinte d'intégrité de l'hôte se désaccorderait de celle du moteur.
#[test]
fn des_octets_de_queue_sont_refuses() {
    let mut bytes = valid();
    bytes.push(0);
    assert_eq!(
        read(&bytes).unwrap_err(),
        Error::InvalidFormat(Malformation::Length)
    );
}

/// Une section d'un genre inconnu refuse le fichier entier.
#[test]
fn une_section_de_genre_inconnu_est_refusee() {
    let bytes = file(MESH, VERSION, &[(*b"LGTS", &[1, 2])]);
    assert_eq!(
        read(&bytes).unwrap_err(),
        Error::InvalidFormat(Malformation::SectionKind)
    );
}

/// Deux sections du même genre sont refusées : au plus une de chaque.
#[test]
fn deux_sections_du_meme_genre_sont_refusees() {
    let bytes = file(MESH, VERSION, &[(*b"TRIS", &[1]), (*b"TRIS", &[2])]);
    assert_eq!(
        read(&bytes).unwrap_err(),
        Error::InvalidFormat(Malformation::SectionOrder)
    );
}

/// Des genres qui ne croissent pas sont refusés, même si chacun est connu et
/// unique.
#[test]
fn des_genres_de_sections_desordonnes_sont_refuses() {
    let bytes = file(MESH, VERSION, &[(*b"TRIS", &[1]), (*b"SURF", &[2])]);
    assert_eq!(
        read(&bytes).unwrap_err(),
        Error::InvalidFormat(Malformation::SectionOrder)
    );
}

/// Un trou entre deux sections est refusé : les sections pavent le fichier.
#[test]
fn un_trou_entre_deux_sections_est_refuse() {
    let mut bytes = valid();
    let offset = field(&bytes, entry_offset(1));
    patch(&mut bytes, entry_offset(1), offset + 1);

    // L'octet de plus garde la longueur totale juste : sans lui, c'est elle qui
    // refuserait le fichier, et le pavage ne serait pas éprouvé.
    let total = bytes.len() as u32 + 1;
    patch(&mut bytes, 12, total);
    bytes.push(0);
    assert_eq!(
        read(&bytes).unwrap_err(),
        Error::InvalidFormat(Malformation::SectionBounds)
    );
}

/// Un recouvrement de deux sections est refusé par le même contrôle.
#[test]
fn un_recouvrement_de_sections_est_refuse() {
    let mut bytes = valid();
    let offset = field(&bytes, entry_offset(1));
    patch(&mut bytes, entry_offset(1), offset - 1);
    assert_eq!(
        read(&bytes).unwrap_err(),
        Error::InvalidFormat(Malformation::SectionBounds)
    );
}

/// Des octets laissés entre la dernière section et la fin sont refusés.
///
/// C'est le trou que le contrôle de décalage ne peut pas voir : il n'a plus
/// d'entrée après lui pour le signaler.
#[test]
fn des_octets_laisses_au_bout_sont_refuses() {
    let mut bytes = valid();
    patch(&mut bytes, entry_length(1), 1);
    assert_eq!(
        read(&bytes).unwrap_err(),
        Error::InvalidFormat(Malformation::SectionBounds)
    );
}

/// Un compte de sections démesuré est refusé, sans déborder son produit.
///
/// Quatre milliards d'entrées de douze octets : le produit déborde un `usize`
/// de 32 bits, largeur de deux des quatre cibles, et la multiplication vérifiée
/// est ce qui fait sortir une erreur au lieu d'une borne repliée.
#[test]
fn un_compte_de_sections_demesure_est_refuse() {
    for count in [u32::MAX, u32::MAX / 12 + 1, 5] {
        let mut bytes = valid();
        patch(&mut bytes, 16, count);
        assert_eq!(
            read(&bytes).unwrap_err(),
            Error::InvalidFormat(Malformation::Truncated),
            "compte {count}"
        );
    }
}

/// Une longueur de section démesurée est refusée.
///
/// La bombe d'allocation classique, dans sa forme la plus courte : quarante
/// octets qui annoncent quatre milliards.
#[test]
fn une_longueur_de_section_demesuree_est_refusee() {
    let mut bytes = valid();
    patch(&mut bytes, entry_length(0), u32::MAX);
    assert_eq!(
        read(&bytes).unwrap_err(),
        Error::InvalidFormat(Malformation::Truncated)
    );
}

/// Une mutation quelconque d'un fichier valide rend un succès ou une erreur de
/// données, jamais autre chose et jamais une panique.
///
/// L'équivalent honnête d'un fuzzer dans un noyau qui n'admet aucune
/// dépendance. La graine est fixe et s'affiche : un échec qui ne se rejoue pas
/// n'a pas été trouvé.
#[test]
fn une_mutation_quelconque_ne_panique_pas() {
    const SEED: u64 = 0x5c67_4c1a_2026_0924;
    let mut rng = Rng::new(SEED);
    let original = valid();
    for round in 0..4096 {
        let mut bytes = original.clone();
        let at = (rng.next() % bytes.len() as u64) as usize;
        bytes[at] ^= (rng.next() % 255 + 1) as u8;
        if let Err(error) = read(&bytes) {
            assert!(
                is_data_error(error),
                "graine {SEED:#x}, tour {round} : {error:?}"
            );
        }
    }
}
