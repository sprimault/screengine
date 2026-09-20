// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que le contexte accepte, et ce qu'il refuse.
//!
//! Les refus comptent autant que le cas nominal : une configuration invalide qui
//! passerait donnerait des tampons incohérents, et le défaut ne se verrait qu'au
//! premier remplissage.

use alloc::vec;

use super::*;

/// Une configuration qui passe, dont les tests dérivent leurs variantes.
fn sane() -> Config {
    Config {
        max_width: 640,
        max_height: 360,
        width: 640,
        height: 360,
        tile_size: 64,
    }
}

/// Le cas nominal, qui garde les tests de refus honnêtes : sans lui, une
/// validation qui refuserait tout passerait tous les autres.
#[test]
fn accepte_une_configuration_saine() {
    let ctx = Context::new(sane()).expect("configuration saine");
    assert_eq!(ctx.resolution(), (640, 360));
}

/// Une largeur nulle donnerait des tampons vides et une boucle de
/// remplissage qui ne s'exécute jamais, sans que rien ne le signale.
#[test]
fn refuse_une_dimension_nulle() {
    let mut config = sane();
    config.width = 0;
    assert_eq!(
        Context::new(config).unwrap_err(),
        Error::InvalidArgument(Argument::Resolution)
    );
}

/// Les tampons sont dimensionnés pour le maximum : une résolution initiale
/// au-delà déborderait dès la première image.
#[test]
fn refuse_une_resolution_initiale_au_dela_du_maximum() {
    let mut config = sane();
    config.width = config.max_width + 1;
    assert_eq!(
        Context::new(config).unwrap_err(),
        Error::InvalidArgument(Argument::Resolution)
    );
}

/// La borne n'est pas un confort : au-delà, les pires cas des formats en
/// virgule fixe cessent d'être vrais et une fonction de bord déborde avant
/// que quoi que ce soit d'autre ne le signale.
#[test]
fn refuse_un_maximum_au_dela_de_la_borne_des_formats() {
    let mut config = sane();
    config.max_width = MAX_RESOLUTION + 1;
    config.width = MAX_RESOLUTION + 1;
    assert_eq!(
        Context::new(config).unwrap_err(),
        Error::InvalidArgument(Argument::Resolution)
    );
}

/// Une taille intermédiaire compilerait et rendrait une image : elle se
/// refuse ici, parce qu'aucun chemin de rendu ne sera écrit pour elle.
#[test]
fn refuse_une_taille_de_tuile_hors_liste() {
    let mut config = sane();
    config.tile_size = 48;
    assert_eq!(
        Context::new(config).unwrap_err(),
        Error::InvalidArgument(Argument::TileSize)
    );
}

/// Le premier des trois messages d'erreur du premier jour. La frontière C
/// ne peut pas faire ce contrôle, faute de recevoir la longueur du tampon.
#[test]
fn refuse_un_stride_plus_court_que_la_largeur() {
    let mut ctx = Context::new(sane()).expect("configuration saine");
    let mut pixels = [0u8; 4];
    assert_eq!(
        ctx.frame_end(&mut pixels, 639).unwrap_err(),
        Error::InvalidArgument(Argument::Stride)
    );
}

/// Un appelant Rust porte la longueur avec sa tranche : c'est le seul
/// contrôle qui distingue ce chemin de celui de la frontière C.
#[test]
fn refuse_un_tampon_trop_court_pour_son_stride() {
    let mut ctx = Context::new(sane()).expect("configuration saine");
    let mut pixels = [0u8; 4];
    assert_eq!(
        ctx.frame_end(&mut pixels, 640).unwrap_err(),
        Error::InvalidArgument(Argument::BufferLength)
    );
}

/// Un contexte de quoi rendre sans y consacrer un mégaoctet.
fn small() -> Context {
    Context::new(Config {
        max_width: 64,
        max_height: 64,
        width: 64,
        height: 64,
        tile_size: 32,
    })
    .expect("configuration saine")
}

/// Un triangle devant une caméra neutre, à dix unités vers l'est.
///
/// Sa face avant regarde la caméra : pris dans l'autre sens, il serait éliminé
/// comme dos de face et chaque test qui compte les triangles préparés
/// mesurerait l'élimination au lieu de ce qu'il croit mesurer.
fn ahead() -> [Vec3; 3] {
    [
        Vec3::new(10.0, -2.0, -2.0),
        Vec3::new(10.0, 0.0, 2.0),
        Vec3::new(10.0, 2.0, -2.0),
    ]
}

/// Le même, derrière elle.
fn behind() -> [Vec3; 3] {
    ahead().map(|v| Vec3::new(-v.x, v.y, v.z))
}

/// Un lot d'un seul triangle, de couleur quelconque.
fn one() -> [Triangle; 1] {
    [Triangle {
        indices: [0, 1, 2],
        color: Color::new(0xFF, 0xFF, 0xFF, 0xFF),
    }]
}

/// Une translation pure.
fn moved(x: f32) -> Affine3 {
    Affine3::from_rotation_translation(crate::math::Quat::IDENTITY, Vec3::new(x, 0.0, 0.0))
}

