// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le balayage d'une boîte, appelé comme le ferait un hôte.
//!
//! Un fichier à lui plutôt qu'une suite dans celui de la carte : c'est le premier
//! point d'entrée de l'ABI à autoriser la concurrence sur un même objet, et le
//! test qui l'éprouve n'a rien à voir avec un chargement.
//!
//! Les octets de la carte s'écrivent à la main, comme partout dans ces tests :
//! ce qu'on veut savoir ici est qu'un appel franchit la frontière et revient,
//! pas que le décodeur est juste — le noyau en répond.

use std::ffi::CStr;
use std::ptr;

use screengine_ffi::*;

/// Lit le message du thread courant, celui des appels sans contexte.
fn last_error() -> String {
    // SAFETY: le pointeur nul demande l'emplacement du thread, et le pointeur
    // rendu reste valide jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ptr::null())) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

/// Les octets d'une suite d'entiers.
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Les octets d'une suite de flottants.
fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Un repère unitaire : origine nulle, axes sur X et Y.
fn unit_frame() -> Vec<u8> {
    floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])
}

/// Une carte d'une cellule et d'un carré au sol, avec un matériau.
fn floor_world() -> Vec<u8> {
    let mut surface = words(&[11, 0, 1, 4]);
    surface.extend_from_slice(&words(&[0, 1, 2, 3]));
    surface.extend_from_slice(&unit_frame());
    surface.extend_from_slice(&unit_frame());

    let mut body = words(&[7, 0, 4, 1, 0]);
    for point in [
        [0.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 4.0, 0.0],
        [0.0, 4.0, 0.0],
    ] {
        body.extend_from_slice(&floats(&point));
    }
    body.extend_from_slice(&surface);

    let mut cells = words(&[body.len() as u32]);
    cells.extend_from_slice(&body);

    let mut mats = words(&[1]);
    mats.extend_from_slice(&3u16.to_le_bytes());
    mats.extend_from_slice(b"mur");

    file(&cells, &mats)
}

