// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! L'empreinte, contre les valeurs de référence de FNV et contre ses propres
//! règles.

use super::*;

/// Le vecteur de référence de FNV-1a 64 : un hôte qui l'implémente dans son
/// langage se vérifie contre la même valeur, et c'est ce qui rend les
/// empreintes comparables.
#[test]
fn rend_la_valeur_de_reference_de_fnv() {
    let mut fnv = Fnv(OFFSET);
    fnv.write(b"foobar");
    assert_eq!(format(fnv.0), "85944171f73967e8");
}

/// Seize chiffres toujours, zéros de tête compris : une comparaison de
/// chaînes entre deux hôtes échouerait sur une empreinte courte.
#[test]
fn le_format_garde_les_zeros_de_tete() {
    assert_eq!(format(0xAB), "00000000000000ab");
}

/// Le `stride` ne change pas l'empreinte : l'hôte C rend avec un `stride`
/// plus grand que la largeur, et doit tomber sur celle du chemin Rust.
#[test]
fn le_stride_n_entre_pas_dans_l_empreinte() {
    let tight = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let loose = [
        1u8, 2, 3, 4, 0xEE, 0xEE, 0xEE, 0xEE, 5, 6, 7, 8, 0xEE, 0xEE, 0xEE, 0xEE,
    ];
    assert_eq!(image(&tight, 1, 2, 1), image(&loose, 1, 2, 2));
}

/// Les dimensions sont hachées : les mêmes octets en 2×1 et en 1×2 ne sont
/// pas la même image.
#[test]
fn les_dimensions_distinguent_deux_images_aux_memes_octets() {
    let bytes = [9u8; 8];
    assert_ne!(image(&bytes, 2, 1, 2), image(&bytes, 1, 2, 1));
}
