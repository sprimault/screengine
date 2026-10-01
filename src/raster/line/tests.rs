// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que le tracé doit tenir, et contre quoi il se valide.
//!
//! **L'oracle est la règle elle-même, appliquée à tous les pixels.**
//! [`exits_diamond`] est une question géométrique posée sur un pixel : la poser
//! sur chacun d'une boîte donne la couverture exacte, en temps quadratique.
//! [`cover`] donne la même réponse en un pas par colonne, et c'est cette égalité
//! qui est testée — le même théorème que la traversée contre le chemin brut, et
//! le balayage contre la force brute.

extern crate std;

use alloc::vec::Vec;

use super::*;

/// Un segment de la boîte d'épreuve, aux extrémités données en sous-pixels.
fn segment(x0: i32, y0: i32, x1: i32, y1: i32) -> Segment {
    Segment {
        x0,
        y0,
        z0: 1 << 31,
        x1,
        y1,
        z1: 1 << 31,
        color: 0x00FF_FFFF,
        tested: false,
    }
}

/// Une fenêtre qui contient tout ce que les épreuves tracent.
fn window() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: 64,
        height: 64,
    }
}

/// La couverture par la forme rapide, triée.
fn fast(segment: &Segment, window: Rect) -> Vec<(i32, i32)> {
    let mut pixels = Vec::new();
    cover(segment, window, |x, y, _| pixels.push((x, y)));
    pixels.sort_unstable();
    pixels
}

/// La couverture par l'oracle : la règle posée sur chaque pixel de la fenêtre.
fn oracle(segment: &Segment, window: Rect) -> Vec<(i32, i32)> {
    let mut pixels = Vec::new();
    for y in window.y as i32..(window.y + window.height) as i32 {
        for x in window.x as i32..(window.x + window.width) as i32 {
            if exits_diamond(segment, x, y) {
                pixels.push((x, y));
            }
        }
    }
    pixels.sort_unstable();
    pixels
}

/// Le générateur des épreuves aléatoires, avec sa graine fixe.
///
/// Écrit ici plutôt qu'emprunté : le noyau n'a aucune dépendance, pas même de
/// test. Un échec se rejoue en relisant la graine affichée.
struct Rng(u64);

impl Rng {
    /// Le prochain entier dans `[0, range)`.
    fn next(&mut self, range: i32) -> i32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((self.0 >> 33) % range as u64) as i32
    }
}

/// Le parcours rapide allume exactement ce que la règle désigne, sur des
/// segments tirés au hasard.
///
/// C'est le test qui porte le lot : il compare une forme close à une
/// énumération exhaustive, et aucune des deux ne peut être juste par accident
/// sur mille cas.
#[test]
fn le_parcours_rapide_egale_la_regle() {
    let mut rng = Rng(0x5347_4C4E_4530_0001);
    for case in 0..1000 {
        let s = segment(
            rng.next(64 * 16),
            rng.next(64 * 16),
            rng.next(64 * 16),
            rng.next(64 * 16),
        );
        assert_eq!(
            fast(&s, window()),
            oracle(&s, window()),
            "cas {case} : {s:?}"
        );
    }
}

