// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que les réglages de [`Play`] demandent au moteur.
//!
//! La boucle elle-même exige une fenêtre et ne se teste pas ici. La traduction
//! des réglages en [`Config`], si : c'est le seul endroit où un réglage peut
//! partir à zéro sans que rien ne rougisse.

use super::{Play, Scale, load_png, load_png_icon};

/// Chaque item du noyau que ce crate promet à plat est joignable sans
/// `screengine::`.
///
/// **Le seul contrôle de la surface plate, et il ne tenait à rien jusqu'ici.**
/// Un item retiré de la liste ne casse aucune compilation : l'hôte passe par le
/// chemin long, personne ne le voit, et la promesse « rien n'est atteignable par
/// un seul chemin » se perd en silence. Ce cas-ci la tient parce qu'il **nomme**
/// chaque item sans préfixe : un nom qui sort de la liste ne compile plus.
///
/// Ce qu'il ne fait pas, et qu'il ne faut pas lui prêter : détecter un item
/// **nouveau** du noyau qu'on aurait oublié de propager. Aucune comparaison des
/// deux surfaces n'est possible sans réflexion, et c'est la question posée à
/// chaque lot qui rattrape celle-là.
#[test]
fn la_surface_plate_reste_joignable() {
    // **L'import fait tout le travail** : un nom absent de la liste plate est une
    // erreur de compilation, que `allow(unused_imports)` ne masque pas — il ne
    // tait que l'inutilisation. Nommer les types dans des annotations en plus
    // n'aurait rien prouvé de mieux, et aurait traîné leurs paramètres de durée
    // de vie dans un cas qui ne parle pas d'eux.
    #[allow(unused_imports)]
    use super::{
        Affine3, Angle, Argument, BYTES_PER_PIXEL, Camera, Color, Config, Context, CoreError,
        CoreOutput, DepthMode, Element, Filter, Frame, Hit, LINE_CAPACITY, Light, Lightmap,
        LightmapFault, Lightmaps, Line, MAX_LIGHTMAP_SIZE, MAX_LIGHTS, MAX_OVERBRIGHT,
        MAX_RESOLUTION, MAX_TEXEL_COORD, MAX_TEXTURE_SIZE, Malformation, Mesh, Point, Quat, Rect,
        Rows, SWEEP_CELLS, SimdPath, Sprite, SpriteOrientation, Surfaces, TILE_SIZES,
        TRAVERSAL_CELLS, TRAVERSAL_DEPTH, TRIANGLE_CAPACITY, Texture, Triangle, Vec3, VertexUv,
        VertexUv2, Visibility, World, lightmap_fault, sweep_reach, sweep_skin,
    };

    // Les bornes de traversée sont lues plutôt que seulement importées : elles
    // sont les deux dernières entrées de la liste, et un `pub use` tronqué à
    // l'avant-dernière ligne se verrait ici autant qu'à la compilation.
    //
    // En bloc `const`, ce que clippy exige d'une assertion sur des constantes —
    // et il a raison : évaluée à la compilation, elle ne peut plus être fausse à
    // l'exécution.
    const { assert!(TRAVERSAL_DEPTH > 0 && TRAVERSAL_CELLS > 0) };
    assert!(TILE_SIZES.iter().all(|&t| t.is_power_of_two()));
}

/// Les réglages par défaut passent la résolution d'ouverture en plafond, et
/// laissent les deux budgets à `0` — ce que le moteur lit comme « prends le
/// défaut ».
#[test]
fn les_defauts_ne_plafonnent_rien() {
    let config = Play::new().config();

    assert_eq!(
        (config.width, config.height),
        (config.max_width, config.max_height)
    );
    assert_eq!(config.max_triangles, 0);
    assert_eq!(config.max_lines, 0);
}

/// Chaque réglage arrive dans le champ correspondant, et un plafond de
/// résolution ne déplace pas la résolution d'ouverture.
#[test]
fn chaque_reglage_arrive_dans_sa_configuration() {
    let config = Play::new()
        .resolution(320, 180)
        .max_resolution(640, 360)
        .tile_size(32)
        .max_triangles(40_000)
        .max_lines(4_096)
        .scale(Scale::Fill)
        .config();

    assert_eq!((config.width, config.height), (320, 180));
    assert_eq!((config.max_width, config.max_height), (640, 360));
    assert_eq!(config.tile_size, 32);
    assert_eq!(config.max_triangles, 40_000);
    assert_eq!(config.max_lines, 4_096);
}

/// Une capacité au-delà de ce que le moteur accepte est refusée par lui, pas
/// par cet étage : la configuration la porte telle quelle.
///
/// C'est la forme voulue — un seul endroit juge, et c'est celui qui alloue.
#[test]
fn une_capacite_demesuree_part_telle_quelle() {
    let config = Play::new().max_triangles(u32::MAX).config();

    assert_eq!(config.max_triangles, u32::MAX);
}

/// L'icône du dépôt se décode, et ses côtés sortent tels quels.
///
/// **Le seul contrôle que l'icône puisse recevoir sans fenêtre**, et il vaut
/// d'être écrit : rien d'autre n'éprouve ce chemin — aucun test n'ouvre de
/// fenêtre, et la pose sous Wayland est un no-op que rien ne fera jamais
/// rougir. Ce qu'il attrape est un PNG versionné devenu illisible, ou un
/// décodage qui perdrait les dimensions en route.
#[test]
fn l_icone_du_depot_se_decode() {
    let icon = load_png_icon(include_bytes!("../assets/icone.png")).expect("icône du dépôt");

    assert_eq!((icon.width, icon.height), (64, 64));
    assert_eq!(icon.rgba.len(), 64 * 64 * 4, "quatre octets par pixel");
}

/// Une icône hors bornes est refusée, et une texture reste tenue aux puissances
/// de deux.
///
/// **Les deux domaines ne sont pas le même, et c'est tout l'objet du cas.** Une
/// icône de 24 est légitime — c'est une taille que les systèmes demandent — là
/// où une texture de 24 ne l'est pas, le repli du moteur se faisant par masque.
/// Partager le contrôle entre les deux aurait donc refusé la moitié des tailles
/// d'icône usuelles, et c'est l'erreur que le paramètre de dimensions évite.
#[test]
fn les_bornes_d_une_icone_ne_sont_pas_celles_d_une_texture() {
    let png = png_uni(24, 24);

    let icon = load_png_icon(&png).expect("vingt-quatre est une taille d'icône");
    assert_eq!((icon.width, icon.height), (24, 24));
    assert!(
        load_png(&png).is_err(),
        "et ce n'est pas une taille de texture"
    );

    // L'autre bout du domaine propre à l'icône : au-delà de 256, c'est le
    // compositeur qui réduirait, et moins bien qu'un outil d'image.
    assert!(load_png_icon(&png_uni(512, 512)).is_err(), "trop grande");
}

/// Un PNG RGBA d'une seule couleur, écrit pour le test.
///
/// Encodé ici plutôt que versionné : le projet écrit ses fichiers d'épreuve en
/// octets, et une image de plus dans `assets/` pour deux assertions serait un
/// binaire que personne ne relit.
fn png_uni(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("en-tête PNG");
    let pixels = vec![0x40; (width * height * 4) as usize];
    writer.write_image_data(&pixels).expect("données PNG");
    writer.finish().expect("PNG complet");
    out
}
