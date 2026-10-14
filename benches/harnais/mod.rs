// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que les quatre bancs partagent : le chronomètre et le témoin.
//!
//! **Il n'y a pas de crate de bancs**, et il n'y en aura pas : un banc est un
//! binaire, et ce module entre dans chacun par `#[path]`. Le fichier vit dans un
//! **sous-répertoire** pour cette raison seule — Cargo découvre tout
//! `benches/*.rs` comme une cible, et un `benches/harnais.rs` deviendrait un
//! banc sans `main`.
//!
//! **Deux choses y entrent, et la seconde est celle qui compte.** Le
//! chronomètre était recopié à l'identique dans les quatre fichiers, au nom de
//! sa constante près : l'extraire n'est qu'un gain de lecture. Le témoin, lui,
//! existait en trois formes — une assertion sur une part, un simple compte, et
//! les deux attentes opposées —, et un témoin recopié est autant d'occasions
//! qu'une de ses copies cesse de vérifier quoi que ce soit sans que rien ne le
//! dise. C'est le mode de panne le plus discret d'une mesure, puisqu'il se
//! présente comme un succès.
//!
//! Ce qui n'y entre pas : la ligne de résultat, que chaque banc rapporte à son
//! unité — par pixel, par cellule, par sommet —, et la construction du
//! contexte, que chacun règle pour ce qu'il mesure.

// Chaque banc n'appelle qu'une partie du harnais, et le module est compilé dans
// chacun : ce qu'un banc n'emploie pas y est mort pour lui seul. L'allow se
// retire le jour où les quatre appellent tout.
#![allow(dead_code)]

use core::time::Duration;
use std::time::Instant;

use screengine::BYTES_PER_PIXEL;

/// Le plus court temps observé sur `tours` répétitions.
///
/// **Le minimum et non la moyenne** : une durée n'a qu'une borne basse vraie,
/// tout ce qui la dépasse venant de la machine et non du code.
///
/// Deux tours à blanc d'abord : le premier tour d'un processus paie le cache
/// d'instructions et les défauts de page du tampon, qui ne sont pas ce qu'on
/// mesure. Le nombre de tours reste au banc — douze suffisent pour un
/// chargement de plusieurs millisecondes, là où une image en demande soixante.
pub fn mesure(tours: u32, mut tour: impl FnMut()) -> Duration {
    tour();
    tour();

    let mut court = Duration::MAX;
    for _ in 0..tours {
        let debut = Instant::now();
        tour();
        court = court.min(debut.elapsed());
    }
    court
}

/// Combien de pixels d'un tampon portent autre chose que le fond.
///
/// Le fond est le noir, qu'aucune scène de banc n'emploie comme couleur. Sort du
/// harnais parce qu'un banc s'en sert sans juger de part : celui de la carte
/// exige que la traversée peigne **exactement** le même nombre de pixels que le
/// chemin brut, une élimination qui en retire étant plus rapide et fausse.
pub fn peints(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(BYTES_PER_PIXEL)
        .filter(|p| p[..3] != [0, 0, 0])
        .count()
}

/// Ce que l'image d'un cas doit porter, vérifié à chaque mesure.
///
/// **Les deux attentes se trompent dans des sens opposés**, et c'est pourquoi le
/// témoin en porte deux plutôt qu'un seuil : un cas qui se croit vide et peint
/// mesure du remplissage sous un nom qui annonce le contraire, un cas qui se
/// croit plein et ne peint rien ne mesure qu'un rejet. Les deux rendent un
/// chiffre flatteur.
pub enum Image {
    /// Rien n'est peint : les primitives sont rejetées avant le remplissage.
    Vide,
    /// Au moins cette part du tampon est peinte.
    Peinte(f64),
}

/// Refuse une mesure dont l'image n'est pas celle que le cas annonce.
///
/// La surface se déduit du tampon, les bancs rendant tous avec un `stride` égal
/// à leur largeur. Un banc qui prendrait un `stride` plus long surestimerait son
/// total, donc exigerait **davantage** de pixels peints : l'erreur échoue, elle
/// ne passe pas.
pub fn exige(pixels: &[u8], quoi: &str, attendu: &Image) {
    let peints = peints(pixels);
    let total = (pixels.len() / BYTES_PER_PIXEL) as f64;
    match attendu {
        Image::Vide => assert_eq!(
            peints, 0,
            "{quoi} : {peints} pixels peints, la mesure porte sur du remplissage"
        ),
        Image::Peinte(part) => assert!(
            peints as f64 >= total * part,
            "{quoi} : {peints} pixels peints sur {total}, la mesure ne porte sur rien"
        ),
    }
}
