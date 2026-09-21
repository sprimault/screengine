// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La projection et la symétrie du point d'intersection.
//!
//! Le test qui porte le plus est `l_intersection_est_symetrique_au_bit_pres` :
//! c'est lui qui interdit la fissure le long d'une arête partagée qui traverse
//! le plan proche, et le seul défaut qu'il attrape ne se voit qu'en mouvement.

use super::*;
use crate::math::Angle;
use crate::testing::Rng;

/// Une projection ordinaire, qui sert de base aux cas qui ne l'éprouvent pas.
fn projection() -> Projection {
    Projection::new(640, 360, 1.0, 0.1).unwrap_or_else(|_| unreachable!())
}

/// Le repliement d'`Angle::from_radians` est le piège : trois demi-tours y
/// deviendraient un demi-tour, accepté sans rien signaler. Le refus porte donc
/// sur les radians, avant toute conversion.
#[test]
fn refuse_un_champ_de_vision_hors_bornes() {
    for fov in [0.0, -1.0, PI, PI + 0.1, 3.0 * PI, f32::NAN, f32::INFINITY] {
        assert!(Projection::new(640, 360, fov, 0.1).is_err(), "fov {fov}");
    }
    assert!(Projection::new(640, 360, 3.0, 0.1).is_ok());
}

/// Sous quelques milliardièmes de radian, la demi-ouverture s'arrondit à zéro
/// en angle binaire et la cotangente serait infinie.
#[test]
fn refuse_un_champ_de_vision_sous_le_pas_de_l_angle_binaire() {
    assert!(Projection::new(640, 360, 1.0e-9, 0.1).is_err());
}

/// Le plan proche divise : nul, négatif ou non fini, il n'y a pas de projection.
#[test]
fn refuse_un_plan_proche_invalide() {
    for near in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(Projection::new(640, 360, 1.0, near).is_err(), "near {near}");
    }
}

/// Une dimension nulle n'a pas de centre.
#[test]
fn refuse_une_resolution_nulle() {
    assert!(Projection::new(0, 360, 1.0, 0.1).is_err());
    assert!(Projection::new(640, 0, 1.0, 0.1).is_err());
}

/// Le centre est en `largeur/2`, jamais en `(largeur−1)/2` : un point sur l'axe
/// de la caméra tombe exactement au centre, quelle que soit sa distance.
#[test]
fn l_axe_tombe_au_centre_de_l_image() {
    let p = projection();
    for depth in [0.5, 1.0, 10.0, 1000.0] {
        let clip = p.to_clip(Vec3::new(0.0, 0.0, depth)).unwrap();
        let v = p.to_vertex(clip);
        assert_eq!((v.x, v.y), (320 * 16, 180 * 16), "profondeur {depth}");
    }
}

/// L'image est symétrique autour de son centre : sans cette propriété, un
/// panoramique fait sauter les sommets inégalement de part et d'autre.
#[test]
fn la_projection_est_symetrique_autour_du_centre() {
    let p = projection();
    let mut rng = Rng::new(0x5115);
    for _ in 0..1000 {
        let x = rng.unit_f32() * 4.0 - 2.0;
        let y = rng.unit_f32() * 4.0 - 2.0;
        let w = 0.2 + rng.unit_f32() * 8.0;
        let a = p.to_vertex(p.to_clip(Vec3::new(x, y, w)).unwrap());
        let b = p.to_vertex(p.to_clip(Vec3::new(-x, -y, w)).unwrap());
        assert_eq!(a.x - 320 * 16, 320 * 16 - b.x);
        assert_eq!(a.y - 180 * 16, 180 * 16 - b.y);
        assert_eq!(a.z, b.z, "même profondeur, même w");
    }
}

/// Le bord du champ de vision vertical tombe sur le bord de l'image : c'est ce
/// qui définit `sx` et `sy`, et un facteur faux ne se verrait pas autrement.
#[test]
fn le_bord_du_champ_tombe_sur_le_bord_de_l_image() {
    let p = projection();
    // À la distance 1, la demi-hauteur du tronc vaut tan(fov/2).
    let half = Angle::from_radians(1.0).half();
    let half_height = half.sin() / half.cos();
    let clip = p.to_clip(Vec3::new(0.0, half_height, 1.0)).unwrap();
    assert_eq!(p.to_vertex(clip).y, 360 * 16);
}

/// Bit pour bit contre l'expression écrite : c'est l'ordre des opérations qui
/// est testé, pas la valeur. Un chemin SIMD devra reproduire celui-ci.
#[test]
fn rend_les_bits_de_l_expression_ecrite() {
    let p = projection();
    let view = Vec3::new(0.37, -1.2, 3.5);
    let clip = p.to_clip(view).unwrap();
    let v = p.to_vertex(clip);

    let inverse = 1.0 / 3.5;
    assert_eq!(v.x, to_subpixel(320.0 + (p.scale_x * 0.37) * inverse));
    assert_eq!(v.y, to_subpixel(180.0 + (p.scale_y * -1.2) * inverse));
    assert_eq!(v.z, to_depth(0.1 * inverse));
}

