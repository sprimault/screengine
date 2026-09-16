// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! La mise à l'échelle, sur de petites images dont chaque pixel se vérifie.

use super::*;

/// Une image de `w × h` dont chaque pixel a une couleur distincte, au format
/// du moteur.
fn image(w: usize, h: usize) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| [(i as u8).wrapping_mul(16), 0x80, !(i as u8), 255])
        .collect()
}

/// Rend l'image dans une fenêtre de `tw × th`.
fn render(mode: Scale, w: usize, h: usize, tw: usize, th: usize) -> (Scaler, Vec<u32>) {
    let mut scaler = Scaler::new(mode, w as u32, h as u32);
    scaler.resize(tw as u32, th as u32);
    let mut out = vec![0xDEAD_BEEF; tw * th];
    scaler.blit(&image(w, h), &mut out);
    (scaler, out)
}

/// L'ordre des canaux : R, G, B en mémoire côté moteur, `0x00RRGGBB` côté
/// softbuffer. Une permutation rendrait une image aux rouges et aux bleus
/// échangés, sans erreur.
#[test]
fn convertit_le_rgba_du_moteur_au_format_de_la_surface() {
    assert_eq!(to_xrgb(&[0x11, 0x22, 0x33, 0xFF]), 0x0011_2233);
}

/// Chaque pixel source devient un carré de `k` de côté, et les bandes sont
/// noires : c'est le mode par défaut, et il ne mélange rien.
#[test]
fn le_facteur_entier_fait_des_carres_et_centre_l_image() {
    let (w, h) = (3, 2);
    // 7×5 : facteur 2, une colonne et une ligne de bande.
    let (_, out) = render(Scale::Integer, w, h, 7, 5);
    let source = image(w, h);

    for y in 0..5 {
        for x in 0..7 {
            let pixel = out[y * 7 + x];
            if x >= 6 || y >= 4 {
                assert_eq!(pixel, 0, "bande en ({x}, {y})");
            } else {
                let s = (y / 2) * w + x / 2;
                assert_eq!(pixel, to_xrgb(&source[s * 4..]), "pixel ({x}, {y})");
            }
        }
    }
}

/// Le centrage : une fenêtre de 10×6 pour une image de 3×2 en facteur 3
/// laisse une demi-bande de chaque côté.
#[test]
fn l_image_est_centree() {
    let (scaler, out) = render(Scale::Integer, 3, 2, 10, 6);
    assert_eq!(scaler.origin, (0, 0));
    assert_eq!(scaler.extent, (9, 6));
    assert_eq!(out[9], 0);

    let (scaler, _) = render(Scale::Integer, 3, 2, 11, 8);
    assert_eq!(scaler.extent, (9, 6));
    assert_eq!(scaler.origin, (1, 1));
}

/// Un facteur imposé plus grand que la fenêtre se replie sur celui qui tient,
/// sans erreur : la fenêtre a pu être réduite par l'utilisateur.
#[test]
fn un_facteur_impose_se_replie_quand_la_fenetre_rapetisse() {
    let (scaler, _) = render(Scale::Fixed(4), 3, 2, 7, 5);
    assert_eq!(scaler.extent, (6, 4));

    let (scaler, _) = render(Scale::Fixed(2), 3, 2, 30, 20);
    assert_eq!(scaler.extent, (6, 4));
}

/// Sur un facteur entier, le remplissage rend exactement l'agrandissement
/// entier : aucun fondu là où les frontières tombent sur des pixels entiers.
#[test]
fn le_remplissage_sur_un_facteur_entier_ne_melange_rien() {
    let (_, fill) = render(Scale::Fill, 4, 3, 12, 9);
    let (_, integer) = render(Scale::Integer, 4, 3, 12, 9);
    assert_eq!(fill, integer);
}

/// Sur un facteur non entier, l'intérieur de chaque aplat reste la couleur
/// exacte du pixel source : le fondu n'occupe que la frontière. Un bilinéaire
/// classique mélangerait presque partout.
#[test]
fn le_remplissage_garde_les_aplats_nets() {
    // 2×1 dans 5×10 : facteur 2,5, image de 5×2.
    let (scaler, out) = render(Scale::Fill, 2, 1, 5, 10);
    assert_eq!(scaler.extent, (5, 2));
    let source = image(2, 1);
    let (left, right) = (to_xrgb(&source[0..]), to_xrgb(&source[4..]));

    let row = &out[scaler.origin.1 * 5..][..5];
    assert_eq!(&row[..2], &[left, left]);
    assert_eq!(&row[3..], &[right, right]);
    assert_eq!(row[2], mix(left, right, 128));
}

/// Les tables d'un agrandissement non entier : poids extrêmes partout sauf
/// sur le pixel qui chevauche la frontière.
#[test]
fn les_tables_ne_fondent_que_la_frontiere() {
    let table = taps(2, 5);
    let blended: Vec<_> = table
        .iter()
        .filter(|t| t.weight != 0 && t.weight != 256)
        .collect();
    assert_eq!(blended.len(), 1, "{table:?}");
}

/// Une fenêtre plus petite que l'image : aucun facteur entier ne tient, et
/// le repli réduit sans paniquer ni écrire hors de la surface.
#[test]
fn une_fenetre_plus_petite_que_l_image_ne_panique_pas() {
    let (scaler, out) = render(Scale::Integer, 8, 4, 3, 5);
    assert_eq!(scaler.extent, (3, 1));
    assert_eq!(out.len(), 15);
    assert!(out.iter().all(|p| *p <= 0x00FF_FFFF));
}

/// La souris se ramène à la résolution interne, et vaut `None` dans les
/// bandes : un clic dans le noir ne doit pas viser le bord de l'image.
#[test]
fn la_position_se_ramene_a_la_resolution_interne() {
    let (scaler, _) = render(Scale::Integer, 3, 2, 11, 8);
    assert_eq!(scaler.to_source(1.0, 1.0), Some((0, 0)));
    assert_eq!(scaler.to_source(9.9, 6.9), Some((2, 1)));
    assert_eq!(scaler.to_source(0.5, 3.0), None);
    assert_eq!(scaler.to_source(10.0, 3.0), None);
}
