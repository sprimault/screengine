// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

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
#[test]
fn la_garde_se_pose_et_se_retire() {
    let env = FpEnv::enter();
    drop(env);
}

/// Quand l'hôte est déjà dans l'environnement par défaut, rien n'est écrit.
#[test]
fn ne_retient_rien_quand_l_hote_est_deja_conforme() {
    let _normalize = FpEnv::enter();
    let env = FpEnv::enter();
    assert!(env.0.is_none());
}

/// Normaliser deux fois rend la même valeur : la normalisation est idempotente,
/// sans quoi deux appels imbriqués ne restaureraient pas le même état.
#[test]
fn la_normalisation_est_idempotente() {
    let host = arch::read();
    let once = arch::normalize(host);
    assert_eq!(arch::normalize(once), once);
}
