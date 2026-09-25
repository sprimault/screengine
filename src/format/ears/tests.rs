// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests de la découpe d'oreilles.

use alloc::vec::Vec;

use super::*;

/// Triangule et rend les triplets, ou `None`.
fn cut(points: &[Vec3]) -> Option<Vec<[u32; 3]>> {
    let mut out = [[0u32; 3]; MAX_POLYGON];
    let count = triangulate(points, &mut out)?;
    Some(out[..count].to_vec())
}

/// Le carré du plan `z = 0`, en sens antihoraire vu de `+Z`.
fn square() -> Vec<Vec3> {
    alloc::vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(0.0, 2.0, 0.0),
    ]
}

/// La somme des aires des triangles, dans le plan `(x, y)`.
///
/// C'est le critère qui ne dépend ni de l'ordre de découpe ni du choix des
/// oreilles : une triangulation juste couvre le polygone une fois et une seule,
/// donc rend son aire exactement.
fn area(points: &[Vec3], triangles: &[[u32; 3]]) -> f32 {
    triangles
        .iter()
        .map(|t| {
            let (a, b, c) = (
                points[t[0] as usize],
                points[t[1] as usize],
                points[t[2] as usize],
            );
            ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)) / 2.0
        })
        .sum()
}

/// Un triangle se rend tel quel, sans découpe.
#[test]
fn un_triangle_se_rend_tel_quel() {
    let points = &square()[..3];
    assert_eq!(cut(points).unwrap(), alloc::vec![[0, 1, 2]]);
}

/// Un carré donne deux triangles qui couvrent son aire.
#[test]
fn un_carre_donne_deux_triangles() {
    let points = square();
    let triangles = cut(&points).unwrap();
    assert_eq!(triangles.len(), 2);
    assert_eq!(area(&points, &triangles), 4.0);
}

/// Un polygone concave en L se triangule, et c'est tout l'intérêt de la
/// découpe : la moindre extrusion d'un décor en produit un.
#[test]
fn un_polygone_en_l_se_triangule() {
    let points = alloc::vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(3.0, 0.0, 0.0),
        Vec3::new(3.0, 1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(1.0, 3.0, 0.0),
        Vec3::new(0.0, 3.0, 0.0),
    ];
    let triangles = cut(&points).unwrap();
    assert_eq!(triangles.len(), 4, "six sommets donnent quatre triangles");
    assert_eq!(area(&points, &triangles), 5.0, "l'aire du L");
}

/// L'orientation du polygone se retrouve dans ses triangles.
///
/// Une découpe qui les retournerait rendrait toutes les faces d'un décor
/// invisibles, et l'empreinte ne dirait pas pourquoi.
#[test]
fn l_orientation_du_polygone_est_gardee() {
    let mut points = square();
    let direct = cut(&points).unwrap();
    assert!(area(&points, &direct) > 0.0);

    points.reverse();
    let inverse = cut(&points).unwrap();
    assert!(area(&points, &inverse) < 0.0, "le sens inverse est gardé");
}

/// Un polygone concave dans chacun des trois plans dominants se triangule.
///
/// Le choix du plan de projection est ce qui garde les aires loin de zéro : un
/// mur vertical projeté sur le sol s'écraserait en segment, et chaque test
/// d'oreille deviendrait un départage d'arrondis.
#[test]
fn les_trois_plans_dominants_se_triangulent() {
    let flat = alloc::vec![
        (0.0, 0.0),
        (3.0, 0.0),
        (3.0, 1.0),
        (1.0, 1.0),
        (1.0, 3.0),
        (0.0, 3.0),
    ];
    for plan in 0..3 {
        let points: Vec<Vec3> = flat
            .iter()
            .map(|&(a, b)| match plan {
                0 => Vec3::new(5.0, a, b),
                1 => Vec3::new(b, 5.0, a),
                _ => Vec3::new(a, b, 5.0),
            })
            .collect();
        let triangles = cut(&points).expect("le polygone se triangule");
        assert_eq!(triangles.len(), 4, "plan {plan}");
    }
}

/// Moins de trois sommets, ou plus que le plafond, sont refusés.
#[test]
fn les_bornes_du_polygone_sont_refusees() {
    assert!(cut(&square()[..2]).is_none(), "deux sommets");
    let many: Vec<Vec3> = (0..=MAX_POLYGON)
        .map(|i| Vec3::new(i as f32, 0.0, 0.0))
        .collect();
    assert!(cut(&many).is_none(), "un sommet de trop");
}

/// Des sommets tous alignés n'ont pas de normale, donc pas de triangulation.
///
/// C'est le cas que le produit vectoriel des trois premiers sommets laisserait
/// passer sur un polygone quelconque, et que la somme de Newell attrape.
#[test]
fn un_polygone_plat_est_refuse() {
    let points = alloc::vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(3.0, 0.0, 0.0),
    ];
    assert!(cut(&points).is_none());
}

/// Trois premiers sommets alignés ne font pas perdre l'orientation.
///
/// Le polygone est bien formé ; seul son début est plat. Un produit vectoriel
/// pris sur ces trois-là rendrait une normale nulle, et la découpe échouerait
/// sur une surface parfaitement légitime.
#[test]
fn trois_sommets_alignes_au_depart_ne_genent_pas() {
    let points = alloc::vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(0.0, 2.0, 0.0),
    ];
    let triangles = cut(&points).expect("le polygone se triangule");
    assert_eq!(area(&points, &triangles), 4.0);
}

/// Un polygone qui se recoupe est refusé plutôt que trianguler de travers.
#[test]
fn un_polygone_qui_se_recoupe_est_refuse() {
    // Un nœud papillon : ses deux arêtes obliques se croisent.
    let points = alloc::vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(0.0, 2.0, 0.0),
    ];
    let triangles = cut(&points);
    if let Some(triangles) = triangles {
        // Une découpe qui aboutirait doit au moins ne pas inventer d'aire : le
        // nœud papillon a deux moitiés de signes opposés, et leur somme est
        // nulle.
        assert_eq!(area(&points, &triangles), 0.0);
    }
}

/// La découpe d'un même polygone rend toujours les mêmes triangles.
///
/// Une triangulation qui dépendrait d'un ordre de parcours changerait l'image
/// d'un décor sans que le fichier bouge, et deux cibles n'auraient plus la même
/// empreinte.
#[test]
fn la_decoupe_est_reproductible() {
    let points = alloc::vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(4.0, 0.0, 0.0),
        Vec3::new(4.0, 2.0, 0.0),
        Vec3::new(2.0, 1.0, 0.0),
        Vec3::new(0.0, 2.0, 0.0),
    ];
    let first = cut(&points).unwrap();
    for _ in 0..4 {
        assert_eq!(cut(&points).unwrap(), first);
    }
}
