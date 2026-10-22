// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que le générateur des tests doit garantir pour que les tests qui s'en
//! servent mesurent ce qu'ils croient mesurer.

use alloc::collections::BTreeSet;

use super::Rng;

/// Deux graines distinctes donnent deux suites distinctes.
///
/// **Sans quoi une boucle sur des graines consécutives ment sur sa couverture.**
/// La graine était forcée impaire, si bien que `2n` et `2n + 1` partaient du
/// même état : une boucle de quatre cents graines n'éprouvait que deux cents
/// cas, chacun deux fois, et la garde qui compte les itérations la déclarait
/// honnête.
#[test]
fn deux_graines_consecutives_ne_donnent_pas_la_meme_suite() {
    let premiers: BTreeSet<u64> = (0..64u64)
        .map(|seed| {
            let mut rng = Rng::new(seed);
            rng.next()
        })
        .collect();

    assert_eq!(
        premiers.len(),
        64,
        "{} suites distinctes sur soixante-quatre graines",
        premiers.len()
    );
}

/// La graine nulle ne bloque pas la suite.
///
/// C'est la raison d'être du remplacement, et elle reste : xorshift a le zéro
/// pour point fixe, une suite qui y tombe n'en sort plus.
#[test]
fn la_graine_nulle_ne_bloque_pas() {
    let mut rng = Rng::new(0);
    let tirages: BTreeSet<u64> = (0..8).map(|_| rng.next()).collect();
    assert_eq!(tirages.len(), 8, "la suite s'est bloquée");
    assert!(!tirages.contains(&0), "un tirage nul");
}

/// `coord` tient ses bornes, les atteint toutes deux, et se rejoue.
///
/// **C'est l'instrument de tous les tirages du noyau, et rien ne le vérifiait.**
/// La doctrine du projet dit que l'instrument se vérifie avant ce qu'il mesure :
/// un `coord` qui ne sortirait jamais de son milieu laisserait chaque test
/// aléatoire vert en n'explorant rien, et sa sortie ne l'en distinguerait pas.
///
/// Trois propriétés, et aucune ne remplace les autres. Les bornes **tenues**,
/// sans quoi un tirage sortirait du domaine que l'appelant croit borné ; les
/// bornes **atteintes**, sans quoi un intervalle serait silencieusement plus
/// étroit qu'annoncé — c'est le cas d'un `hi` exclu par erreur ; et la suite
/// **rejouable**, puisque c'est tout ce qu'une graine fixe promet.
///
/// L'intervalle est petit à dessein : sur `[0, 3]`, quelques centaines de
/// tirages touchent les quatre valeurs si la loi est à peu près uniforme, là où
/// un grand intervalle ne dirait rien des bords.
#[test]
fn coord_tient_ses_bornes_les_atteint_et_se_rejoue() {
    let mut rng = Rng::new(0xC007_0000_0000_0001);
    let mut vus = [false; 4];
    for _ in 0..512 {
        let value = rng.coord(0, 3);
        assert!(
            (0..=3).contains(&value),
            "coord est sorti de ses bornes : {value}"
        );
        vus[value as usize] = true;
    }
    assert!(vus.iter().all(|seen| *seen), "coord n'atteint pas {vus:?}");

    // Les bornes négatives passent par la même soustraction, qui déborderait si
    // elle était écrite sur des entiers signés sans élargissement.
    let mut rng = Rng::new(7);
    for _ in 0..256 {
        let value = rng.coord(-5, -2);
        assert!(
            (-5..=-2).contains(&value),
            "coord négatif est sorti de ses bornes : {value}"
        );
    }

    // La même graine rejoue la même suite : un échec qui ne se rejoue pas n'a
    // pas été trouvé.
    let suite = |seed| {
        let mut rng = Rng::new(seed);
        let mut out = [0i32; 16];
        for slot in &mut out {
            *slot = rng.coord(-100, 100);
        }
        out
    };
    assert_eq!(suite(42), suite(42), "la graine ne rejoue pas sa suite");
    assert_ne!(suite(42), suite(43), "deux graines rendent la même suite");
}

/// `unit_f32` tient `[0, 1)`, le parcourt, s'y convertit sans perte et se
/// rejoue.
///
/// Même raison que pour son voisin : c'est l'instrument de trente-huit tirages
/// du noyau, dont les positions des scènes dont l'empreinte fait foi.
///
/// **La propriété qui lui est propre est l'exactitude**, que sa documentation
/// annonce et que rien ne mesurait : vingt-quatre bits tiennent dans la mantisse
/// d'un `f32`, donc chaque valeur est un multiple exact de 2⁻²⁴ et l'aller-retour
/// par l'entier la rend intacte. C'est ce qui sépare un diviseur en puissance de
/// deux d'un diviseur en `2²⁴ − 1` — la confusion naturelle, celle qui ferait
/// atteindre un —, et le second déplacerait toutes les empreintes sans sortir de
/// l'intervalle annoncé.
#[test]
fn unit_f32_tient_son_intervalle_sans_perte_et_se_rejoue() {
    const SCALE: f32 = (1u32 << 24) as f32;

    let mut rng = Rng::new(0x5EED_0000_0000_0003);
    let mut vus = [false; 8];
    for _ in 0..1024 {
        let value = rng.unit_f32();
        assert!(
            (0.0..1.0).contains(&value),
            "unit_f32 est sorti de [0, 1) : {value}"
        );
        let scaled = value * SCALE;
        assert_eq!(
            scaled,
            (scaled as u32) as f32,
            "{value} ne se représente pas en vingt-quatre bits"
        );
        vus[(value * 8.0) as usize] = true;
    }
    assert!(
        vus.iter().all(|seen| *seen),
        "unit_f32 n'atteint pas {vus:?}"
    );

    let suite = |seed| {
        let mut rng = Rng::new(seed);
        let mut out = [0f32; 16];
        for slot in &mut out {
            *slot = rng.unit_f32();
        }
        out
    };
    assert_eq!(suite(42), suite(42), "la graine ne rejoue pas sa suite");
    assert_ne!(suite(42), suite(43), "deux graines rendent la même suite");
}
