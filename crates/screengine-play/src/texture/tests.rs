// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le décodage PNG, sur des images de deux texels de côté dont chaque octet se
//! vérifie, puis sur les textures que le dépôt versionne.

use png::{BitDepth, Encoder};

use super::*;

/// Les textures que l'étage d'accueil versionne, avec le nom qui les désigne
/// dans un message d'échec.
///
/// **Toutes**, et pas seulement celles qu'un exemple affiche aujourd'hui : ce
/// qui entre dans le dépôt doit se charger, et une texture qu'aucun test ne
/// touche n'est vérifiée que le jour où quelqu'un lance la fenêtre. Les quatre
/// dernières sont arrivées sans que cette liste bouge, ce qui est exactement le
/// défaut qu'une liste écrite à la main finit par avoir.
const ASSETS: [(&str, &[u8]); 7] = [
    ("brick", include_bytes!("../../assets/brick.png")),
    ("stone", include_bytes!("../../assets/stone.png")),
    ("wood", include_bytes!("../../assets/wood.png")),
    ("mur-mousse", include_bytes!("../../assets/mur-mousse.png")),
    (
        "sol-pave-mousse",
        include_bytes!("../../assets/sol-pave-mousse.png"),
    ),
    ("ciel-jour", include_bytes!("../../assets/ciel-jour.png")),
    (
        "malle-rouillee",
        include_bytes!("../../assets/malle-rouillee.png"),
    ),
];

/// Encode un PNG de test, sans palette.
fn encode(width: u32, height: u32, color: ColorType, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(data).unwrap();
    writer.finish().unwrap();
    out
}

/// Encode un PNG palettisé de `width × height` indices.
fn encode_indexed(width: u32, height: u32, palette: &[u8], indices: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, width, height);
    encoder.set_color(ColorType::Indexed);
    encoder.set_depth(BitDepth::Eight);
    encoder.set_palette(palette.to_vec());
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(indices).unwrap();
    writer.finish().unwrap();
    out
}

/// Le texel attendu au niveau 0, dans l'ordre mémoire du moteur.
fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from_le_bytes([r, g, b, a])
}

/// Quatre texels distincts traversent le décodage sans être touchés.
#[test]
fn un_png_rgba_garde_ses_texels() {
    let pixels = [
        0x10, 0x20, 0x30, 0xFF, //
        0x40, 0x50, 0x60, 0x80, //
        0x70, 0x80, 0x90, 0x40, //
        0xA0, 0xB0, 0xC0, 0x00,
    ];
    let texture = load_png(&encode(2, 2, ColorType::Rgba, &pixels)).unwrap();

    assert_eq!((texture.width(), texture.height()), (2, 2));
    assert_eq!(texture.texel(0, 0, 0), rgba(0x10, 0x20, 0x30, 0xFF));
    assert_eq!(texture.texel(0, 1, 0), rgba(0x40, 0x50, 0x60, 0x80));
    assert_eq!(texture.texel(0, 0, 1), rgba(0x70, 0x80, 0x90, 0x40));
    assert_eq!(texture.texel(0, 1, 1), rgba(0xA0, 0xB0, 0xC0, 0x00));
}

/// Une image sans canal alpha devient opaque plutôt que transparente : un alpha
/// nul par défaut donnerait des surfaces invisibles sans rien signaler.
#[test]
fn un_png_sans_alpha_devient_opaque() {
    let pixels = [
        0x11, 0x22, 0x33, //
        0x44, 0x55, 0x66, //
        0x77, 0x88, 0x99, //
        0xAA, 0xBB, 0xCC,
    ];
    let texture = load_png(&encode(2, 2, ColorType::Rgb, &pixels)).unwrap();

    assert_eq!(texture.texel(0, 0, 0), rgba(0x11, 0x22, 0x33, 0xFF));
    assert_eq!(texture.texel(0, 1, 1), rgba(0xAA, 0xBB, 0xCC, 0xFF));
}

