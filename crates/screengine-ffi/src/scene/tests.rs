// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La disposition des structures de scène, et ce qu'elles refusent.
//!
//! La disposition compte autant que le comportement : une liaison JavaScript
//! écrit ces structures octet par octet dans la mémoire linéaire, à partir des
//! décalages du header. Un champ qui bouge ne casse rien à la compilation et
//! rend une image fausse.

use super::*;

/// Le format RGBA8 vaut un, jamais zéro.
///
/// C'est ce qui fait refuser une description laissée à zéro plutôt que la lire
/// comme valide — et un hôte qui oublie de remplir la sienne est précisément
/// celui qui en a le plus besoin.
#[test]
fn le_format_de_texture_ne_vaut_pas_zero() {
    assert_eq!(SCG_TEXTURE_FORMAT_RGBA8, 1);
}

/// Un sommet quelconque, que les tests dérivent.
fn vertex(x: f32) -> ScgVertex {
    ScgVertex { x, y: 0.0, z: 0.0 }
}

/// Les coordonnées non finies sont refusées à la frontière, et nommées.
///
/// Le noyau les ferait disparaître sans un mot — c'est le bon comportement
/// pour un sommet qu'il a lui-même transformé, pas pour un sommet que l'hôte
/// lui tend.
#[test]
fn un_sommet_non_fini_est_refuse() {
    assert_eq!(check_finite(&[vertex(1.0), vertex(2.0)]), Ok(()));
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            check_finite(&[vertex(1.0), vertex(bad)]),
            Err(AbiError::VERTEX_NOT_FINITE)
        );
    }
}

/// Un sommet à normale quelconque, que les tests dérivent.
fn vertex_uvn() -> ScgVertexUvN {
    ScgVertexUvN {
        x: 1.0,
        y: 2.0,
        z: 3.0,
        u: 4.0,
        v: 5.0,
        nx: 0.0,
        ny: 0.0,
        nz: 1.0,
    }
}

/// Les six composantes qu'un sommet à normale fait vérifier, et les deux qu'il
/// laisse à une autre couche.
///
/// **Ce rempart n'avait aucun cas, et c'est lui qui refusait seul une normale
/// non finie** avant que le noyau ne le fasse aussi. Le test dit donc aussi ce
/// que cette couche **ne** couvre pas : `u` et `v` passent ici, le noyau les
/// bornant par `MAX_TEXEL_COORD` — poser un cas qui les attendrait refusés
/// l'aurait fait rougir pour la mauvaise raison.
#[test]
fn un_sommet_a_normale_non_fini_est_refuse() {
    assert_eq!(check_finite_uvn(&[vertex_uvn()]), Ok(()));
    for mauvais in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for pose in [
            |v: &mut ScgVertexUvN, m: f32| v.x = m,
            |v: &mut ScgVertexUvN, m: f32| v.y = m,
            |v: &mut ScgVertexUvN, m: f32| v.z = m,
            |v: &mut ScgVertexUvN, m: f32| v.nx = m,
            |v: &mut ScgVertexUvN, m: f32| v.ny = m,
            |v: &mut ScgVertexUvN, m: f32| v.nz = m,
        ] {
            let mut v = vertex_uvn();
            pose(&mut v, mauvais);
            assert_eq!(
                check_finite_uvn(&[vertex_uvn(), v]),
                Err(AbiError::VERTEX_NOT_FINITE)
            );
        }
        for pose in [
            |v: &mut ScgVertexUvN, m: f32| v.u = m,
            |v: &mut ScgVertexUvN, m: f32| v.v = m,
        ] {
            let mut v = vertex_uvn();
            pose(&mut v, mauvais);
            assert_eq!(check_finite_uvn(&[v]), Ok(()));
        }
    }
}

/// Un sommet éclairé fait vérifier sa position, et rien d'autre ici.
///
/// Même absence de cas que son voisin à normale, et **le partage des couches est
/// ce que ce test épingle** : les quatre coordonnées passent ici, le noyau les
/// bornant par `MAX_TEXEL_COORD` sous `Argument::TextureCoordinate` — un `NaN`
/// y tombe aussi. Les attendre refusées ici aurait fait rougir le cas pour la
/// mauvaise raison, et c'est l'erreur que sa première écriture a commise.
#[test]
fn un_sommet_eclaire_non_fini_est_refuse() {
    let bon = ScgVertexUv2 {
        x: 1.0,
        y: 2.0,
        z: 3.0,
        u: 4.0,
        v: 5.0,
        u2: 6.0,
        v2: 7.0,
    };
    assert_eq!(check_finite_uv2(&[bon]), Ok(()));
    for mauvais in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for pose in [
            |v: &mut ScgVertexUv2, m: f32| v.x = m,
            |v: &mut ScgVertexUv2, m: f32| v.y = m,
            |v: &mut ScgVertexUv2, m: f32| v.z = m,
        ] {
            let mut v = bon;
            pose(&mut v, mauvais);
            assert_eq!(
                check_finite_uv2(&[bon, v]),
                Err(AbiError::VERTEX_NOT_FINITE)
            );
        }
        for pose in [
            |v: &mut ScgVertexUv2, m: f32| v.u = m,
            |v: &mut ScgVertexUv2, m: f32| v.v = m,
            |v: &mut ScgVertexUv2, m: f32| v.u2 = m,
            |v: &mut ScgVertexUv2, m: f32| v.v2 = m,
        ] {
            let mut v = bon;
            pose(&mut v, mauvais);
            assert_eq!(check_finite_uv2(&[v]), Ok(()));
        }
    }
}

