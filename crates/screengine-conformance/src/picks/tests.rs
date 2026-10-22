// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la scène d'interrogation doit éprouver, et qu'une empreinte ne dit
//! pas.
//!
//! **Une empreinte dit qu'un résultat a changé, jamais qu'il est juste.** Une
//! scène dont aucun rayon ne toucherait rien se figerait aussi bien qu'une
//! autre, et une scène dont les deux filtres rendraient la même chose figerait
//! une question qu'elle ne pose pas. Ces tests gardent les deux.

use super::*;

/// Joue les rayons et range leurs résultats à côté de leur description.
fn played() -> Vec<(Pick, Option<Hit>)> {
    let world = World::load(&collision_file::bytes()).expect("décor valide");
    all()
        .into_iter()
        .map(|pick| {
            let hit = play(&world, &pick);
            (pick, hit)
        })
        .collect()
}

/// La scène touche vraiment quelque chose, et souvent.
///
/// Le contrôle que le premier jet de la scène de collision avait manqué : un
/// décor où presque tout part hors cellule se chronomètre et se hache très bien,
/// et n'éprouve rien.
#[test]
fn les_rayons_touchent_pour_de_bon() {
    let played = played();
    let touches = played
        .iter()
        .filter(|(_, hit)| hit.is_some_and(|hit| hit.surface != 0))
        .count();
    assert!(
        touches * 4 >= played.len(),
        "seulement {touches} rayons sur {} touchent une surface",
        played.len()
    );
}

/// **Les deux filtres ne rendent pas la même chose**, et c'est ce que la scène
/// existe pour figer.
///
/// Le décor porte une surface non solide — le plafond du couloir —, donc il
/// existe des rayons que `Surfaces::All` arrête et que `Surfaces::Solid`
/// traverse. Sans cet écart, le filtre entrerait dans l'empreinte sans rien y
/// changer, et un défaut qui l'ignorerait resterait vert.
#[test]
fn les_deux_filtres_different_quelque_part() {
    let played = played();
    let moitie = played.len() / 2;
    let differents = (0..moitie)
        .filter(|&i| {
            let solide = played[i].1.map(|hit| hit.surface);
            let toutes = played[i + moitie].1.map(|hit| hit.surface);
            solide != toutes
        })
        .count();
    assert!(
        differents > 0,
        "aucun rayon ne distingue les deux filtres : le décor n'a rien de non solide à voir"
    );
}

/// Ce qu'`agree` refuse d'un rayon tronqué, et qu'aucune donnée ne lui présente.
///
/// **La branche est atteinte** — les deux rayons de l'enfilade saturent le budget,
/// un par filtre, et ce sont les deux seuls sur mille neuf cent vingt-deux —, mais
/// elle l'est par un résultat conforme : un `Ok(())` rendu d'emblée passerait la
/// conformance sans rien dire. Le rayon n'a pas les deux clauses du balayage sur
/// le point et la cellule, le contrat ne lui promettant que le conservatisme de
/// sa fraction.
#[test]
fn un_tronque_qui_rend_le_rayon_entier_est_refuse() {
    let slow = free(Vec3::ZERO);
    let mut fast = free(Vec3::ZERO);
    fast.incomplete = true;

    fast.fraction = 0.5;
    assert!(
        agree(&fast, &slow).is_ok(),
        "un tronqué conservateur passe, quoi que rende la force brute"
    );

    fast.fraction = 1.0;
    assert!(
        agree(&fast, &slow).is_err(),
        "une troncature qui rend le rayon entier"
    );
}

/// La scène rencontre un rayon tronqué, et il n'y en a que deux.
///
/// Ils tiennent au seul trajet de l'enfilade, posé en queue de chaque moitié :
/// les rayons du treillis sont trop courts pour saturer, si bien que retirer ce
/// cas-là retirerait le statut de l'empreinte sans que rien d'autre ne le dise.
#[test]
fn la_scene_rencontre_un_rayon_tronque() {
    let played = played();
    let tronques = played
        .iter()
        .filter(|(_, hit)| hit.is_some_and(|hit| hit.incomplete))
        .count();
    assert!(tronques > 0, "aucun rayon ne sature le budget de cellules");
}

/// Les deux moitiés de la liste posent les mêmes rayons, dans le même ordre.
///
/// C'est ce qui rend le test précédent lisible — il compare `i` et
/// `i + moitié` —, et c'est une propriété de la génération qu'un réordonnancement
/// casserait en silence.
#[test]
fn les_deux_moities_sont_les_memes_rayons() {
    let picks = all();
    let moitie = picks.len() / 2;
    assert_eq!(picks.len() % 2, 0, "la liste se coupe en deux moitiés");
    for i in 0..moitie {
        assert_eq!(picks[i].from, picks[i + moitie].from, "départ du rayon {i}");
        assert_eq!(picks[i].to, picks[i + moitie].to, "arrivée du rayon {i}");
        assert_ne!(
            picks[i].filter,
            picks[i + moitie].filter,
            "filtre du rayon {i}"
        );
    }
}

/// Le fichier des rayons porte sa magie, son compte, et rien de plus.
///
/// C'est ce qu'un hôte lit : un en-tête de douze octets puis vingt-huit par
/// rayon. Une taille qui ne tomberait pas juste voudrait dire qu'un champ a été
/// ajouté d'un côté sans l'autre.
#[test]
fn le_fichier_des_rayons_a_la_taille_annoncee() {
    let bytes = file_bytes();
    let count = all().len();
    assert_eq!(&bytes[..8], MAGIC);
    assert_eq!(
        u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize,
        count
    );
    assert_eq!(bytes.len(), 12 + count * 28);
}
