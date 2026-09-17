// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le remplissage, en coordonnées entières.
//!
//! Tout ce qui est ici s'évalue en coordonnées globales de l'image. Rien ne
//! dépend d'un découpage : une valeur rebasée à l'origine d'une tuile reste
//! exacte parce qu'une translation entière l'est, mais son point de départ se
//! calcule toujours par la forme close, au coin global.

mod triangle;

pub use triangle::{Clip, Point, fill_triangle};

/// Où le remplissage écrit ses pixels.
///
/// Un puits plutôt qu'une tranche, et ce n'est pas de l'abstraction gratuite :
/// un pixel écrit deux fois est invisible dans le tampon final, donc un
/// recouvrement d'arête partagée ne se détecte qu'en comptant les écritures.
/// Sans ce point de passage, le défaut le plus coûteux du projet n'aurait aucun
/// test capable de l'attraper.
///
/// La généricité est résolue à la compilation : il n'y a pas d'appel indirect
/// dans la boucle de remplissage.
pub trait Target {
    /// Écrit un pixel, en coordonnées entières de l'image.
    ///
    /// Les coordonnées sont toujours dans la fenêtre passée au remplissage :
    /// l'implémentation n'a pas à les vérifier.
    fn put(&mut self, x: i32, y: i32, color: u32);
}
