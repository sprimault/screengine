// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le pas fixe de la boucle.

use std::time::Duration;

/// Nanosecondes par seconde.
const NANOS: u128 = 1_000_000_000;

/// Le plus grand nombre de pas rattrapés en un réveil.
///
/// Au-delà, le retard est abandonné plutôt que rattrapé : une mise à jour plus
/// lente que son pas ferait grossir la dette à chaque réveil, et la boucle ne
/// rendrait plus jamais d'image.
pub(crate) const MAX_CATCH_UP: u32 = 5;

/// Un accumulateur de pas fixe, en entiers.
///
/// Il cumule `temps écoulé × fréquence` en nanosecondes, et retire une seconde
/// par pas. Un pas de 1/60 s ne tombe pas juste en nanosecondes : cumuler un pas
/// arrondi dériverait d'un pas toutes les quelques minutes, alors que ce produit
/// reste exact indéfiniment.
#[derive(Debug)]
pub(crate) struct Clock {
    rate: u32,
    debt: u128,
}

impl Clock {
    /// Un accumulateur vide, à `rate` pas par seconde. `rate` n'est jamais nul.
    pub(crate) fn new(rate: u32) -> Self {
        Self { rate, debt: 0 }
    }

    /// Ajoute le temps écoulé, et rend le nombre de pas à exécuter.
    pub(crate) fn advance(&mut self, elapsed: Duration) -> u32 {
        self.debt += elapsed.as_nanos() * u128::from(self.rate);
        let due = self.debt / NANOS;
        self.debt %= NANOS;
        // Un `due` au-delà du plafond ne tient peut-être pas dans un `u32` :
        // la comparaison se fait avant la conversion.
        if due > u128::from(MAX_CATCH_UP) {
            MAX_CATCH_UP
        } else {
            due as u32
        }
    }

    /// Le temps qui reste avant le prochain pas.
    pub(crate) fn until_next(&self) -> Duration {
        let missing = NANOS - self.debt;
        // Arrondi au-dessus : se réveiller une nanoseconde trop tôt rendrait zéro
        // pas, et un réveil pour rien.
        let nanos = missing.div_ceil(u128::from(self.rate));
        Duration::from_nanos(nanos as u64)
    }

    /// Abandonne le temps accumulé.
    ///
    /// Au retour d'une fenêtre minimisée ou masquée, et après un
    /// redimensionnement qui a bloqué la boucle : ce temps n'a pas été joué, et
    /// le rattraper ferait avancer la partie d'un coup.
    pub(crate) fn reset(&mut self) {
        self.debt = 0;
    }

    /// La durée d'un pas, en secondes.
    pub(crate) fn step_seconds(&self) -> f32 {
        1.0 / self.rate as f32
    }
}

#[cfg(test)]
mod tests;
