// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La référence de performance, prise avant que l'étape 3 touche au remplissage.
//!
//! **Deux mesures, et pas une de plus.** Un quadrilatère texturé plein cadre
//! isole la boucle de pixels ; une scène de six cents triangles donne le coût
//! réel, répartition et recopie comprises. La première dit *où* une régression
//! est tombée, la seconde dit *si* elle compte. Une suite de micro-mesures
//! dirait surtout ce que le compilateur a bien voulu garder.
//!
//! **Aucun seuil, et aucune de ces mesures ne peut échouer.** Une durée dépend
//! de la charge de la machine ; un `assert` sur un temps deviendrait rouge
//! parce qu'un antivirus s'est réveillé, et on le relèverait jusqu'à ce qu'il
//! ne mesure plus rien. `make bench` reste donc hors de la liste fixe, et la
//! comparaison se fait à la main, contre les chiffres notés plus bas.
//!
//! **Harnais maison** : le noyau n'a aucune dépendance, `dev-dependencies`
//! comprises, et `#[bench]` n'existe qu'en nightly. Le minimum plutôt que la
//! moyenne — le code est déterministe, donc tout ce qui dépasse le minimum est
//! du bruit de la machine, jamais du travail en plus.
//!
//! # Ce que ces chiffres valent sur un téléphone
//!
//! **Rien directement.** Ce poste est de dernière génération ; un cœur de
//! téléphone actuel est plus lent, et surtout il *ralentit* — la fréquence
//! retombe sous la contrainte thermique au bout de quelques minutes. Le rapport
//! qui décide d'une image régulière est celui du **régime soutenu**, pas celui
//! des pointes, et aucune mesure prise ici ne le donne.
//!
//! Le facteur de travail retenu est **×4**, repris d'un autre projet et
//! **jamais mesuré ici** : il sert à savoir si une régression laisse encore de
//! la marge, pas à prédire une cadence. Écrit avec sa date pour qu'il se
//! rediscute le jour où une mesure sur appareil existera — un facteur transmis
//! sans sa justification finit par passer pour une vérité.
//!
//! Noté le 2026-09-23, avant le premier lot de l'étape 3.
//!
//! # La référence, reprise le 2026-09-23 après les lightmaps
//!
//! ```text
//! plein cadre, tramage       0.69 ms    3.0 ns/pixel
//! plein cadre, bilineaire    1.79 ms    7.8 ns/pixel
//! scene, 600 triangles       7.08 ms   30.7 ns/pixel
//! ```
//!
//! 640×360, tuiles de 64, un seul thread, sur une machine dédiée au repos —
//! charge inférieure à 0,25 sur trente-deux cœurs. **Ces chiffres ne valent que
//! comparés à eux-mêmes**, sur la même machine : une autre les déplacera tous,
//! sans que rien n'ait changé dans le moteur.
//!
//! **Le relevé de charge et l'alternance des versions comparées ne sont pas du
//! zèle.** Un poste de travail ordinaire rend cinq pour cent d'écart d'une
//! exécution à l'autre, ce qui suffit à inventer une régression ou à en cacher
//! une : les chiffres ci-dessus ont été établis en alternant deux tours, et les
//! deux tours concordent à moins d'un pour cent.
//!
//! L'éclairage a coûté au passage, puis rendu : le second jeu de coordonnées a
//! d'abord porté le plein cadre tramé de 3,1 à 3,9 ns/pixel, **y compris sur
//! une scène sans lightmap**. Porter l'ombrage et le filtrage dans des types
//! distincts, au lieu de les examiner à chaque pixel, l'a ramené sous sa valeur
//! d'avant. Le bilinéaire, lui, garde une dizaine de pour cent : le test qu'on
//! y a supprimé pesait peu devant quatre lectures de texel et leur mélange.
//!
//! Le rapport de 2,2 entre les deux filtrages est ce qu'on attend — quatre
//! texels lus au lieu d'un, et leur mélange. La scène chargée est un **pire cas
//! de recouvrement**, six cents quadrilatères empilés dans un champ étroit :
//! elle donne un plafond, pas une prévision de couloir.

use std::hint::black_box;
use std::time::{Duration, Instant};

use screengine::{
    Affine3, BYTES_PER_PIXEL, Color, Config, Context, Filter, Texture, Triangle, Vec3, VertexUv,
};

