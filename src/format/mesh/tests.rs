// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests du décodeur de maillage.
//!
//! Même doctrine que le conteneur : les fichiers s'écrivent en octets ici, par
//! un constructeur qui pose les champs un à un. C'est le seul endroit où les
//! dispositions du maillage sont écrites une seconde fois, et un fichier produit
//! par un écrivain du projet rendrait le test tautologique.

use alloc::vec;
use alloc::vec::Vec;

use super::*;
use crate::testing::Rng;

/// Un sommet d'épreuve, écrit en octets.
fn vertex(x: f32, y: f32, z: f32, u: f32, v: f32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [x, y, z, u, v] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Un triangle d'épreuve : trois indices puis quatre composantes de couleur.
fn triangle(i0: u32, i1: u32, i2: u32, color: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in [i0, i1, i2] {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes.extend_from_slice(&color);
    bytes
}

/// Un groupe d'épreuve : identifiant, premier triangle, compte, emplacement.
fn group(id: u32, first: u32, count: u32, slot: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [id, first, count, slot] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Un nom d'emplacement : sa longueur en deux octets, puis ses octets.
fn name(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(text.len() as u16).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// Un fichier de maillage bien formé, à partir de ses quatre sections.
///
/// Les décalages sont posés ici — vingt octets d'en-tête, douze par entrée de
/// table — et les sections rangées par genre croissant, comme le conteneur
/// l'exige. Une section vide n'entre pas dans la table.
fn file(surf: &[u8], texn: &[u8], tris: &[u8], vtxs: &[u8]) -> Vec<u8> {
    let sections: Vec<([u8; 4], &[u8])> = [
        (*b"SURF", surf),
        (*b"TEXN", texn),
        (*b"TRIS", tris),
        (*b"VTXS", vtxs),
    ]
    .into_iter()
    .filter(|(_, body)| !body.is_empty())
    .collect();

    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"MESH");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(sections.len() as u32).to_le_bytes());

    let mut offset = first;
    for (tag, body) in &sections {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        offset += body.len();
    }
    for (_, body) in &sections {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// Quatre sommets, deux triangles, deux groupes, deux emplacements : le maillage
/// que les tests abîment.
fn valid() -> Vec<u8> {
    let mut vertices = Vec::new();
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        vertices.extend_from_slice(&vertex(x, y, -2.0, x * 64.0, y * 64.0));
    }
    let mut triangles = triangle(0, 1, 2, [10, 20, 30, 255]);
    triangles.extend_from_slice(&triangle(0, 2, 3, [40, 50, 60, 255]));
    let mut groups = group(7, 0, 1, 0);
    groups.extend_from_slice(&group(9, 1, 1, 1));
    let mut names = name("mur");
    names.extend_from_slice(&name("sol"));

    file(&groups, &names, &triangles, &vertices)
}

/// L'erreur attendue, écrite court.
fn refused(malformation: Malformation) -> Error {
    Error::InvalidFormat(malformation)
}

/// Un maillage bien formé rend ce que le fichier porte, champ par champ.
///
/// C'est le seul test qui prouve que le décodeur rend la *bonne* ressource et
/// pas seulement une ressource bien formée : les bornes et les refus ne disent
/// rien du contenu.
#[test]
fn un_maillage_bien_forme_rend_son_contenu() {
    let mesh = Mesh::load(&valid()).unwrap();

    assert_eq!(mesh.triangle_count(), 2);
    assert_eq!(mesh.texture_count(), 2);
    assert_eq!(mesh.texture_name(0), Some("mur"));
    assert_eq!(mesh.texture_name(1), Some("sol"));
    assert_eq!(mesh.texture_name(2), None, "au-delà du dernier emplacement");

    assert_eq!(mesh.vertices.len(), 4);
    assert_eq!(mesh.vertices[2].position, Vec3::new(1.0, 1.0, -2.0));
    assert_eq!(mesh.vertices[2].u, 64.0);
    assert_eq!(mesh.vertices[2].v, 64.0);

    assert_eq!(mesh.triangles[1].indices, [0, 2, 3]);
    assert_eq!(mesh.triangles[1].color, Color::new(40, 50, 60, 255));

    assert_eq!(
        mesh.groups[1],
        Group {
            id: 9,
            first_triangle: 1,
            triangle_count: 1,
            texture_slot: 1,
        }
    );
}

/// Un maillage vide est légitime, et ses sections absentes ne sont pas une
/// erreur.
#[test]
fn un_maillage_vide_est_legitime() {
    let mesh = Mesh::load(&file(&[], &[], &[], &[])).unwrap();
    assert_eq!(mesh.triangle_count(), 0);
    assert_eq!(mesh.texture_count(), 0);
    assert_eq!(mesh.bounds, [Vec3::ZERO; 2], "aucun sommet, aucune boîte");
}

/// Un genre de fichier autre qu'un maillage est refusé par le conteneur.
///
/// Le décodeur de maillage ne choisit pas son genre : il le passe au socle, et
/// ce test vérifie qu'il passe bien `MESH` et non ce que le fichier annonce.
#[test]
fn une_carte_n_est_pas_un_maillage() {
    let mut bytes = valid();
    bytes[4..8].copy_from_slice(b"WRLD");
    assert_eq!(Mesh::load(&bytes).unwrap_err(), refused(Malformation::Kind));
}

/// Un indice de sommet au-delà du tableau est refusé, et la borne est stricte :
/// un indice égal au compte est déjà hors du tableau.
#[test]
fn un_indice_de_sommet_hors_borne_est_refuse() {
    let vertices = vertex(0.0, 0.0, 0.0, 0.0, 0.0);
    for indices in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
        let triangles = triangle(indices.0, indices.1, indices.2, [0; 4]);
        let bytes = file(&group(1, 0, 1, 0), &name("t"), &triangles, &vertices);
        assert_eq!(
            Mesh::load(&bytes).unwrap_err(),
            refused(Malformation::Index),
            "indices {indices:?} pour un seul sommet"
        );
    }
}

/// Un emplacement de texture au-delà des noms est refusé.
///
/// Conséquence assumée : un maillage qui porte des triangles déclare au moins un
/// nom. « Sans texture » se dit à la soumission, par un handle nul, pas dans le
/// fichier.
#[test]
fn un_emplacement_de_texture_hors_borne_est_refuse() {
    let vertices = vertex(0.0, 0.0, 0.0, 0.0, 0.0);
    let triangles = triangle(0, 0, 0, [0; 4]);
    for (names, slot) in [(name("t"), 1), (Vec::new(), 0)] {
        let bytes = file(&group(1, 0, 1, slot), &names, &triangles, &vertices);
        assert_eq!(
            Mesh::load(&bytes).unwrap_err(),
            refused(Malformation::Index),
            "emplacement {slot} pour {} nom(s)",
            names.len()
        );
    }
}

/// Un identifiant de groupe nul est refusé : l'éditeur le réserve à « aucun ».
#[test]
fn un_identifiant_de_groupe_nul_est_refuse() {
    let vertices = vertex(0.0, 0.0, 0.0, 0.0, 0.0);
    let bytes = file(
        &group(0, 0, 1, 0),
        &name("t"),
        &triangle(0, 0, 0, [0; 4]),
        &vertices,
    );
    assert_eq!(
        Mesh::load(&bytes).unwrap_err(),
        refused(Malformation::Identifier)
    );
}

/// Deux groupes du même identifiant sont refusés, l'unicité étant lue sur une
/// table triée.
///
/// Les deux groupes sont par ailleurs bien formés : sans le contrôle d'unicité,
/// le fichier passerait, et une recherche par identifiant rendrait plus tard la
/// mauvaise surface sans jamais rien signaler.
#[test]
fn deux_groupes_du_meme_identifiant_sont_refuses() {
    let vertices = vertex(0.0, 0.0, 0.0, 0.0, 0.0);
    let mut triangles = triangle(0, 0, 0, [0; 4]);
    triangles.extend_from_slice(&triangle(0, 0, 0, [0; 4]));
    let mut groups = group(4, 0, 1, 0);
    groups.extend_from_slice(&group(4, 1, 1, 0));

    let bytes = file(&groups, &name("t"), &triangles, &vertices);
    assert_eq!(
        Mesh::load(&bytes).unwrap_err(),
        refused(Malformation::Identifier)
    );
}

/// Les groupes pavent les triangles : trou, recouvrement, débordement et reste
/// sont refusés.
///
/// Les quatre cas passent par la même porte, et c'est voulu : un groupe qui ne
/// serait pas contigu imposerait de rassembler ses triangles à chaque image.
#[test]
fn des_groupes_qui_ne_pavent_pas_sont_refuses() {
    let vertices = vertex(0.0, 0.0, 0.0, 0.0, 0.0);
    let mut triangles = triangle(0, 0, 0, [0; 4]);
    triangles.extend_from_slice(&triangle(0, 0, 0, [0; 4]));

    let mut hole = group(1, 0, 1, 0);
    hole.extend_from_slice(&group(2, 2, 0, 0));
    let mut overlap = group(1, 0, 2, 0);
    overlap.extend_from_slice(&group(2, 1, 1, 0));

    let cases: [(&str, Vec<u8>); 4] = [
        ("un trou entre deux groupes", hole),
        ("un recouvrement", overlap),
        (
            "un groupe qui dépasse le dernier triangle",
            group(1, 0, 3, 0),
        ),
        ("un reste que personne ne porte", group(1, 0, 1, 0)),
    ];
    for (what, groups) in cases {
        let bytes = file(&groups, &name("t"), &triangles, &vertices);
        assert_eq!(
            Mesh::load(&bytes).unwrap_err(),
            refused(Malformation::GroupBounds),
            "{what}"
        );
    }
}

/// Un compte de triangles démesuré est refusé sans déborder sa somme.
///
/// Deux groupes qui annoncent chacun deux milliards de triangles : leur somme
/// déborde un `usize` de 32 bits, largeur de deux des quatre cibles, et c'est
/// l'addition vérifiée qui fait sortir une erreur au lieu d'un total replié qui
/// paverait.
#[test]
fn un_compte_de_triangles_demesure_est_refuse() {
    let vertices = vertex(0.0, 0.0, 0.0, 0.0, 0.0);
    let triangles = triangle(0, 0, 0, [0; 4]);
    let bytes = file(&group(1, 0, u32::MAX, 0), &name("t"), &triangles, &vertices);
    assert_eq!(
        Mesh::load(&bytes).unwrap_err(),
        refused(Malformation::GroupBounds)
    );
}

/// Un nom qui n'est pas de l'UTF-8 valide est refusé.
#[test]
fn un_nom_qui_n_est_pas_de_l_utf8_est_refuse() {
    let mut names = 2u16.to_le_bytes().to_vec();
    names.extend_from_slice(&[0xff, 0xfe]);
    let bytes = file(&[], &names, &[], &[]);
    assert_eq!(
        Mesh::load(&bytes).unwrap_err(),
        refused(Malformation::NonUtf8)
    );
}

/// Un nom qui dépasse sa section est refusé comme une troncature.
///
/// La longueur d'un nom est le seul compte du format que le fichier déclare, et
/// c'est le curseur qui le borne.
#[test]
fn un_nom_qui_deborde_sa_section_est_refuse() {
    let mut names = 9u16.to_le_bytes().to_vec();
    names.extend_from_slice(b"mur");
    let bytes = file(&[], &names, &[], &[]);
    assert_eq!(
        Mesh::load(&bytes).unwrap_err(),
        refused(Malformation::Truncated)
    );
}

/// Une section dont la longueur n'est pas un multiple de son élément est refusée
/// comme une troncature, sans contrôle qui lui soit propre.
#[test]
fn une_section_a_element_partiel_est_refusee() {
    let vertices = vertex(0.0, 0.0, 0.0, 0.0, 0.0);
    let partial = &vertices[..VERTEX_LEN - 1];
    assert_eq!(
        Mesh::load(&file(&[], &[], &[], partial)).unwrap_err(),
        refused(Malformation::Truncated),
        "un sommet coupé"
    );

    let triangles = triangle(0, 0, 0, [0; 4]);
    let partial = &triangles[..TRIANGLE_LEN - 1];
    assert_eq!(
        Mesh::load(&file(&[], &name("t"), partial, &vertices)).unwrap_err(),
        refused(Malformation::Truncated),
        "un triangle coupé"
    );

    let groups = group(1, 0, 0, 0);
    let partial = &groups[..GROUP_LEN - 1];
    assert_eq!(
        Mesh::load(&file(partial, &name("t"), &[], &[])).unwrap_err(),
        refused(Malformation::Truncated),
        "un groupe coupé"
    );
}

/// Une coordonnée non finie est refusée, à la position comme aux coordonnées de
/// texture.
#[test]
fn une_coordonnee_non_finie_est_refusee() {
    for champ in 0..5 {
        let mut values = [0.0f32; 5];
        values[champ] = f32::NAN;
        let vertices = vertex(values[0], values[1], values[2], values[3], values[4]);
        assert_eq!(
            Mesh::load(&file(&[], &[], &[], &vertices)).unwrap_err(),
            refused(Malformation::NonFinite),
            "champ {champ}"
        );
    }
}

/// La boîte englobante est le minimum et le maximum par composante, et le zéro
/// négatif garde son signe.
///
/// Ce test fige ce que **toutes** les cibles doivent rendre ; il ne prouve pas à
/// lui seul que le calcul ne dérive pas. `f32::min` rend l'un ou l'autre des deux
/// zéros « non déterministement », dit sa documentation : il passe ce test sur
/// cette cible-ci et pourrait le manquer ailleurs, ce qu'un test exécuté sur une
/// seule cible ne peut pas voir. Ce qui l'attraperait est la conformance croisée,
/// le jour où une scène chargée depuis un fichier y entrera. D'où la comparaison
/// écrite à la main dans le décodeur, et cette référence ici.
#[test]
fn la_boite_englobante_est_exacte_et_signee() {
    let mut vertices = vertex(-0.0, 2.0, -3.0, 0.0, 0.0);
    vertices.extend_from_slice(&vertex(1.0, -0.0, 5.0, 0.0, 0.0));
    let mesh = Mesh::load(&file(&[], &[], &[], &vertices)).unwrap();

    assert_eq!(mesh.bounds[0], Vec3::new(-0.0, -0.0, -3.0));
    assert_eq!(mesh.bounds[1], Vec3::new(1.0, 2.0, 5.0));
    assert!(
        mesh.bounds[0].x.is_sign_negative(),
        "le zéro du fichier garde son signe"
    );
    assert!(
        mesh.bounds[1].y.is_sign_positive(),
        "le maximum garde le zéro positif"
    );
}

/// Un maillage tronqué à n'importe laquelle de ses longueurs est refusé, sans
/// une seule panique.
#[test]
fn toute_troncature_est_refusee() {
    let bytes = valid();
    for len in 0..bytes.len() {
        match Mesh::load(&bytes[..len]) {
            Ok(_) => panic!("tronqué à {len} octets, et accepté"),
            Err(error) => assert!(
                matches!(
                    error,
                    Error::InvalidFormat(_) | Error::UnsupportedFormatVersion
                ),
                "à {len} octets : {error:?}"
            ),
        }
    }
}

/// Une mutation quelconque rend un succès ou une erreur de données, jamais autre
/// chose et jamais une panique.
///
/// La graine est fixe et s'affiche : un échec qui ne se rejoue pas n'a pas été
/// trouvé. Deux octets mutés à la fois, parce qu'un seul ne peut pas fabriquer
/// un couple indice-compte que le pavage accepterait de travers.
#[test]
fn une_mutation_quelconque_ne_panique_pas() {
    const SEED: u64 = 0x4d45_5348_2026_0925;
    let mut rng = Rng::new(SEED);
    let original = valid();
    for round in 0..8192 {
        let mut bytes = original.clone();
        for _ in 0..2 {
            let at = (rng.next() % bytes.len() as u64) as usize;
            bytes[at] ^= (rng.next() % 255 + 1) as u8;
        }
        if let Err(error) = Mesh::load(&bytes) {
            assert!(
                matches!(
                    error,
                    Error::InvalidFormat(_) | Error::UnsupportedFormatVersion
                ),
                "graine {SEED:#x}, tour {round} : {error:?}"
            );
        }
    }
}

/// Un maillage dont un groupe porte des milliers de triangles se charge sans
/// réallouer : la capacité vient de la longueur de sa section.
///
/// Le test ne mesure pas les allocations — c'est l'affaire de la conformance —,
/// il vérifie que la borne tient sur un fichier qui n'est plus minuscule, là où
/// un `u16` d'indice ou un compte replié se verrait.
#[test]
fn un_maillage_de_milliers_de_triangles_se_charge() {
    const COUNT: u32 = 5000;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for i in 0..COUNT {
        let f = i as f32;
        vertices.extend_from_slice(&vertex(f, 0.0, 0.0, 0.0, 0.0));
        triangles.extend_from_slice(&triangle(i, 0, 0, [0; 4]));
    }
    let bytes = file(&group(1, 0, COUNT, 0), &name("t"), &triangles, &vertices);

    let mesh = Mesh::load(&bytes).unwrap();
    assert_eq!(mesh.triangle_count(), COUNT);
    assert_eq!(mesh.vertices.len(), COUNT as usize);
    assert_eq!(mesh.bounds[1].x, (COUNT - 1) as f32);
}

/// Un nom vide est légitime : le fichier nomme un emplacement, il ne promet pas
/// que le nom serve à retrouver un fichier.
#[test]
fn un_nom_vide_est_legitime() {
    let bytes = file(&[], &name(""), &[], &[]);
    let mesh = Mesh::load(&bytes).unwrap();
    assert_eq!(mesh.texture_count(), 1);
    assert_eq!(mesh.texture_name(0), Some(""));
}

/// Les noms se comptent par le parcours, et un nom de plus ne décale pas les
/// précédents.
#[test]
fn les_noms_se_comptent_par_le_parcours() {
    let mut names = name("a");
    names.extend_from_slice(&name("bb"));
    names.extend_from_slice(&name("ccc"));
    let mesh = Mesh::load(&file(&[], &names, &[], &[])).unwrap();

    assert_eq!(mesh.texture_count(), 3);
    assert_eq!(mesh.texture_name(0), Some("a"));
    assert_eq!(mesh.texture_name(1), Some("bb"));
    assert_eq!(mesh.texture_name(2), Some("ccc"));
}

/// Un accent dans un nom traverse le décodage intact.
#[test]
fn un_nom_accentue_traverse_le_decodage() {
    let mesh = Mesh::load(&file(&[], &name("béton"), &[], &[])).unwrap();
    assert_eq!(mesh.texture_name(0), Some("béton"));
}

/// Deux sections du même genre restent refusées par le conteneur, même écrites
/// avec des dispositions de maillage justes.
#[test]
fn le_conteneur_garde_ses_refus() {
    let vertices = vec![0u8; VERTEX_LEN];
    let mut bytes = valid();
    // Le genre de la première entrée de table, porté à celui de la seconde.
    bytes[20..24].copy_from_slice(b"TEXN");
    assert_eq!(
        Mesh::load(&bytes).unwrap_err(),
        refused(Malformation::SectionOrder)
    );
    assert!(Mesh::load(&file(&[], &[], &[], &vertices)).is_ok());
}
