// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le pas fixe, sans horloge : le temps écoulé est fourni par le test.

use super::*;

/// Mille réveils d'une milliseconde font une seconde, donc soixante pas pile.
/// Un pas arrondi à 16 666 667 ns en rendrait 59 : c'est la dérive que
/// l'accumulateur en entiers existe pour éviter.
#[test]
fn soixante_pas_par_seconde_sans_derive() {
    let mut clock = Clock::new(60);
    let steps: u32 = (0..1000)
        .map(|_| clock.advance(Duration::from_millis(1)))
        .sum();
    assert_eq!(steps, 60);
}

/// La même exactitude sur une heure découpée en réveils irréguliers : une
/// dérive d'un pas par minute passerait le test précédent.
#[test]
fn une_heure_irreguliere_tombe_juste() {
    let mut clock = Clock::new(60);
    let mut steps = 0u64;
    let mut total = Duration::ZERO;
    let pattern = [7u64, 13, 16, 17, 1, 33];
    let mut i = 0;
    while total < Duration::from_secs(3600) {
        let chunk = Duration::from_millis(pattern[i % pattern.len()]);
        i += 1;
        total += chunk;
        steps += u64::from(clock.advance(chunk));
    }
    let expected = total.as_nanos() * 60 / 1_000_000_000;
    assert_eq!(u128::from(steps), expected);
}

/// Une seconde d'un bloc — une fenêtre déplacée à la souris sous Windows —
/// ne se rattrape pas en soixante pas d'un coup.
#[test]
fn le_rattrapage_est_plafonne_et_le_reste_abandonne() {
    let mut clock = Clock::new(60);
    assert_eq!(clock.advance(Duration::from_secs(1)), MAX_CATCH_UP);
    assert_eq!(clock.advance(Duration::ZERO), 0);
}

/// Un retard absurde ne déborde pas la conversion vers `u32`.
#[test]
fn un_retard_enorme_rend_le_plafond() {
    let mut clock = Clock::new(1000);
    assert_eq!(
        clock.advance(Duration::from_secs(u64::MAX / 1000)),
        MAX_CATCH_UP
    );
}

/// L'échéance rendue suffit à produire un pas : un réveil arrondi en
/// dessous tomberait juste avant, pour rien.
#[test]
fn se_reveiller_a_l_echeance_produit_un_pas() {
    let mut clock = Clock::new(60);
    clock.advance(Duration::from_millis(5));
    let wait = clock.until_next();
    assert_eq!(clock.advance(wait), 1);
}

/// Le temps passé fenêtre minimisée n'est pas joué au retour.
#[test]
fn reset_abandonne_le_temps_accumule() {
    let mut clock = Clock::new(60);
    clock.advance(Duration::from_millis(16));
    clock.reset();
    assert_eq!(clock.until_next(), Duration::from_nanos(16_666_667));
}
