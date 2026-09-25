// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La fenêtre se teste sans rendu, sans cellule et sans contexte : elle ne prend
//! que des points, une pose de caméra et un cadrage, et rend un rectangle. C'est
//! ce qui la rend vérifiable — un trou dans l'image serait autrement le seul
//! symptôme d'une réduction fautive, et il ne se lit pas dans une empreinte.

use super::*;
use crate::math::Quat;
use crate::scene::Camera;

/// La largeur de l'image des tests.
const WIDTH: u32 = 640;
/// Sa hauteur.
const HEIGHT: u32 = 360;

/// Le cadrage des tests : quatre-vingt-dix degrés de champ vertical.
fn projection() -> Projection {
    Projection::new(WIDTH, HEIGHT, core::f32::consts::FRAC_PI_2, 0.1).expect("cadrage valide")
}

/// La pose d'une caméra à l'origine, regardant le `+X` du monde.
///
/// Elle passe par [`Camera::view`] et non par une matrice écrite ici : la
/// permutation des axes de vue y vit, et la reconstruire à la main donnerait une
/// vue qui regarde ailleurs — les portails tomberaient derrière le plan proche
/// sans que rien ne l'explique.
fn neutral() -> Affine3 {
    Camera {
        position: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        fov_y: core::f32::consts::FRAC_PI_2,
        near: 0.1,
    }
    .view()
}

/// La fenêtre de départ : l'image entière.
fn full() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: WIDTH,
        height: HEIGHT,
    }
}

/// Un carré à `x = distance`, de demi-côté `half`, dans le plan perpendiculaire
/// au regard.
fn facing(distance: f32, half: f32) -> [Vec3; 4] {
    [
        Vec3::new(distance, -half, -half),
        Vec3::new(distance, half, -half),
        Vec3::new(distance, half, half),
        Vec3::new(distance, -half, half),
    ]
}

/// Un portail vu de face occupe un rectangle centré, et la fenêtre s'y réduit.
#[test]
fn un_portail_de_face_reduit_la_fenetre_a_son_rectangle() {
    let window = reduce(full(), &facing(4.0, 1.0), neutral(), &projection());

    assert!(
        window.width > 0 && window.height > 0,
        "le portail est visible"
    );
    assert!(
        window.width < WIDTH,
        "la fenêtre s'est resserrée en largeur"
    );
    assert!(window.height < HEIGHT, "et en hauteur");

    // Le portail est centré : la fenêtre doit l'être aussi, à un pixel près de
    // dilatation de chaque côté.
    let left = window.x;
    let right = WIDTH - (window.x + window.width);
    assert!(
        left.abs_diff(right) <= 2,
        "fenêtre décentrée : {left} à gauche, {right} à droite"
    );
}

/// Un portail plus grand que l'image ne réduit rien.
///
/// Le cas compte parce qu'il est celui de la cellule où vit la caméra : ses
/// portails débordent de l'écran, et une réduction qui mordrait ici trouerait
/// l'image dès le premier pas.
#[test]
fn un_portail_qui_contient_l_image_ne_la_reduit_pas() {
    let window = reduce(full(), &facing(1.0, 100.0), neutral(), &projection());
    assert_eq!(window, full());
}

/// Un portail derrière la caméra ne laisse rien voir.
#[test]
fn un_portail_derriere_la_camera_vide_la_fenetre() {
    let window = reduce(full(), &facing(-4.0, 1.0), neutral(), &projection());
    assert_eq!(window.width, 0, "la branche doit s'arrêter");
}

/// Un portail entièrement hors de la fenêtre reçue la vide.
///
/// La fenêtre reçue est ici la moitié gauche de l'image, et le portail se
/// projette à droite : c'est le cas que la traversée rencontre dès qu'une porte
/// s'ouvre du mauvais côté d'une autre.
#[test]
fn un_portail_hors_de_la_fenetre_la_vide() {
    let half = Rect {
        x: 0,
        y: 0,
        width: WIDTH / 4,
        height: HEIGHT,
    };
    // Le monde est en main droite, Z en haut, et le `−Y` de vue est la droite de
    // l'écran : un portail en `y` négatif se projette donc à droite.
    let right = [
        Vec3::new(4.0, -3.0, -1.0),
        Vec3::new(4.0, -1.5, -1.0),
        Vec3::new(4.0, -1.5, 1.0),
        Vec3::new(4.0, -3.0, 1.0),
    ];
    let window = reduce(half, &right, neutral(), &projection());
    assert_eq!(window.width, 0, "rien de ce portail n'est dans la fenêtre");
}