/// Une caméra de composantes finies, que les tests dérivent.
fn camera() -> ScgCamera {
    ScgCamera {
        position: [0.0; 3],
        orientation: [0.0, 0.0, 0.0, 1.0],
        fov_y: 1.0,
        near_plane: 0.1,
    }
}

/// La caméra passe le quaternion dans l'ordre `x, y, z, w`.
///
/// L'ordre inverse est la confusion naturelle : `{0, 0, 0, 1}` y deviendrait
/// une rotation d'un demi-tour, et la caméra regarderait derrière elle sans
/// qu'aucun contrôle ne s'en aperçoive.
#[test]
fn la_camera_lit_le_quaternion_en_xyzw() {
    let mut raw = camera();
    raw.orientation = [0.1, 0.2, 0.3, 0.4];
    let core = raw.to_core().expect("caméra finie");
    assert_eq!(
        (
            core.orientation.x,
            core.orientation.y,
            core.orientation.z,
            core.orientation.w
        ),
        (0.1, 0.2, 0.3, 0.4)
    );
}

/// Une position ou une orientation non finie est refusée.
#[test]
fn une_camera_non_finie_est_refusee() {
    let mut raw = camera();
    raw.position[1] = f32::NAN;
    assert_eq!(raw.to_core(), Err(AbiError::CAMERA_NOT_FINITE));

    let mut raw = camera();
    raw.orientation[3] = f32::INFINITY;
    assert_eq!(raw.to_core(), Err(AbiError::CAMERA_NOT_FINITE));
}

/// L'identité 4×4, par colonnes.
fn identity() -> ScgMat4 {
    let mut m = [0.0f32; 16];
    for i in 0..4 {
        m[i * 4 + i] = 1.0;
    }
    ScgMat4 { m }
}

/// La matrice se lit par colonnes, translation dans la dernière.
///
/// Prise par lignes, une translation deviendrait une projection : la dernière
/// ligne serait alors non nulle, ce que le contrôle attraperait — mais une
/// rotation, elle, passerait transposée, donc inversée.
#[test]
fn la_matrice_se_lit_par_colonnes() {
    let mut raw = identity();
    // Colonne 3 : la translation.
    raw.m[12] = 5.0;
    raw.m[13] = 6.0;
    raw.m[14] = 7.0;
    // Ligne 1 de la colonne 0, pour distinguer les deux lectures.
    raw.m[1] = 2.0;

    let core = raw.to_core().expect("matrice affine");
    assert_eq!(&core.m[9..], &[5.0, 6.0, 7.0]);
    assert_eq!(core.m[1], 2.0);
}

/// Une dernière ligne autre que `0, 0, 0, 1` est refusée.
///
/// C'est le seul contrôle qui sépare une transformation rigide d'une matrice à
/// perspective. Sans lui, `inverse_rigid` en rendrait un faux inverse en
/// silence, puisque sa précondition est justement celle-ci.
#[test]
fn une_matrice_a_perspective_est_refusee() {
    for (index, value) in [(3, 0.5), (7, -1.0), (11, 1.0), (15, 0.0), (15, 2.0)] {
        let mut raw = identity();
        raw.m[index] = value;
        assert_eq!(raw.to_core(), Err(AbiError::MATRIX), "m[{index}] = {value}");
    }
    assert!(identity().to_core().is_ok());
}

/// Un coefficient non fini est refusé avant tout le reste.
#[test]
fn une_matrice_non_finie_est_refusee() {
    let mut raw = identity();
    raw.m[5] = f32::NAN;
    assert_eq!(raw.to_core(), Err(AbiError::MATRIX));
}

/// La couleur traverse dans l'ordre mémoire des pixels.
#[test]
fn la_couleur_traverse_en_rgba() {
    let triangle = ScgTriangle {
        i0: 0,
        i1: 1,
        i2: 2,
        r: 0x10,
        g: 0x20,
        b: 0x30,
        a: 0x40,
    };
    assert_eq!(
        triangle.color(),
        screengine::Color::new(0x10, 0x20, 0x30, 0x40)
    );
}
