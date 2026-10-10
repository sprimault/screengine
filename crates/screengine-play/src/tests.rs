// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que les réglages de [`Play`] demandent au moteur.
//!
//! La boucle elle-même exige une fenêtre et ne se teste pas ici. La traduction
//! des réglages en [`Config`], si : c'est le seul endroit où un réglage peut
//! partir à zéro sans que rien ne rougisse.

use super::{Play, Scale};

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
