// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que l'accès au tampon doit tenir, et qu'une fenêtre ne dira pas.
//!
//! **Aucun de ces contrôles ne passe par la boucle** : elle ouvre une fenêtre,
//! donc rien ne l'éprouve en intégration continue. Ce qui est éprouvable ici est
//! l'arithmétique de l'adressage, et c'est précisément là que le défaut se
//! trouve — un pas de ligne confondu avec une largeur.

use super::*;

/// Un tampon de `stride × height` pixels, rempli d'une valeur reconnaissable.
fn buffer(stride: u32, height: u32) -> Vec<u8> {
    vec![0xA5; stride as usize * height as usize * BYTES_PER_PIXEL]
}

/// Un pixel se trouve au pas de ligne, et non à la largeur.
///
/// **Le défaut que ce test ferme** : avec un pas plus grand que la zone utile,
/// un adressage sur la largeur glisse d'une ligne à l'autre et l'interface
/// apparaît en biais. Il ne se verrait pas tant que l'étage rend sans marge,
/// donc jamais aujourd'hui — et le jour où il rendrait avec, il serait déjà
/// partout.
#[test]
fn un_pixel_se_trouve_au_pas_de_ligne() {
    let mut pixels = buffer(16, 4);
    let mut output = Output::new(&mut pixels, 10, 4, 16);

    output
        .pixel(0, 2)
        .unwrap_or_else(|| unreachable!("dans la zone utile"))
        .copy_from_slice(&[1, 2, 3, 4]);

    let base = 2 * 16 * BYTES_PER_PIXEL;
    assert_eq!(&pixels[base..base + 4], &[1, 2, 3, 4]);
    // Et rien à l'endroit qu'un adressage sur la largeur aurait touché.
    let faux = 2 * 10 * BYTES_PER_PIXEL;
    assert_eq!(&pixels[faux..faux + 4], &[0xA5; 4]);
}

/// Hors de la zone utile, `pixel` rend `None` plutôt que la marge.
///
/// La borne porte sur la **résolution courante**, pas sur la taille du tampon,
/// qui est dimensionné sur le plafond : écrire entre les deux ne planterait pas
/// et ne se verrait pas non plus, puisque la recopie n'en prend que l'image.
#[test]
fn hors_zone_utile_rend_rien() {
    let mut pixels = buffer(16, 4);
    let mut output = Output::new(&mut pixels, 10, 4, 16);

    assert!(output.pixel(9, 3).is_some());
    assert!(output.pixel(10, 0).is_none(), "au-delà de la largeur");
    assert!(output.pixel(0, 4).is_none(), "au-delà de la hauteur");
    assert!(output.pixel(u32::MAX, u32::MAX).is_none());
}

/// Les trois dimensions sont celles que la boucle a passées.
///
/// Une ligne, parce qu'il n'y a rien de plus à dire — mais elles sont ce sur
/// quoi tout dessin d'interface se calcule, et une permutation entre largeur et
/// hauteur s'y verrait.
#[test]
fn les_dimensions_sont_celles_de_l_image() {
    let mut pixels = buffer(16, 4);
    let output = Output::new(&mut pixels, 10, 4, 16);

    assert_eq!(
        (output.width(), output.height(), output.stride()),
        (10, 4, 16)
    );
}

/// La tranche brute couvre le tampon entier, plafond compris.
///
/// C'est voulu, et c'est ce qui distingue `pixels` de `pixel` : la première rend
/// ce que la boucle possède, la seconde borne sur l'image. Un hôte qui remplit
/// tout par la première écrit dans la marge sans conséquence, celle-ci n'étant
/// pas recopiée.
#[test]
fn la_tranche_brute_couvre_le_tampon() {
    let mut pixels = buffer(16, 4);
    let attendue = pixels.len();
    let mut output = Output::new(&mut pixels, 10, 4, 16);

    assert_eq!(output.pixels().len(), attendue);
}