/// La résolution interne de référence du projet.
const WIDTH: u32 = 640;

/// Sa hauteur.
const HEIGHT: u32 = 360;

/// Le facteur de travail supposé entre ce poste et un cœur de téléphone en
/// régime soutenu. Voir la documentation du module : il n'est pas mesuré.
const FACTEUR_TELEPHONE: u32 = 4;

/// Images mesurées par cas. Assez pour que le minimum se stabilise, assez peu
/// pour que la mesure entière tienne en quelques secondes.
const IMAGES: u32 = 60;

/// Un contexte à la résolution de référence.
fn contexte() -> Context {
    Context::new(Config {
        max_width: WIDTH,
        max_height: HEIGHT,
        width: WIDTH,
        height: HEIGHT,
        tile_size: 64,
        max_triangles: 0,
    })
    .expect("configuration valide")
}

/// Une texture en damier de 256 texels de côté.
///
/// Un damier plutôt qu'un aplat : il oblige la lecture à sauter d'un texel à
/// l'autre, donc à sortir du cache comme le ferait un décor. Un aplat mesurerait
/// surtout la vitesse d'une ligne de cache qui ne change jamais.
fn damier() -> Texture {
    let side = 256usize;
    let mut pixels = vec![0u8; side * side * BYTES_PER_PIXEL];
    for (i, texel) in pixels.chunks_exact_mut(BYTES_PER_PIXEL).enumerate() {
        let (x, y) = (i % side, i / side);
        let v = if ((x / 4) ^ (y / 4)) & 1 == 0 {
            0x30
        } else {
            0xD0
        };
        texel.copy_from_slice(&[v, v, v, 0xFF]);
    }
    Texture::load(side as u32, side as u32, &pixels).expect("texture valide")
}

/// Le plus court temps observé sur `IMAGES` répétitions.
fn mesure(mut image: impl FnMut()) -> Duration {
    // Deux tours à blanc : la première image d'un processus paie le cache
    // d'instructions et les défauts de page du tampon, qui ne sont pas ce
    // qu'on mesure.
    image();
    image();

    let mut court = Duration::MAX;
    for _ in 0..IMAGES {
        let debut = Instant::now();
        image();
        court = court.min(debut.elapsed());
    }
    court
}

/// Refuse de mesurer une image qui ne couvre presque rien.
///
/// Le piège de toute mesure de rendu : une scène mal cadrée, un triangle pris
/// de dos, et on chronomètre un tampon qu'on remplit de noir. Le chiffre est
/// alors excellent et ne veut rien dire. Le seuil est volontairement bas — il
/// ne juge pas le cadrage, il attrape le zéro.
fn exige_couverture(pixels: &[u8], quoi: &str, part: f64) {
    let peints = pixels
        .chunks_exact(BYTES_PER_PIXEL)
        .filter(|p| p[..3] != [0, 0, 0])
        .count();
    let total = (WIDTH * HEIGHT) as f64;
    assert!(
        peints as f64 >= total * part,
        "{quoi} : {peints} pixels peints sur {total}, la mesure ne porte sur rien"
    );
}

/// Écrit une ligne de résultat, avec le coût par pixel et la cadence supposée
/// sur téléphone.
fn ligne(quoi: &str, duree: Duration) {
    let pixels = (WIDTH * HEIGHT) as f64;
    let ns = duree.as_secs_f64() * 1e9;
    let par_image = duree.as_secs_f64() * 1e3;
    let cadence = 1.0 / (duree.as_secs_f64() * f64::from(FACTEUR_TELEPHONE));
    println!(
        "{quoi:<28} {par_image:>7.2} ms   {:>5.1} ns/pixel   ~{cadence:>5.0} i/s à ×{FACTEUR_TELEPHONE}",
        ns / pixels
    );
}

