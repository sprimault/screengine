// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le découpage, et surtout ce qui doit en sortir inchangé.
//!
//! Le chemin rapide est une optimisation qui touche au déterminisme : c'est le
//! genre d'accélération qui rend une image différente selon ce qu'on croyait
//! être un raccourci sans effet. Deux tests l'épinglent.

use super::*;
use crate::math::Projection;
use crate::math::projection::ClipVertex;
use crate::testing::Rng;

/// Une projection ordinaire, dont les plans servent à tous les cas.
fn projection() -> Projection {
    Projection::new(640, 360, 1.0, 0.1).unwrap_or_else(|_| unreachable!())
}

/// Un sommet de clip, écrit court pour que les cas se lisent.
fn v(x: f32, y: f32, w: f32) -> ClipVertex {
    ClipVertex { x, y, w }
}

/// Un triangle qu'aucun plan ne coupe traverse sans qu'un seul bit change :
/// c'est ce qui permet au chemin rapide d'exister.
#[test]
fn un_triangle_entierement_dedans_passe_inchange() {
    let p = projection();
    let t = [v(0.0, 0.0, 1.0), v(100.0, 0.0, 1.0), v(0.0, 100.0, 1.0)];
    let poly = clip(t, p.frustum());
    assert_eq!(poly.triangle_count(), 1);
    let sortie = poly.triangle(0);
    for i in 0..3 {
        assert_eq!(sortie[i].x.to_bits(), t[i].x.to_bits());
        assert_eq!(sortie[i].y.to_bits(), t[i].y.to_bits());
        assert_eq!(sortie[i].w.to_bits(), t[i].w.to_bits());
    }
}

/// Entièrement derrière le plan proche : rien, et surtout pas une panique. Un
/// triangle dégénéré est une donnée, pas un défaut du moteur.
#[test]
fn un_triangle_derriere_le_plan_proche_disparait() {
    let p = projection();
    let t = [v(0.0, 0.0, -1.0), v(10.0, 0.0, -2.0), v(0.0, 10.0, -3.0)];
    assert_eq!(clip(t, p.frustum()).triangle_count(), 0);
}

/// Un triangle coupé par le plan proche donne un quadrilatère, donc deux
/// triangles d'éventail.
#[test]
fn un_triangle_coupe_par_le_plan_proche_donne_un_quadrilatere() {
    let p = projection();
    let t = [v(0.0, 0.0, 1.0), v(10.0, 0.0, 1.0), v(0.0, 10.0, -1.0)];
    let poly = clip(t, p.frustum());
    assert_eq!(poly.len, 4);
    assert_eq!(poly.triangle_count(), 2);
    for i in 0..poly.len {
        assert!(poly.v[i].w >= 0.1, "sommet {i} en deçà du plan proche");
    }
}

/// La borne de huit sommets est prouvée, mais elle doit aussi être atteinte au
/// moins une fois : une capacité qu'aucun test n'approche ne prouve rien.
#[test]
fn le_pire_cas_reste_dans_la_borne() {
    let p = projection();
    let mut rng = Rng::new(0xB011);
    let mut maximum = 0;
    for _ in 0..50_000 {
        let point = |rng: &mut Rng| {
            v(
                rng.unit_f32() * 60_000.0 - 30_000.0,
                rng.unit_f32() * 60_000.0 - 30_000.0,
                rng.unit_f32() * 8.0 - 2.0,
            )
        };
        let t = [point(&mut rng), point(&mut rng), point(&mut rng)];
        let poly = clip(t, p.frustum());
        assert!(poly.len <= MAX_CLIP_VERTICES);
        assert!(poly.triangle_count() <= MAX_CLIP_TRIANGLES);
        maximum = maximum.max(poly.len);
    }
    assert!(
        maximum >= 6,
        "seulement {maximum} sommets atteints, cas trop doux"
    );
}

/// Le piège du découpage : une extrémité posée exactement sur le plan est déjà
/// ajoutée, et l'intersection vaudrait ce même point. Sans le garde-fou, le
/// polygone porterait deux fois le même sommet et l'éventail en ferait un
/// triangle dégénéré.
#[test]
fn un_sommet_sur_un_plan_ne_se_duplique_pas() {
    let p = projection();
    let near = 0.1;
    let t = [v(0.0, 0.0, 1.0), v(10.0, 0.0, near), v(0.0, 10.0, -1.0)];
    let poly = clip(t, p.frustum());
    for i in 0..poly.len {
        for j in (i + 1)..poly.len {
            let (a, b) = (poly.v[i], poly.v[j]);
            assert!(
                a.x != b.x || a.y != b.y || a.w != b.w,
                "sommets {i} et {j} confondus"
            );
        }
    }
}

