// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests du chargement et de la chaîne de mipmaps.

use alloc::vec;
use alloc::vec::Vec;

use super::*;

/// Un bloc de `width × height` texels dont chaque canal vaut la valeur rendue
/// par `f`, pour écrire une texture de test sans aligner des octets à la main.
fn block(width: u32, height: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for y in 0..height {
        for x in 0..width {
            bytes.extend_from_slice(&f(x, y));
        }
    }
    bytes
}

/// Un bloc uni, quand seule la forme de la chaîne est en cause.
fn plain(width: u32, height: u32) -> Vec<u8> {
    vec![0; (width * height) as usize * 4]
}

/// Un côté qui n'est pas une puissance de deux est refusé, et les deux côtés
/// sont vérifiés : c'est le repli par masque qui en dépend, et il ne se replie
/// pas à moitié.
#[test]
fn un_cote_non_puissance_de_deux_est_refuse() {
    for (w, h) in [(3, 4), (4, 3), (6, 6), (0, 4), (4, 0)] {
        assert_eq!(
            Texture::load(w, h, &plain(w.max(1), h.max(1))).unwrap_err(),
            Error::InvalidArgument(Argument::TextureSize)
        );
    }
}

/// Le plafond est refusé par la même porte, et le côté juste en dessous passe :
/// une borne écrite à l'envers laisserait entrer 4096 ou refuserait 2048.
#[test]
fn un_cote_au_dela_du_plafond_est_refuse() {
    let too_wide = MAX_TEXTURE_SIZE * 2;
    assert_eq!(
        Texture::load(too_wide, 1, &plain(too_wide, 1)).unwrap_err(),
        Error::InvalidArgument(Argument::TextureSize)
    );
    assert!(Texture::load(MAX_TEXTURE_SIZE, 1, &plain(MAX_TEXTURE_SIZE, 1)).is_ok());
}

/// Un bloc plus court ou plus long que `width × height × 4` est refusé plutôt
/// que tronqué : une ligne manquante donnerait une texture à moitié noire, que
/// personne ne rapprocherait d'un appel mal formé.
#[test]
fn un_bloc_de_la_mauvaise_longueur_est_refuse() {
    for len in [0, 4 * 4 * 4 - 1, 4 * 4 * 4 + 1] {
        assert_eq!(
            Texture::load(4, 4, &vec![0u8; len]).unwrap_err(),
            Error::InvalidArgument(Argument::TextureLength)
        );
    }
}

/// Les quatre octets d'un texel se lisent dans l'ordre R, G, B, A, celui des
/// pixels de sortie. Un `u32` reconstruit à l'envers échangerait le rouge et le
/// bleu sans qu'aucune empreinte ne dise pourquoi.
#[test]
fn le_niveau_zero_reprend_les_octets_dans_l_ordre_recu() {
    let pixels = block(2, 1, |x, _| [10 + x as u8, 20, 30, 40]);
    let texture = Texture::load(2, 1, &pixels).expect("texture valide");
    assert_eq!(
        texture.level_texels(0),
        [
            u32::from_le_bytes([10, 20, 30, 40]),
            u32::from_le_bytes([11, 20, 30, 40]),
        ]
    );
}

/// Une texture d'un seul texel n'a qu'un niveau : la boucle de construction
/// doit s'arrêter avant de diviser, et non après.
#[test]
fn une_texture_de_un_texel_n_a_qu_un_niveau() {
    let texture = Texture::load(1, 1, &plain(1, 1)).expect("texture valide");
    assert_eq!(texture.level_count(), 1);
    assert_eq!(texture.level_size(0), (1, 1));
}

/// La chaîne descend toujours jusqu'à 1×1, sans s'arrêter en route : c'est ce
/// qui permet au choix du niveau de ne jamais manquer de mipmap, si loin que la
/// surface parte.
#[test]
fn la_chaine_descend_jusqu_a_un_texel() {
    let texture = Texture::load(8, 8, &plain(8, 8)).expect("texture valide");
    assert_eq!(texture.level_count(), 4);
    for (level, side) in [8, 4, 2, 1].iter().enumerate() {
        assert_eq!(texture.level_size(level), (*side, *side));
        assert_eq!(texture.level_texels(level).len(), (side * side) as usize);
    }
}

/// Une texture non carrée réduit chaque côté tant qu'il le peut, et continue
/// sur l'autre une fois le premier à 1. S'arrêter là laisserait une texture
/// 2048×1 sans niveau au-delà du deuxième.
#[test]
fn une_texture_non_carree_poursuit_sur_le_cote_le_plus_long() {
    let texture = Texture::load(8, 2, &plain(8, 2)).expect("texture valide");
    assert_eq!(texture.level_count(), 4);
    assert_eq!(texture.level_size(0), (8, 2));
    assert_eq!(texture.level_size(1), (4, 1));
    assert_eq!(texture.level_size(2), (2, 1));
    assert_eq!(texture.level_size(3), (1, 1));
}