/// La matrice de soumission porte l'objet vers le monde, et le noyau y ajoute
/// la vue. Prise à l'envers — ou appliquée après la vue —, elle emmènerait le
/// triangle dans la direction opposée.
#[test]
fn la_matrice_de_soumission_porte_l_objet_dans_le_monde() {
    let mut ctx = small();
    ctx.submit(Affine3::IDENTITY, &behind(), &one())
        .expect("capacité");
    assert_eq!(ctx.triangles.len(), 0, "derrière la caméra");

    ctx.submit(moved(20.0), &behind(), &one())
        .expect("capacité");
    assert_eq!(ctx.triangles.len(), 1, "ramené devant elle");
}

/// La caméra se déplace dans le monde, et non l'inverse : la reculer fait
/// entrer dans le champ ce qui était derrière.
#[test]
fn deplacer_la_camera_deplace_le_point_de_vue() {
    let mut ctx = small();
    ctx.set_camera(Camera {
        position: Vec3::new(-20.0, 0.0, 0.0),
        ..Camera::DEFAULT
    })
    .expect("caméra valide");
    ctx.submit(Affine3::IDENTITY, &behind(), &one())
        .expect("capacité");
    assert_eq!(ctx.triangles.len(), 1);
}

/// Un lot est accepté ou refusé en entier : le triangle qui précède l'indice
/// fautif ne reste pas dans l'image.
///
/// Sans cela, un hôte qui se trompe d'indice obtient un mur dont il manque la
/// moitié, et le code de retour ne dit pas où la coupure est tombée.
#[test]
fn un_lot_refuse_ne_laisse_rien_derriere_lui() {
    let mut ctx = small();
    let triangles = [
        one()[0],
        Triangle {
            indices: [0, 1, 3],
            color: Color::new(0, 0, 0, 0xFF),
        },
    ];
    assert_eq!(
        ctx.submit(Affine3::IDENTITY, &ahead(), &triangles),
        Err(Error::InvalidArgument(Argument::VertexIndex))
    );
    assert_eq!(ctx.triangles.len(), 0);
}

/// Un sommet qu'aucune projection ne peut porter fait disparaître son triangle
/// sans erreur : c'est une donnée, pas un défaut du moteur.
#[test]
fn un_sommet_demesure_disparait_sans_erreur() {
    let mut ctx = small();
    let mut vertices = ahead();
    vertices[1].y = f32::INFINITY;
    assert_eq!(ctx.submit(Affine3::IDENTITY, &vertices, &one()), Ok(()));
    assert_eq!(ctx.triangles.len(), 0);
}

/// La fin d'une image périme sa liste de dessin, et c'est la soumission
/// suivante qui la vide.
///
/// Le vidage ne peut pas avoir lieu au début de l'image suivante : un hôte
/// soumet dès le retour de la fin, et sa scène serait jetée sans un mot.
#[test]
fn la_fin_d_image_perime_la_liste_de_dessin() {
    let mut ctx = small();
    let mut pixels = vec![0u8; 64 * 64 * BYTES_PER_PIXEL];
    ctx.submit(Affine3::IDENTITY, &ahead(), &one())
        .expect("capacité");
    ctx.frame_end(&mut pixels, 64).expect("image rendue");

    ctx.submit(Affine3::IDENTITY, &ahead(), &one())
        .expect("capacité");
    assert_eq!(ctx.triangles.len(), 1, "le lot précédent a été vidé");
}

/// Une scène dont tous les triangles sont éliminés reste une scène soumise :
/// la démonstration ne revient pas par-dessus.
///
/// Le défaut qu'un hôte verrait autrement : il soumet un mur qui tourne le dos
/// à la caméra, ne prépare donc aucun triangle, et voit apparaître une scène
/// qu'il n'a jamais décrite.
#[test]
fn une_scene_entierement_eliminee_ne_rappelle_pas_la_demonstration() {
    let mut ctx = small();
    ctx.submit(Affine3::IDENTITY, &behind(), &one())
        .expect("capacité");
    ctx.begin().expect("image commencée");
    assert!(ctx.triangles.is_empty());
}

/// Une image que personne n'a alimentée rend la scène de démonstration.
///
/// Elle disparaîtra avec elle ; jusque-là, c'est ce qui garde une image aux
/// hôtes qui ne soumettent rien.
#[test]
fn une_image_sans_soumission_rend_la_demonstration() {
    let mut ctx = small();
    ctx.begin().expect("image commencée");
    assert!(!ctx.triangles.is_empty());
}

/// Tout ce qui écrit dans l'état du contexte est refusé pendant le rendu :
/// des tuiles le lisent peut-être depuis d'autres threads.
#[test]
fn la_scene_ne_se_change_pas_pendant_le_rendu() {
    let mut ctx = small();
    ctx.begin().expect("image commencée");
    assert_eq!(
        ctx.submit(Affine3::IDENTITY, &ahead(), &one()),
        Err(Error::InvalidState)
    );
    assert_eq!(ctx.set_camera(Camera::DEFAULT), Err(Error::InvalidState));
}