/// Une coordonnée non finie ou démesurée est écartée avant le découpage : une
/// seule d'entre elles empoisonnerait les deux autres par l'intersection.
#[test]
fn ecarte_une_coordonnee_non_finie_ou_demesuree() {
    let p = projection();
    for bad in [f32::NAN, f32::INFINITY, -f32::INFINITY, 1.0e35] {
        assert!(p.to_clip(Vec3::new(bad, 0.0, 1.0)).is_none(), "{bad}");
        assert!(p.to_clip(Vec3::new(0.0, bad, 1.0)).is_none(), "{bad}");
        assert!(p.to_clip(Vec3::new(0.0, 0.0, bad)).is_none(), "{bad}");
    }
    assert!(p.to_clip(Vec3::new(1.0, 2.0, 3.0)).is_some());
}

/// La borne porte sur la coordonnée de clip, après la mise à l'échelle.
///
/// Un champ de vision très fermé porte `scale` à plusieurs millions : une
/// coordonnée de vue que le test d'amont acceptait sort alors de la borne une
/// fois multipliée. Testée avant la multiplication, elle laissait passer des
/// abscisses de clip que le découpage ne peut pas porter.
#[test]
fn la_borne_porte_sur_la_coordonnee_de_clip() {
    let etroit = Projection::new(640, 360, 1.0e-5, 0.1).unwrap_or_else(|_| unreachable!());
    assert!(etroit.scale_x > 1.0e6, "champ trop ouvert pour ce cas");

    let juste_sous = COORDINATE_LIMIT / etroit.scale_x * 0.5;
    assert!(etroit.to_clip(Vec3::new(juste_sous, 0.0, 1.0)).is_some());
    assert!(
        etroit
            .to_clip(Vec3::new(juste_sous * 8.0, 0.0, 1.0))
            .is_none(),
        "une coordonnée de vue modeste donne ici une coordonnée de clip hors borne"
    );
}

/// Aucune sortie d'`intersect` n'est non finie, sur tout le domaine que
/// `to_clip` laisse passer.
///
/// C'est la propriété que la borne existe pour tenir, et elle porte sur les
/// **produits** de l'intersection, quadratiques en la coordonnée — pas sur les
/// distances aux plans, qui sont linéaires. Un `inf − inf` y donnerait un
/// `NaN`, que `to_subpixel` convertirait en zéro : un triangle de bruit, sans
/// erreur ni panique.
#[test]
fn aucune_intersection_ne_deborde_dans_le_domaine_admis() {
    let p = projection();
    let f = p.frustum();
    let mut rng = Rng::new(0x5EED_1234);

    // Les extrêmes du domaine, et eux seuls, tiennent le pire cas : la borne
    // est quadratique, donc c'est là que le produit est le plus grand.
    let extreme = |rng: &mut Rng| {
        let sign = if rng.next() & 1 == 0 { 1.0 } else { -1.0 };
        sign * COORDINATE_LIMIT * (0.25 + 0.75 * rng.unit_f32())
    };

    for _ in 0..2_000 {
        let a = ClipVertex {
            x: extreme(&mut rng),
            y: extreme(&mut rng),
            w: extreme(&mut rng),
        };
        let b = ClipVertex {
            x: extreme(&mut rng),
            y: extreme(&mut rng),
            w: extreme(&mut rng),
        };
        for plane in 0..PLANE_COUNT {
            let (da, db) = (f.distance(a, plane), f.distance(b, plane));
            assert!(da.is_finite() && db.is_finite(), "distance débordée");
            if da == 0.0 || db == 0.0 || da == db {
                continue;
            }
            let v = Frustum::intersect(a, b, da, db);
            assert!(
                v.x.is_finite() && v.y.is_finite() && v.w.is_finite(),
                "intersection non finie : {v:?}"
            );
        }
    }
}

/// **Le test central du lot.**
///
/// Deux triangles qui partagent une arête la parcourent en sens opposés. Si le
/// point d'intersection différait d'un seul bit entre les deux sens, une
/// fissure s'ouvrirait le long de l'arête, invisible à l'arrêt et visible en
/// mouvement. La comparaison est sur `to_bits`, jamais sur l'égalité flottante,
/// qui confondrait `+0` et `−0`.
#[test]
fn l_intersection_est_symetrique_au_bit_pres() {
    let p = projection();
    let f = p.frustum();
    let mut rng = Rng::new(0xC11B);
    let mut croisements = 0;
    for _ in 0..20_000 {
        let point = |rng: &mut Rng| ClipVertex {
            x: rng.unit_f32() * 2000.0 - 1000.0,
            y: rng.unit_f32() * 2000.0 - 1000.0,
            w: rng.unit_f32() * 20.0 - 5.0,
        };
        let (a, b) = (point(&mut rng), point(&mut rng));
        for plane in 0..PLANE_COUNT {
            let (da, db) = (f.distance(a, plane), f.distance(b, plane));
            if (da >= 0.0) == (db >= 0.0) {
                continue;
            }
            croisements += 1;
            let ab = Frustum::intersect(a, b, da, db);
            let ba = Frustum::intersect(b, a, db, da);
            assert_eq!(
                ab.x.to_bits(),
                ba.x.to_bits(),
                "graine 0xC11B, plan {plane}"
            );
            assert_eq!(
                ab.y.to_bits(),
                ba.y.to_bits(),
                "graine 0xC11B, plan {plane}"
            );
            assert_eq!(
                ab.w.to_bits(),
                ba.w.to_bits(),
                "graine 0xC11B, plan {plane}"
            );
        }
    }
    assert!(
        croisements > 1000,
        "{croisements} croisements, échantillon trop maigre"
    );
}