/// Le niveau suivant est la moyenne des quatre texels qu'il recouvre, canal par
/// canal. Un échantillonnage au plus proche rendrait ici le premier des quatre.
#[test]
fn la_reduction_moyenne_les_quatre_texels() {
    let values: [[u8; 4]; 4] = [
        [0, 10, 20, 30],
        [40, 50, 60, 70],
        [80, 90, 100, 110],
        [120, 130, 140, 150],
    ];
    let pixels = block(2, 2, |x, y| values[(y * 2 + x) as usize]);
    let texture = Texture::load(2, 2, &pixels).expect("texture valide");

    let mut expected = [0u8; 4];
    for (channel, byte) in expected.iter_mut().enumerate() {
        let sum: u32 = values.iter().map(|texel| u32::from(texel[channel])).sum();
        *byte = ((sum + 2) / 4) as u8;
    }
    assert_eq!(texture.level_texels(1), [u32::from_le_bytes(expected)]);
}

/// L'arrondi est celui de `(a+b+c+d+2) >> 2` : la demi-unité s'ajoute avant le
/// décalage. Sans elle, une texture uniforme s'assombrirait d'un demi-niveau à
/// chaque réduction, donc de six niveaux sur une chaîne complète.
#[test]
fn la_reduction_arrondit_a_la_demi_unite() {
    for (quad, expected) in [
        ([0, 0, 0, 1], 0),
        ([0, 0, 1, 1], 1),
        ([1, 1, 1, 2], 1),
        ([255, 255, 255, 255], 255),
        ([254, 255, 255, 255], 255),
    ] {
        let pixels = block(2, 2, |x, y| {
            let v = quad[(y * 2 + x) as usize];
            [v, v, v, v]
        });
        let texture = Texture::load(2, 2, &pixels).expect("texture valide");
        assert_eq!(
            texture.level_texels(1),
            [u32::from_le_bytes([expected; 4])],
            "quadruplet {quad:?}"
        );
    }
}

/// Quand un seul côté se réduit, la moyenne porte sur deux texels et non
/// quatre. Diviser par quatre dans ce cas assombrirait de moitié tous les
/// niveaux d'une texture non carrée à partir du premier côté épuisé.
#[test]
fn la_reduction_d_un_seul_cote_moyenne_deux_texels() {
    let pixels = block(2, 1, |x, _| [if x == 0 { 100 } else { 200 }; 4]);
    let texture = Texture::load(2, 1, &pixels).expect("texture valide");
    assert_eq!(texture.level_texels(1), [u32::from_le_bytes([150; 4])]);
}

/// Chaque canal se moyenne séparément, alpha compris : une somme faite sur
/// l'entier plutôt que sur les octets ferait déborder un canal dans le suivant.
#[test]
fn chaque_canal_se_moyenne_separement() {
    let pixels = block(2, 2, |x, y| match (x, y) {
        (0, 0) => [255, 0, 0, 0],
        (1, 0) => [255, 0, 0, 0],
        (0, 1) => [0, 255, 0, 8],
        _ => [0, 255, 0, 8],
    });
    let texture = Texture::load(2, 2, &pixels).expect("texture valide");
    assert_eq!(
        texture.level_texels(1),
        [u32::from_le_bytes([128, 128, 0, 4])]
    );
}

/// Un niveau au-delà de la chaîne rend le dernier. C'est ce qui permet au
/// choix du mipmap de ne rien borner dans la boucle qui échantillonne, et le
/// niveau 1×1 est le bon résultat pour une surface plus petite qu'un texel.
#[test]
fn un_niveau_au_dela_de_la_chaine_rend_le_dernier() {
    let texture = Texture::load(4, 4, &plain(4, 4)).expect("texture valide");
    let last = texture.level_count() - 1;
    assert_eq!(texture.level_size(last + 7), texture.level_size(last));
    assert_eq!(texture.level_texels(last + 7), texture.level_texels(last));
}

/// Les niveaux ne se chevauchent pas et couvrent le tampon sans trou : une
/// erreur d'un texel dans les décalages ferait lire à un niveau la dernière
/// ligne de son voisin, ce qu'aucune valeur uniforme ne montrerait.
#[test]
fn les_niveaux_se_suivent_sans_trou_ni_recouvrement() {
    let texture = Texture::load(8, 4, &plain(8, 4)).expect("texture valide");
    let mut expected = 0;
    for level in 0..texture.level_count() {
        let (w, h) = texture.level_size(level);
        assert_eq!(texture.levels[level].offset, expected);
        expected += w * h;
    }
    assert_eq!(texture.texels.len(), expected as usize);
}

/// Le plus grand côté admis tient dans le tableau de niveaux de taille fixe.
/// Une chaîne d'un niveau de trop écrirait hors de ce tableau.
#[test]
fn le_plus_grand_cote_tient_dans_le_tableau_de_niveaux() {
    let texture =
        Texture::load(MAX_TEXTURE_SIZE, 1, &plain(MAX_TEXTURE_SIZE, 1)).expect("texture valide");
    assert_eq!(texture.level_count(), MAX_LEVELS);
}

/// Le centre du texel `x`, en 16.16.
fn center(x: i32) -> i32 {
    (x << UV_BITS) + HALF_TEXEL
}

