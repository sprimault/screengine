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
//! **Les deux cartes sont celles du dépôt**, `hosts/couloir.world` et
//! `hosts/salles.world`, intégrées à la compilation par `include_bytes!` : le
//! noyau n'ouvre aucun fichier, et un bench qui construirait sa propre carte
//! mesurerait un encodeur écrit pour lui. Le couloir reste ici alors que les hôtes
//! ne le parcourent plus : c'est le cas où tout est visible, donc celui où
//! l'élimination ne peut que perdre, et le retirer ne laisserait que le décor qui
//! l'avantage.
//!
//! Le harnais, les règles et ce que ces chiffres valent sur un téléphone sont
//! dans [`remplissage`](../remplissage/index.html) : aucun seuil, jamais
//! d'échec sur une durée, le minimum plutôt que la moyenne, et des chiffres qui
//! ne valent que comparés à eux-mêmes sur la même machine.
//!
//! # La référence, prise le 2026-09-25, et ce que la traversée en a fait
//!
//! ```text
//! chargement du couloir          0.003 ms
//! chargement des salles          0.007 ms
//! couloir — brut                 0.828 ms
//! couloir — traverse             0.824 ms
//! salles  — brut                 0.928 ms
//! salles  — traverse             0.903 ms
//! ```
//!
//! La référence de la 0.4.0 était `0.874 ms` pour le couloir, avant que la
//! traversée existe ; l'écart avec les `0.828` d'aujourd'hui est du bruit de
//! machine, pas un gain — c'est le même code de soumission.
//!
//! **Ce que ces chiffres disent, et c'est le résultat du lot** : la traversée ne
//! coûte rien là où tout est visible, et ne gagne presque rien sur quatre cellules.
//! Les deux moitiés comptent. Qu'elle ne coûte rien sur le couloir était la vraie
//! question — une élimination qui fait perdre sur le cas défavorable ne se garde
//! pas —, et les quatre microsecondes d'écart sont sous le bruit. Qu'elle ne gagne
//! que 2,7 % sur les salles n'est pas décevant : trois cellules sur quatre y sont
//! hors de vue, mais le troncature du champ de vision les écartait déjà presque
//! aussi vite, et ce que la traversée évite en plus — la transformation de leurs
//! sommets — ne pèse rien devant le remplissage. Le gain d'une traversée se lit sur
//! un décor où les cellules invisibles sont nombreuses *et* dans le champ, ce
//! qu'aucun décor du dépôt n'est encore.
//!
//! 640×360, tuiles de 64, un seul thread. Le couloir de conformance : deux
//! cellules, quatre surfaces par cellule, deux matériaux. Les salles : quatre
//! cellules, dont une salle non convexe et un étage que rien ne relie au reste.
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

use screengine::{Affine3, BYTES_PER_PIXEL, Camera, Config, Context, Texture, Vec3, World};

/// Largeur de référence.
const WIDTH: u32 = 640;

/// Hauteur de référence.
const HEIGHT: u32 = 360;

/// Répétitions par cas, comme pour le remplissage.
const IMAGES: u32 = 60;

/// La carte du dépôt, celle que les quatre hôtes chargent.
const COULOIR: &[u8] = include_bytes!("../hosts/couloir.world");

/// Le décor à quatre cellules, où la traversée a quelque chose à éliminer.
const SALLES: &[u8] = include_bytes!("../hosts/salles.world");

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

/// Le contexte et le tampon des mesures d'image.
///
/// Rendus une fois et repris par tous les cas : recréer un contexte par mesure
/// ferait payer ses allocations, qui sont précisément ce que le moteur promet de
/// ne pas refaire par image.
fn scene() -> (Context, Vec<u8>, std::sync::Arc<Texture>) {
    let context = Context::new(Config {
        max_width: WIDTH,
        max_height: HEIGHT,
        width: WIDTH,
        height: HEIGHT,
        tile_size: 64,
        max_triangles: 0,
    })
    .expect("configuration valide");
    let pixels = vec![0u8; WIDTH as usize * HEIGHT as usize * BYTES_PER_PIXEL];
    (context, pixels, std::sync::Arc::new(texture()))
}

/// Combien de pixels l'image porte, hors fond.
///
/// **Toute mesure d'image passe par là.** Sans ce contrôle, une régression qui
/// viderait l'image se lirait comme un gain — et c'est exactement le risque qu'une
/// élimination introduit : elle est d'autant plus rapide qu'elle retire trop.
fn peints(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(BYTES_PER_PIXEL)
        .filter(|p| p[..3] != [0, 0, 0])
        .count()
}

fn main() {
    println!("screengine — carte, {WIDTH}x{HEIGHT}, tuiles de 64, minimum sur {IMAGES} tours\n");

    ligne(
        "chargement du couloir",
        mesure(|| {
            World::load(black_box(COULOIR)).expect("carte valide");
        }),
    );
    ligne(
        "chargement des salles",
        mesure(|| {
            World::load(black_box(SALLES)).expect("carte valide");
        }),
    );

    let (mut context, mut pixels, texture) = scene();

    // **Les deux décors, les deux chemins, dans cet ordre.** Le couloir d'abord,
    // parce que c'est lui qui porte la mesure de référence et que tout y est
    // visible : la traversée n'y a rien à retirer et ne peut donc qu'y perdre.
    // Les salles ensuite, où trois cellules sur quatre sont hors de vue.
    for (nom, bytes, position) in [
        ("couloir", COULOIR, Vec3::new(2.0, 2.0, 2.0)),
        ("salles", SALLES, Vec3::new(2.0, 2.0, 2.0)),
    ] {
        let world = World::load(bytes).expect("carte valide");
        context
            .set_camera(Camera {
                position,
                ..Camera::DEFAULT
            })
            .expect("caméra valide");

        let brute = mesure(|| {
            context
                .submit_world(Affine3::IDENTITY, &world, |_| Some(&texture))
                .expect("capacité");
            context
                .frame_end(black_box(&mut pixels), WIDTH)
                .expect("image rendue");
        });
        let tout = peints(&pixels);
        assert!(
            tout as f64 >= f64::from(WIDTH * HEIGHT) * 0.5,
            "{nom} brut : {tout} pixels peints, la mesure ne porte sur rien"
        );

        let cell = world.locate(position);
        let visible = mesure(|| {
            context
                .submit_world_visible(Affine3::IDENTITY, &world, cell, None, |_| Some(&texture))
                .expect("capacité");
            context
                .frame_end(black_box(&mut pixels), WIDTH)
                .expect("image rendue");
        });
        // **La traversée doit rendre la même image que le chemin brut**, et c'est
        // ce qui rend les deux durées comparables : une élimination qui retire du
        // visible serait plus rapide et fausse.
        assert_eq!(
            peints(&pixels),
            tout,
            "{nom} : la traversée ne peint pas la même image que le chemin brut"
        );

        ligne(&alloc_nom(nom, "brut"), brute);
        ligne(&alloc_nom(nom, "traverse"), visible);
    }
}

/// Le libellé d'une ligne, décor puis chemin.
fn alloc_nom(decor: &str, chemin: &str) -> String {
    format!("{decor} — {chemin}")
}