/// Une réduction ne peut jamais élargir la fenêtre reçue.
///
/// C'est la propriété dont dépend la terminaison de la traversée, et elle se
/// vérifie sur des portails obliques — ceux dont la boîte est la plus lâche.
#[test]
fn la_reduction_est_monotone() {
    let start = Rect {
        x: 100,
        y: 50,
        width: 200,
        height: 150,
    };
    // Trois orientations obliques : la boîte d'un portail de biais est plus large
    // que sa projection, et c'est là qu'un débordement se verrait.
    for angle in [0.0f32, 0.3, 0.8] {
        let oblique = [
            Vec3::new(4.0, -1.0, -1.0),
            Vec3::new(4.0 + angle * 4.0, 1.0, -1.0),
            Vec3::new(4.0 + angle * 4.0, 1.0, 1.0),
            Vec3::new(4.0, -1.0, 1.0),
        ];
        let window = reduce(start, &oblique, neutral(), &projection());
        assert!(
            window.x >= start.x && window.y >= start.y,
            "la fenêtre a débordé en haut ou à gauche pour {angle}"
        );
        if window.width > 0 {
            assert!(
                window.x + window.width <= start.x + start.width
                    && window.y + window.height <= start.y + start.height,
                "la fenêtre a débordé en bas ou à droite pour {angle}"
            );
        }
    }
}

/// Réduire deux fois par le même portail rend la même fenêtre.
///
/// L'idempotence n'est pas une élégance : la traversée peut atteindre une cellule
/// par deux chemins, et une réduction qui rétrécirait à chaque passage ferait
/// dépendre l'image de l'ordre de parcours.
#[test]
fn reduire_deux_fois_par_le_meme_portail_est_stable() {
    let portal = facing(4.0, 1.0);
    let once = reduce(full(), &portal, neutral(), &projection());
    let twice = reduce(once, &portal, neutral(), &projection());
    assert_eq!(once, twice);
}

/// Un portail dégénéré — moins de trois points — vide la fenêtre.
///
/// Le chargement refuse déjà un tel portail ; la fonction ne s'en remet pas à
/// lui, parce qu'elle est appelable depuis la traversée comme depuis un test.
#[test]
fn un_portail_sans_surface_vide_la_fenetre() {
    let line = [Vec3::new(4.0, -1.0, 0.0), Vec3::new(4.0, 1.0, 0.0)];
    assert_eq!(reduce(full(), &line, neutral(), &projection()).width, 0);
}

/// Une fenêtre déjà vide le reste.
#[test]
fn une_fenetre_vide_reste_vide() {
    let empty = Rect {
        x: 10,
        y: 10,
        width: 0,
        height: 20,
    };
    assert_eq!(
        reduce(empty, &facing(4.0, 1.0), neutral(), &projection()).width,
        0
    );
}

/// La fenêtre d'un portail lointain est plus petite que celle du même portail
/// vu de près.
///
/// C'est la propriété qui fait décroître le coût avec la profondeur, et le
/// chiffrage de la réduction en dépend : sans elle, une enfilade coûterait sa
/// longueur au lieu de s'éteindre.
#[test]
fn un_portail_lointain_reduit_davantage() {
    let near = reduce(full(), &facing(2.0, 1.0), neutral(), &projection());
    let far = reduce(full(), &facing(8.0, 1.0), neutral(), &projection());
    assert!(
        far.width < near.width && far.height < near.height,
        "près {}×{}, loin {}×{}",
        near.width,
        near.height,
        far.width,
        far.height
    );
}