/// Quatre texels dont aucun canal ne coïncide : un mélange qui confondrait deux
/// canaux ou deux voisins rendrait une valeur qu'aucun d'eux ne porte.
fn quartet() -> Texture {
    Texture::load(
        2,
        2,
        &block(2, 2, |x, y| match (x, y) {
            (0, 0) => [0x00, 0x10, 0x20, 0x30],
            (1, 0) => [0x40, 0x50, 0x60, 0x70],
            (0, 1) => [0x80, 0x90, 0xA0, 0xB0],
            _ => [0xC0, 0xD0, 0xE0, 0xFF],
        }),
    )
    .expect("texture valide")
}

/// Au centre d'un texel, le bilinéaire rend ce texel et rien d'autre.
///
/// C'est ce que le recentrage d'un demi-texel achète : sans lui, le centre du
/// texel tomberait à la frontière de quatre voisins et toute la texture
/// glisserait d'un demi-texel au changement de filtre.
#[test]
fn au_centre_d_un_texel_le_bilineaire_le_rend_seul() {
    let texture = quartet();
    for y in 0..2 {
        for x in 0..2 {
            assert_eq!(
                texture.bilinear(0, center(x), center(y)),
                texture.texel(0, x, y),
                "texel ({x}, {y})"
            );
        }
    }
}

/// À mi-chemin de deux voisins, chaque canal rend leur moyenne.
#[test]
fn a_mi_chemin_chaque_canal_rend_la_moyenne() {
    let texture = quartet();
    let milieu = texture.bilinear(0, 1 << UV_BITS, center(0));

    // (a · 128 + b · 128 + 128) >> 8 sur chaque canal des texels (0,0) et (1,0).
    assert_eq!(milieu.to_le_bytes(), [0x20, 0x30, 0x40, 0x50]);
}

/// Au centre exact des quatre, les quatre pèsent pareil.
#[test]
fn au_centre_des_quatre_ils_pesent_pareil() {
    let texture = quartet();
    let centre = texture.bilinear(0, 1 << UV_BITS, 1 << UV_BITS);

    // Moyenne des quatre, canal par canal. L'alpha ne tombe pas rond — 0x30,
    // 0x70, 0xB0 et 0xFF ne progressent pas régulièrement — et c'est lui qui
    // montre que l'arrondi va bien au plus proche : 147,75 rend 148.
    assert_eq!(centre.to_le_bytes(), [0x60, 0x70, 0x80, 0x94]);
}

/// Le repli par masque vaut pour les quatre voisins, pas seulement pour le
/// texel de base : une surface pavée mélange son dernier texel avec le premier,
/// sinon une couture apparaîtrait à chaque répétition.
#[test]
fn les_voisins_se_replient_comme_le_texel_de_base() {
    let texture = quartet();
    let a = texture.bilinear(0, 2 << UV_BITS, center(0));
    let b = texture.bilinear(0, 0, center(0));

    // Au-delà du bord droit, le voisin est la colonne 0 ; un demi-texel avant
    // l'origine, c'est la colonne 1. Les deux mélangent la même paire.
    assert_eq!(a.to_le_bytes(), [0x20, 0x30, 0x40, 0x50]);
    assert_eq!(b, a);
}

/// Une coordonnée négative descend vers le texel inférieur, comme les autres.
///
/// C'est le décalage arithmétique qui l'obtient ; une division tronquerait vers
/// zéro et doublerait un texel de part et d'autre de l'origine.
#[test]
fn une_coordonnee_negative_replie_du_bon_cote() {
    let texture = quartet();

    assert_eq!(
        texture.bilinear(0, center(-1), center(0)),
        texture.texel(0, 1, 0)
    );
    assert_eq!(
        texture.bilinear(0, center(-2), center(0)),
        texture.texel(0, 0, 0)
    );
}

/// Le poids ne vaut jamais 256 : juste avant le centre du voisin, il manque
/// une marche.
///
/// Mesurée sur le pire écart possible, du noir au blanc, où elle vaut une unité
/// sur 255 ; un écart ordinaire la voit absorbée par l'arrondi. Le centre du
/// voisin, lui, rend la valeur exacte — c'est ce que garantit le recentrage.
#[test]
fn le_poids_maximal_laisse_une_marche() {
    let contraste = Texture::load(
        2,
        1,
        &block(2, 1, |x, _| if x == 0 { [0; 4] } else { [0xFF; 4] }),
    )
    .expect("texture valide");

    let avant = contraste.bilinear(0, center(1) - 1, center(0));

    assert_eq!(avant.to_le_bytes(), [0xFE; 4]);
    assert_eq!(
        contraste.bilinear(0, center(1), center(0)).to_le_bytes(),
        [0xFF; 4]
    );
}

/// Un niveau au-delà de la chaîne se borne comme pour [`Texture::texel`].
#[test]
fn le_bilineaire_borne_le_niveau_comme_le_texel() {
    let texture = quartet();
    let last = texture.level_count() - 1;

    assert_eq!(
        texture.bilinear(last + 5, center(0), center(0)),
        texture.bilinear(last, center(0), center(0))
    );
}