/// Un quadrilatère texturé qui couvre tout l'écran.
///
/// La boucle de pixels seule, ou presque : deux triangles, aucune profondeur à
/// départager, et chaque pixel de l'image échantillonné une fois. C'est ici
/// qu'une lecture de texture ajoutée par l'étape 3 se verra.
fn plein_cadre(context: &mut Context, texture: &std::sync::Arc<Texture>, filter: Filter) {
    // Un plan perpendiculaire au regard, assez large pour déborder de l'écran.
    let coin = |x: f32, z: f32, u: f32, v: f32| VertexUv {
        position: Vec3::new(2.0, x, z),
        u,
        v,
    };
    let vertices = [
        coin(-3.0, 2.0, 0.0, 0.0),
        coin(3.0, 2.0, 256.0, 0.0),
        coin(3.0, -2.0, 256.0, 256.0),
        coin(-3.0, -2.0, 0.0, 256.0),
    ];
    // L'ordre décide de la face : pris dans l'autre sens, le quadrilatère est
    // un dos, le moteur l'élimine, et la mesure porte sur un tampon vide.
    let triangles = [
        Triangle {
            indices: [0, 1, 2],
            color: Color::new(0xFF, 0xFF, 0xFF, 0xFF),
        },
        Triangle {
            indices: [0, 2, 3],
            color: Color::new(0xFF, 0xFF, 0xFF, 0xFF),
        },
    ];

    context.set_filter(filter).expect("hors image");
    context
        .submit_textured(Affine3::IDENTITY, &vertices, &triangles, texture)
        .expect("capacité");
}

/// Une scène chargée : trois cents quadrilatères texturés qui se recouvrent.
///
/// Le coût réel d'une image, répartition par tuile et recopie comprises, avec
/// assez de profondeur pour que le test rejette du travail comme il le ferait
/// dans un couloir.
fn scene_chargee(context: &mut Context, texture: &std::sync::Arc<Texture>) {
    // Un générateur d'une dizaine de lignes, graine fixe : le noyau n'a pas de
    // dépendance, et une scène qui change d'un lancement à l'autre ne se
    // comparerait pas.
    let mut etat = 0x2545_F491_4F6C_DD1Du64;
    let mut suivant = || {
        etat ^= etat << 13;
        etat ^= etat >> 7;
        etat ^= etat << 17;
        (etat >> 33) as u32
    };
    let mut flottant = |min: f32, max: f32| min + (suivant() % 1000) as f32 / 1000.0 * (max - min);

    for _ in 0..300 {
        let (devant, cote, haut) = (
            flottant(2.0, 30.0),
            flottant(-8.0, 8.0),
            flottant(-4.0, 4.0),
        );
        let taille = flottant(0.5, 3.0);
        let coin = |dx: f32, dz: f32, u: f32, v: f32| VertexUv {
            position: Vec3::new(devant, cote + dx, haut + dz),
            u,
            v,
        };
        let vertices = [
            coin(-taille, taille, 0.0, 0.0),
            coin(taille, taille, 256.0, 0.0),
            coin(taille, -taille, 256.0, 256.0),
            coin(-taille, -taille, 0.0, 256.0),
        ];
        let couleur = Color::new(0xC0, 0xC0, 0xC0, 0xFF);
        let triangles = [
            Triangle {
                indices: [0, 1, 2],
                color: couleur,
            },
            Triangle {
                indices: [0, 2, 3],
                color: couleur,
            },
        ];
        context
            .submit_textured(Affine3::IDENTITY, &vertices, &triangles, texture)
            .expect("capacité");
    }
}

fn main() {
    let texture = std::sync::Arc::new(damier());
    let mut pixels = vec![0u8; WIDTH as usize * HEIGHT as usize * BYTES_PER_PIXEL];

    println!(
        "screengine — {WIDTH}x{HEIGHT}, tuiles de 64, minimum sur {IMAGES} images\n\
         le facteur telephone n'est pas mesure : voir la documentation du module\n"
    );

    for (nom, filtre) in [
        ("plein cadre, tramage", Filter::Dither),
        ("plein cadre, bilineaire", Filter::Bilinear),
    ] {
        let mut context = contexte();
        let duree = mesure(|| {
            plein_cadre(&mut context, &texture, filtre);
            context
                .frame_end(black_box(&mut pixels), WIDTH)
                .expect("image rendue");
        });
        // Plein cadre veut dire plein cadre : sous 95 %, le quadrilatère est
        // mal placé et ce n'est plus la boucle de pixels qu'on chronomètre.
        exige_couverture(&pixels, nom, 0.95);
        ligne(nom, duree);
    }

    let mut context = contexte();
    let duree = mesure(|| {
        scene_chargee(&mut context, &texture);
        context
            .frame_end(black_box(&mut pixels), WIDTH)
            .expect("image rendue");
    });
    exige_couverture(&pixels, "scene", 0.5);
    ligne("scene, 600 triangles", duree);
}
