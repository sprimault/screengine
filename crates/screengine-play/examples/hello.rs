// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le plus petit programme qui affiche le moteur : une fenêtre, deux triangles
//! qui partagent une arête, Échap pour fermer. C'est celui que montrent les
//! README.

use screengine_play::{Affine3, Color, KeyCode, Play, Triangle, Vec3};

/// Le quadrilatère, en coordonnées de monde : X vers l'est, Z en haut.
const VERTICES: [Vec3; 4] = [
    Vec3::new(2.0, 2.5, 1.6),
    Vec3::new(3.5, -2.5, 1.6),
    Vec3::new(3.5, -2.5, -1.6),
    Vec3::new(2.0, 2.5, -1.6),
];

/// Deux triangles qui partagent l'arête des sommets 0 et 2.
const TRIANGLES: [Triangle; 2] = [
    Triangle {
        indices: [0, 2, 1],
        color: Color::new(0xE0, 0xA0, 0x30, 0xFF),
    },
    Triangle {
        indices: [0, 3, 2],
        color: Color::new(0xA0, 0xE0, 0x30, 0xFF),
    },
];

/// Ouvre la fenêtre ; Espace affiche le numéro du pas courant.
fn main() -> Result<(), screengine_play::Error> {
    Play::new().run(
        (),
        |_, tick| {
            if tick.input().pressed(KeyCode::Space) {
                println!("pas {}", tick.index());
            }
        },
        |_, context| {
            // La scène se resoumet à chaque image : la fin de la précédente a
            // périmé sa liste de dessin. Un refus ne peut venir que de la
            // capacité, que deux triangles n'atteignent pas.
            let _ = context.submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES);
        },
    )
}
