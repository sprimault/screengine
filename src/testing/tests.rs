// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que le générateur des tests doit garantir pour que les tests qui s'en
//! servent mesurent ce qu'ils croient mesurer.

use alloc::collections::BTreeSet;

use super::Rng;

/// Deux graines distinctes donnent deux suites distinctes.
///
/// **Sans quoi une boucle sur des graines consécutives ment sur sa couverture.**
/// La graine était forcée impaire, si bien que `2n` et `2n + 1` partaient du
/// même état : une boucle de quatre cents graines n'éprouvait que deux cents
/// cas, chacun deux fois, et la garde qui compte les itérations la déclarait
/// honnête.
#[test]
fn deux_graines_consecutives_ne_donnent_pas_la_meme_suite() {
    let premiers: BTreeSet<u64> = (0..64u64)
        .map(|seed| {
            let mut rng = Rng::new(seed);
            rng.next()
        })
        .collect();

    assert_eq!(
        premiers.len(),
        64,
        "{} suites distinctes sur soixante-quatre graines",
        premiers.len()
    );
}

/// La graine nulle ne bloque pas la suite.
///
/// C'est la raison d'être du remplacement, et elle reste : xorshift a le zéro
/// pour point fixe, une suite qui y tombe n'en sort plus.
#[test]
fn la_graine_nulle_ne_bloque_pas() {
    let mut rng = Rng::new(0);
    let tirages: BTreeSet<u64> = (0..8).map(|_| rng.next()).collect();
    assert_eq!(tirages.len(), 8, "la suite s'est bloquée");
    assert!(!tirages.contains(&0), "un tirage nul");
}
