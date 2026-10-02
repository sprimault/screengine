// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que les deux traits promettent à un appelant Rust.
//!
//! L'unicité des messages et leur couverture se vérifient là où la frontière C
//! les expose, dans `screengine-ffi` : c'est elle qui en rend un pointeur, et
//! c'est son test qui tient la liste exhaustive des variantes.

use alloc::boxed::Box;
use alloc::format;

use super::{Argument, Error, Malformation};

/// `Display` n'ajoute rien au littéral, sur les trois formes de variante : nue,
/// portant un [`Argument`], portant une [`Malformation`].
#[test]
fn display_ecrit_le_message() {
    for error in [
        Error::OutOfMemory,
        Error::InvalidArgument(Argument::TileSize),
        Error::InvalidFormat(Malformation::Signature),
    ] {
        assert_eq!(format!("{error}"), error.message());
    }
}

/// Une erreur du noyau remonte par `?` dans une fonction qui rend
/// `Box<dyn Error>`, sans enveloppe intermédiaire.
///
/// C'est tout l'objet des deux implémentations : un hôte Rust n'a plus à écrire
/// sa propre traduction pour propager, ni à se rabattre sur `{:?}`.
#[test]
fn propage_en_boite() {
    fn refuse() -> Result<(), Box<dyn core::error::Error>> {
        Err(Error::InvalidArgument(Argument::Stride))?;
        Ok(())
    }

    let boxed = refuse().unwrap_err();
    assert_eq!(
        format!("{boxed}"),
        Error::InvalidArgument(Argument::Stride).message()
    );
}
