// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les codes de retour, vus comme une liaison les traite.
//!
//! Un code publié ne change jamais de sens : ces tests figent la plage et
//! l'unicité, pas les valeurs elles-mêmes, qui sont dans le header.

use super::*;

/// La catégorie d'un code est `(-code) / 100`, et les codes généraux sont
/// tous dans la catégorie 0.
#[test]
fn les_codes_generaux_sont_dans_la_premiere_plage() {
    for code in [
        SCG_ERR_NULL,
        SCG_ERR_INVALID_ARGUMENT,
        SCG_ERR_OUT_OF_MEMORY,
        SCG_ERR_INVALID_STATE,
        SCG_ERR_PANIC,
        SCG_ERR_FAULTED,
    ] {
        assert!(code < 0, "un code d'erreur est négatif");
        assert_eq!((-code) / 100, 0, "code {code} hors de la plage générale");
    }
}

/// Les codes des données sont dans la deuxième plage, et c'est ce qui les
/// distingue d'un refus d'argument pour une liaison qui ne les connaît pas :
/// elle les dégrade en « mauvais fichier » et non en « mauvais appel ».
#[test]
fn les_codes_des_donnees_sont_dans_la_deuxieme_plage() {
    for code in [
        SCG_ERR_UNKNOWN_RESOURCE,
        SCG_ERR_INVALID_FORMAT,
        SCG_ERR_UNSUPPORTED_FORMAT_VERSION,
    ] {
        assert_eq!((-code) / 100, 1, "code {code} hors de la plage des données");
    }
}

/// L'ancien nom reste un alias exact : un hôte compilé contre le header de la
/// 0.0.0 doit recevoir le même code qu'avant le renommage.
#[test]
fn l_ancien_nom_du_code_defaillant_garde_sa_valeur() {
    assert_eq!(SCG_ERR_POISONED, SCG_ERR_FAULTED);
}

/// Deux erreurs distinctes ne partagent jamais un code : un code publié ne
/// change pas de sens, et deux sens pour un code reviendrait au même.
#[test]
fn chaque_erreur_du_noyau_a_son_code() {
    let errors = [
        Error::InvalidArgument(Argument::Resolution),
        Error::OutOfMemory,
        Error::InvalidState,
        Error::InvalidFormat(Malformation::Signature),
        Error::UnsupportedFormatVersion,
    ];
    for (i, a) in errors.iter().enumerate() {
        for b in &errors[i + 1..] {
            assert_ne!(code_of(*a), code_of(*b));
        }
    }
}

/// L'inverse du précédent : les arguments refusés partagent un code, et
/// seul le message les distingue. Deux messages identiques laisseraient
/// l'intégrateur chercher lequel de ses arguments est en cause.
#[test]
fn chaque_argument_refuse_a_son_message() {
    let arguments = [
        Argument::Resolution,
        Argument::TileSize,
        Argument::Stride,
        Argument::BufferLength,
        Argument::TileIndex,
        Argument::Region,
        Argument::ScratchLength,
        Argument::TriangleCapacity,
        Argument::VertexIndex,
        Argument::Projection,
        Argument::VertexCoordinate,
        Argument::TextureCapacity,
        Argument::TextureCoordinate,
        Argument::TextureSize,
        Argument::TextureLength,
        Argument::Overbright,
        Argument::Fog,
        Argument::LightCapacity,
        Argument::Light,
        Argument::Grade,
    ];
    for (i, a) in arguments.iter().enumerate() {
        let error = Error::InvalidArgument(*a);
        assert_eq!(code_of(error), SCG_ERR_INVALID_ARGUMENT);
        for b in &arguments[i + 1..] {
            assert_ne!(message_of(error), message_of(Error::InvalidArgument(*b)));
        }
    }

    // La liste ci-dessus est écrite à la main, et elle s'est déjà trouvée
    // incomplète deux fois — une variante ajoutée au noyau n'y entrait que si
    // quelqu'un y pensait, et le test restait vert en mesurant moins qu'il ne
    // prétendait. Ce `match` est exhaustif : une variante nouvelle ne compile
    // plus tant qu'elle n'est pas nommée ici, et le rang qu'elle y reçoit doit
    // être celui qu'elle occupe là-haut.
    let rank = |argument: Argument| match argument {
        Argument::Resolution => 0,
        Argument::TileSize => 1,
        Argument::Stride => 2,
        Argument::BufferLength => 3,
        Argument::TileIndex => 4,
        Argument::Region => 5,
        Argument::ScratchLength => 6,
        Argument::TriangleCapacity => 7,
        Argument::VertexIndex => 8,
        Argument::Projection => 9,
        Argument::VertexCoordinate => 10,
        Argument::TextureCapacity => 11,
        Argument::TextureCoordinate => 12,
        Argument::TextureSize => 13,
        Argument::TextureLength => 14,
        Argument::Overbright => 15,
        Argument::Fog => 16,
        Argument::LightCapacity => 17,
        Argument::Light => 18,
        Argument::Grade => 19,
    };
    for (i, a) in arguments.iter().enumerate() {
        assert_eq!(rank(*a), i, "{a:?} n'est pas à sa place");
    }
    assert_eq!(arguments.len(), 20, "une variante manque à la liste");
}

/// Même règle pour un bloc refusé : un seul code, et un message par cause.
///
/// C'est tout ce qui reste à l'auteur d'un exportateur pour trouver son défaut :
/// deux causes qui partageraient un texte lui feraient relire la mauvaise partie
/// de son écrivain.
#[test]
fn chaque_malformation_a_son_message() {
    let malformations = [
        Malformation::Truncated,
        Malformation::Signature,
        Malformation::Kind,
        Malformation::Length,
        Malformation::SectionKind,
        Malformation::SectionOrder,
        Malformation::SectionBounds,
        Malformation::NonFinite,
        Malformation::NonUtf8,
        Malformation::Index,
        Malformation::Identifier,
        Malformation::GroupBounds,
        Malformation::Count,
        Malformation::Flags,
        Malformation::Polygon,
        Malformation::Mapping,
        Malformation::Portal,
    ];
    for (i, a) in malformations.iter().enumerate() {
        let error = Error::InvalidFormat(*a);
        assert_eq!(code_of(error), SCG_ERR_INVALID_FORMAT);
        for b in &malformations[i + 1..] {
            assert_ne!(message_of(error), message_of(Error::InvalidFormat(*b)));
        }
    }

    // Exhaustif pour la même raison que la liste des arguments : une variante
    // nouvelle ne compile plus tant qu'elle n'est pas nommée ici.
    let rank = |malformation: Malformation| match malformation {
        Malformation::Truncated => 0,
        Malformation::Signature => 1,
        Malformation::Kind => 2,
        Malformation::Length => 3,
        Malformation::SectionKind => 4,
        Malformation::SectionOrder => 5,
        Malformation::SectionBounds => 6,
        Malformation::NonFinite => 7,
        Malformation::NonUtf8 => 8,
        Malformation::Index => 9,
        Malformation::Identifier => 10,
        Malformation::GroupBounds => 11,
        Malformation::Count => 12,
        Malformation::Flags => 13,
        Malformation::Polygon => 14,
        Malformation::Mapping => 15,
        Malformation::Portal => 16,
    };
    for (i, a) in malformations.iter().enumerate() {
        assert_eq!(rank(*a), i, "{a:?} n'est pas à sa place");
    }
    assert_eq!(malformations.len(), 17, "une variante manque à la liste");
}
