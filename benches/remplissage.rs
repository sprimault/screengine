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
//! **Les cas texturés ne bougent pas**, et c'est attendu : aucune variante ne
//! touche encore ce chemin. Ils servent de témoin — un écart y signalerait une
//! mesure qui ne porte pas sur ce qu'elle annonce.
//!
//! **Et la scène chargée ne bouge presque pas non plus**, pour la même raison :
//! elle est texturée de bout en bout, si bien que le chemin uni n'y tient
//! presque aucune place. C'est ce qui dit où va la suite de l'étape — le gain
//! visible sur un décor réel passe par le chemin texturé, où l'échantillonnage
//! domine.
//!
//! # Où part le temps de l'échantillonnage — mesuré le 2026-10-07
//!
//! ```text
//! plein cadre, uni            0.23 ms    ce qu'un chemin vectoriel touche
//! plein cadre, tramage        0.94 ms    la référence
//! texture, densite nulle      0.94 ms    un seul texel, cache parfait
//! texture, cote de 4          0.99 ms    pile de mipmaps en L1
//! texture, densite 16x        0.98 ms    plusieurs texels par pixel
//! ```
//!
//! **L'accès mémoire à la texture ne coûte rien de mesurable**, et c'est contraire
//! à ce qu'on attend d'un rendu logiciel. Que la texture pèse trois cents
//! kilo-octets ou tienne en L1, que chaque pixel relise le même texel ou saute de
//! plusieurs, les quatre cas rendent la même durée à cinq pour cent près — l'écart
//! d'une exécution à l'autre sur ce poste. Deux tours concordants.
//!
//! **Les sept dixièmes de milliseconde qui séparent le cas uni du cas tramé
//! partent donc en calcul par pixel** : interpolation perspective des
//! coordonnées, adressage, choix de niveau, combinaison avec l'éclairage. C'est
//! une information actionnable, parce que du calcul entier se vectorise là où des
//! accès dispersés ne se vectorisent pas.
//!
//! **Cette soustraction mélangeait deux boucles, et la section suivante la
//! refait autrement.** Le cas uni passe par le bloc que `Target::span` propose,
//! le cas tramé par des segments de seize pixels avec leur division de
//! perspective : leur écart porte donc le changement de boucle en plus de
//! l'échantillonnage. La conclusion tient — c'est bien l'échantillonnage qu'il
//! faut viser —, sa quantité était sous-estimée d'un tiers.
//!
//! **Ce que cela ne change pas** : le gain d'un chemin texturé vectorisé sur le
//! seul test de profondeur et l'écriture reste borné par le cas uni, soit
//! 0,15 ms — un pour cent et demi sur la scène chargée. La décomposition ci-dessus
//! n'éclaire pas ce lot-là, elle en désigne un autre : vectoriser
//! l'échantillonnage lui-même.
//!
//! # Ce que coûte le chemin dominant — mesuré le 2026-10-08
//!
//! Trois tours concordants, le scalaire seul, sur un poste de travail
//! ordinaire :
//!
//! ```text
//! plein cadre, uni                 0.23 ms    1.0 ns/px   le bloc de Target::span
//! segments, rampe seule            0.75 ms    3.3         la boucle a segments, aucune image lue
//! plein cadre, tramage             0.94 ms    4.1         un echantillonnage trame
//! segments, texture et rampe       1.71 ms    7.4         les deux
//! segments, texture et lightmap    3.34 ms   14.5         ce qu'une surface de carte emprunte
//! ```
//!
//! **La boucle à segments nue ne coûte presque rien, et c'est ce que les trois
//! premières lignes établissent sans qu'un quatrième cas soit nécessaire.** En
//! notant `S` la boucle, `T` l'échantillonnage et `R` la rampe, les mesures
//! donnent `S + T`, `S + R` et `S + T + R` : leur combinaison rend `S ≈ −0,02`,
//! soit le bruit de la mesure. Le coût est donc presque entièrement dans les
//! attributs, division de perspective, test et écriture compris.
//!
//! **Le chemin qu'un décor emprunte vraiment coûte trois fois et demi le
//! quadrilatère tramé**, et il n'était mesuré nulle part : une surface de carte
//! porte sa texture **et** l'atlas de sa cellule. Les 2,4 ms que la lightmap
//! ajoute sont cohérentes avec le coût du bilinéaire mesuré à part — une lightmap
//! se lit toujours ainsi, quel que soit le filtrage —, et elles en font le
//! premier poste du remplissage, devant l'échantillonnage de la texture.
//!
//! **Ce que cela désigne n'est donc pas ce que la section précédente annonçait.**
//! Ce qui domine le chemin dominant est la lecture bilinéaire de la lightmap et
//! la combinaison qui suit, pas l'adressage tramé de la texture. Les deux sont du
//! calcul entier par pixel, donc vectorisables ; c'est la seconde qu'il faut
//! prendre d'abord.
//!
//! **La lightmap de ce cas est étirée une fois sur toute l'étendue** quand la
//! texture se répète, ce qui est le régime d'une carte. Une lightmap plus dense
//! ne changerait pas la nature du coût, les accès mémoire ne pesant rien :
//! c'est établi par les trois lignes de densité de la section précédente.
//!
//! # Ce que le chemin texturé vectorisé rapporte — mesuré le 2026-10-08
//!
//! Deux tours concordants, SSE2 contre le scalaire dans le même tour :
//!
//! ```text
//! plein cadre, tramage         0.87 -> 0.58 ms    -31 %
//! salles traverse (carte.rs)   1.06 -> 0.80 ms    -23 %
//! scene, 600 triangles         8.76 -> 8.59 ms     -2 %
//! segments, texture et rampe   1.66 -> 1.70 ms   inchange, temoin
//! ```
//!
//! **Le chiffre qui décide est celui du décor**, pas celui du plein cadre : un
//! quart du temps de rendu d'une carte traversée. Il est dans
//! [`carte`](../carte/index.html), qui mesure désormais par chemin — le seul
//! endroit du dépôt qui dise ce qu'une variante rend sur une vraie carte.
//!
//! **La scène chargée ne gagne presque rien, et c'est explicable** : ses six
//! cents quadrilatères sont petits et vus de près, si bien que la plupart de ses
//! segments font moins de quatre pixels et que la variante n'a rien à prendre.
//! Un décor réel a de grandes surfaces vues de biais, d'où l'écart entre les
//! deux lignes.
//!
//! **Le cas éclairé est le témoin, et il ne doit pas bouger** : la variante ne
//! couvre que le chemin texturé sans éclairage. Un écart là signalerait qu'elle
//! s'applique où elle ne devrait pas.
//!
//! # Ce que la lightmap vectorisée rapporte — mesuré le 2026-10-08
//!
//! Deux tours concordants, SSE2 contre le scalaire dans le même tour. Les
//! chiffres du scalaire ont monté depuis la section précédente, la machine étant
//! plus chargée : **ce sont les rapports qui se comparent, jamais les valeurs
//! d'un tour à l'autre.**
//!
//! ```text
//! segments, texture et lightmap   3.57 -> 1.36 ms    -62 %   le cas dominant
//! plein cadre, tramage            0.98 -> 0.58 ms    -41 %
//! salles traverse (carte.rs)      1.24 -> 0.83 ms    -33 %
//! scene, 600 triangles           10.95 -> 8.92 ms    -18 %
//! plein cadre, bilineaire         2.14 -> 2.14 ms   inchange, temoin
//! ```
//!
//! **Le facteur deux et demi du cas dominant vient du bilinéaire**, pas de
//! l'adressage : une lightmap coûte trois mélanges et quatre lectures par pixel,
//! et les trois mélanges sont du calcul entier pur — exactement ce qui se
//! vectorise, là où les lectures ne se vectorisent pas. C'est ce que la
//! décomposition annonçait, et c'est la première fois qu'une mesure la confirme
//! dans ce sens.
//!
//! **Le bilinéaire de texture est le témoin et il ne bouge pas** : seule la
//! lightmap est lue ainsi par la variante, une texture bilinéaire restant sur le
//! chemin scalaire.
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
    Affine3, BYTES_PER_PIXEL, Color, Config, Context, Filter, Light, SimdPath, Texture, Triangle,
    Vec3, VertexUv, VertexUv2,
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