/// Un angle de champ plus large rend une fenêtre plus petite pour le même
/// portail.
///
/// Le cadrage entre dans la réduction, et ce test le fixe : une fenêtre calculée
/// avec une autre projection que celle de l'image serait fausse sans qu'aucune
/// empreinte ne bouge, le portail restant au bon endroit.
#[test]
fn le_cadrage_entre_dans_la_reduction() {
    let portal = facing(4.0, 1.0);
    let narrow = Projection::new(WIDTH, HEIGHT, 0.6, 0.1).expect("cadrage valide");
    let wide = Projection::new(WIDTH, HEIGHT, 1.4, 0.1).expect("cadrage valide");

    let by_narrow = reduce(full(), &portal, neutral(), &narrow);
    let by_wide = reduce(full(), &portal, neutral(), &wide);
    assert!(by_wide.width < by_narrow.width);
}

/// La fenêtre vient du portail **découpé par elle**, et non de l'intersection de
/// sa boîte avec elle.
///
/// C'est le seul test qui distingue les deux, et sans lui tout le découpage de
/// [`accumulate`] pourrait être remplacé par une intersection de rectangles sans
/// qu'aucun test ne rougisse. Le cas est celui que le chiffrage avait isolé : un
/// grand portail oblique regardé à travers une ouverture étroite. Ce qu'il gagne
/// n'est pas du remplissage — c'est le liseré où un portail paraît visible sans
/// l'être, donc les cellules que la traversée ramène pour rien.
#[test]
fn la_fenetre_decoupe_le_portail_avant_de_le_borner() {
    // Un triangle dont la projection couvre la moitié basse de l'image : son
    // sommet au centre, sa base aux deux coins du bas. Sa boîte est donc large,
    // et sa surface réelle ne l'est pas.
    let oblique = [
        Vec3::new(4.0, 0.0, 0.0),
        Vec3::new(4.0, 7.111, -4.0),
        Vec3::new(4.0, -7.111, -4.0),
    ];
    // Une ouverture dans le coin bas gauche, là où l'arête du triangle traverse.
    let narrow = Rect {
        x: 0,
        y: 300,
        width: 80,
        height: 60,
    };

    let by_box = reduce(full(), &oblique, neutral(), &projection()).intersect(narrow);
    let by_clip = reduce(narrow, &oblique, neutral(), &projection());

    assert!(by_clip.height > 0, "le portail traverse bien l'ouverture");
    assert!(
        by_clip.height < by_box.height,
        "le découpage n'a rien resserré : {} contre {}",
        by_clip.height,
        by_box.height
    );
}

/// Les bornes en sous-pixels d'un rectangle prennent le dernier sous-pixel du
/// dernier pixel.
///
/// Un décalage d'un seul sous-pixel ici retirerait une colonne entière de pixels
/// à la fenêtre, et c'est exactement la façon dont une réduction troue l'image.
#[test]
fn les_bornes_couvrent_le_dernier_pixel() {
    let bounds = Bounds::of(Rect {
        x: 2,
        y: 3,
        width: 4,
        height: 5,
    });
    assert_eq!(bounds.min_x, 32);
    assert_eq!(bounds.max_x, 6 * 16 - 1);
    assert_eq!(bounds.min_y, 48);
    assert_eq!(bounds.max_y, 8 * 16 - 1);
}

/// Des bornes que rien n'a remplies rendent un rectangle vide.
#[test]
fn des_bornes_vides_rendent_un_rectangle_vide() {
    assert_eq!(Bounds::EMPTY.to_rect().width, 0);
}

/// Une borne négative se ramène au bord de l'image sans passer par un entier
/// non signé.
///
/// La conversion `as` sur un négatif rendrait un nombre immense, et la fenêtre
/// couvrirait l'image entière au lieu de sa part gauche : le bornage est écrit.
#[test]
fn une_borne_negative_se_ramene_au_bord() {
    let mut bounds = Bounds::EMPTY;
    bounds.add(-100, -100);
    bounds.add(160, 160);
    let rect = bounds.to_rect();
    assert_eq!(rect.x, 0);
    assert_eq!(rect.y, 0);
    assert!(rect.width <= 12, "largeur inattendue : {}", rect.width);
}
