// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la sélection doit tenir, quelle que soit la machine qui l'exécute.
//!
//! **Aucun de ces cas ne peut exiger qu'un jeu d'instructions soit là** : ils
//! tournent sur le poste, sur la machine de construction et sous `qemu-user`,
//! qui ne portent pas les mêmes. Ce qu'ils vérifient est donc la **forme** de la
//! sélection — ce qui est toujours vrai, ce qui est cohérent avec soi-même — et
//! l'égalité des images est l'affaire de la conformance, qui joue les chemins
//! l'un contre l'autre.

use super::SimdPath as Path;

/// Le scalaire est utilisable partout, et c'est ce qui garantit qu'un hôte ne
/// peut pas se retrouver sans chemin.
#[test]
fn le_scalaire_est_toujours_disponible() {
    assert!(Path::Scalar.available());
    assert!(Path::Auto.available());
}

/// `Auto` se résout toujours vers un chemin que la machine porte.
///
/// C'est la propriété qui fait tenir tout le reste : sans elle, une cible sans
/// jeu vectoriel choisirait un chemin absent et tomberait à l'exécution plutôt
/// qu'à la compilation.
#[test]
fn auto_se_resout_vers_un_chemin_disponible() {
    let chosen = Path::Auto.resolve();
    assert_ne!(chosen, Path::Auto, "`Auto` doit désigner un chemin concret");
    assert!(
        chosen.available(),
        "`Auto` a choisi {chosen:?}, que cette machine ne porte pas"
    );
}

/// Un chemin déjà explicite se résout en lui-même.
///
/// Sans cette clause, forcer un chemin ne forcerait rien : `resolve` le
/// remplacerait par ce que la machine préfère, et les trois chemins ne se
/// compareraient jamais sur le même processeur — ce pour quoi le forçage existe.
#[test]
fn un_chemin_explicite_se_rend_lui_meme() {
    for path in [
        Path::Scalar,
        Path::Sse2,
        Path::Avx2,
        Path::Neon,
        Path::Simd128,
    ] {
        assert_eq!(path.resolve(), path);
    }
}

/// La disponibilité ne dépend pas de l'appel : deux interrogations de suite
/// rendent la même réponse.
///
/// Le `cpuid` est relu à chaque fois plutôt que mis en cache, et ce cas garde ce
/// choix honnête — un processeur ne change pas de jeu d'instructions en cours de
/// route, donc deux lectures qui divergeraient désigneraient une faute de
/// lecture, pas une machine qui a bougé.
#[test]
fn la_disponibilite_est_stable() {
    for path in [
        Path::Scalar,
        Path::Sse2,
        Path::Avx2,
        Path::Neon,
        Path::Simd128,
    ] {
        assert_eq!(path.available(), path.available());
    }
}

/// Sur `x86_64`, SSE2 est dans la base de l'architecture.
///
/// Écrit comme un cas et non supposé : c'est sur cette garantie que la sélection
/// se passe d'interroger le processeur pour SSE2, et elle ne vaut que pour cette
/// cible — `x86` 32 bits ne la donne pas.
#[test]
#[cfg(target_arch = "x86_64")]
fn sse2_est_acquis_sur_x86_64() {
    assert!(Path::Sse2.available());
}

/// Sur `aarch64`, NEON est dans la base de l'architecture.
#[test]
#[cfg(target_arch = "aarch64")]
fn neon_est_acquis_sur_aarch64() {
    assert!(Path::Neon.available());
}

/// AVX2 n'est jamais annoncé hors de x86, quelle que soit la machine.
#[test]
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn avx2_n_existe_pas_hors_de_x86() {
    assert!(!Path::Avx2.available());
}
