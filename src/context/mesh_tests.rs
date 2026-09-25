// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La soumission d'un maillage chargé.
//!
//! Un fichier à part des autres tests du contexte, qui portent la configuration,
//! l'image et les réglages : ce qui est éprouvé ici est un chemin de soumission
//! de plus, et sa seule promesse propre — refusé en entier ou pas du tout.

use alloc::vec;
use alloc::vec::Vec;

use super::*;
use crate::math::Quat;
use crate::testing::{
    group_bytes as group, mesh_file as file, name_bytes as name, triangle_bytes as triangle,
    vertex_bytes as vertex,
};

/// Un contexte de 64×64, celui des autres tests de rendu.
fn small_ctx() -> Context {
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

/// Rend une image de 64×64 et en donne les pixels.
fn pixels_of(ctx: &mut Context) -> Vec<u8> {
    let mut pixels = vec![0u8; 64 * 64 * BYTES_PER_PIXEL];
    ctx.frame_end(&mut pixels, 64).expect("image rendue");
    pixels
}

/// Les quatre coins d'un carré devant la caméra, avec leurs coordonnées de
/// texture.
///
/// L'enroulement des triangles qui s'en tirent est celui du triangle des autres
/// tests : une face arrière disparaîtrait au découpage, et un test qui la
/// soumettrait ne mesurerait plus rien — vu en écrivant ceux-ci.
const CORNERS: [(f32, f32, f32, f32, f32); 4] = [
    (10.0, -2.0, -2.0, 0.0, 0.0),
    (10.0, 2.0, -2.0, 4.0, 0.0),
    (10.0, 2.0, 2.0, 4.0, 4.0),
    (10.0, -2.0, 2.0, 0.0, 4.0),
];

/// La couleur des triangles des maillages d'épreuve.
const TINT: [u8; 4] = [0x40, 0xC0, 0x80, 0xFF];

/// Les sommets de `CORNERS`, en octets de fichier.
fn corner_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (x, y, z, u, v) in CORNERS {
        bytes.extend_from_slice(&vertex(x, y, z, u, v));
    }
    bytes
}

/// Les mêmes sommets, en sommets du noyau.
fn corner_vertices() -> [VertexUv; 4] {
    CORNERS.map(|(x, y, z, u, v)| VertexUv {
        position: Vec3::new(x, y, z),
        u,
        v,
    })
}

/// Les deux triangles du carré, dans l'enroulement visible.
const QUAD: [[u32; 3]; 2] = [[0, 3, 2], [0, 2, 1]];

/// Un maillage d'un triangle, un groupe, un emplacement nommé.
fn one_group() -> Mesh {
    Mesh::load(&file(
        &group(1, 0, 1, 0),
        &name("mur"),
        &triangle(QUAD[0][0], QUAD[0][1], QUAD[0][2], TINT),
        &corner_bytes(),
    ))
    .expect("maillage valide")
}

/// Un maillage de deux groupes, chacun sa moitié du carré et son emplacement.
///
/// Les deux moitiés sont visibles et disjointes : c'est ce qui permet de voir
/// laquelle a reçu quelle texture, et de faire déborder la capacité sur le
/// second groupe.
fn two_groups() -> Mesh {
    let mut groups = group(1, 0, 1, 0);
    groups.extend_from_slice(&group(2, 1, 1, 1));
    let mut names = name("mur");
    names.extend_from_slice(&name("sol"));
    let mut triangles = triangle(QUAD[0][0], QUAD[0][1], QUAD[0][2], TINT);
    triangles.extend_from_slice(&triangle(
        QUAD[1][0],
        QUAD[1][1],
        QUAD[1][2],
        [0xFF, 0x20, 0x20, 0xFF],
    ));

    Mesh::load(&file(&groups, &names, &triangles, &corner_bytes())).expect("maillage valide")
}

/// Une texture unie d'un texel par canal donné.
fn plain_texture(r: u8, g: u8, b: u8) -> Arc<Texture> {
    Arc::new(Texture::load(2, 2, &[r, g, b, 0xFF].repeat(4)).expect("texture valide"))
}

