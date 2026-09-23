// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les deux bouts du mélange, l'index, et la forme de la rampe.

use super::*;

/// Une couleur écrite par ses quatre octets, dans l'ordre mémoire de la sortie.
fn rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    r | (g << 8) | (b << 16) | (a << 24)
}

/// Le plan proche des tests, celui des scènes de conformance.
const NEAR: f32 = 0.1;

/// **Les deux bouts du mélange sont exacts.** C'est ce qui justifie les neuf
/// bits du facteur : à huit, le plein rendrait la couleur du brouillard à une
/// unité près, et cet écart d'un seul niveau entre la géométrie lointaine et
/// le fond est exactement la couture d'horizon.
#[test]
fn le_melange_est_exact_aux_deux_bouts() {
    let fog = rgba(0x40, 0x60, 0x90, 0);
    for value in [0u32, 1, 127, 128, 254, 255] {
        let pixel = rgba(value, 255 - value, value / 2, 0xFF);
        assert_eq!(blend(pixel, fog, 0, 128) & 0x00FF_FFFF, pixel & 0x00FF_FFFF);
        assert_eq!(
            blend(pixel, fog, FULL, 128) & 0x00FF_FFFF,
            fog & 0x00FF_FFFF
        );
    }
}

/// Chaque canal se mélange avec le sien : un brouillard rouge n'éteint pas le
/// bleu du pixel.
///
/// Sans ce contrôle, une voie qui déborderait sur sa voisine passerait les
/// deux bouts, qui ne font que recopier.
#[test]
fn chaque_canal_se_melange_avec_le_sien() {
    let pixel = rgba(0xFF, 0xFF, 0xFF, 0);
    let fog = rgba(0, 0, 0, 0);
    let demi = blend(pixel, fog, FULL / 2, 128);
    for index in [0u32, 8, 16] {
        assert_eq!((demi >> index) & 0xFF, 0x80, "canal {index}");
    }
}

/// Le mélange est monotone : plus de brouillard ne rapproche jamais un canal
/// de la couleur d'origine.
///
/// C'est ce qui rend un dégradé de distance lisible, et une retenue entre les
/// deux voies le briserait par endroits.
#[test]
fn plus_de_brouillard_ne_revient_jamais_en_arriere() {
    let pixel = rgba(0xFF, 0x80, 0x00, 0);
    let fog = rgba(0x00, 0x80, 0xFF, 0);
    let (mut rouge, mut bleu) = (256i32, -1i32);
    for factor in 0..=FULL {
        let mixed = blend(pixel, fog, factor, 128);
        let (r, b) = ((mixed & 0xFF) as i32, ((mixed >> 16) & 0xFF) as i32);
        assert!(r <= rouge, "rouge remonte à {factor}");
        assert!(b >= bleu, "bleu redescend à {factor}");
        // Le canal vert est commun aux deux couleurs : il ne bouge pas.
        assert_eq!((mixed >> 8) & 0xFF, 0x80, "vert à {factor}");
        rouge = r;
        bleu = b;
    }
}

/// L'index croît avec la distance, et le fond occupe la dernière tranche.
///
/// La profondeur décroît avec la distance — c'est `near/w` —, donc un index
/// qui croîtrait avec elle rendrait le brouillard à l'envers : épais tout
/// près, clair à l'horizon.
#[test]
fn l_index_croit_avec_la_distance() {
    let mut precedent = 0;
    // De la plus proche à la plus lointaine, en divisant par deux.
    let mut depth = u32::MAX;
    while depth > 1 {
        let index = index_of(depth);
        assert!(index >= precedent, "profondeur {depth}");
        precedent = index;
        depth >>= 1;
    }
    assert_eq!(index_of(0), TABLE_LEN - 1, "le fond n'est pas au plus loin");
    assert!(index_of(u32::MAX) < TABLE_LEN);
}

