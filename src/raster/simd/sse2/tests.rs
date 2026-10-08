// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la variante SSE2 doit au rasteriseur scalaire.
//!
//! **L'égalité au bit près sur les deux tampons**, et rien d'autre : une
//! divergence est une variante fausse, jamais une différence acceptable. Ces cas
//! comparent donc la variante à l'expression du scalaire plutôt qu'à des valeurs
//! écrites à la main, qui ne diraient que ce qu'on y aurait mis.

use alloc::vec;
use alloc::vec::Vec;

#[cfg(target_arch = "x86")]
use core::arch::x86::{__m128i, _mm_loadu_si128, _mm_storeu_si128};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{__m128i, _mm_loadu_si128, _mm_storeu_si128};

use super::{fill_flat_row, fill_sampled_row, mix_vec, modulate_vec};
use crate::light::{MAX_OVERBRIGHT, modulate};
use crate::math::fixed::UV_BITS;
use crate::raster::plane::GRADIENT_BITS;
use crate::raster::simd::{FlatRow, SampledRow};
use crate::raster::triangle::dither_offsets;
use crate::texture::mix;

/// Ce que le chemin scalaire écrit, repris ici mot pour mot.
///
/// **Recopié plutôt que partagé, et c'est l'exception qui confirme la règle** :
/// la duplication entre le scalaire et ses variantes est voulue sur ce projet,
/// parce que le premier est la référence contre laquelle les secondes se
/// valident. Les fondre supprimerait la référence, et ce test ne comparerait
/// plus qu'une expression à elle-même.
fn scalar(color: &mut [u32], depth: &mut [u32], start: i64, step: i64, fill: u32) {
    let mut z = start;
    for i in 0..depth.len() {
        let value = (z >> GRADIENT_BITS) as u32;
        if value > depth[i] {
            depth[i] = value;
            color[i] = fill;
        }
        z = z.wrapping_add(step);
    }
}

/// Un générateur à graine fixe, écrit ici comme le noyau l'exige.
///
/// Pas de bibliothèque de tests par propriétés : le noyau n'admet aucune
/// dépendance, et un échec qui ne se rejoue pas n'a pas été trouvé.
struct Seed(u64);

impl Seed {
    /// Le prochain mot, par un xorshift dont la graine est écrite dans le test.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Un tampon de profondeurs déjà peuplé, pour que le test ait à trancher.
///
/// **Un tampon vide ne prouverait rien** : tout pixel y passerait, et l'écriture
/// masquée serait un masque plein. Ce qui doit être vérifié est précisément le
/// mélange — des pixels qui passent, d'autres non, dans le même registre.
fn peuple(seed: &mut Seed, count: usize) -> (Vec<u32>, Vec<u32>) {
    let color = (0..count).map(|_| seed.next() as u32).collect();
    let depth = (0..count).map(|_| seed.next() as u32).collect();
    (color, depth)
}

/// Les deux chemins écrivent les mêmes bits, sur les quatre restes possibles.
///
/// **Les quatre parités comptent**, et c'est le défaut que la forme vectorielle
/// invite à écrire : la boucle avance de quatre, donc une ligne dont la longueur
/// n'est pas un multiple de quatre laisse un à trois pixels que seule la queue
/// traite. Une variante qui l'oublierait rendrait juste sur un quart des lignes
/// d'un triangle.
#[test]
fn les_deux_chemins_ecrivent_les_memes_bits() {
    let mut seed = Seed(0x5eed_1234_abcd_ef01);
    for count in 0..34usize {
        for _ in 0..8 {
            // Des profondeurs quelconques, pas des valeurs rondes : une pente
            // nulle ou une puissance de deux rendraient le décalage exact des
            // deux côtés sans rien éprouver de l'accumulation.
            let start = seed.next() as i64;
            let step = (seed.next() as i64) >> 20;
            let fill = seed.next() as u32;
            let (mut color, mut depth) = peuple(&mut seed, count);
            let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());

            fill_flat_row(FlatRow {
                color: &mut color,
                depth: &mut depth,
                start,
                step,
                fill,
            });
            scalar(&mut attendu_color, &mut attendu_depth, start, step, fill);

            assert_eq!(depth, attendu_depth, "profondeurs, {count} pixels");
            assert_eq!(color, attendu_color, "couleurs, {count} pixels");
        }
    }
}

