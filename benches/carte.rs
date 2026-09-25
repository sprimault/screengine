// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La référence du chemin des cartes, prise avant que l'étape 5 l'élimine.
//!
//! **Deux mesures, pour deux coûts de nature différente.** Le chargement est
//! ponctuel — il triangule, calcule les coordonnées de texture et apparie les
//! portails —, et se paie une fois par carte. La soumission est **par image**,
//! et c'est elle que la traversée par portails va changer : aujourd'hui elle
//! transforme toutes les cellules, murs cachés compris.
//!
//! Sans cette seconde mesure prise maintenant, l'étape 5 n'aurait rien à quoi
//! se comparer, et on ne pourrait dire ni ce que l'élimination gagne, ni ce
//! qu'elle coûte sur une scène où tout est visible — le cas où elle ne peut que
//! perdre.
//!
//! **La carte est celle du dépôt**, `hosts/couloir.world`, intégrée à la
//! compilation par `include_bytes!` : le noyau n'ouvre aucun fichier, et un
//! bench qui construirait sa propre carte mesurerait un encodeur écrit pour
//! lui.
//!
//! Le harnais, les règles et ce que ces chiffres valent sur un téléphone sont
//! dans [`remplissage`](../remplissage/index.html) : aucun seuil, jamais
//! d'échec sur une durée, le minimum plutôt que la moyenne, et des chiffres qui
//! ne valent que comparés à eux-mêmes sur la même machine.
//!
//! # La référence, prise le 2026-09-25
//!
//! ```text
//! chargement d'une carte         0.003 ms
//! soumission + image             0.874 ms
//! ```
//!
//! 640×360, tuiles de 64, un seul thread. Le couloir de conformance : deux
//! cellules, quatre surfaces par cellule, deux matériaux.
//!
//! **Le chargement ne pèse rien, et c'est attendu** : trois microsecondes pour
//! seize surfaces. Il est mesuré quand même, parce que c'est là que
//! triangulation, coordonnées de texture et appariement des portails se paient,
//! et qu'une carte de mille cellules ne se déduit pas de celle-ci par
//! proportionnalité — l'appariement compare les portails entre eux.
//!
//! **Point de comparaison hors machine** : la même scène, avec trois caisses en
//! plus, tourne en 2,0 à 2,2 ms par image sur un téléphone arm64 de 2025. Le
//! rapport entre les deux est ce qu'il faut garder à l'esprit en lisant tout ce
//! fichier.

use core::hint::black_box;
use core::time::Duration;
use std::time::Instant;

use screengine::{Affine3, BYTES_PER_PIXEL, Config, Context, Texture, World};

/// Largeur de référence.
const WIDTH: u32 = 640;

/// Hauteur de référence.
const HEIGHT: u32 = 360;

/// Répétitions par cas, comme pour le remplissage.
const IMAGES: u32 = 60;

/// La carte du dépôt, celle que les quatre hôtes chargent.
const COULOIR: &[u8] = include_bytes!("../hosts/couloir.world");

/// Le plus court temps observé sur `IMAGES` répétitions.
///
/// Deux tours à blanc d'abord : la première image paie le cache d'instructions
/// et les défauts de page du tampon.
fn mesure(mut tour: impl FnMut()) -> Duration {
    tour();
    tour();

    let mut court = Duration::MAX;
    for _ in 0..IMAGES {
        let debut = Instant::now();
        tour();
        court = court.min(debut.elapsed());
    }
    court
}

/// Écrit une ligne de résultat.
///
/// Trois décimales, là où le remplissage en met deux : un chargement se compte
/// en microsecondes, et deux décimales l'afficheraient comme gratuit.
fn ligne(quoi: &str, duree: Duration) {
    println!("{quoi:<28} {:>8.3} ms", duree.as_secs_f64() * 1e3);
}

/// Un aplat clair : la carte porte deux matériaux, et ce bench ne mesure pas
/// l'échantillonnage — le damier du remplissage y est à sa place, pas ici.
fn texture() -> Texture {
    let side = 64usize;
    let pixels = vec![0xC0u8; side * side * BYTES_PER_PIXEL];
    Texture::load(side as u32, side as u32, &pixels).expect("texture valide")
}

fn main() {
    println!(
        "screengine — carte, {WIDTH}x{HEIGHT}, tuiles de 64, minimum sur {IMAGES} tours\n\
         aucune elimination : toutes les cellules sont soumises\n"
    );

    ligne(
        "chargement d'une carte",
        mesure(|| {
            World::load(black_box(COULOIR)).expect("carte valide");
        }),
    );

    let world = World::load(COULOIR).expect("carte valide");
    let texture = std::sync::Arc::new(texture());
    let mut context = Context::new(Config {
        max_width: WIDTH,
        max_height: HEIGHT,
        width: WIDTH,
        height: HEIGHT,
        tile_size: 64,
        max_triangles: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; WIDTH as usize * HEIGHT as usize * BYTES_PER_PIXEL];

    let duree = mesure(|| {
        context
            .submit_world(Affine3::IDENTITY, &world, |_| Some(&texture))
            .expect("capacité");
        context
            .frame_end(black_box(&mut pixels), WIDTH)
            .expect("image rendue");
    });

    // La caméra par défaut est dans le couloir, mais rien ne garantit qu'elle y
    // voie quelque chose : sans ce contrôle, une régression qui viderait l'image
    // se lirait comme un gain.
    let peints = pixels
        .chunks_exact(BYTES_PER_PIXEL)
        .filter(|p| p[..3] != [0, 0, 0])
        .count();
    assert!(
        peints as f64 >= f64::from(WIDTH * HEIGHT) * 0.5,
        "carte : {peints} pixels peints, la mesure ne porte sur rien"
    );
    ligne("soumission + image", duree);
}
