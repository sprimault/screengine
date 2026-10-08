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

use super::{fill_flat_row, fill_sampled_row};
use crate::math::fixed::UV_BITS;
use crate::raster::plane::GRADIENT_BITS;
use crate::raster::simd::{FlatRow, SampledRow};
use crate::raster::triangle::dither_offsets;

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

            let traites = fill_sampled_row(SampledRow {
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

        let traites = fill_sampled_row(SampledRow {
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