/// Un segment horizontal allume une ligne continue de pixels.
///
/// Le cas le plus simple, et celui qu'un parcours mal borné rate par une
/// extrémité : il est écrit à part pour que son échec nomme le défaut.
#[test]
fn un_segment_horizontal_est_continu() {
    // D'un centre de pixel à un autre, cinq pixels plus loin.
    let s = segment(8, 8, 8 + 5 * 16, 8);
    let pixels = fast(&s, window());
    assert_eq!(pixels, [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
}

/// Une polyligne ne peint son sommet partagé qu'une fois.
///
/// **C'est la raison d'être de la règle du losange.** Sans elle, le sommet est
/// peint deux fois : invisible sur un trait opaque, visible dès qu'un éditeur
/// trace en couleur modulée, et visible en mouvement sur une sélection qui
/// clignote.
#[test]
fn une_polyligne_ne_peint_son_sommet_qu_une_fois() {
    let a = segment(8, 8, 8 + 4 * 16, 8 + 4 * 16);
    let b = segment(8 + 4 * 16, 8 + 4 * 16, 8 + 8 * 16, 8);
    let mut counts = std::collections::BTreeMap::new();
    for s in [a, b] {
        for pixel in fast(&s, window()) {
            *counts.entry(pixel).or_insert(0u32) += 1;
        }
    }
    let shared = (4, 4);
    assert_eq!(
        counts.get(&shared),
        Some(&1),
        "le sommet partagé est peint {:?} fois",
        counts.get(&shared)
    );
    assert!(
        counts.values().all(|n| *n == 1),
        "un pixel est peint deux fois : {counts:?}"
    );
}

/// Un segment plus court qu'un demi-pixel n'allume rien.
///
/// Conséquence assumée de la règle : il n'en sort d'aucun losange. C'est ce qui
/// évite que deux segments minuscules consécutifs peignent deux fois le même
/// pixel.
#[test]
fn un_segment_minuscule_n_allume_rien() {
    let s = segment(8, 8, 10, 8);
    assert!(fast(&s, window()).is_empty());
}

/// Un segment de longueur nulle n'allume rien et ne divise par rien.
#[test]
fn un_segment_nul_n_allume_rien() {
    let s = segment(8, 8, 8, 8);
    assert!(fast(&s, window()).is_empty());
    assert!(!exits_diamond(&s, 0, 0));
}

/// **Le découpage en fenêtres ne change pas l'image.**
///
/// Le même théorème que pour les tuiles d'un triangle : la réunion de ce que
/// quatre quadrants allument est exactement ce que la fenêtre entière allume.
/// Un parcours qui redémarrerait son pas au bord d'une fenêtre échouerait ici.
#[test]
fn la_fenetre_ne_borne_que_la_boucle() {
    let mut rng = Rng(0x5347_4C4E_4530_0002);
    for case in 0..200 {
        let s = segment(
            rng.next(64 * 16),
            rng.next(64 * 16),
            rng.next(64 * 16),
            rng.next(64 * 16),
        );
        let entiere = fast(&s, window());

        let mut morceaux = Vec::new();
        for (x, y) in [(0, 0), (32, 0), (0, 32), (32, 32)] {
            let quadrant = Rect {
                x,
                y,
                width: 32,
                height: 32,
            };
            morceaux.extend(fast(&s, quadrant));
        }
        morceaux.sort_unstable();
        assert_eq!(entiere, morceaux, "cas {case} : {s:?}");
    }
}

/// La profondeur rendue reste entre celles des deux extrémités.
///
/// Un pixel du bout peut tomber au-delà de l'extrémité : sans bornage écrit, il
/// extrapolerait, et un repère passerait devant ce qui doit le cacher.
#[test]
fn la_profondeur_ne_sort_jamais_des_bouts() {
    let s = Segment {
        z0: 1000,
        z1: 2000,
        ..segment(8, 8, 8 + 10 * 16, 8 + 3 * 16)
    };
    cover(&s, window(), |_, _, z| {
        assert!((1000..=2000).contains(&z), "profondeur hors bornes : {z}");
    });
}

/// La boîte englobante contient tout ce que la couverture allume.
///
/// C'est ce que la répartition par tuiles exige : une tuile oubliée trouerait la
/// ligne dans une seule configuration.
#[test]
fn la_boite_contient_la_couverture() {
    let mut rng = Rng(0x5347_4C4E_4530_0003);
    for case in 0..200 {
        let s = segment(
            rng.next(64 * 16),
            rng.next(64 * 16),
            rng.next(64 * 16),
            rng.next(64 * 16),
        );
        let (bx0, by0, bx1, by1) = s.bounds();
        for (x, y) in fast(&s, window()) {
            assert!(
                x >= bx0 && x <= bx1 && y >= by0 && y <= by1,
                "cas {case} : ({x}, {y}) hors de la boîte {:?}",
                s.bounds()
            );
        }
    }
}

/// Le sens de parcours ne change que les deux pixels d'extrémité.
///
/// **La règle dépend du sens, et c'est voulu** : un segment allume le pixel
/// d'où il part s'il en sort, et n'allume pas celui où il s'arrête. Retourné, il
/// échange les deux. C'est exactement ce qui fait qu'une polyligne ne peint son
/// sommet partagé qu'une fois, et un lecteur qui croirait l'ensemble invariant
/// chercherait un défaut là où il n'y en a pas.
///
/// Ce qui est invariant, et que ce test fixe : **partout ailleurs**, les deux
/// sens allument les mêmes pixels.
#[test]
fn le_sens_ne_change_que_les_bouts() {
    let mut rng = Rng(0x5347_4C4E_4530_0004);
    for case in 0..200 {
        let (x0, y0, x1, y1) = (
            rng.next(64 * 16),
            rng.next(64 * 16),
            rng.next(64 * 16),
            rng.next(64 * 16),
        );
        let aller = fast(&segment(x0, y0, x1, y1), window());
        let retour = fast(&segment(x1, y1, x0, y0), window());

        let bouts = [
            (x0.div_euclid(16), y0.div_euclid(16)),
            (x1.div_euclid(16), y1.div_euclid(16)),
        ];
        for pixel in aller.iter().chain(retour.iter()) {
            let commun = aller.contains(pixel) && retour.contains(pixel);
            assert!(
                commun || bouts.contains(pixel),
                "cas {case} : {pixel:?} ne diffère pas sur un bout — \
                 ({x0},{y0}) → ({x1},{y1})"
            );
        }
    }
}
