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
//! **Aucune cadence n'est donc imprimée ici.** Une colonne l'a été, sur un
//! facteur de travail de ×4 repris d'un autre projet et jamais mesuré, à
//! rediscuter « le jour où une mesure sur appareil existera ». Ce jour est venu
//! sans rien donner : [`carte`](../carte/index.html) relève 2,0 à 2,2 ms sur un
//! arm64 de 2025, mais sur la même scène **avec trois caisses en plus**, et
//! depuis la démonstration Android, dont la résolution et le nombre de threads
//! ne sont écrits nulle part. Le quotient des deux mélangerait le rapport des
//! machines et le coût des caisses, et aucune des deux parts ne s'en déduit.
//!
//! Le facteur est parti plutôt que d'être redaté : un nombre que personne ne
//! peut valider finit par passer pour une vérité, et celui-ci n'alimentait
//! qu'une colonne d'affichage. Ce qui reste — les millisecondes et le coût par
//! pixel — est ce que ce fichier compare à lui-même.
//!
//! Ce qu'une mesure sur téléphone demanderait, et c'est pourquoi elle n'est pas
//! là : la même scène des deux côtés, à la même résolution et au même nombre de
//! threads.
//!
//! # Ce que les chemins vectoriels rapportent, mesuré le 2026-10-07
//!
//! Sur un poste de travail ordinaire — donc des chiffres qui ne valent que les
//! uns contre les autres, pris dans le même tour :
//!
//! ```text
//! plein cadre, uni — scalaire    0.24 ms
//! plein cadre, uni — SSE2        0.10 ms
//! plein cadre, uni — AVX2        0.09 ms
//! ```
//!
//! **Un facteur 2,4 et 2,7**, et il a fallu une première tentative pour trouver
//! où il était. Celle-ci ne vectorisait que le calcul des profondeurs, puis les
//! écrivait dans un tampon que la boucle relisait pixel par pixel pour tester et
//! écrire : elle rendait **0,34 ms, soit plus lent que le scalaire**, cet
//! aller-retour en mémoire coûtant davantage que l'addition `i64` épargné. Le
//! gain est dans la comparaison et l'écriture masquée de plusieurs pixels à la
//! fois, pas dans l'interpolation qui les précède.
//!
//! **Les trois cas texturés ne bougent pas**, et c'est attendu : aucune variante
//! ne touche encore ce chemin. Ils servent de témoin — un écart y signalerait une
//! mesure qui ne porte pas sur ce qu'elle annonce.
//!
//! **Et la scène chargée ne bouge presque pas non plus**, pour la même raison :
//! elle est texturée de bout en bout, si bien que le chemin uni n'y tient
//! presque aucune place. C'est ce qui dit où va la suite de l'étape — le gain
//! visible sur un décor réel passe par le chemin texturé, où l'échantillonnage
//! domine.
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
    Affine3, BYTES_PER_PIXEL, Color, Config, Context, Filter, SimdPath, Texture, Triangle, Vec3,
    VertexUv,
};

/// La résolution interne de référence du projet.
const WIDTH: u32 = 640;

/// Sa hauteur.
const HEIGHT: u32 = 360;

/// Images mesurées par cas. Assez pour que le minimum se stabilise, assez peu
/// pour que la mesure entière tienne en quelques secondes.
const IMAGES: u32 = 60;

