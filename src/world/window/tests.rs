// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La fenêtre se teste sans rendu, sans cellule et sans contexte : elle ne prend
//! que des points, une pose de caméra et un cadrage, et rend un rectangle. C'est
//! ce qui la rend vérifiable — un trou dans l'image serait autrement le seul
//! symptôme d'une réduction fautive, et il ne se lit pas dans une empreinte.

use super::*;
use crate::math::{Angle, Quat};
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

/// `reduce` vue comme avant l'octogone : un rectangle en entrée, un en sortie.
///
/// **Elle masque délibérément [`super::reduce`]** dans ce module. La plupart des
/// cas d'ici éprouvent ce que la fenêtre borne — un rectangle de pixels —, et
/// non la forme qui le transporte : les réécrire pour convertir à chaque appel
/// n'aurait rien ajouté à ce qu'ils vérifient. Ceux qui éprouvent l'octogone
/// lui-même appellent `super::reduce` par son chemin.
fn reduce(window: Rect, points: &[Vec3], view: Affine3, projection: &Projection) -> Rect {
    super::reduce(Window::of(window), points, view, projection).to_rect()
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

/// Une fente à `x = distance`, allongée en `y` et étroite en `z`.
///
/// **Le mauvais cas du rectangle**, et le seul que `facing` ne donne pas : plus
/// une ouverture est allongée, plus son rectangle englobant s'écarte d'elle dès
/// qu'elle penche à l'écran. Un carré vu à quarante-cinq degrés de roulis coûte
/// deux fois son aire ; une fente de rapport huit, bien davantage.
fn slit(distance: f32, half_y: f32, half_z: f32) -> [Vec3; 4] {
    [
        Vec3::new(distance, -half_y, -half_z),
        Vec3::new(distance, half_y, -half_z),
        Vec3::new(distance, half_y, half_z),
        Vec3::new(distance, -half_y, half_z),
    ]
}

/// La pose d'une caméra à l'origine, regardant `+X`, roulée de `degrees`.
///
/// Le roulis tourne autour de l'axe du regard, donc il ne change pas ce que la
/// caméra pointe : c'est ce qui rend la comparaison honnête — à roulis nul et à
/// quarante-cinq degrés, la même ouverture est vue depuis le même endroit.
fn rolled(degrees: f32) -> Affine3 {
    Camera {
        position: Vec3::ZERO,
        orientation: Quat::from_axis_angle(
            Vec3::new(1.0, 0.0, 0.0),
            Angle::from_radians(degrees.to_radians()),
        ),
        fov_y: core::f32::consts::FRAC_PI_2,
        near: 0.1,
    }
    .view()
}

/// **Le roulis ne doit pas élargir la fenêtre d'une ouverture.**
///
/// Un roulis tourne la caméra autour de son regard : la même ouverture est vue
/// du même endroit, et son image a la même aire — seule son orientation à
/// l'écran change. Une fenêtre qui gonfle alors ne borne plus l'ouverture mais
/// le rectangle qui la contient, et ce qu'elle laisse passer en trop est le
/// liseré où naissent les portails faussement visibles.
///
/// **Les deux seuils sont mesurés, et le rectangle échoue aux deux.** Avec lui,
/// l'aire passait de 2 208 à 6 400 sur une fente de rapport quatre penchée de
/// quarante-cinq degrés, et de 1 104 à 5 184 sur une fente de rapport huit —
/// **4,7 fois ce que l'ouverture laisse voir**. L'octogone rend 1,00 et 1,01 aux
/// mêmes points, et au pire 2,47 à quinze et trente degrés.
///
/// D'où la forme de ce cas, qui dit la propriété plutôt qu'un chiffre rond :
/// **exact quand l'ouverture s'aligne sur une diagonale** — c'est la raison même
/// de ces quatre normales —, et **jamais plus du double et demi** ailleurs, où
/// un octogone à normales figées ne peut pas suivre une orientation
/// quelconque. Entre les deux, à vingt-deux degrés et demi, se trouve son pire
/// cas, et il reste sous la moitié de ce que coûtait le rectangle.
#[test]
fn le_roulis_n_elargit_pas_la_fenetre() {
    let depart = Window::of(full());
    for (rapport, points) in [
        (1, slit(4.0, 1.0, 1.0)),
        (4, slit(4.0, 1.0, 0.25)),
        (8, slit(4.0, 1.0, 0.125)),
    ] {
        // `super::reduce` et non l'aide locale : c'est l'aire de l'octogone
        // qu'on compare, et son rectangle englobant ne dirait rien — il est le
        // même avec ou sans les quatre bords obliques.
        let droite = super::reduce(depart, &points, neutral(), &projection()).area();

        let alignee = super::reduce(depart, &points, rolled(45.0), &projection()).area();
        assert!(
            alignee * 100 <= droite * 105,
            "fente 1:{rapport} à 45° : {alignee} contre {droite} de face, \
             alors que la diagonale l'épouse"
        );

        for degres in [15.0f32, 30.0] {
            let penchee = super::reduce(depart, &points, rolled(degres), &projection()).area();
            assert!(
                penchee * 2 <= droite * 5,
                "fente 1:{rapport} à {degres}° : {penchee} contre {droite} de face"
            );
        }
    }
}

/// **Une branche que le rectangle propageait, l'octogone la coupe.**
///
/// C'est le gain tout entier, et il ne se lit nulle part ailleurs : une fenêtre
/// plus serrée ne change aucune image — le remplissage reçoit le même rectangle
/// — mais elle vide la réduction du portail suivant, et la cellule derrière lui
/// n'est jamais ramenée.
///
/// La chaîne reproduit le mauvais cas en deux portails : une fente de rapport
/// huit penchée de quarante-cinq degrés, puis une petite ouverture placée dans
/// un coin de son rectangle englobant. Mesuré sur ce couple, l'octogone couvre
/// 267 493 sous-pixels carrés contre **1 320 201** pour sa boîte, et les quatre
/// placements se comportent de même.
///
/// [`Window::axial_only`] rejoue la fenêtre d'avant l'octogone, sur ce code et
/// ce décor : c'est elle qui fait de ce cas une comparaison et non une
/// affirmation.
#[test]
fn l_octogone_coupe_une_branche_que_le_rectangle_propage() {
    let depart = Window::of(full());
    let fente = slit(4.0, 1.0, 0.125);
    let apres = super::reduce(depart, &fente, rolled(45.0), &projection());

    for (y, z) in [(0.8f32, 0.8f32), (0.8, -0.8), (-0.8, 0.8), (0.5, 0.5)] {
        let second = [
            Vec3::new(8.0, y - 0.1, z - 0.1),
            Vec3::new(8.0, y + 0.1, z - 0.1),
            Vec3::new(8.0, y + 0.1, z + 0.1),
            Vec3::new(8.0, y - 0.1, z + 0.1),
        ];
        assert!(
            super::reduce(apres, &second, rolled(45.0), &projection()).is_empty(),
            "l'octogone a propagé la branche en ({y}, {z})"
        );
        assert!(
            !super::reduce(apres.axial_only(), &second, rolled(45.0), &projection()).is_empty(),
            "le rectangle la coupait déjà en ({y}, {z}) : ce cas ne compare plus rien"
        );
    }
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

/// **Un portail que le plan proche atteint laisse voir, et largement.**
///
/// Le cas d'un intégrateur, ramené au seul calcul qui le décide : en marchant, une
/// bande du champ se vidait de tout décor dès que l'œil approchait du plan d'un
/// portail, et le phénomène suivait `near` — vingt-neuf épisodes à 0,1, aucun à
/// 0,001. Descendre n'était pas une sortie, la profondeur se rangeant en `near/w`.
///
/// Ce que la géométrie impose : plus l'œil est près du plan d'un portail, **plus
/// large** est ce qu'on voit à travers, jusqu'à l'écran entier quand on le
/// franchit. Une fenêtre qui se referme là est donc fausse dans le sens le pire,
/// celui qui troue l'image.
///
/// Les trois distances encadrent la tangence au plan proche, où le polygone
/// découpé dégénère : c'est à quelques millièmes d'unité que tout se joue.
#[test]
fn un_portail_au_plan_proche_laisse_voir_largement() {
    for distance in [0.15f32, 0.1, 0.05] {
        let window = reduce(full(), &facing(distance, 2.0), neutral(), &projection());

        assert!(
            window.width > 0 && window.height > 0,
            "distance {distance} : la fenêtre s'est refermée sur un portail \
             qu'on est en train de franchir"
        );
        assert_eq!(
            window.width, WIDTH,
            "distance {distance} : à cette distance le portail couvre l'écran"
        );
    }
}

/// **Un portail dont un bord seulement passe le plan proche ne réduit rien non
/// plus**, et c'est le second régime du même défaut.
///
/// Celui-là est le plus trompeur des deux : le découpage ne vide pas la fenêtre,
/// il l'**ampute** du côté qui a été emporté. La traversée garde donc la cellule
/// d'en face, mais par une fenêtre trop étroite — une bande du champ se vide au
/// lieu d'un pan entier, et c'est exactement ce qu'un intégrateur décrit en
/// parlant d'« une bande dont la largeur suit l'orientation de la caméra ».
///
/// Ce que ses mesures ont apporté : des épisodes relevés **au-delà** de `near`,
/// jusqu'à 0,110 pour un plan proche de 0,1. Un portail vu de face ne peut pas les
/// produire — il est tout entier d'un côté —, mais un portail vu de biais, si : un
/// de ses bords passe le plan quand son centre ne l'a pas encore atteint.
///
/// Ce qui est visible à travers la part trop proche n'est pas rien : elle est en
/// deçà du plan de projection, donc ce qu'elle cache occupe l'écran sans borne. La
/// seule réponse sûre est de rendre la fenêtre reçue.
#[test]
fn un_portail_de_biais_au_plan_proche_ne_reduit_rien() {
    // Le bord gauche est à `0.05` de l'œil, donc en deçà du plan proche ; le
    // droit à trois unités, donc bien au-delà. Un plan, et convexe.
    let oblique = [
        Vec3::new(0.05, -2.0, -2.0),
        Vec3::new(3.0, 2.0, -2.0),
        Vec3::new(3.0, 2.0, 2.0),
        Vec3::new(0.05, -2.0, 2.0),
    ];
    let window = reduce(full(), &oblique, neutral(), &projection());

    assert_eq!(
        window.width, WIDTH,
        "un bord passé le plan proche ampute la fenêtre : {window:?}"
    );
}

/// **Un portail dont un bord est passé derrière l'œil ne réduit rien non plus**,
/// et c'est le cas que la première clause manquait.
///
/// L'œil presque dans le plan d'un portail en voit un bord devant lui et l'autre
/// **derrière** : celui-là a une profondeur négative, et une clause qui exige un
/// sommet *devant* l'œil en deçà du plan proche ne le voit pas. Le découpage
/// ampute alors la fenêtre comme si rien n'avait été corrigé.
///
/// Mesuré chez un intégrateur après la 0.8.5 : le régime de face réglé, le régime
/// de biais **inchangé au pixel près** sur cinq écarts au plan. Le cas précédent
/// de ce fichier plaçait son bord proche à `0,05`, donc devant l'œil, et ne
/// couvrait qu'une moitié du régime qu'il prétendait garder.
#[test]
fn un_portail_dont_un_bord_est_derriere_l_oeil_ne_reduit_rien() {
    // Le bord gauche est à `-0.5`, derrière l'œil ; le droit à trois unités
    // devant. L'œil est donc presque dans le plan du portail.
    let across = [
        Vec3::new(-0.5, -2.0, -2.0),
        Vec3::new(3.0, 2.0, -2.0),
        Vec3::new(3.0, 2.0, 2.0),
        Vec3::new(-0.5, -2.0, 2.0),
    ];
    let window = reduce(full(), &across, neutral(), &projection());

    assert_eq!(
        window.width, WIDTH,
        "un bord derrière l'œil ampute encore la fenêtre : {window:?}"
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
///
/// **C'est ce qui arrête la traversée**, et non un cas dégénéré sans
/// conséquence : la réduction est appelée en chaîne, un portail après l'autre,
/// et une fenêtre vide qui reprendrait de la largeur ferait repartir la
/// descente dans des cellules que le portail précédent avait déjà écartées.
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
    let window = Window::of(Rect {
        x: 2,
        y: 3,
        width: 4,
        height: 5,
    });
    // L'aller-retour est exact : un décalage d'un seul sous-pixel retirerait ou
    // ajouterait une colonne entière de pixels, et c'est ainsi qu'une réduction
    // troue l'image ou déborde de la fenêtre reçue.
    assert_eq!(
        window.to_rect(),
        Rect {
            x: 2,
            y: 3,
            width: 4,
            height: 5,
        }
    );
}

/// **Un rectangle devient un octogone sans rien élargir.**
///
/// Ses diagonales passent par ses coins, donc elles ne coupent rien : l'aire de
/// l'octogone est celle de la boîte. C'est ce qui permet à la fenêtre de départ
/// d'être l'image entière sans cas particulier, et ce qui rend la conversion
/// sûre dans ce sens — l'autre, lui, perd les coins coupés.
#[test]
fn un_rectangle_devient_un_octogone_qui_ne_coupe_rien() {
    let window = Window::of(Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    });
    let cote = 4 * 16;
    assert_eq!(window.area(), cote * cote, "l'octogone a coupé un coin");
}

/// Des bornes que rien n'a remplies rendent un rectangle vide.
#[test]
fn des_bornes_vides_rendent_un_rectangle_vide() {
    assert_eq!(Window::EMPTY.to_rect().width, 0);
}

/// Une borne négative se ramène au bord de l'image sans passer par un entier
/// non signé.
///
/// La conversion `as` sur un négatif rendrait un nombre immense, et la fenêtre
/// couvrirait l'image entière au lieu de sa part gauche : le bornage est écrit.
#[test]
fn une_borne_negative_se_ramene_au_bord() {
    let mut window = Window::EMPTY;
    window.add(-100, -100);
    window.add(160, 160);
    let rect = window.to_rect();
    assert_eq!(rect.x, 0);
    assert_eq!(rect.y, 0);
    assert!(rect.width <= 12, "largeur inattendue : {}", rect.width);
}
