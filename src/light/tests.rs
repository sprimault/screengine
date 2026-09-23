// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les deux bouts de la combinaison, et ce qui se passe entre.

use super::*;

/// Un texel écrit par ses quatre octets, dans l'ordre mémoire de la sortie.
fn rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    r | (g << 8) | (b << 16) | (a << 24)
}

/// Sous pleine lumière, le texel ressort **intact** — c'est la raison d'être
/// de la forme retenue.
///
/// `(t·l + 128) >> 8` rendrait 254 pour 255 : invisible sur un pixel, et un
/// assombrissement de toute surface éclairée du moteur.
#[test]
fn la_pleine_lumiere_rend_le_texel_intact() {
    for value in [0u32, 1, 127, 128, 254, 255] {
        let texel = rgba(value, 255 - value, value / 2, 0x7F);
        assert_eq!(modulate(texel, rgba(255, 255, 255, 0), 0), texel, "{value}");
    }
}

/// À l'autre bout, l'obscurité rend le noir sur les trois canaux, et l'alpha
/// du texel seul.
#[test]
fn l_obscurite_rend_le_noir() {
    let texel = rgba(200, 100, 50, 0xFF);
    assert_eq!(modulate(texel, 0, 0), 0xFF00_0000);
}

/// Chaque canal s'éclaire par le sien : un éclairage rouge ne laisse passer que
/// le rouge du texel.
///
/// Sans ce contrôle, une combinaison qui prendrait le même canal d'éclairage
/// pour les trois passerait tous les autres tests.
#[test]
fn chaque_canal_prend_son_propre_eclairage() {
    let texel = rgba(0xFF, 0xFF, 0xFF, 0);
    assert_eq!(modulate(texel, rgba(0xFF, 0, 0, 0), 0), rgba(0xFF, 0, 0, 0));
    assert_eq!(modulate(texel, rgba(0, 0xFF, 0, 0), 0), rgba(0, 0xFF, 0, 0));
    assert_eq!(modulate(texel, rgba(0, 0, 0xFF, 0), 0), rgba(0, 0, 0xFF, 0));
}

/// Le sur-éclairement double la valeur combinée, et **sature au lieu
/// d'enrouler**.
///
/// Un enroulement rendrait une surface bien éclairée plus sombre que la même
/// surface à demi éclairée, avec une frontière franche là où le produit
/// dépasse : c'est le défaut le plus visible que ce décalage puisse produire.
#[test]
fn le_sur_eclairement_double_puis_sature() {
    let demi = rgba(0x80, 0x80, 0x80, 0);
    let texel = rgba(0x40, 0x80, 0xFF, 0);
    // À demi-éclairage, doubler rend presque le texel : `l + 1` vaut 129, donc
    // le produit dépasse le texel d'un deux-cent-cinquante-sixième.
    let double = modulate(texel, demi, 1);
    for index in [0u32, 8, 16] {
        let (rendu, attendu) = ((double >> index) & 0xFF, (texel >> index) & 0xFF);
        assert!(
            rendu >= attendu && rendu <= attendu + 1,
            "canal {index} : {rendu} pour {attendu}"
        );
    }
    // Sous pleine lumière, le quadruplement sature les trois canaux plutôt que
    // de rendre un produit tronqué.
    let plein = rgba(0xFF, 0xFF, 0xFF, 0);
    assert_eq!(
        modulate(rgba(0x40, 0x80, 0xFF, 0), plein, MAX_OVERBRIGHT),
        rgba(0xFF, 0xFF, 0xFF, 0)
    );
}

/// La combinaison est monotone en l'éclairage : plus de lumière ne rend jamais
/// un canal plus sombre.
///
/// C'est ce qui rend un dégradé de lightmap lisible. Une troncature posée au
/// mauvais endroit le briserait par endroits, et rien d'autre ne le verrait.
#[test]
fn plus_de_lumiere_n_assombrit_jamais() {
    for overbright in 0..=MAX_OVERBRIGHT {
        for texel in [1u32, 17, 64, 200, 255] {
            let mut precedent = 0;
            for light in 0..=255u32 {
                let rendu = modulate(texel, light, overbright) & 0xFF;
                assert!(rendu >= precedent, "texel {texel}, éclairage {light}");
                precedent = rendu;
            }
        }
    }
}