/// Une texture en damier, de `side` texels de côté.
///
/// Un damier plutôt qu'un aplat : il oblige la lecture à sauter d'un texel à
/// l'autre, donc à sortir du cache comme le ferait un décor. Un aplat mesurerait
/// surtout la vitesse d'une ligne de cache qui ne change jamais.
///
/// **Le côté est un paramètre depuis qu'on décompose le coût du texturé** : à
/// 256, la pile de mipmaps pèse plus de trois cents kilo-octets et ne tient pas
/// en L1 ; à 4, elle y tient entièrement. Deux mesures qui ne diffèrent que par
/// là disent ce que l'échantillonnage paie en accès mémoire.
fn damier(side: usize) -> Texture {
    let mut pixels = vec![0u8; side * side * BYTES_PER_PIXEL];
    for (i, texel) in pixels.chunks_exact_mut(BYTES_PER_PIXEL).enumerate() {
        let (x, y) = (i % side, i / side);
        // La case fait quatre texels sur une grande texture, un seul sur une
        // petite : à 4 de côté, des cases de quatre rendraient un aplat, et la
        // comparaison porterait sur deux motifs au lieu de deux tailles.
        let case = if side >= 16 { 4 } else { 1 };
        let v = if ((x / case) ^ (y / case)) & 1 == 0 {
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
fn plein_cadre(
    context: &mut Context,
    texture: &std::sync::Arc<Texture>,
    filter: Filter,
    uv_max: f32,
) {
    // Un plan perpendiculaire au regard, assez large pour déborder de l'écran.
    let coin = |x: f32, z: f32, u: f32, v: f32| VertexUv {
        position: Vec3::new(2.0, x, z),
        u,
        v,
    };
    let vertices = [
        coin(-3.0, 2.0, 0.0, 0.0),
        coin(3.0, 2.0, uv_max, 0.0),
        coin(3.0, -2.0, uv_max, uv_max),
        coin(-3.0, -2.0, 0.0, uv_max),
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

/// Le même quadrilatère, texturé **et** couvert d'une lightmap.
///
/// **C'est le chemin dominant d'un décor réel**, et aucune autre ligne ne le
/// mesure : une surface de carte porte sa texture et son atlas de cellule, donc
/// deux jeux de coordonnées, deux marches, et deux lectures par pixel — dont
/// celle de la lightmap, toujours bilinéaire quel que soit le filtrage.
///
/// La lightmap est volontairement **étirée une seule fois** sur toute l'étendue,
/// là où la texture se répète : c'est ce que fait une carte, et c'est ce qui
/// donne aux deux marches des pentes d'ordres différents.
fn plein_cadre_lightmap(
    context: &mut Context,
    texture: &std::sync::Arc<Texture>,
    lightmap: &std::sync::Arc<Texture>,
) {
    let coin = |x: f32, z: f32, u: f32, v: f32, u2: f32, v2: f32| VertexUv2 {
        position: Vec3::new(2.0, x, z),
        u,
        v,
        u2,
        v2,
        normal: Vec3::ZERO,
    };
    let vertices = [
        coin(-3.0, 2.0, 0.0, 0.0, 0.0, 0.0),
        coin(3.0, 2.0, 256.0, 0.0, 16.0, 0.0),
        coin(3.0, -2.0, 256.0, 256.0, 16.0, 16.0),
        coin(-3.0, -2.0, 0.0, 256.0, 0.0, 16.0),
    ];
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

    context.set_filter(Filter::Dither).expect("hors image");
    context
        .submit_lit(
            Affine3::IDENTITY,
            &vertices,
            &triangles,
            Some(texture),
            lightmap,
        )
        .expect("capacité");
}

/// Pose une lumière unique, dont seule la **portée** nous intéresse.
///
/// **Ce qu'elle sert à mesurer n'est pas l'éclairage mais un chemin.** Dès
/// qu'une lumière est réglée, tout triangle devient éclairé, donc passe par les
/// segments de perspective au lieu du chemin uni que `Target::span` propose d'un
/// bloc. C'est le seul moyen d'obtenir la boucle à segments **sans qu'elle lise
/// une image** : la lightmap y est absente, et un lot uni n'a pas de texel.
///
/// Sans cela, la part de l'échantillonnage ne se déduit pas. L'écart entre le
/// cas uni et le cas tramé mélange deux choses — la lecture du texel, et le
/// passage d'une boucle d'un bloc à une boucle par segments avec sa division de
/// perspective. Un chemin vectoriel ne reprendrait que la première.
///
/// La portée décide de ce que le remplissage compile : au-delà de la distance
/// des coins, les trois canaux de la rampe s'ajoutent à chaque pixel ; en deçà,
/// aucun sommet n'est atteint, les plans restent nuls et le remplissage le sait.
fn lumiere(context: &mut Context, portee: f32) {
    context
        .set_lights(&[Light {
            // Au centre du quadrilatère, dont les coins sont à 3,6 unités.
            position: Vec3::new(2.0, 0.0, 0.0),
            radius: portee,
            color: Color::new(0xFF, 0xE0, 0xC0, 0xFF),
        }])
        .expect("hors image");
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
    let texture = std::sync::Arc::new(damier(256));
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
                plein_cadre(&mut context, &texture, filtre, 256.0);
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

    // **Où part le temps dans l'échantillonnage**, qui est les trois quarts du
    // chemin texturé. Ces trois lignes disent ce que l'accès mémoire y pèse ;
    // ce que pèse la boucle elle-même se lit plus bas, le cas uni empruntant un
    // autre chemin que celui-ci.
    //
    // **Ce ne sont pas des micro-mesures**, que ce module proscrit : c'est le même
    // quadrilatère plein cadre et la même boucle de pixels, du premier sommet à la
    // recopie de tuile. Seules deux données de scène changent — la densité des
    // coordonnées et la taille de la texture —, donc le compilateur ne peut rien
    // élider, et chaque ligne reste une image entière.
    //
    // - **densité nulle** : les quatre sommets portent la même coordonnée, donc un
    //   seul texel sert à toute l'image. L'adressage, le choix de niveau et la
    //   combinaison ont lieu pour chaque pixel ; seul l'accès mémoire devient
    //   gratuit, la ligne de cache ne changeant jamais.
    // - **texture de 4** : la densité du cas de référence sur une pile de mipmaps
    //   de quelques centaines d'octets, qui tient en L1 entière. L'adressage saute
    //   d'un texel à l'autre comme dans le cas de référence, mais sans jamais
    //   sortir du cache.
    // - **densité 16 fois** : des sauts de plusieurs texels par pixel. Le mipmap
    //   doit choisir un niveau plus petit, donc ce cas dit aussi si ce choix
    //   protège le cache comme il le promet.
    let minuscule = std::sync::Arc::new(damier(4));
    for (nom, source, uv_max) in [
        ("texture, densite nulle", &texture, 0.0),
        ("texture, cote de 4", &minuscule, 256.0),
        ("texture, densite 16x", &texture, 4096.0),
    ] {
        for (chemin, simd) in chemins() {
            let mut context = contexte(simd);
            let duree = mesure(|| {
                plein_cadre(&mut context, source, Filter::Dither, uv_max);
                context
                    .frame_end(black_box(&mut pixels), WIDTH)
                    .expect("image rendue");
            });
            exige_couverture(&pixels, nom, 0.95);
            ligne(&format!("{nom} — {chemin}"), duree);
        }
    }

    // **Ce que la boucle à segments coûte avant d'échantillonner quoi que ce
    // soit**, et c'est la part que la décomposition précédente ne sépare pas.
    //
    // Les lignes ci-dessus comparent le cas uni au cas tramé, et leur écart
    // mélange deux choses : la lecture du texel, et le changement de boucle — le
    // cas uni passe par le bloc que `Target::span` propose, le cas tramé par des
    // segments de seize pixels avec leur division de perspective. Un chemin
    // vectoriel ne reprendrait que la première, donc l'écart surestime ce qu'il
    // peut rendre.
    //
    // La ligne qui suit emprunte **la boucle à segments sans lire aucune
    // image** : la lumière ouvre ce chemin, la lightmap est absente, et le lot
    // est uni. Sa distance au cas texturé est la part vraiment en jeu.
    //
    // **Une lumière hors de portée aurait donné un cas de plus** — la boucle à
    // segments sans même la rampe — et il n'y est pas : une surface que plus
    // aucune lumière n'atteint s'éteint, par contrat, donc l'image est noire. Le
    // contrôle de couverture la refuse, et il a raison de ne pas savoir
    // distinguer ce noir-là d'une scène mal cadrée.
    for (chemin, simd) in chemins() {
        let mut context = contexte(simd);
        let duree = mesure(|| {
            lumiere(&mut context, 8.0);
            plein_cadre_uni(&mut context);
            context
                .frame_end(black_box(&mut pixels), WIDTH)
                .expect("image rendue");
        });
        exige_couverture(&pixels, "segments, rampe seule", 0.95);
        ligne(&format!("segments, rampe seule — {chemin}"), duree);
    }

    // Et le même chemin **avec** la texture, pour que la soustraction porte sur
    // deux cas qui ne diffèrent que par l'échantillonnage : mêmes segments, même
    // rampe, même division de perspective.
    for (chemin, simd) in chemins() {
        let mut context = contexte(simd);
        let duree = mesure(|| {
            lumiere(&mut context, 8.0);
            plein_cadre(&mut context, &texture, Filter::Dither, 256.0);
            context
                .frame_end(black_box(&mut pixels), WIDTH)
                .expect("image rendue");
        });
        exige_couverture(&pixels, "segments, texture et rampe", 0.95);
        ligne(&format!("segments, texture et rampe — {chemin}"), duree);
    }

    // **Et le chemin dominant d'un décor réel**, qui n'était mesuré nulle part :
    // une surface de carte porte sa texture et l'atlas de sa cellule. C'est lui
    // qu'un chemin vectoriel doit viser, et pas le quadrilatère nu des lignes du
    // haut — une lightmap se lit toujours en bilinéaire, donc quatre texels et
    // leur mélange, là où la rampe ci-dessus n'évalue que trois plans.
    let atlas = std::sync::Arc::new(damier(16));
    for (chemin, simd) in chemins() {
        let mut context = contexte(simd);
        let duree = mesure(|| {
            plein_cadre_lightmap(&mut context, &texture, &atlas);
            context
                .frame_end(black_box(&mut pixels), WIDTH)
                .expect("image rendue");
        });
        exige_couverture(&pixels, "segments, texture et lightmap", 0.95);
        ligne(&format!("segments, texture et lightmap — {chemin}"), duree);
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