/// Toute profondeur tombe dans la table, et aucune tranche n'est hors bornes.
#[test]
fn aucune_profondeur_ne_sort_de_la_table() {
    for shift in 0..32 {
        for offset in [0u32, 1, 17, 255] {
            let depth = (1u32 << shift).wrapping_add(offset);
            assert!(index_of(depth) < TABLE_LEN, "profondeur {depth}");
        }
    }
    assert!(index_of(u32::MAX) < TABLE_LEN);
    assert!(index_of(1) < TABLE_LEN);
}

/// `depth_of` est la réciproque de `index_of` **sur les tranches qu'une
/// profondeur réelle peut atteindre**.
///
/// C'est ce qui rend le remplissage juste : sans cette propriété, la table
/// serait remplie pour des profondeurs que rien ne lit, et décalée d'une
/// tranche sur toute son étendue.
///
/// La réserve n'est pas une facilité. `to_depth` borne la profondeur à
/// [`DEPTH_MARGIN`], donc l'exposant ne dépasse jamais 25 ; au-delà, il ne
/// reste plus six bits de mantisse à porter et plusieurs tranches désignent la
/// même profondeur. Ces tranches-là existent dans la table et n'y sont jamais
/// lues, hormis la dernière, qui est le fond.
///
/// [`DEPTH_MARGIN`]: crate::math::fixed::DEPTH_MARGIN
#[test]
fn la_profondeur_d_une_tranche_retombe_dans_sa_tranche() {
    let mut atteintes = 0;
    for index in 0..TABLE_LEN - 1 {
        let depth = depth_of(index);
        if depth < crate::math::fixed::DEPTH_MARGIN {
            continue;
        }
        assert_eq!(index_of(depth), index, "tranche {index}");
        atteintes += 1;
    }
    // Sans ce compte, un `depth_of` qui rendrait zéro partout passerait le test
    // en ne vérifiant rien.
    assert!(atteintes > 1600, "{atteintes} tranches vérifiées, trop peu");
}

/// Aucune profondeur que `to_depth` peut produire ne tombe dans les tranches
/// mortes, celles qui n'ont plus assez de bits pour leur mantisse.
///
/// C'est la contrepartie du test précédent : lui montre que les tranches
/// utiles sont justes, celui-ci que les autres ne servent pas.
#[test]
fn aucune_profondeur_reelle_ne_tombe_dans_les_tranches_mortes() {
    let vivantes = 26 << MANTISSA_BITS;
    let mut rng = crate::testing::Rng::new(0xF06);
    for _ in 0..20_000 {
        let depth = crate::math::fixed::to_depth(rng.unit_f32());
        assert!(index_of(depth) < vivantes, "profondeur {depth}");
    }
    // Les deux extrêmes que `to_depth` sait rendre, bornes comprises.
    assert!(index_of(crate::math::fixed::DEPTH_MARGIN) < vivantes);
    assert!(index_of(u32::MAX - crate::math::fixed::DEPTH_MARGIN) < vivantes);
}

/// La rampe est **linéaire en distance**, pas en profondeur.
///
/// Le critère est le milieu : à mi-distance de la rampe, le facteur vaut la
/// moitié. Une rampe linéaire en `near/w` y serait déjà à plus des trois
/// quarts, ce que ce test refuse.
#[test]
fn la_rampe_est_lineaire_en_distance() {
    let mut fog = Fog::new().expect("table réservée");
    let (start, end) = (10.0f32, 110.0f32);
    fog.set(0, NEAR, start, end).expect("rampe valide");

    let at = |distance: f32| {
        let depth = crate::math::fixed::to_depth(NEAR / distance);
        fog.factor(depth)
    };
    // La quantification de la table est d'une tranche : six bits de mantisse
    // donnent moins d'un pour cent d'écart, soit deux unités sur 256.
    let milieu = at((start + end) / 2.0);
    assert!(
        (i32::from(milieu) - 128).abs() <= 4,
        "{milieu} au milieu de la rampe, attendu 128"
    );
    assert_eq!(at(start - 1.0), 0, "avant la rampe");
    assert_eq!(at(end + 1.0), FULL, "après la rampe");
}