/// Le test de profondeur est **non signé**, et c'est le piège de SSE2.
///
/// `_mm_cmpgt_epi32` compare en signé : sans le biais appliqué aux deux côtés,
/// toute profondeur dont le bit de poids fort est posé — c'est-à-dire tout ce
/// qui est proche de la caméra — se comparerait à l'envers. Ce cas place
/// exprès des valeurs des deux côtés de `0x8000_0000`.
#[test]
fn la_comparaison_reste_non_signee() {
    let bornes = [0u32, 1, 0x7FFF_FFFF, 0x8000_0000, 0x8000_0001, u32::MAX];
    for &ancien in &bornes {
        for &neuf in &bornes {
            let mut color = vec![0u32; 4];
            let mut depth = vec![ancien; 4];
            let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
            // Pente nulle : les quatre pixels portent la même profondeur, et
            // c'est la comparaison seule qu'on éprouve.
            let start = i64::from(neuf) << GRADIENT_BITS;

            fill_flat_row(FlatRow {
                color: &mut color,
                depth: &mut depth,
                start,
                step: 0,
                fill: 0xAABB_CCDD,
            });
            scalar(
                &mut attendu_color,
                &mut attendu_depth,
                start,
                0,
                0xAABB_CCDD,
            );

            assert_eq!(depth, attendu_depth, "ancien={ancien:#x} neuf={neuf:#x}");
            assert_eq!(color, attendu_color, "ancien={ancien:#x} neuf={neuf:#x}");
        }
    }
}

/// Une ligne vide ne lit ni n'écrit rien.
///
/// Elle arrive : le remplissage borne le span par la fenêtre, et une ligne du
/// triangle peut tomber entièrement hors de la tuile courante.
#[test]
fn une_ligne_vide_ne_touche_rien() {
    let mut color: [u32; 0] = [];
    let mut depth: [u32; 0] = [];
    fill_flat_row(FlatRow {
        color: &mut color,
        depth: &mut depth,
        start: 1 << 40,
        step: -7,
        fill: 0,
    });
}

/// Une profondeur **vraiment négative** descend comme le scalaire.
///
/// Écrit à part parce que le décalage est **logique** ici et arithmétique dans
/// la référence : les deux rendent les mêmes trente-deux bits bas, qui ne
/// dépendent que des bits 12 à 43, mais c'est une coïncidence qu'il faut
/// vérifier plutôt que supposer. **La première version de ce cas ne la
/// vérifiait pas** : partie de `i64::MAX / 2` et descendant de dix-sept fois
/// 2³⁰, elle laissait le bit de signe libre de bout en bout, et les deux
/// décalages y étaient identiques par construction.
#[test]
fn une_pente_negative_traverse_zero() {
    let mut seed = Seed(0x1357_9bdf_0246_8ace);
    let (mut color, mut depth) = peuple(&mut seed, 17);
    let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
    // Huit pixels au-dessus de zéro, neuf au-dessous : la bascule tombe au
    // milieu d'un registre, pas sur sa frontière.
    let (start, step) = (8 * (1 << 38), -(1 << 38));

    fill_flat_row(FlatRow {
        color: &mut color,
        depth: &mut depth,
        start,
        step,
        fill: 0x1234_5678,
    });
    scalar(
        &mut attendu_color,
        &mut attendu_depth,
        start,
        step,
        0x1234_5678,
    );

    assert_eq!(depth, attendu_depth);
    assert_eq!(color, attendu_color);
}