/// Un maillage sans texture rend exactement ce que les mêmes triangles rendent
/// par la soumission ordinaire.
///
/// C'est le test qui compte : il prouve que le chemin du maillage n'ajoute ni
/// transformation, ni arrondi, ni ordre à ce que le fichier porte. Sans lui, une
/// empreinte de conformance figerait ce que le chemin fait, juste ou non.
#[test]
fn un_maillage_rend_ce_que_ses_triangles_rendent() {
    let mesh = one_group();
    let mut par_maillage = small_ctx();
    par_maillage
        .submit_mesh(Affine3::IDENTITY, &mesh, |_| None)
        .expect("capacité");

    let mut par_triangles = small_ctx();
    par_triangles
        .submit_uv(
            Affine3::IDENTITY,
            &corner_vertices(),
            &[Triangle {
                indices: QUAD[0],
                color: Color::new(TINT[0], TINT[1], TINT[2], TINT[3]),
            }],
        )
        .expect("capacité");

    assert_eq!(
        pixels_of(&mut par_maillage),
        pixels_of(&mut par_triangles),
        "le chemin du maillage change l'image"
    );
}

/// La même égalité avec une texture, qui est le chemin qu'un décor emprunte.
#[test]
fn un_maillage_texture_rend_ce_que_ses_triangles_rendent() {
    let texture = plain_texture(0x80, 0x40, 0x20);
    let mesh = one_group();

    let mut par_maillage = small_ctx();
    par_maillage
        .submit_mesh(Affine3::IDENTITY, &mesh, |_| Some(&texture))
        .expect("capacité");

    let mut par_triangles = small_ctx();
    par_triangles
        .submit_textured(
            Affine3::IDENTITY,
            &corner_vertices(),
            &[Triangle {
                indices: QUAD[0],
                color: Color::new(TINT[0], TINT[1], TINT[2], TINT[3]),
            }],
            &texture,
        )
        .expect("capacité");

    assert_eq!(pixels_of(&mut par_maillage), pixels_of(&mut par_triangles));
}

/// La matrice de modèle traverse : le même maillage déplacé ne rend pas la même
/// image.
///
/// Un chemin qui ignorerait `model` rendrait la première image partout, et
/// aucune empreinte prise sur une scène immobile ne le verrait.
#[test]
fn la_matrice_de_modele_deplace_le_maillage() {
    let mesh = one_group();
    let mut immobile = small_ctx();
    immobile
        .submit_mesh(Affine3::IDENTITY, &mesh, |_| None)
        .expect("capacité");

    let mut deplace = small_ctx();
    let model = Affine3::from_rotation_translation(Quat::IDENTITY, Vec3::new(0.0, 1.0, 0.0));
    deplace
        .submit_mesh(model, &mesh, |_| None)
        .expect("capacité");

    assert_ne!(pixels_of(&mut immobile), pixels_of(&mut deplace));
}

/// Chaque groupe prend la texture de son emplacement, et non celle du premier.
///
/// Les deux groupes couvrent la même surface ; seul celui de devant se voit. Une
/// soumission qui prendrait toujours l'emplacement zéro rendrait l'autre image.
#[test]
fn chaque_groupe_prend_la_texture_de_son_emplacement() {
    let mesh = two_groups();
    let rouge = plain_texture(0xF0, 0x20, 0x20);
    let bleu = plain_texture(0x20, 0x20, 0xF0);

    let mut ctx = small_ctx();
    ctx.submit_mesh(Affine3::IDENTITY, &mesh, |slot| match slot {
        0 => Some(&rouge),
        _ => Some(&bleu),
    })
    .expect("capacité");
    let par_emplacement = pixels_of(&mut ctx);

    let mut ctx = small_ctx();
    ctx.submit_mesh(Affine3::IDENTITY, &mesh, |_| Some(&rouge))
        .expect("capacité");
    let tout_rouge = pixels_of(&mut ctx);

    assert_ne!(
        par_emplacement, tout_rouge,
        "les deux emplacements reçoivent la même texture"
    );
}

