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
        max_triangles: 0,
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
        max_triangles: 0,
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
///
/// Démesuré mais fini, donc : une coordonnée que la caméra ne peut pas porter
/// aujourd'hui pourrait l'être d'un autre point de vue, et refuser le lot
/// rendrait la scène irrendable selon l'endroit où la caméra se place.
#[test]
fn un_sommet_demesure_disparait_sans_erreur() {
    let mut ctx = small();
    let mut vertices = ahead();
    vertices[1].y = 1.0e30;
    assert_eq!(ctx.submit(Affine3::IDENTITY, &vertices, &one()), Ok(()));
    assert_eq!(ctx.triangles.len(), 0);
}

/// Un sommet non fini refuse le lot entier, lui, et ne laisse rien derrière.
///
/// Le contraire du précédent, et la différence est celle qu'énonce le contrat
/// d'ABI : un `NaN` ou un infini soumis ne dépend ni de la caméra ni de la
/// matrice, c'est une donnée fausse. Le vérifier sur le second sommet, après
/// qu'un triangle valide a déjà été posé, est ce qui attrape un refus qui
/// laisserait le lot à moitié soumis.
#[test]
fn un_sommet_non_fini_refuse_le_lot_entier() {
    for bad in [f32::NAN, f32::INFINITY, -f32::INFINITY] {
        let mut ctx = small();
        let triangles = [one()[0], one()[0]];
        let mut vertices = ahead();
        ctx.submit(Affine3::IDENTITY, &vertices, &one())
            .expect("capacité");

        vertices[1].z = bad;
        assert_eq!(
            ctx.submit(Affine3::IDENTITY, &vertices, &triangles),
            Err(Error::InvalidArgument(Argument::VertexCoordinate)),
            "{bad}"
        );
        assert_eq!(ctx.triangles.len(), 1, "le lot refusé a laissé un triangle");
    }
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

/// Une image que personne n'a alimentée se rend sans erreur, et ne montre que
/// le fond.
///
/// Une scène vide est une scène, pas un appel fautif : c'est ce qu'obtient un
/// hôte qui n'a encore rien à montrer, et il ne doit pas avoir à distinguer ce
/// cas d'un refus.
#[test]
fn une_image_sans_soumission_ne_montre_que_le_fond() {
    let mut ctx = small();
    let mut pixels = vec![0u8; 64 * 64 * BYTES_PER_PIXEL];
    assert_eq!(ctx.begin().map(|_| ()), Ok(()));
    assert!(ctx.triangles.is_empty());
    assert_eq!(ctx.frame_end(&mut pixels, 64), Ok(()));
    assert!(
        pixels
            .chunks_exact(BYTES_PER_PIXEL)
            .all(|p| p == [0, 0, 0, 0xFF])
    );
}

/// La capacité de triangles se choisit à la création, et zéro vaut le défaut.
///
/// Zéro et non un champ absent : c'est un champ qui était réservé dans l'ABI
/// publiée, et un hôte qui passait des zéros doit obtenir le défaut sans rien
/// reprendre.
#[test]
fn la_capacite_de_triangles_se_choisit_a_la_creation() {
    let mut config = sane();
    assert_eq!(config.capacity(), TRIANGLE_CAPACITY);
    config.max_triangles = 3;
    assert_eq!(config.capacity(), 3);

    config.width = 64;
    config.height = 64;
    let mut ctx = Context::new(config).expect("configuration saine");
    let triangles = [one()[0]; 4];
    assert_eq!(
        ctx.submit(Affine3::IDENTITY, &ahead(), &triangles),
        Err(Error::InvalidArgument(Argument::TriangleCapacity))
    );
    assert_eq!(ctx.triangles.len(), 0, "le lot entier est refusé");
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
    assert_eq!(ctx.set_filter(Filter::Bilinear), Err(Error::InvalidState));
}

/// Le filtrage par défaut est le tramage : un contexte qu'on ne configure pas
/// rend ce que la classe de moteurs visée rendait.
#[test]
fn le_filtrage_par_defaut_est_le_tramage() {
    let ctx = small();
    assert_eq!(ctx.filter(), Filter::Dither);
}

/// Le filtre se change entre deux images, et il tient d'une image à l'autre :
/// c'est un réglage du contexte, pas un paramètre de soumission.
#[test]
fn le_filtre_se_change_entre_deux_images_et_tient() {
    let mut ctx = small();
    ctx.set_filter(Filter::Bilinear).expect("hors rendu");
    assert_eq!(ctx.filter(), Filter::Bilinear);

    let mut pixels = vec![0u8; ctx.width as usize * ctx.height as usize * BYTES_PER_PIXEL];
    ctx.frame_end(&mut pixels, ctx.width).expect("image rendue");
    assert_eq!(ctx.filter(), Filter::Bilinear);
}

/// Les sommets de `ahead`, avec les coordonnées de texture qu'on lui donne.
fn ahead_uv(u: f32, v: f32) -> [VertexUv; 3] {
    ahead().map(|position| VertexUv { position, u, v })
}

/// Une soumission texturée pose les mêmes triangles qu'une soumission sans
/// texture : les coordonnées voyagent à côté de la géométrie, elles ne la
/// changent pas.
#[test]
fn une_soumission_texturee_pose_la_meme_geometrie() {
    let mut avec = small();
    let mut sans = small();
    avec.submit_uv(Affine3::IDENTITY, &ahead_uv(4.0, 8.0), &one())
        .expect("capacité");
    sans.submit(Affine3::IDENTITY, &ahead(), &one())
        .expect("capacité");

    assert_eq!(avec.triangles.len(), sans.triangles.len());
    for (a, b) in avec.triangles.iter().zip(&sans.triangles) {
        assert_eq!(a.bounds(), b.bounds());
    }
}

/// Une coordonnée de texture hors borne refuse le lot entier, et ne laisse rien
/// derrière : contrairement à une position que la vue ne peut pas porter, elle
/// ne dépend ni de la caméra ni de la matrice, donc c'est une donnée fausse.
#[test]
fn une_coordonnee_de_texture_hors_borne_refuse_le_lot() {
    for bad in [
        f32::NAN,
        f32::INFINITY,
        -f32::INFINITY,
        MAX_TEXEL_COORD * 1.5,
        -MAX_TEXEL_COORD * 1.5,
    ] {
        let mut ctx = small();
        ctx.submit(Affine3::IDENTITY, &ahead(), &one())
            .expect("capacité");

        let mut vertices = ahead_uv(0.0, 0.0);
        vertices[2].u = bad;
        assert_eq!(
            ctx.submit_uv(Affine3::IDENTITY, &vertices, &one()),
            Err(Error::InvalidArgument(Argument::TextureCoordinate)),
            "{bad}"
        );
        vertices[2].u = 0.0;
        vertices[1].v = bad;
        assert_eq!(
            ctx.submit_uv(Affine3::IDENTITY, &vertices, &one()),
            Err(Error::InvalidArgument(Argument::TextureCoordinate)),
            "{bad}"
        );
        assert_eq!(ctx.triangles.len(), 1, "un lot refusé a laissé un triangle");
    }
}

/// La borne est inclusive : c'est la valeur exacte qui rend le produit par la
/// profondeur juste un bit sous le débordement, et la refuser rétrécirait le
/// domaine sans raison.
#[test]
fn la_borne_des_coordonnees_de_texture_est_inclusive() {
    let mut ctx = small();
    assert_eq!(
        ctx.submit_uv(
            Affine3::IDENTITY,
            &ahead_uv(MAX_TEXEL_COORD, -MAX_TEXEL_COORD),
            &one()
        ),
        Ok(())
    );
    assert_eq!(ctx.triangles.len(), 1);
}

/// Une texture unie de deux texels de côté, pour les cas où seul son identité
/// compte.
fn texture() -> Arc<Texture> {
    Arc::new(Texture::load(2, 2, &[0x80; 16]).expect("texture valide"))
}

/// Une texture soumise deux fois n'occupe qu'une entrée de la table : la
/// déduplication porte sur l'identité de l'allocation, puisque comparer les
/// contenus voudrait dire relire des mégaoctets à chaque lot.
#[test]
fn une_texture_resoumise_ne_prend_pas_deux_entrees() {
    let mut ctx = small();
    let (a, b) = (texture(), texture());

    ctx.submit_textured(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one(), &a)
        .expect("capacité");
    ctx.submit_textured(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one(), &a)
        .expect("capacité");
    assert_eq!(ctx.textures.len(), 1, "la même texture a pris deux entrées");

    ctx.submit_textured(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one(), &b)
        .expect("capacité");
    assert_eq!(
        ctx.textures.len(),
        2,
        "deux textures de même contenu sont deux ressources"
    );
}

/// Les textures d'une image se rangent dans l'ordre de première soumission :
/// c'est cet ordre que l'index porté par chaque triangle désigne.
#[test]
fn les_textures_se_rangent_dans_l_ordre_de_premiere_soumission() {
    let mut ctx = small();
    let (a, b) = (texture(), texture());

    ctx.submit_textured(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one(), &b)
        .expect("capacité");
    ctx.submit_textured(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one(), &a)
        .expect("capacité");
    ctx.submit_textured(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one(), &b)
        .expect("capacité");

    assert_eq!(ctx.textures.len(), 2);
    assert!(Arc::ptr_eq(&ctx.textures[0], &b), "la première soumise");
    assert!(Arc::ptr_eq(&ctx.textures[1], &a));
}

/// La table se vide avec la liste de dessin, et au même moment : garder une
/// texture ferait vivre une ressource que plus rien ne dessine.
#[test]
fn la_table_de_textures_se_vide_avec_la_liste_de_dessin() {
    let mut ctx = small();
    let mut pixels = vec![0u8; 64 * 64 * BYTES_PER_PIXEL];
    let t = texture();

    ctx.submit_textured(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one(), &t)
        .expect("capacité");
    ctx.frame_end(&mut pixels, 64).expect("image rendue");
    assert_eq!(ctx.textures.len(), 1, "l'image close la tient encore");

    ctx.submit_uv(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one())
        .expect("capacité");
    assert_eq!(ctx.textures.len(), 0, "le lot suivant ne l'a pas vidée");
    assert_eq!(Arc::strong_count(&t), 1, "le moteur la retient encore");
}

/// Un lot refusé ne laisse pas sa texture dans la table, et ne retire pas
/// celle qu'un lot précédent y avait mise.
#[test]
fn un_lot_texture_refuse_ne_laisse_pas_sa_texture() {
    let mut ctx = small();
    let (a, b) = (texture(), texture());
    ctx.submit_textured(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one(), &a)
        .expect("capacité");

    let mut vertices = ahead_uv(0.0, 0.0);
    vertices[1].u = f32::NAN;
    assert!(
        ctx.submit_textured(Affine3::IDENTITY, &vertices, &one(), &b)
            .is_err()
    );
    assert_eq!(ctx.textures.len(), 1, "la texture du lot refusé est restée");

    // Le même lot, refusé lui aussi, mais dont la texture était déjà là : la
    // troncature ne doit pas retirer ce qu'un lot accepté avait posé.
    assert!(
        ctx.submit_textured(Affine3::IDENTITY, &vertices, &one(), &a)
            .is_err()
    );
    assert_eq!(ctx.textures.len(), 1, "une texture acceptée a été retirée");
}

/// Un lot **accepté** dont aucun triangle ne survit à la projection ne laisse
/// pas sa texture dans la table.
///
/// C'est le cas que le refus ne couvre pas : la soumission rend `Ok`, puisqu'un
/// triangle hors du champ est une donnée et non une erreur, et pourtant elle ne
/// pose rien. Une texture restée là occuperait une entrée que plus aucun
/// triangle ne désigne — et le plafond de la table, qui se déduit du nombre de
/// triangles préparés, cesserait d'être une borne : il suffirait de soumettre
/// des lots invisibles pour le remplir.
#[test]
fn un_lot_texture_sans_triangle_visible_ne_laisse_pas_sa_texture() {
    let mut ctx = small();
    let t = texture();

    let invisible = behind().map(|position| VertexUv {
        position,
        u: 0.0,
        v: 0.0,
    });
    assert_eq!(
        ctx.submit_textured(Affine3::IDENTITY, &invisible, &one(), &t),
        Ok(()),
        "un triangle hors du champ est une donnée, pas une erreur"
    );

    assert_eq!(ctx.triangles.len(), 0, "le triangle aurait dû disparaître");
    assert_eq!(ctx.textures.len(), 0, "sa texture est restée dans la table");
}

/// Une texture déjà posée par un lot visible survit à un lot invisible qui la
/// réemploie.
///
/// La troncature porte sur ce que le lot a ajouté, jamais sur ce qu'il a
/// seulement retrouvé : retirer l'entrée d'un lot précédent invaliderait les
/// index que ses triangles portent déjà.
#[test]
fn un_lot_invisible_ne_retire_pas_une_texture_deja_posee() {
    let mut ctx = small();
    let t = texture();
    ctx.submit_textured(Affine3::IDENTITY, &ahead_uv(0.0, 0.0), &one(), &t)
        .expect("capacité");

    let invisible = behind().map(|position| VertexUv {
        position,
        u: 0.0,
        v: 0.0,
    });
    ctx.submit_textured(Affine3::IDENTITY, &invisible, &one(), &t)
        .expect("capacité");

    assert_eq!(ctx.triangles.len(), 1);
    assert_eq!(ctx.textures.len(), 1, "la texture du lot visible a sauté");
    assert_eq!(ctx.triangles[0].texture(), 0, "son index a changé");
}

/// Le tampon de l'hôte ressort opaque, quelle que soit la couleur soumise.
///
/// Le contrat d'ABI le promet, et c'est ce qui permet aux hôtes d'annoncer
/// l'opacité pour que le compositeur saute le mélange. Sur le web, seule cible
/// où ce canal est réellement composité, un alpha laissé à la valeur de l'hôte
/// rend un décor troué — sans erreur, et sans que rien d'autre ne le signale.
#[test]
fn le_tampon_ressort_opaque_quelle_que_soit_la_couleur_soumise() {
    let mut ctx = small();
    let mut pixels = vec![0u8; 64 * 64 * BYTES_PER_PIXEL];

    let translucide = [Triangle {
        indices: [0, 1, 2],
        color: Color::new(0x20, 0x40, 0x60, 0x00),
    }];
    ctx.submit(Affine3::IDENTITY, &ahead(), &translucide)
        .expect("capacité");
    ctx.frame_end(&mut pixels, 64).expect("image rendue");

    let opaques = pixels
        .chunks_exact(BYTES_PER_PIXEL)
        .filter(|p| p[3] == 0xFF)
        .count();
    assert_eq!(opaques, 64 * 64, "des pixels sont ressortis translucides");

    // Et le triangle a bien été peint : sans ce contrôle, un tampon resté au
    // fond passerait le test précédent, le fond étant opaque lui aussi.
    let peints = pixels
        .chunks_exact(BYTES_PER_PIXEL)
        .filter(|p| p[0] == 0x20 && p[1] == 0x40 && p[2] == 0x60)
        .count();
    assert!(
        peints > 100,
        "{peints} pixels peints, le cas ne couvre rien"
    );
}

/// Les sommets de `ahead`, habillés des deux jeux de coordonnées.
fn ahead_uv2(u: f32, v: f32, u2: f32, v2: f32) -> [VertexUv2; 3] {
    ahead().map(|position| VertexUv2 {
        position,
        u,
        v,
        u2,
        v2,
    })
}

/// Soumet un lot éclairé d'un seul triangle, par la forme à fonction d'accès.
fn submit_lit(ctx: &mut Context, vertices: [VertexUv2; 3]) -> Result<()> {
    ctx.submit_each_lit(Affine3::IDENTITY, 1, None, |_| {
        Ok((vertices, one()[0].color))
    })
}

/// Un lot éclairé range ses plans dans le tableau annexe, et chaque triangle
/// préparé désigne sa propre place.
///
/// Le triangle est **découpé par le plan proche**, donc il en produit
/// plusieurs : c'est le seul cas où la correspondance peut se décaler, un
/// triangle soumis donnant plusieurs triangles préparés.
#[test]
fn un_lot_eclaire_range_ses_plans_dans_l_ordre() {
    let mut ctx = small();
    // Un sommet derrière la caméra, deux devant : le découpage par le plan
    // proche engendre un quadrilatère, donc deux triangles préparés.
    let coupe = [
        Vec3::new(-10.0, -2.0, -2.0),
        Vec3::new(10.0, 0.0, 2.0),
        Vec3::new(10.0, 2.0, -2.0),
    ];
    let vertices = [0, 1, 2].map(|i| VertexUv2 {
        position: coupe[i],
        u: 0.0,
        v: 0.0,
        u2: i as f32,
        v2: -(i as f32),
    });
    submit_lit(&mut ctx, vertices).expect("capacité");

    assert!(ctx.triangles.len() > 1, "le cas n'a pas été découpé");
    assert_eq!(ctx.lighting.len(), ctx.triangles.len());
    for (place, triangle) in ctx.triangles.iter().enumerate() {
        assert_eq!(
            triangle.lighting() as usize,
            place,
            "le triangle {place} désigne la place d'un autre"
        );
    }
}

/// Un lot ordinaire ne touche pas au tableau annexe, et ses triangles portent
/// la sentinelle : le second jeu existe dans les sommets, mais ses plans ne se
/// construisent que pour un lot qui les demande.
#[test]
fn un_lot_ordinaire_ne_remplit_pas_le_tableau_annexe() {
    let mut ctx = small();
    ctx.submit_uv(Affine3::IDENTITY, &ahead_uv(4.0, 8.0), &one())
        .expect("capacité");

    assert_eq!(ctx.triangles.len(), 1);
    assert!(ctx.lighting.is_empty());
    assert_eq!(ctx.triangles[0].lighting(), NO_LIGHTING);
}

/// Un lot éclairé refusé ne laisse rien dans le tableau annexe.
///
/// Sans la troncature, les places survivantes décaleraient toutes celles des
/// lots suivants : chaque triangle lirait les coordonnées de son voisin, sans
/// qu'aucune longueur ne cesse de concorder.
#[test]
fn un_lot_eclaire_refuse_ne_laisse_aucune_place() {
    let mut ctx = small();
    submit_lit(&mut ctx, ahead_uv2(0.0, 0.0, 1.0, 2.0)).expect("capacité");
    let pose = ctx.lighting.len();
    assert_eq!(pose, 1);

    // Le second triangle du lot est refusé, le premier était bon : c'est le
    // cas où une troncature manquante ne se voit pas dans les longueurs.
    let vertices = ahead_uv2(0.0, 0.0, 1.0, 2.0);
    let mauvais = ahead_uv2(0.0, 0.0, MAX_TEXEL_COORD * 1.5, 0.0);
    assert_eq!(
        ctx.submit_each_lit(Affine3::IDENTITY, 2, None, |i| {
            Ok((if i == 0 { vertices } else { mauvais }, one()[0].color))
        }),
        Err(Error::InvalidArgument(Argument::TextureCoordinate))
    );
    assert_eq!(ctx.lighting.len(), pose, "une place a survécu au refus");
    assert_eq!(ctx.triangles.len(), 1);
}

/// La borne des coordonnées porte aussi sur le second jeu.
#[test]
fn une_coordonnee_de_lightmap_hors_borne_refuse_le_lot() {
    for bad in [f32::NAN, f32::INFINITY, MAX_TEXEL_COORD * 1.5] {
        let mut ctx = small();
        assert_eq!(
            submit_lit(&mut ctx, ahead_uv2(0.0, 0.0, bad, 0.0)),
            Err(Error::InvalidArgument(Argument::TextureCoordinate)),
            "{bad}"
        );
        assert_eq!(
            submit_lit(&mut ctx, ahead_uv2(0.0, 0.0, 0.0, bad)),
            Err(Error::InvalidArgument(Argument::TextureCoordinate)),
            "{bad}"
        );
    }
}

/// **Le critère du lot** : un lot éclairé rend exactement l'image qu'il
/// rendrait sans l'être.
///
/// Le second jeu de coordonnées traverse toute la chaîne — soumission,
/// projection, découpage, préparation — et rien ne le lit encore. Un seul
/// pixel de différence voudrait dire qu'il a débordé sur le premier jeu quelque
/// part, et c'est le genre d'écart qu'on ne retrouve plus une fois le
/// remplissage éclairé écrit par-dessus.
#[test]
fn un_lot_eclaire_rend_la_meme_image_qu_un_lot_ordinaire() {
    let mut eclaire = small();
    let mut ordinaire = small();
    submit_lit(&mut eclaire, ahead_uv2(4.0, 8.0, 0.25, -0.75)).expect("capacité");
    ordinaire
        .submit_uv(Affine3::IDENTITY, &ahead_uv(4.0, 8.0), &one())
        .expect("capacité");

    let render = |ctx: &mut Context| {
        let mut pixels = vec![0u8; 64 * 64 * BYTES_PER_PIXEL];
        ctx.frame_end(&mut pixels, 64).expect("image rendue");
        pixels
    };
    let (a, b) = (render(&mut eclaire), render(&mut ordinaire));
    assert!(
        a.chunks_exact(BYTES_PER_PIXEL).any(|p| p[0] == 0xFF),
        "le cas ne peint rien"
    );
    assert!(a == b, "le second jeu de coordonnées a changé l'image");
}

/// La fin d'image vide le tableau annexe comme elle vide les triangles : gardé,
/// il ferait démarrer l'image suivante sur des places déjà prises.
#[test]
fn la_fin_d_image_vide_le_tableau_annexe() {
    let mut ctx = small();
    submit_lit(&mut ctx, ahead_uv2(0.0, 0.0, 1.0, 2.0)).expect("capacité");
    let mut pixels = vec![0u8; 64 * 64 * BYTES_PER_PIXEL];
    ctx.frame_end(&mut pixels, 64).expect("image rendue");

    submit_lit(&mut ctx, ahead_uv2(0.0, 0.0, 1.0, 2.0)).expect("capacité");
    assert_eq!(ctx.lighting.len(), 1);
    assert_eq!(ctx.triangles[0].lighting(), 0);
}