/// Quatre mots dans un registre, et le retour.
///
/// Écrits ici plutôt qu'exposés par le module : ce sont des commodités de test,
/// et le code livré n'a pas à porter une conversion dont il ne se sert pas.
fn charge(v: [u32; 4]) -> __m128i {
    // SAFETY: seize octets lus dans un tableau local de quatre `u32`, sans
    // exigence d'alignement.
    unsafe { _mm_loadu_si128(v.as_ptr().cast::<__m128i>()) }
}

/// L'inverse de [`charge`].
fn extrait(r: __m128i) -> [u32; 4] {
    let mut v = [0u32; 4];
    // SAFETY: même garantie, en écriture.
    unsafe { _mm_storeu_si128(v.as_mut_ptr().cast::<__m128i>(), r) };
    v
}

/// Le mélange vectoriel rend les bits de `texture::mix`, poids par poids.
///
/// **La brique se valide avant la boucle qui l'emploie**, et séparément : une
/// divergence d'un bit sur un canal se verrait ici sur le canal fautif, là où
/// elle n'apparaîtrait dans une image que comme une empreinte différente, sans
/// dire où.
///
/// Les bornes du poids sont jouées explicitement : à zéro le premier texel sort
/// intact, à deux cent cinquante-cinq c'est presque le second — et c'est là que
/// l'arrondi `+ 128` décide.
#[test]
fn le_melange_rend_les_bits_du_scalaire() {
    let mut seed = Seed(0x2468_ace0_1357_9bdf);
    let mut cas: Vec<([u32; 4], [u32; 4], [u32; 4])> = vec![
        ([0; 4], [0xFFFF_FFFF; 4], [0, 1, 128, 255]),
        ([0xFFFF_FFFF; 4], [0; 4], [0, 1, 128, 255]),
        ([0x8040_2010; 4], [0x1020_4080; 4], [0, 85, 170, 255]),
    ];
    for _ in 0..400 {
        let mut quatre = || {
            [
                seed.next() as u32,
                seed.next() as u32,
                seed.next() as u32,
                seed.next() as u32,
            ]
        };
        let (a, b) = (quatre(), quatre());
        let t = quatre().map(|w| w & 0xFF);
        cas.push((a, b, t));
    }

    for (a, b, t) in cas {
        let obtenu = extrait(mix_vec(charge(a), charge(b), charge(t)));
        let attendu = [
            mix(a[0], b[0], t[0]),
            mix(a[1], b[1], t[1]),
            mix(a[2], b[2], t[2]),
            mix(a[3], b[3], t[3]),
        ];
        assert_eq!(obtenu, attendu, "a={a:08x?} b={b:08x?} t={t:?}");
    }
}

/// La combinaison vectorielle rend les bits de `light::modulate`.
///
/// Les trois sur-éclairements sont joués, et l'alpha est tiré au hasard dans les
/// texels : c'est le seul canal que la combinaison ne touche pas, et une voie
/// haute mal masquée le ferait sortir modulé — ce qui, dans une image, donnerait
/// un tampon dont l'alpha n'est plus 255 sans que rien d'autre ne change.
#[test]
fn la_combinaison_rend_les_bits_du_scalaire() {
    let mut seed = Seed(0x1111_2222_3333_4444);
    for overbright in 0..=MAX_OVERBRIGHT {
        let shift = charge([8 - overbright, 0, 0, 0]);
        let mut cas: Vec<([u32; 4], [u32; 4])> = vec![
            ([0xFFFF_FFFF; 4], [0xFFFF_FFFF; 4]),
            ([0xFFFF_FFFF; 4], [0; 4]),
            ([0; 4], [0xFFFF_FFFF; 4]),
            ([0x7F80_8182; 4], [0x00FF_7F01; 4]),
        ];
        for _ in 0..300 {
            let mut quatre = || {
                [
                    seed.next() as u32,
                    seed.next() as u32,
                    seed.next() as u32,
                    seed.next() as u32,
                ]
            };
            cas.push((quatre(), quatre()));
        }

        for (texel, light) in cas {
            let obtenu = extrait(modulate_vec(charge(texel), charge(light), shift));
            let attendu = [
                modulate(texel[0], light[0], overbright),
                modulate(texel[1], light[1], overbright),
                modulate(texel[2], light[2], overbright),
                modulate(texel[3], light[3], overbright),
            ];
            assert_eq!(
                obtenu, attendu,
                "overbright={overbright} texel={texel:08x?} light={light:08x?}"
            );
        }
    }
}

