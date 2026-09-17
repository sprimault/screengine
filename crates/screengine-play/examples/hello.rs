// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le plus petit programme qui affiche le moteur : une fenêtre, le triangle de
//! l'étape 0, Échap pour fermer. C'est celui que montrent les README.
//!
//! Il n'y a encore rien à soumettre au moteur, d'où la fermeture de rendu vide.

use screengine_play::{KeyCode, Play};

/// Ouvre la fenêtre ; Espace affiche le numéro du pas courant.
fn main() -> Result<(), screengine_play::Error> {
    Play::new().run(
        (),
        |_, tick| {
            if tick.input().pressed(KeyCode::Space) {
                println!("pas {}", tick.index());
            }
        },
        |_, _context| {},
    )
}