/// **La table de tramage est bien la transposée de celle du rasteriseur.**
///
/// Les deux sont écrites à la main, chacune à son échelle : celle des
/// coordonnées de texture en seizièmes de texel, celle-ci en arrondis de
/// mélange. Rien dans le code ne les lie, et rien ne dirait qu'elles ont cessé
/// de l'être — sinon ce test. Deux motifs identiques superposés se
/// renforceraient au lieu de se disperser, ce qui est exactement ce que la
/// transposition existe pour éviter.
#[test]
fn le_tramage_est_la_transposee_de_celui_du_rasteriseur() {
    // La table du rasteriseur porte `(2·M − 15) << 11` : on en retire le rang.
    let rank = |entry: i32| ((entry >> 11) + 15) / 2;
    for y in 0..4u32 {
        for x in 0..4u32 {
            let transpose = rank(crate::raster::DITHER[(x * 4 + y) as usize]) as u32;
            // L'échelle du brouillard est l'arrondi d'un mélange sur huit
            // bits : seize niveaux répartis sur 256, centrés sur 128.
            assert_eq!(dither(x, y), transpose * 16 + 8, "en ({x}, {y})");
        }
    }
}

/// Les seize arrondis sont une permutation, et leur moyenne est exactement
/// l'arrondi au plus proche.
///
/// Un biais, même d'une unité, décalerait toute la rampe : le brouillard
/// commencerait un peu trop tôt ou un peu trop tard partout, ce qu'aucune
/// image ne montre et qu'aucun autre test ne verrait.
#[test]
fn les_arrondis_sont_une_permutation_centree() {
    let mut vus = DITHER;
    vus.sort_unstable();
    for (i, value) in vus.iter().enumerate() {
        assert_eq!(*value, i as u32 * 16 + 8, "niveau {i} manquant ou doublé");
    }
    let somme: u32 = DITHER.iter().sum();
    assert_eq!(somme, 128 * 16, "la moyenne n'est pas 128");
}

/// Un brouillard éteint rend zéro partout, et son mélange laisse le pixel
/// intact : l'appelant n'a aucun cas à distinguer.
#[test]
fn un_brouillard_eteint_ne_change_rien() {
    let fog = Fog::new().expect("table réservée");
    assert!(!fog.is_set());
    for depth in [0u32, 1, 1 << 20, u32::MAX] {
        assert_eq!(fog.factor(depth), 0, "profondeur {depth}");
    }
}

/// Une rampe vide ou renversée est refusée : c'est une division par zéro, et
/// l'appelant voulait vraisemblablement éteindre le brouillard.
#[test]
fn une_rampe_vide_ou_renversee_est_refusee() {
    let mut fog = Fog::new().expect("table réservée");
    for (start, end) in [(10.0, 10.0), (10.0, 5.0), (-1.0, 10.0)] {
        assert_eq!(
            fog.set(0, NEAR, start, end),
            Err(Error::InvalidArgument(Argument::Fog)),
            "{start} à {end}"
        );
    }
    assert_eq!(
        fog.set(0, NEAR, 1.0, f32::INFINITY),
        Err(Error::InvalidArgument(Argument::Fog))
    );
    assert!(!fog.is_set(), "un refus a réglé le brouillard");
}

/// Le réglage n'alloue pas : la table est réservée à la création, et se
/// remplit sans jamais grandir.
///
/// C'est l'invariant de l'étape 0, et il porte ici sur un appel que l'hôte
/// fera entre deux images — donc dans une boucle de jeu.
#[test]
fn le_reglage_n_alloue_pas() {
    let mut fog = Fog::new().expect("table réservée");
    let capacite = fog.table.capacity();
    for end in [50.0f32, 80.0, 200.0] {
        fog.set(0, NEAR, 10.0, end).expect("rampe valide");
        assert_eq!(fog.table.capacity(), capacite, "la table a grandi");
        assert_eq!(fog.table.len(), TABLE_LEN);
    }
}