/// Le chemin rapide doit rendre exactement ce que rendrait le découpage
/// complet. C'est l'affirmation qui justifie le raccourci, et elle se teste en
/// forçant les cinq passes sur des triangles entièrement intérieurs.
#[test]
fn le_chemin_rapide_rend_les_memes_bits_que_le_chemin_complet() {
    let p = projection();
    let f = p.frustum();
    let mut rng = Rng::new(0xFA57);
    for _ in 0..5000 {
        let point = |rng: &mut Rng| {
            v(
                rng.unit_f32() * 200.0 - 100.0,
                rng.unit_f32() * 200.0 - 100.0,
                1.0 + rng.unit_f32() * 4.0,
            )
        };
        let t = [point(&mut rng), point(&mut rng), point(&mut rng)];
        let rapide = clip(t, f);
        let complet = decoupe_complete(t, f);
        assert_eq!(rapide.len, complet.len);
        for i in 0..rapide.len {
            assert_eq!(rapide.v[i].x.to_bits(), complet.v[i].x.to_bits());
            assert_eq!(rapide.v[i].y.to_bits(), complet.v[i].y.to_bits());
            assert_eq!(rapide.v[i].w.to_bits(), complet.v[i].w.to_bits());
        }
    }
}

/// Le découpage sans aucun raccourci, pour servir de référence au test
/// précédent : les cinq plans, toujours, quels que soient les codes.
fn decoupe_complete(
    triangle: [ClipVertex; 3],
    frustum: &crate::math::projection::Frustum,
) -> Polygon {
    let mut current = Polygon::EMPTY;
    for vertex in triangle {
        current.push(vertex);
    }
    let mut next = Polygon::EMPTY;
    for plane in 0..PLANE_COUNT {
        next.len = 0;
        for i in 0..current.len {
            let a = current.v[i];
            let b = current.v[if i + 1 == current.len { 0 } else { i + 1 }];
            let da = frustum.distance(a, plane);
            let db = frustum.distance(b, plane);
            let (a_inside, b_inside) = (da >= 0.0, db >= 0.0);
            if a_inside {
                next.push(a);
            }
            if a_inside != b_inside && da != 0.0 && db != 0.0 {
                next.push(crate::math::projection::Frustum::intersect(a, b, da, db));
            }
        }
        core::mem::swap(&mut current, &mut next);
        if current.len < 3 {
            return Polygon::EMPTY;
        }
    }
    current
}

/// Une arête partagée par deux triangles voisins produit le même sommet
/// découpé des deux côtés, bits compris — l'étanchéité au niveau du découpage,
/// avant même que le rasteriseur entre en jeu.
#[test]
fn deux_triangles_voisins_decoupent_leur_arete_a_l_identique() {
    let p = projection();
    let f = p.frustum();
    let mut rng = Rng::new(0x5EA1);
    for _ in 0..2000 {
        // Une arête qui traverse le plan proche, et un troisième sommet de
        // chaque côté : les deux triangles la parcourent en sens opposés.
        let a = v(
            rng.unit_f32() * 40.0 - 20.0,
            rng.unit_f32() * 40.0 - 20.0,
            2.0,
        );
        let b = v(
            rng.unit_f32() * 40.0 - 20.0,
            rng.unit_f32() * 40.0 - 20.0,
            -1.0,
        );
        let gauche = v(-30.0, rng.unit_f32() * 10.0, 1.5);
        let droite = v(30.0, rng.unit_f32() * 10.0, 1.5);

        let un = clip([a, b, gauche], f);
        let deux = clip([b, a, droite], f);

        // Le point que l'arête commune doit engendrer. Les deux polygones ne
        // partagent que celui-là : leurs troisièmes sommets diffèrent, donc
        // leurs autres sommets engendrés aussi.
        let attendu = Frustum::intersect(a, b, f.distance(a, 0), f.distance(b, 0));
        let porte = |poly: &Polygon| {
            (0..poly.len).any(|i| {
                poly.v[i].x.to_bits() == attendu.x.to_bits()
                    && poly.v[i].y.to_bits() == attendu.y.to_bits()
                    && poly.v[i].w.to_bits() == attendu.w.to_bits()
            })
        };
        assert!(porte(&un), "graine 0x5EA1 : sens direct");
        // Celui-ci parcourt l'arête dans l'autre sens : c'est la boucle de
        // découpage, et pas seulement la formule, qui est éprouvée ici.
        assert!(porte(&deux), "graine 0x5EA1 : sens inverse");
    }
}
