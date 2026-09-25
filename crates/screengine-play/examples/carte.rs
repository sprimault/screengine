// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Un décor **chargé depuis un fichier**, parcouru à la souris et au clavier.
//!
//! **Ce que `couloir.rs` ne montre pas.** Celui-là décrit sa géométrie en Rust,
//! panneau par panneau, et cuit ses propres lightmaps : c'est ce qu'il faut pour
//! éprouver l'éclairage, et ce n'est pas ainsi qu'un décor arrive dans un jeu.
//! Ici, la carte et les caisses viennent de deux fichiers, exactement ceux que
//! les hôtes C, C++, wasm et Android chargent — **la même scène des cinq
//! côtés**, ce qui est la seule façon de vérifier qu'aucun des deux chemins vers
//! le moteur n'est de seconde classe.
//!
//! Les deux fichiers sont intégrés par `include_bytes!` plutôt qu'ouverts :
//! un exemple qui lirait un chemin relatif ne marcherait que lancé depuis la
//! racine du dépôt, et l'hôte qui lit les fichiers est déjà montré ailleurs.
//!
//! Les textures, elles, restent celles de l'hôte : la carte nomme ses matériaux
//! — `mur` et `sol` —, l'hôte décide de ce qu'il met derrière chaque nom. Ce
//! sont celles de `couloir.rs`, et c'est le sujet : **un même décor change
//! entièrement d'aspect sans qu'un octet du fichier bouge**, parce que rien de
//! ce qui habille n'est dedans. Les autres hôtes y mettent des damiers, faute de
//! décodeur PNG.

use std::sync::Arc;

use screengine_play::screengine::Angle;
use screengine_play::{Affine3, FreeCamera, KeyCode, Mesh, Play, Texture, Vec3, World, load_png};

/// La carte du dépôt, celle que les quatre autres hôtes chargent.
const COULOIR: &[u8] = include_bytes!("../../../hosts/couloir.world");

/// Le maillage des caisses, de la même provenance.
const CAISSE: &[u8] = include_bytes!("../../../hosts/caisse.mesh");

/// Les murs du couloir, la texture de `couloir.rs`.
const MUR: &[u8] = include_bytes!("../assets/mur-mousse.png");

/// Le sol et le plafond, de la même provenance.
const SOL: &[u8] = include_bytes!("../assets/sol-pave-mousse.png");

/// Les côtés d'une caisse.
const MALLE: &[u8] = include_bytes!("../assets/malle-rouillee.png");

/// Où sont posées les caisses : abscisse, ordonnée, et l'angle qui les tourne.
const CRATES: [(f32, f32, f32); 3] = [(6.0, -1.2, 0.4), (11.0, 1.4, -0.7), (17.0, -0.6, 1.1)];

/// L'échelle des caisses.
///
/// Le maillage fait deux unités de côté, le couloir six de large : posée telle
/// quelle, une caisse en occupe le tiers. Le fichier ne se redimensionne pas —
/// son empreinte de conformance est figée —, donc l'échelle va dans la matrice
/// de modèle, qui est faite pour ça.
const CRATE_SCALE: f32 = 0.5;

/// La cote du centre d'une caisse, sa demi-hauteur au-dessus du sol du couloir.
const CRATE_Z: f32 = -1.0;

/// La matrice d'une caisse : une rotation autour du zénith mise à l'échelle,
/// puis une translation.
///
/// Écrite coefficient par coefficient faute d'un constructeur qui compose les
/// trois — `Affine3` n'en a qu'un, sans échelle. Les sinus viennent des tables
/// du noyau : un exemple n'est comparé à aucune empreinte, mais appeler la libm
/// ici serait le premier pas vers l'appeler ailleurs.
fn crate_model(x: f32, y: f32, angle: f32) -> Affine3 {
    let a = Angle::from_radians(angle);
    let (c, s) = (a.cos() * CRATE_SCALE, a.sin() * CRATE_SCALE);
    Affine3 {
        m: [c, s, 0.0, -s, c, 0.0, 0.0, 0.0, CRATE_SCALE, x, y, CRATE_Z],
    }
}

/// Ce que la boucle garde entre deux images.
struct Scene {
    /// La caméra dirigée au clavier et à la souris.
    camera: FreeCamera,
    /// Le décor.
    world: World,
    /// Le maillage posé trois fois.
    crate_mesh: Mesh,
    /// Une texture par matériau de la carte, dans l'ordre qu'elle déclare.
    materials: Vec<Arc<Texture>>,
    /// La texture des caisses, sur leurs deux emplacements.
    crate_texture: Arc<Texture>,
}

/// Ouvre la fenêtre ; Échap ferme.
fn main() -> Result<(), screengine_play::Error> {
    let world = World::load(COULOIR).expect("carte du dépôt valide");
    // L'hôte lit les noms et décide de ce qu'il charge : le moteur ne connaît
    // que des emplacements à remplir, dans l'ordre de la table. Un nom qu'il ne
    // reconnaît pas prend le pavé, plutôt que de laisser un trou sans texture
    // dans une image qu'on regarde.
    let mur = Arc::new(load_png(MUR)?);
    let sol = Arc::new(load_png(SOL)?);
    let materials = (0..world.material_count())
        .map(|rank| match world.material_name(rank) {
            Some("mur") => Arc::clone(&mur),
            _ => Arc::clone(&sol),
        })
        .collect();

    let scene = Scene {
        camera: FreeCamera::new(Vec3::ZERO),
        world,
        crate_mesh: Mesh::load(CAISSE).expect("maillage du dépôt valide"),
        materials,
        crate_texture: Arc::new(load_png(MALLE)?),
    };

    Play::new().title("Screengine — carte chargée").run(
        scene,
        |scene, tick| {
            if tick.input().pressed(KeyCode::Escape) {
                tick.exit();
            }
            scene.camera.update(tick);
        },
        |scene, context| {
            let _ = context.set_camera(scene.camera.camera());
            // Un refus ne peut venir que de la capacité, que cette scène
            // n'approche pas ; le laisser passer vaut mieux qu'arrêter la
            // boucle sur une image manquante.
            let _ = context.submit_world(Affine3::IDENTITY, &scene.world, |rank| {
                scene.materials.get(rank as usize)
            });
            for &(x, y, angle) in &CRATES {
                let model = crate_model(x, y, angle);
                // Les deux emplacements portent la même texture. Les hôtes de
                // conformance laissent le second vide, et la caisse y prend le
                // bleu que son fichier donne au dessus : c'est ainsi qu'ils
                // montrent un lot sans texture. Un exemple qu'on regarde n'a pas
                // à porter cette démonstration.
                let _ =
                    context.submit_mesh(model, &scene.crate_mesh, |_| Some(&scene.crate_texture));
            }
        },
    )
}
