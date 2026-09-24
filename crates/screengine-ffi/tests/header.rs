// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le header versionné est la documentation de la frontière, et `docs/abi.md`
//! l'exige en anglais : c'est le seul document qu'un auteur de liaison lise.
//!
//! `make header-verif` ne peut pas le tenir. Il compare le header versionné au
//! header régénéré, si bien qu'une docstring française mal rattachée — un bloc
//! `///` qui décrit du code interne et se retrouve collé à la déclaration
//! suivante — est recopiée à l'identique dans les deux et passe. Ce contrôle-ci
//! lit le fichier publié.

/// Le header, lu depuis la racine du dépôt.
const HEADER: &str = include_str!("../../../include/screengine.h");

/// Les mots dont la présence dans le header trahit du français.
///
/// Aucun n'existe en anglais, et c'est le seul critère de la liste : `plus`,
/// `son`, `sur` et `a` en sont écartés pour cette raison. Elle n'a pas à être
/// exhaustive — une docstring française en franchit plusieurs dès sa première
/// phrase.
const FRENCH: &[&str] = &[
    "que", "qui", "les", "des", "une", "est", "sont", "pour", "dans", "avec", "sans", "donc",
    "cette", "chaque", "jamais", "elle", "leur", "aux", "ainsi", "alors", "déjà", "être", "faire",
    "tout", "celui", "doit", "peut", "soit", "ne", "où", "ce", "se",
];

/// Les mots français d'un texte, chacun rendu une fois, dans l'ordre.
///
/// Découpe sur tout ce qui n'est pas une lettre : le header porte des tirets
/// cadratins, des accolades et des identifiants soulignés, et un mot collé à
/// l'un d'eux doit être vu.
fn french_words(text: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    for word in text.split(|c: char| !c.is_alphabetic()) {
        if word.is_empty() {
            continue;
        }
        let lower = word.to_lowercase();
        if let Some(hit) = FRENCH.iter().find(|w| **w == lower) {
            if !found.contains(hit) {
                found.push(*hit);
            }
        }
    }
    found
}

/// Le header publié ne porte pas un mot de français.
///
/// Le défaut que ce test attrape ne se voit ni à la compilation, ni dans un
/// diff de header, ni dans la source : la docstring fautive y est correcte en
/// soi, elle est seulement au mauvais endroit.
#[test]
fn le_header_ne_publie_aucun_mot_francais() {
    let found = french_words(HEADER);
    assert!(
        found.is_empty(),
        "mots français dans include/screengine.h : {found:?} — une docstring `///` du crate est \
         rattachée à la déclaration que cbindgen exporte, au lieu du code interne qu'elle décrit"
    );
}

/// La détection mord sur le texte qui avait réellement fui dans le header.
///
/// Sans elle, le test précédent serait vert quoi qu'il arrive et ne prouverait
/// rien : c'est cette liste de mots qui porte toute sa valeur. Le texte est
/// celui que la 0.3.0 publiait en tête de `ScgGrade`.
#[test]
fn la_detection_mord_sur_la_docstring_qui_avait_fui() {
    let fuite = "Les tailles et décalages que le header publie, vérifiés à la compilation. \
                 En assertions de constante et non en test : un test ne s'exécute que sur la \
                 cible hôte, alors qu'une liaison JavaScript reproduit ces décalages sur wasm32.";
    let found = french_words(fuite);
    assert!(
        found.len() >= 3,
        "la liste de mots laisse passer une docstring française entière : {found:?}"
    );
}

/// Un texte anglais du header ne déclenche rien.
///
/// La contrepartie du test précédent : une liste trop large refuserait la
/// documentation légitime, et le contrôle finirait désarmé pour cette raison.
#[test]
fn la_detection_epargne_la_documentation_anglaise() {
    let anglais = "Near plane distance, positive and finite. Not named `near`: windows.h still \
                   defines near and far as empty macros, inherited from 16-bit segmented memory, \
                   and a field by that name vanishes in any translation unit that includes it.";
    assert_eq!(french_words(anglais), Vec::<&str>::new());
}