/// Une entrée nulle vaut « sans texture », et la couleur du triangle décide
/// alors.
#[test]
fn un_emplacement_sans_texture_laisse_la_couleur_decider() {
    let mesh = one_group();
    let texture = plain_texture(0xF0, 0x20, 0x20);

    let mut sans = small_ctx();
    sans.submit_mesh(Affine3::IDENTITY, &mesh, |_| None)
        .expect("capacité");
    let uni = pixels_of(&mut sans);

    let mut avec = small_ctx();
    avec.submit_mesh(Affine3::IDENTITY, &mesh, |_| Some(&texture))
        .expect("capacité");

    assert_ne!(uni, pixels_of(&mut avec));
    // Le triangle a bien peint : ce qu'il peint est déjà comparé au chemin
    // ordinaire par le test nominal, qui répond de la teinte.
    let center = (32 * 64 + 32) * BYTES_PER_PIXEL;
    assert_ne!(
        &uni[center..center + 3],
        &[0, 0, 0],
        "le centre est resté au fond"
    );
}

/// Un maillage vide se soumet sans rien peindre et sans erreur.
#[test]
fn un_maillage_vide_ne_peint_rien() {
    let mesh = Mesh::load(&file(&[], &[], &[], &[])).expect("maillage vide");
    let mut ctx = small_ctx();
    ctx.submit_mesh(Affine3::IDENTITY, &mesh, |_| None)
        .expect("capacité");

    let mut vierge = small_ctx();
    assert_eq!(pixels_of(&mut ctx), pixels_of(&mut vierge));
}

/// Un maillage dont un groupe dépasse la capacité est refusé en entier : ni ses
/// triangles, ni ceux de ses groupes précédents ne restent dans l'image.
///
/// C'est la promesse du contrat d'ABI, et elle ne tient pas d'elle-même : le
/// dépassement se découvre après que les premiers groupes ont été posés, et un
/// refus qui les laisserait donnerait une image fausse sans erreur.
#[test]
fn un_maillage_qui_deborde_la_capacite_ne_laisse_rien() {
    let mesh = two_groups();
    let mut ctx = Context::new(Config {
        max_width: 64,
        max_height: 64,
        width: 64,
        height: 64,
        tile_size: 32,
        // Un seul triangle : le premier groupe passe, le second déborde.
        max_triangles: 1,
    })
    .expect("configuration saine");

    assert_eq!(
        ctx.submit_mesh(Affine3::IDENTITY, &mesh, |_| None),
        Err(Error::InvalidArgument(Argument::TriangleCapacity))
    );
    assert_eq!(ctx.triangles.len(), 0, "le premier groupe est resté");

    let mut vierge = small_ctx();
    assert_eq!(
        pixels_of(&mut ctx),
        pixels_of(&mut vierge),
        "l'image porte un groupe d'un maillage refusé"
    );
}

/// Un maillage refusé ne laisse pas non plus de texture dans la table de
/// l'image.
///
/// Sans quoi le plafond de textures ne se déduirait plus du nombre de triangles
/// préparés : il suffirait de soumettre des maillages refusés pour le remplir.
#[test]
fn un_maillage_refuse_ne_laisse_pas_sa_texture() {
    let mesh = two_groups();
    let texture = plain_texture(0x80, 0x80, 0x80);
    let mut ctx = Context::new(Config {
        max_width: 64,
        max_height: 64,
        width: 64,
        height: 64,
        tile_size: 32,
        max_triangles: 1,
    })
    .expect("configuration saine");

    assert!(
        ctx.submit_mesh(Affine3::IDENTITY, &mesh, |_| Some(&texture))
            .is_err()
    );
    assert_eq!(ctx.textures.len(), 0);
}

// Le refus d'une soumission pendant une image n'est pas éprouvé ici : `Frame`
// emprunte le contexte, si bien que l'API Rust ne permet pas de soumettre
// pendant qu'une image est ouverte. C'est la frontière C, dont les handles ne
// portent pas cet emprunt, qui l'éprouve — voir `tests/mesh.rs` de
// `screengine-ffi`.

/// Le même maillage se soumet deux fois dans une image, avec deux matrices.
///
/// C'est ce qu'un décor fait de ses accessoires, et cela vérifie qu'une
/// soumission ne consomme pas la ressource.
#[test]
fn un_maillage_se_soumet_deux_fois() {
    let mesh = one_group();
    let mut ctx = small_ctx();
    ctx.submit_mesh(Affine3::IDENTITY, &mesh, |_| None)
        .expect("capacité");
    ctx.submit_mesh(
        Affine3::from_rotation_translation(Quat::IDENTITY, Vec3::new(0.0, 1.0, 0.0)),
        &mesh,
        |_| None,
    )
    .expect("capacité");
    assert_eq!(ctx.triangles.len(), 2);
}