/// L'échantillonnage du chemin scalaire, repris ici mot pour mot.
///
/// Même exception que [`scalar`], et pour la même raison : c'est le scalaire qui
/// fait référence, et le partager supprimerait la référence. Elle réunit ce que
/// trois endroits font là-bas — le décalage de niveau et le tramage de
/// `Crawl::sample`, le repli et l'adressage de `Texture::texel`, le test strict
/// et l'écriture de `Walk::run`.
#[allow(clippy::too_many_arguments)]
fn scalar_sampled(
    color: &mut [u32],
    depth: &mut [u32],
    start: i64,
    step: i64,
    uv: [i64; 2],
    uv_step: [i64; 2],
    shift: u32,
    texels: &[u32],
    size: (u32, u32),
    x0: i32,
    y: i32,
) {
    let (width, height) = size;
    let mut z = start;
    let mut uv = uv;
    for i in 0..depth.len() {
        let value = (z >> GRADIENT_BITS) as u32;
        let offsets = dither_offsets(x0 + i as i32, y);
        // Le tramage s'ajoute **entre** les deux décalages, et la conversion en
        // non signé précède le masque : un `u32` négatif se replie, il ne sature
        // pas.
        let coord = |c: i64, d: i32| (((c >> shift) + i64::from(d)) >> UV_BITS) as i32;
        let tx = (coord(uv[0], offsets[0]) as u32) & (width - 1);
        let ty = (coord(uv[1], offsets[1]) as u32) & (height - 1);
        let texel = texels[(ty * width + tx) as usize];
        if value > depth[i] {
            depth[i] = value;
            color[i] = texel;
        }
        z = z.wrapping_add(step);
        uv[0] = uv[0].wrapping_add(uv_step[0]);
        uv[1] = uv[1].wrapping_add(uv_step[1]);
    }
}

/// Une texture dont chaque texel porte son propre indice.
///
/// **Un damier ne vaudrait rien ici** : deux texels voisins y sont identiques,
/// donc une erreur d'adressage d'un texel passerait inaperçue. Avec l'indice
/// pour valeur, toute erreur de repli, de niveau ou de tramage change la
/// couleur écrite.
fn marquee(width: u32, height: u32) -> Vec<u32> {
    (0..width * height).map(|i| i | 0xFF00_0000).collect()
}

