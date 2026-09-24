// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La garde d'environnement flottant.
//!
//! Ces tests tournent sur la cible de développement, donc sur une seule des
//! quatre implémentations. Ce qu'ils vérifient vaut malgré tout partout : la
//! garde se pose et se retire sans rien casser, et sa normalisation est
//! idempotente — sans quoi deux appels imbriqués ne restaureraient pas le même
//! état.

use super::*;

/// Sur une cible sans registre de contrôle, la garde se construit et se
/// détruit sans rien faire — c'est ce qui permet une enveloppe unique.
///
/// Sans assertion, et c'est voulu : il n'y a rien à observer là où la garde est
/// vide. Ce qu'elle fait là où elle agit est le sujet du test suivant.
#[test]
fn la_garde_se_pose_et_se_retire() {
    let env = FpEnv::enter();
    drop(env);
}

/// La garde **rend à l'hôte le registre qu'il avait**, bit pour bit.
///
/// La moitié du contrat que rien ne vérifiait : `enter` était éprouvé par la
/// normalisation, `drop` par personne. Une restauration supprimée laissait
/// tous les tests verts, et l'hôte reprenait la main dans l'environnement du
/// moteur — ce que `docs/abi.md` promet justement de ne pas faire, au motif
/// qu'un hôte ayant démasqué une exception pour traquer un défaut chez lui ne
/// doit pas la retrouver masquée.
///
/// Le registre est par thread, donc ce test n'en perturbe aucun autre ; il le
/// remet malgré tout au propre, le harnais réutilisant ses threads.
#[test]
#[cfg(target_arch = "x86_64")]
fn la_garde_rend_a_l_hote_son_registre() {
    // Zéro forcé et dénormaux à zéro : deux bits que la normalisation efface,
    // donc l'hôte n'est pas déjà conforme et la garde a quelque chose à
    // retenir.
    let hostile = arch::normalize(arch::read()) | 0x8000 | 0x0040;
    arch::write(hostile);

    let pendant = {
        let _env = FpEnv::enter();
        arch::read()
    };
    let apres = arch::read();
    arch::write(arch::normalize(hostile));

    assert_eq!(pendant & 0x8040, 0, "la garde n'a pas normalisé l'entrée");
    assert_eq!(apres, hostile, "le registre de l'hôte n'a pas été rendu");
}

/// Quand l'hôte est déjà dans l'environnement par défaut, rien n'est écrit.
#[test]
fn ne_retient_rien_quand_l_hote_est_deja_conforme() {
    let _normalize = FpEnv::enter();
    let env = FpEnv::enter();
    assert!(env.0.is_none());
}

/// Un hôte qui a démasqué toutes les exceptions les retrouve masquées pendant
/// l'appel : le moteur produit des résultats inexacts, et un piège autorisé
/// tuerait le processus à la première image.
#[test]
#[cfg(target_arch = "x86_64")]
fn la_normalisation_masque_les_exceptions() {
    assert_eq!(arch::normalize(0) & 0x1F80, 0x1F80);
}

/// La normalisation **efface** aussi ce que l'hôte a posé : arrondi, zéro
/// forcé, dénormaux mis à zéro.
///
/// L'autre moitié du travail, et la seule qui change l'image. Un hôte en
/// arrondi vers le haut, ou en zéro forcé, rendrait des pixels différents des
/// empreintes versionnées ; poser les masques d'exception ne suffit donc pas, et
/// un test qui ne vérifie que ceux-là laisse passer une normalisation qui
/// n'efface rien.
#[test]
#[cfg(target_arch = "x86_64")]
fn la_normalisation_efface_l_arrondi_et_le_zero_force() {
    // Arrondi vers le haut (14:13 = 10), zéro forcé (15), dénormaux à zéro (6).
    let hostile = 0x4000 | 0x8000 | 0x0040;
    let normalized = arch::normalize(hostile);

    assert_eq!(normalized & 0x6000, 0, "arrondi au plus proche");
    assert_eq!(normalized & 0x8000, 0, "zéro forcé");
    assert_eq!(normalized & 0x0040, 0, "dénormaux mis à zéro");
}

/// L'équivalent ARM : les autorisations de piège de FPCR sont effacées.
#[test]
#[cfg(target_arch = "aarch64")]
fn la_normalisation_masque_les_exceptions() {
    assert_eq!(arch::normalize(0x9F00) & 0x9F00, 0);
}

/// Normaliser deux fois rend la même valeur : la normalisation est idempotente,
/// sans quoi deux appels imbriqués ne restaureraient pas le même état.
#[test]
fn la_normalisation_est_idempotente() {
    let host = arch::read();
    let once = arch::normalize(host);
    assert_eq!(arch::normalize(once), once);
}