/// La forme usuelle `a + t·(b − a)` ne l'est pas, et ce test le montre : il
/// justifie que la forme symétrique existe plutôt que d'être remplacée un jour
/// par l'écriture qui « se lit mieux ».
#[test]
fn la_forme_usuelle_n_est_pas_symetrique() {
    let lerp = |a: f32, b: f32, da: f32, db: f32| {
        let t = da / (da - db);
        a + t * (b - a)
    };
    let mut rng = Rng::new(0x1E12);
    let mut divergences = 0;
    for _ in 0..20_000 {
        let (a, b) = (rng.unit_f32() * 100.0, rng.unit_f32() * 100.0);
        let (da, db) = (rng.unit_f32(), -rng.unit_f32());
        if lerp(a, b, da, db).to_bits() != lerp(b, a, db, da).to_bits() {
            divergences += 1;
        }
    }
    assert!(
        divergences > 0,
        "la forme usuelle se révèle symétrique : revoir le test"
    );
}

/// Un sommet posé exactement sur un plan ressort identique à lui-même : la
/// forme générale en produirait une copie voisine, donc un triangle en aiguille.
#[test]
fn un_sommet_sur_le_plan_ressort_identique() {
    let a = ClipVertex {
        x: 1.0,
        y: 2.0,
        w: 3.0,
    };
    let b = ClipVertex {
        x: 9.0,
        y: 8.0,
        w: 7.0,
    };
    let sur_a = Frustum::intersect(a, b, 0.0, -1.5);
    assert_eq!(sur_a.x.to_bits(), a.x.to_bits());
    assert_eq!(sur_a.w.to_bits(), a.w.to_bits());
    let sur_b = Frustum::intersect(a, b, 2.5, 0.0);
    assert_eq!(sur_b.y.to_bits(), b.y.to_bits());
}

/// Le point d'intersection est bien sur le plan, à l'arrondi près : la symétrie
/// ne servirait à rien si la formule était fausse.
#[test]
fn l_intersection_tombe_sur_le_plan() {
    let p = projection();
    let f = p.frustum();
    let mut rng = Rng::new(0x7A1E);
    for _ in 0..2000 {
        let point = |rng: &mut Rng| ClipVertex {
            x: rng.unit_f32() * 100.0 - 50.0,
            y: rng.unit_f32() * 100.0 - 50.0,
            w: rng.unit_f32() * 10.0 - 2.0,
        };
        let (a, b) = (point(&mut rng), point(&mut rng));
        for plane in 0..PLANE_COUNT {
            let (da, db) = (f.distance(a, plane), f.distance(b, plane));
            if (da >= 0.0) == (db >= 0.0) || da == 0.0 || db == 0.0 {
                continue;
            }
            let d = f.distance(Frustum::intersect(a, b, da, db), plane);
            // Comparaison écrite, jamais `f32::max` : son traitement de NaN et
            // de −0 n'est pas celui des chemins SIMD, et `clippy.toml` le
            // refuse pour cette raison jusque dans les tests.
            let (ada, adb) = (da.abs(), db.abs());
            let echelle = if ada > adb { ada } else { adb };
            assert!(
                d.abs() <= echelle * 1.0e-4,
                "plan {plane} : {d} pour {echelle}"
            );
        }
    }
}

/// Les codes de position disent bien quels plans sont violés, et un NaN tombe
/// du côté extérieur de tous.
#[test]
fn les_codes_de_position_designent_les_plans_violes() {
    let p = projection();
    let f = p.frustum();
    let dedans = ClipVertex {
        x: 0.0,
        y: 0.0,
        w: 1.0,
    };
    assert_eq!(f.outcode(dedans), 0);

    let derriere = ClipVertex {
        x: 0.0,
        y: 0.0,
        w: -1.0,
    };
    assert!(
        f.outcode(derriere) & 1 != 0,
        "le plan proche doit être violé"
    );

    // Un NaN en abscisse viole les deux plans latéraux et eux seuls : toute
    // comparaison avec lui est fausse, donc `d >= 0` l'est aussi.
    let nan = ClipVertex {
        x: f32::NAN,
        y: 0.0,
        w: 1.0,
    };
    assert_eq!(f.outcode(nan), 0b00110);
}
