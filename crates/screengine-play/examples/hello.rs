// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le plus petit programme qui affiche le moteur : une fenêtre, un triangle,
//! Échap pour fermer. C'est celui que montrent les README, mot pour mot.
//!
//! Il reste petit délibérément. Ce qu'une caméra, un couloir et des entrées
//! demandent est dans `couloir.rs` ; ici, on montre qu'une fenêtre qui rend
//! quelque chose tient en vingt lignes.

use screengine_play::{Affine3, Color, KeyCode, Play, Triangle, Vec3};

/// Le triangle, à quatre unités devant la caméra par défaut.
const VERTICES: [Vec3; 3] = [
    Vec3::new(4.0, 1.5, -1.0),
    Vec3::new(4.0, 0.0, 1.5),
    Vec3::new(4.0, -1.5, -1.0),
];

/// Sa face avant regarde la caméra ; l'autre sens en ferait un dos, éliminé.
const TRIANGLES: [Triangle; 1] = [Triangle {
    indices: [0, 1, 2],
    color: Color::new(0xE0, 0xA0, 0x30, 0xFF),
}];

/// Ouvre la fenêtre ; Échap ferme.
fn main() -> Result<(), screengine_play::Error> {
    Play::new().run(
        (),
        |_, tick| {
            if tick.input().pressed(KeyCode::Escape) {
                tick.exit();
            }
        },
        |_, context| {
            // La scène se resoumet à chaque image : la fin de la précédente a
            // périmé sa liste de dessin.
            let _ = context.submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES);
        },
    )
}