/// Les deux chemins échantillonnent les mêmes bits, sur les quatre restes.
///
/// Les coordonnées sont tirées **des deux côtés de zéro**, et les niveaux vont
/// de zéro à quarante : ce cas garde le décalage, le tramage, le repli,
/// l'adressage et le test de profondeur.
///
/// **Ce qu'il ne garde pas, et c'est vérifié plutôt que supposé** :
/// l'émulation du décalage arithmétique de `shift_right_arithmetic`. Retirée, ce
/// cas reste vert — les coordonnées tenant dans un `i32`, les bits où les deux
/// décalages diffèrent sont ceux que le repli jette. Sa documentation dit
/// pourquoi elle reste écrite malgré cela ; prétendre ici qu'un cas la protège
/// serait faux.
#[test]
fn les_deux_chemins_echantillonnent_les_memes_bits() {
    let mut seed = Seed(0xfeed_4321_0fed_cba9);
    let texels = marquee(64, 32);
    for count in 0..34usize {
        for shift in [0u32, 1, 5, 40] {
            let start = (seed.next() >> 1) as i64;
            let step = (seed.next() as i64) >> 20;
            // Des coordonnées qui traversent zéro et dépassent la texture, pour
            // que le repli et le signe travaillent tous les deux.
            let uv = [(seed.next() as i64) >> 28, (seed.next() as i64) >> 28];
            let uv_step = [(seed.next() as i64) >> 44, (seed.next() as i64) >> 44];
            let (mut color, mut depth) = peuple(&mut seed, count);
            let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
            let (x0, y) = (seed.next() as i32 & 0x3FF, seed.next() as i32 & 0x3FF);

            let traites = fill_sampled_row::<false>(SampledRow {
                lit: false,
                lightmap: crate::raster::simd::LightmapRow::NONE,
                color: &mut color,
                depth: &mut depth,
                start,
                step,
                uv,
                uv_step,
                shift,
                texels: &texels,
                size: (64, 32),
                dither: [
                    [
                        dither_offsets(x0, y)[0],
                        dither_offsets(x0 + 1, y)[0],
                        dither_offsets(x0 + 2, y)[0],
                        dither_offsets(x0 + 3, y)[0],
                    ],
                    [
                        dither_offsets(x0, y)[1],
                        dither_offsets(x0 + 1, y)[1],
                        dither_offsets(x0 + 2, y)[1],
                        dither_offsets(x0 + 3, y)[1],
                    ],
                ],
                x0,
                y,
            });
            assert_eq!(traites, count & !3, "pixels traités, {count} pixels");

            // La référence ne joue que ce que la variante a traité : le reste du
            // segment appartient à l'appelant, qui le finit par le vrai chemin
            // scalaire.
            scalar_sampled(
                &mut attendu_color[..traites],
                &mut attendu_depth[..traites],
                start,
                step,
                uv,
                uv_step,
                shift,
                &texels,
                (64, 32),
                x0,
                y,
            );

            assert_eq!(depth, attendu_depth, "profondeurs, {count} pixels");
            assert_eq!(color, attendu_color, "couleurs, {count} pixels");
        }
    }
}