/// Un fichier de carte, ses deux sections et son en-tête.
fn file(cells: &[u8], mats: &[u8]) -> Vec<u8> {
    let table = [(*b"CELL", cells), (*b"MATS", mats)];
    let header = 20 + table.len() * 12;
    let total: usize = header + table.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"WRLD");
    bytes.extend_from_slice(&words(&[1, total as u32, table.len() as u32]));

    let mut offset = header as u32;
    for (kind, body) in &table {
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(&words(&[offset, body.len() as u32]));
        offset += body.len() as u32;
    }
    for (_, body) in &table {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// Charge la carte du fichier et rend son handle.
fn load() -> *mut ScgWorld {
    let bytes = floor_world();
    let mut world = ptr::null_mut();
    // SAFETY: le bloc est vivant pendant tout l'appel, la sortie est inscriptible.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut world) };
    assert_eq!(code, SCG_OK, "{}", last_error());
    world
}

/// Une sortie remplie de valeurs que le moteur doit écraser.
///
/// Jamais mise à zéro : une sortie que l'appel oublierait d'écrire passerait
/// alors pour un contact à l'origine, et le test le confirmerait.
fn dirty_hit() -> ScgSweepHit {
    ScgSweepHit {
        fraction: -1.0,
        normal: [9.0; 3],
        point: [9.0; 3],
        surface_id: 999,
        cell_id: 999,
        reserved0: 7,
        reserved1: 7,
    }
}

/// Un balayage qui ne rencontre rien rend `SCG_OK` et le déplacement entier.
#[test]
fn un_balayage_libre_rend_ok() {
    let world = load();
    let mut hit = dirty_hit();
    let half = [0.25f32; 3];
    let from = [2.0f32, 2.0, 5.0];
    let to = [2.0f32, 2.0, 6.0];

    // SAFETY: handle vivant, trois tableaux de trois flottants, sortie
    // inscriptible.
    let code = unsafe {
        scg_world_sweep(
            world,
            7,
            half.as_ptr(),
            from.as_ptr(),
            to.as_ptr(),
            &mut hit,
        )
    };

    assert_eq!(code, SCG_OK);
    assert_eq!(hit.fraction, 1.0);
    assert_eq!(hit.surface_id, 0, "rien n'est touché");
    assert_eq!(hit.reserved0, 0, "les réservés sont écrits nuls");
    assert_eq!(hit.reserved1, 0);

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Une cellule de départ nulle rend un **statut**, jamais une erreur.
///
/// Un hôte qui juge par `!= SCG_OK` se trompe ici, et c'est la raison d'être du
/// test : poser une caméra dans un interstice pendant une édition est une
/// situation légitime, et le moteur n'a alors rien examiné.
#[test]
fn une_cellule_nulle_rend_un_statut() {
    let world = load();
    let mut hit = dirty_hit();
    let half = [0.25f32; 3];
    let from = [2.0f32, 2.0, 5.0];
    let to = [3.0f32, 2.0, 5.0];

    // SAFETY: handle vivant, trois tableaux de trois flottants, sortie
    // inscriptible.
    let code = unsafe {
        scg_world_sweep(
            world,
            0,
            half.as_ptr(),
            from.as_ptr(),
            to.as_ptr(),
            &mut hit,
        )
    };

    assert_eq!(code, SCG_STATUS_NO_CELL);
    assert!(code > 0, "un statut est un succès");
    assert_eq!(hit.fraction, 1.0, "le déplacement est rendu libre");
    assert_eq!(hit.surface_id, 0);
    assert_eq!(hit.point, to, "le point d'arrivée est celui demandé");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Une cellule que la carte ne porte pas est une ressource inconnue.
///
/// La différence avec le statut ci-dessus est celle qu'`abi.md` écrit : « la
/// caméra n'est nulle part », qui arrive en éditant, et « cette cellule n'existe
/// pas », qui est une faute dans l'appel.
#[test]
fn une_cellule_inconnue_est_une_ressource_inconnue() {
    let world = load();
    let mut hit = dirty_hit();
    let half = [0.25f32; 3];
    let point = [2.0f32, 2.0, 5.0];

    // SAFETY: handle vivant, trois tableaux de trois flottants, sortie
    // inscriptible.
    let code = unsafe {
        scg_world_sweep(
            world,
            99,
            half.as_ptr(),
            point.as_ptr(),
            point.as_ptr(),
            &mut hit,
        )
    };

    assert_eq!(code, SCG_ERR_UNKNOWN_RESOURCE);
    assert!(
        !last_error().is_empty(),
        "le message va dans l'emplacement du thread"
    );

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Les pointeurs nuls et les arguments hors domaine sont refusés sans panique.
///
/// **Une demi-étendue nulle est admise**, et c'est le lancer de rayon ; négative
/// ou non finie, elle ne l'est pas. Le `NaN` se teste par la finitude, avant
/// toute comparaison de bornes, qui le laisseraient passer.
#[test]
fn le_balayage_refuse_ce_qu_il_doit_refuser() {
    let world = load();
    let mut hit = dirty_hit();
    let half = [0.25f32; 3];
    let point = [2.0f32, 2.0, 5.0];

    // SAFETY: tous les pointeurs sont valides sauf celui que chaque appel
    // éprouve, et le handle reste vivant jusqu'à sa destruction.
    unsafe {
        assert_eq!(
            scg_world_sweep(
                ptr::null(),
                7,
                half.as_ptr(),
                point.as_ptr(),
                point.as_ptr(),
                &mut hit
            ),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_sweep(
                world,
                7,
                half.as_ptr(),
                point.as_ptr(),
                point.as_ptr(),
                ptr::null_mut()
            ),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_sweep(
                world,
                7,
                ptr::null(),
                point.as_ptr(),
                point.as_ptr(),
                &mut hit
            ),
            SCG_ERR_NULL
        );

        let zero = [0.0f32; 3];
        assert_eq!(
            scg_world_sweep(
                world,
                7,
                zero.as_ptr(),
                point.as_ptr(),
                point.as_ptr(),
                &mut hit
            ),
            SCG_OK,
            "une étendue nulle balaie un rayon"
        );

        let negative = [-1.0f32, 0.25, 0.25];
        assert_eq!(
            scg_world_sweep(
                world,
                7,
                negative.as_ptr(),
                point.as_ptr(),
                point.as_ptr(),
                &mut hit
            ),
            SCG_ERR_INVALID_ARGUMENT
        );

        let nan = [f32::NAN, 0.25, 0.25];
        assert_eq!(
            scg_world_sweep(
                world,
                7,
                nan.as_ptr(),
                point.as_ptr(),
                point.as_ptr(),
                &mut hit
            ),
            SCG_ERR_INVALID_ARGUMENT
        );

        let far = [f32::INFINITY, 2.0, 5.0];
        assert_eq!(
            scg_world_sweep(
                world,
                7,
                half.as_ptr(),
                far.as_ptr(),
                point.as_ptr(),
                &mut hit
            ),
            SCG_ERR_INVALID_ARGUMENT
        );

        scg_world_destroy(world);
    }
}

/// Le matériau d'une surface se lit par son identifiant stable.
///
/// Sans cet accesseur, l'identifiant que le balayage rend serait un champ mort
/// dans une structure qui ne se modifie plus.
#[test]
fn le_materiau_d_une_surface_se_lit_par_son_identifiant() {
    let world = load();
    let mut material = u32::MAX;

    // SAFETY: handle vivant, sortie inscriptible, détruit une seule fois.
    unsafe {
        assert_eq!(scg_world_surface_material(world, 11, &mut material), SCG_OK);
        assert_eq!(material, 0, "le rang du premier matériau de la table");

        assert_eq!(
            scg_world_surface_material(world, 999, &mut material),
            SCG_ERR_UNKNOWN_RESOURCE
        );
        assert_eq!(
            scg_world_surface_material(world, 11, ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_surface_material(ptr::null(), 11, &mut material),
            SCG_ERR_NULL
        );

        scg_world_destroy(world);
    }
}

/// **Plusieurs threads balaient la même carte et rendent ce qu'un seul rend.**
///
/// Ce qui compte n'est pas que rien n'explose, mais que le résultat soit
/// **identique** à celui d'un appel solitaire : c'est ce que l'immutabilité d'une
/// carte chargée promet, et un partage qui tiendrait par chance se lirait
/// exactement comme un partage qui tient. C'est le premier appel de l'ABI à
/// autoriser la concurrence sur un même objet, et donc le premier à devoir le
/// prouver.
#[test]
fn le_balayage_est_concurrent_sur_une_meme_carte() {
    let world = load();
    let half = [0.5f32; 3];
    let from = [2.0f32, 2.0, 5.0];
    let to = [2.0f32, 2.0, -5.0];

    let sweep_once = |handle: usize| {
        let mut hit = dirty_hit();
        // SAFETY: le handle reste vivant pendant tout l'appel — la destruction
        // n'a lieu qu'après la jonction de tous les threads —, les trois
        // tableaux portent trois flottants, et la sortie est inscriptible.
        let code = unsafe {
            scg_world_sweep(
                handle as *const ScgWorld,
                7,
                half.as_ptr(),
                from.as_ptr(),
                to.as_ptr(),
                &mut hit,
            )
        };
        (code, hit.fraction, hit.surface_id, hit.cell_id, hit.normal)
    };

    // Le handle traverse la frontière des threads par son adresse : l'ABI le
    // déclare partageable en lecture, et c'est exactement cela qu'on éprouve.
    let address = world as usize;
    let alone = sweep_once(address);
    assert_ne!(alone.2, 0, "le balayage de référence touche le sol");

    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| sweep_once(address)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect()
    });

    for (index, result) in results.iter().enumerate() {
        assert_eq!(*result, alone, "thread {index} diverge de l'appel seul");
    }

    // SAFETY: handle vivant, détruit une seule fois, après la jonction.
    unsafe { scg_world_destroy(world) };
}