/// Un niveau de gris se répète sur les trois canaux : le moteur ne connaît que
/// des couleurs directes, et ne saurait pas qu'un seul octet vaut pour trois.
#[test]
fn un_png_en_niveaux_de_gris_se_repete_sur_trois_canaux() {
    let png = encode(2, 2, ColorType::Grayscale, &[0x00, 0x40, 0x80, 0xFF]);
    let texture = load_png(&png).unwrap();

    assert_eq!(texture.texel(0, 1, 0), rgba(0x40, 0x40, 0x40, 0xFF));
    assert_eq!(texture.texel(0, 1, 1), rgba(0xFF, 0xFF, 0xFF, 0xFF));
}

/// Un gris avec alpha garde son alpha et répète le reste.
#[test]
fn un_png_gris_avec_alpha_garde_son_alpha() {
    let pixels = [0x10, 0xFF, 0x20, 0x80, 0x30, 0x40, 0x40, 0x00];
    let texture = load_png(&encode(2, 2, ColorType::GrayscaleAlpha, &pixels)).unwrap();

    assert_eq!(texture.texel(0, 0, 0), rgba(0x10, 0x10, 0x10, 0xFF));
    assert_eq!(texture.texel(0, 1, 1), rgba(0x40, 0x40, 0x40, 0x00));
}

/// Une palette est étendue au chargement : le moteur n'en a pas.
#[test]
fn une_palette_est_etendue() {
    let palette = [0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00];
    let texture = load_png(&encode_indexed(2, 2, &palette, &[0, 1, 1, 0])).unwrap();

    assert_eq!(texture.texel(0, 0, 0), rgba(0xFF, 0x00, 0x00, 0xFF));
    assert_eq!(texture.texel(0, 1, 0), rgba(0x00, 0xFF, 0x00, 0xFF));
}

/// Un côté qui n'est pas une puissance de deux est refusé, et l'erreur vient du
/// moteur : c'est sa contrainte, pas une limite du format.
#[test]
fn un_cote_qui_n_est_pas_une_puissance_de_deux_est_refuse() {
    let refus = load_png(&encode(3, 2, ColorType::Rgba, &[0; 24])).unwrap_err();

    assert!(matches!(
        refus,
        Error::Engine(screengine::Error::InvalidArgument(Argument::TextureSize))
    ));
}

/// Ce qui n'est pas un PNG, ou plus tout à fait, rend une erreur de décodage et
/// non une panique.
#[test]
fn des_octets_tronques_rendent_une_erreur_de_decodage() {
    let complet = encode(2, 2, ColorType::Rgba, &[0; 16]);
    let tronque = load_png(&complet[..30]).unwrap_err();
    let bruit = load_png(b"pas un png").unwrap_err();

    assert!(matches!(tronque, Error::Png(_)));
    assert!(matches!(bruit, Error::Png(_)));
}

/// Les textures versionnées se chargent, et leur chaîne descend jusqu'à 1×1.
///
/// C'est le contrôle qui manquerait le jour où un `.gitattributes` laisserait
/// convertir un binaire : le fichier resterait lisible par un visualiseur et le
/// couloir s'ouvrirait sur une erreur.
#[test]
fn les_textures_du_depot_se_chargent() {
    for (name, bytes) in ASSETS {
        let texture = load_png(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((texture.width(), texture.height()), (512, 512), "{name}");
        assert_eq!(texture.level_count(), 10, "{name}");
    }
}

/// Elles sont opaques. Une texture de décor qui ne l'est pas laisse voir à
/// travers un mur, ce qui ne se remarque que dans l'image et jamais dans une
/// mesure de taille.
#[test]
fn les_textures_du_depot_sont_opaques() {
    for (name, bytes) in ASSETS {
        let texture = load_png(bytes).unwrap();
        let translucide = texture
            .level_texels(0)
            .iter()
            .position(|texel| texel >> 24 != 0xFF);
        assert_eq!(translucide, None, "{name}");
    }
}