/// Un contexte à la résolution de référence, sur le chemin demandé.
///
/// **Le chemin est un paramètre, et c'est tout l'objet de ces mesures** : une
/// variante vectorielle est juste quand elle rend les bits du scalaire, ce que
/// la conformance exige ; elle n'est **utile** que si elle est plus rapide, ce
/// qu'aucun contrôle ne dit et que seul ce fichier peut établir.
fn contexte(simd: SimdPath) -> Context {
    let mut context = Context::new(Config {
        max_width: WIDTH,
        max_height: HEIGHT,
        width: WIDTH,
        height: HEIGHT,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    context.set_simd(simd).expect("chemin disponible");
    context
}

/// Les chemins que cette machine peut mesurer, le scalaire en tête.
///
/// **Le scalaire d'abord, parce que c'est la référence** : les lignes se lisent
/// les unes sous les autres, et c'est au premier chiffre que les suivants se
/// comparent. Un chemin que la machine ne porte pas ne s'imprime pas du tout,
/// plutôt que d'afficher une durée qui serait celle du scalaire déguisée.
fn chemins() -> Vec<(&'static str, SimdPath)> {
    [
        ("scalaire", SimdPath::Scalar),
        ("SSE2", SimdPath::Sse2),
        ("AVX2", SimdPath::Avx2),
        ("NEON", SimdPath::Neon),
        ("simd128", SimdPath::Simd128),
    ]
    .into_iter()
    .filter(|(_, path)| path.available())
    .collect()
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

/// Écrit une ligne de résultat : le coût par image, puis le coût par pixel.
///
/// Pas de cadence extrapolée — voir la documentation du module, qui dit ce que
/// la colonne supprimée prétendait donner et pourquoi rien ne la remplace.
fn ligne(quoi: &str, duree: Duration) {
    let pixels = (WIDTH * HEIGHT) as f64;
    let ns = duree.as_secs_f64() * 1e9;
    let par_image = duree.as_secs_f64() * 1e3;
    println!(
        "{quoi:<38} {par_image:>7.2} ms   {:>5.1} ns/pixel",
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

/// Le même quadrilatère, **sans texture ni éclairage**.
///
/// **C'est le seul cas qui emprunte le chemin uni**, et son absence a laissé
/// toute une étape sans mesure : les trois cas d'origine sont texturés, si bien
/// qu'une variante écrite pour le chemin uni ne s'y voyait pas du tout. Mesuré
/// le 2026-10-07 — les trois lignes texturées ne bougeaient pas d'un chemin à
/// l'autre, et ce n'était ni un gain nul ni une variante lente : c'était du code
/// qui ne tournait jamais.
///
/// Il est bref et c'est voulu : une couleur constante, une profondeur affine, et
/// rien à échantillonner. Ce qu'il chronomètre est la boucle que `Target::span`
/// propose d'un bloc, et elle seule.
fn plein_cadre_uni(context: &mut Context) {
    let coin = |x: f32, z: f32| Vec3::new(2.0, x, z);
    let vertices = [
        coin(-3.0, 2.0),
        coin(3.0, 2.0),
        coin(3.0, -2.0),
        coin(-3.0, -2.0),
    ];
    let triangles = [
        Triangle {
            indices: [0, 1, 2],
            color: Color::new(0xC0, 0xB0, 0x90, 0xFF),
        },
        Triangle {
            indices: [0, 2, 3],
            color: Color::new(0xC0, 0xB0, 0x90, 0xFF),
        },
    ];

    context
        .submit(Affine3::IDENTITY, &vertices, &triangles)
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

/// Joue les mesures dans l'ordre et écrit leur tableau.
///
/// Le minimum et non la moyenne, pour la raison écrite en tête du module : une
/// durée n'a qu'une borne basse vraie.
fn main() {
    let texture = std::sync::Arc::new(damier());
    let mut pixels = vec![0u8; WIDTH as usize * HEIGHT as usize * BYTES_PER_PIXEL];

    println!(
        "screengine — {WIDTH}x{HEIGHT}, tuiles de 64, minimum sur {IMAGES} images\n\
         le facteur telephone n'est pas mesure : voir la documentation du module\n"
    );

    // **Le chemin uni d'abord**, parce que c'est le seul que les variantes
    // touchent aujourd'hui : les lignes texturées qui suivent servent de témoin
    // — elles ne doivent pas bouger d'un chemin à l'autre, n'empruntant aucun
    // code vectoriel.
    for (chemin, simd) in chemins() {
        let mut context = contexte(simd);
        let duree = mesure(|| {
            plein_cadre_uni(&mut context);
            context
                .frame_end(black_box(&mut pixels), WIDTH)
                .expect("image rendue");
        });
        exige_couverture(&pixels, "plein cadre, uni", 0.95);
        ligne(&format!("plein cadre, uni — {chemin}"), duree);
    }

    for (nom, filtre) in [
        ("plein cadre, tramage", Filter::Dither),
        ("plein cadre, bilineaire", Filter::Bilinear),
    ] {
        for (chemin, simd) in chemins() {
            let mut context = contexte(simd);
            let duree = mesure(|| {
                plein_cadre(&mut context, &texture, filtre);
                context
                    .frame_end(black_box(&mut pixels), WIDTH)
                    .expect("image rendue");
            });
            // Plein cadre veut dire plein cadre : sous 95 %, le quadrilatère est
            // mal placé et ce n'est plus la boucle de pixels qu'on chronomètre.
            exige_couverture(&pixels, nom, 0.95);
            ligne(&format!("{nom} — {chemin}"), duree);
        }
    }

    // **La scène chargée se mesure aussi par chemin**, et c'est elle qui décide :
    // le plein cadre est un quadrilatère unique, quand celle-ci empile six cents
    // triangles avec leur mise en place, leur répartition et leur recouvrement.
    // Un gain qui n'apparaîtrait que sur le premier ne serait pas un gain.
    for (chemin, simd) in chemins() {
        let mut context = contexte(simd);
        let duree = mesure(|| {
            scene_chargee(&mut context, &texture);
            context
                .frame_end(black_box(&mut pixels), WIDTH)
                .expect("image rendue");
        });
        exige_couverture(&pixels, "scene", 0.5);
        ligne(&format!("scene, 600 triangles — {chemin}"), duree);
    }
}