/// Les deux chemins éclairent les mêmes bits.
///
/// **La référence appelle `Texture::bilinear` et `light::modulate` plutôt que de
/// les recopier**, à l'inverse de [`scalar_sampled`] : ce qu'il faut localiser
/// ici n'est pas une erreur d'arithmétique — [`mix_vec`] et [`modulate_vec`] se
/// valident déjà contre leurs scalaires, séparément — mais une erreur de
/// **chaînage** : coordonnée, niveau, poids, adressage.
#[test]
fn les_deux_chemins_eclairent_les_memes_bits() {
    let mut seed = Seed(0x0f1e_2d3c_4b5a_6978);
    let texels = marquee(64, 32);
    let atlas_pixels: Vec<u8> = (0..16 * 16 * 4).map(|i| (i * 7) as u8).collect();
    let atlas = crate::texture::Texture::load(16, 16, &atlas_pixels).expect("lightmap valide");

    for count in [4usize, 8, 16, 20] {
        for lshift in [0u32, 1, 3] {
            for overbright in 0..=MAX_OVERBRIGHT {
                let start = (seed.next() >> 1) as i64;
                let step = (seed.next() as i64) >> 20;
                let uv = [(seed.next() as i64) >> 28, (seed.next() as i64) >> 28];
                let uv_step = [(seed.next() as i64) >> 44, (seed.next() as i64) >> 44];
                // La lightmap est étirée : des pentes bien plus faibles que celles
                // de la texture, comme sur une surface de carte.
                let luv = [(seed.next() as i64) >> 30, (seed.next() as i64) >> 30];
                let luv_step = [(seed.next() as i64) >> 48, (seed.next() as i64) >> 48];
                let (mut color, mut depth) = peuple(&mut seed, count);
                let (mut attendu_color, mut attendu_depth) = (color.clone(), depth.clone());
                let (x0, y) = (seed.next() as i32 & 0x3FF, seed.next() as i32 & 0x3FF);
                let dither = [
                    [
                        dither_offsets(x0, y)[0],
                        dither_offsets(x0 + 1, y)[0],
                        dither_offsets(x0 + 2, y)[0],
                        dither_offsets(x0 + 3, y)[0],
                    ],
                    [
                        dither_offsets(x0, y)[1],
                        dither_offsets(x0 + 1, y)[1],
                        dither_offsets(x0 + 2, y)[1],
                        dither_offsets(x0 + 3, y)[1],
                    ],
                ];

                let traites = fill_sampled_row::<true>(SampledRow {
                    color: &mut color,
                    depth: &mut depth,
                    start,
                    step,
                    uv,
                    uv_step,
                    shift: 0,
                    texels: &texels,
                    size: (64, 32),
                    dither,
                    x0,
                    y,
                    lit: true,
                    lightmap: crate::raster::simd::LightmapRow {
                        uv: luv,
                        uv_step: luv_step,
                        shift: lshift,
                        texels: atlas.level_texels(lshift as usize),
                        size: atlas.level_size(lshift as usize),
                        shade: 8 - overbright,
                    },
                });
                assert_eq!(traites, count & !3);

                // La référence, pixel par pixel, par les fonctions du scalaire.
                let mut z = start;
                let mut t = uv;
                let mut l = luv;
                for i in 0..traites {
                    let value = (z >> GRADIENT_BITS) as u32;
                    let offsets = dither_offsets(x0 + i as i32, y);
                    // La texture est au niveau zéro dans ce cas : son décalage
                    // de niveau est l'identité, et seul celui du format reste.
                    let coord = |c: i64, d: i32| ((c + i64::from(d)) >> UV_BITS) as i32;
                    let tx = (coord(t[0], offsets[0]) as u32) & 63;
                    let ty = (coord(t[1], offsets[1]) as u32) & 31;
                    let texel = texels[(ty * 64 + tx) as usize];
                    let light = atlas.bilinear(
                        lshift as usize,
                        (l[0] >> lshift) as i32,
                        (l[1] >> lshift) as i32,
                    );
                    if value > attendu_depth[i] {
                        attendu_depth[i] = value;
                        attendu_color[i] = modulate(texel, light, overbright);
                    }
                    z = z.wrapping_add(step);
                    t[0] = t[0].wrapping_add(uv_step[0]);
                    t[1] = t[1].wrapping_add(uv_step[1]);
                    l[0] = l[0].wrapping_add(luv_step[0]);
                    l[1] = l[1].wrapping_add(luv_step[1]);
                }

                assert_eq!(
                    depth, attendu_depth,
                    "profondeurs, {count} px, niveau={lshift}, ob={overbright}"
                );
                assert_eq!(
                    color, attendu_color,
                    "couleurs, {count} px, niveau={lshift}, ob={overbright}"
                );
            }
        }
    }
}

/// Un segment plus court qu'un bloc n'est pas traité du tout.
///
/// **Et ce n'est pas un échec** : l'appelant parcourt alors le segment entier en
/// scalaire. Le cas est écrit parce que la variante doit rendre zéro plutôt que
/// d'écrire hors des tranches — c'est la borne de la boucle, et elle se vérifie
/// plutôt qu'elle se relit.
#[test]
fn un_segment_plus_court_qu_un_bloc_reste_intact() {
    let mut seed = Seed(0x0bad_c0de_dead_beef);
    let texels = marquee(16, 16);
    for count in 0..4usize {
        let (mut color, mut depth) = peuple(&mut seed, count);
        let (attendu_color, attendu_depth) = (color.clone(), depth.clone());

        let traites = fill_sampled_row::<false>(SampledRow {
            lit: false,
            lightmap: crate::raster::simd::LightmapRow::NONE,
            color: &mut color,
            depth: &mut depth,
            start: 1 << 40,
            step: 1 << 30,
            uv: [0, 0],
            uv_step: [1 << 16, 1 << 16],
            shift: 0,
            texels: &texels,
            size: (16, 16),
            dither: [[0; 4], [0; 4]],
            x0: 0,
            y: 0,
        });

        assert_eq!(traites, 0, "{count} pixels");
        assert_eq!(depth, attendu_depth, "{count} pixels");
        assert_eq!(color, attendu_color, "{count} pixels");
    }
}
