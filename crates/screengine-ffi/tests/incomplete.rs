// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `SCG_STATUS_INCOMPLETE` sur les trois points d'entrée qui le rendent.
//!
//! **Un fichier par statut et non par point d'entrée**, parce que c'est le décor
//! qui est partagé : une enfilade assez longue sature la profondeur de traversée
//! d'un côté et le budget de cellules du balayage de l'autre, et l'écrire une fois
//! vaut mieux que la recopier dans celui de la carte puis dans celui du balayage.
//!
//! Ce que ces cas ferment : le statut n'était observé **nulle part à travers la
//! frontière**, ni pour la traversée, ni pour le balayage, ni pour la sélection.
//! Les bornes qui le produisent sont éprouvées côté noyau, où elles ont leur
//! géométrie ; ce qui manquait est qu'un appelant le reçoive, puisque c'est lui
//! qui doit le distinguer d'un échec — un code positif est un succès.

use std::ffi::CStr;
use std::ptr;

use screengine_ffi::*;

/// Lit le message de l'emplacement par thread, celui des appels sans contexte.
fn last_error() -> String {
    // SAFETY: le pointeur nul demande l'emplacement du thread, et le pointeur
    // rendu reste valide jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ptr::null())) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

/// Lit le message d'un contexte.
fn context_error(ctx: *const ScgContext) -> String {
    // SAFETY: `ctx` est un handle vivant, et le pointeur rendu reste valide
    // jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ctx)) };
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

/// Un repère d'origine nulle porté par ces deux axes.
///
/// Les axes doivent appartenir au plan de la surface et être orthogonaux, ce que
/// le chargement vérifie : d'où un repère par orientation de face plutôt qu'un
/// repère unitaire commun.
fn frame(u: [f32; 3], v: [f32; 3]) -> Vec<u8> {
    let mut bytes = floats(&[0.0, 0.0, 0.0]);
    bytes.extend_from_slice(&floats(&u));
    bytes.extend_from_slice(&floats(&v));
    bytes
}

/// Les octets d'une surface opaque de la cellule, repères compris.
fn surface(id: u32, indices: &[u32], u: [f32; 3], v: [f32; 3]) -> Vec<u8> {
    let mut bytes = words(&[id, 0, 1, indices.len() as u32]);
    bytes.extend_from_slice(&words(indices));
    bytes.extend_from_slice(&frame(u, v));
    bytes.extend_from_slice(&frame(u, v));
    bytes
}

/// Les octets d'un portail : ni drapeaux ni matériau, il n'est pas dessiné.
fn portal(id: u32, indices: &[u32]) -> Vec<u8> {
    let mut bytes = words(&[id, indices.len() as u32]);
    bytes.extend_from_slice(&words(indices));
    bytes
}

/// Les octets d'une cellule, son enregistrement préfixé de sa longueur.
fn cell(id: u32, points: &[[f32; 3]], surfaces: &[Vec<u8>], portals: &[Vec<u8>]) -> Vec<u8> {
    let mut body = words(&[
        id,
        0,
        points.len() as u32,
        surfaces.len() as u32,
        portals.len() as u32,
    ]);
    for point in points {
        body.extend_from_slice(&floats(point));
    }
    for surface in surfaces {
        body.extend_from_slice(surface);
    }
    for portal in portals {
        body.extend_from_slice(portal);
    }

    let mut bytes = words(&[body.len() as u32]);
    bytes.extend_from_slice(&body);
    bytes
}

/// Un fichier de carte, sa table de sections et son en-tête.
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

/// Une enfilade de `count` cubes de quatre unités, alignés sur `X`.
///
/// **Toutes les coordonnées sont des multiples de quatre**, et c'est ce qui fait
/// que ces cas mesurent ce qu'ils prétendent : deux cellules voisines écrivent
/// alors leur portail commun avec **les mêmes bits**, donc il s'apparie.
/// L'appariement étant exact et sans tolérance, un bit de travers en ferait un
/// mur — la traversée s'arrêterait à la première cellule, le balayage au premier
/// portail, et les deux cas passeraient au vert sans jamais approcher leur borne.
///
/// Les deux faces perpendiculaires à `X` sont des portails ; aux deux bouts de la
/// chaîne ils restent non appariés, donc solides.
fn chain(count: u32) -> Vec<u8> {
    let mut cells = Vec::new();
    for index in 0..count {
        let x0 = (index * 4) as f32;
        let x1 = x0 + 4.0;
        let points = [
            [x0, 0.0, 0.0],
            [x1, 0.0, 0.0],
            [x1, 4.0, 0.0],
            [x0, 4.0, 0.0],
            [x0, 0.0, 4.0],
            [x1, 0.0, 4.0],
            [x1, 4.0, 4.0],
            [x0, 4.0, 4.0],
        ];
        let first = 100 + index * 10;
        let surfaces = [
            surface(first + 1, &[0, 3, 2, 1], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            surface(first + 2, &[4, 5, 6, 7], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            surface(first + 3, &[0, 1, 5, 4], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            surface(first + 4, &[3, 7, 6, 2], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ];
        let portals = [
            portal(first + 5, &[0, 4, 7, 3]),
            portal(first + 6, &[1, 2, 6, 5]),
        ];
        cells.extend_from_slice(&cell(index + 1, &points, &surfaces, &portals));
    }

    let mut mats = words(&[1]);
    mats.extend_from_slice(&3u16.to_le_bytes());
    mats.extend_from_slice(b"mur");
    file(&cells, &mats)
}

/// Charge une carte et rend son handle.
fn load(bytes: &[u8]) -> *mut ScgWorld {
    let mut out = ptr::null_mut();
    // SAFETY: le bloc et la sortie sont des valeurs locales vivantes, et la
    // longueur est celle de la tranche.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_OK, "chargement refusé : {}", last_error());
    assert!(!out.is_null());
    out
}

/// Une configuration de contexte qui passe.
fn config() -> ScgContextConfig {
    ScgContextConfig {
        max_width: 64,
        max_height: 64,
        width: 64,
        height: 64,
        tile_size: 32,
        // La chaîne porte quatre surfaces par cellule et la traversée en déplie
        // soixante-cinq : la capacité par défaut n'y suffit pas, et un refus de
        // capacité masquerait le statut qu'on vient lire.
        max_triangles: 8192,
        max_lines: 0,
        reserved2: 0,
    }
}

/// La matrice identité, par colonnes.
fn identity() -> ScgMat4 {
    ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    }
}

/// Une caméra posée dans la première cellule, regardant l'enfilade.
///
/// L'orientation neutre regarde le `+X` du monde, qui est l'axe de la chaîne : la
/// caméra voit donc à travers tous les portails, et c'est ce qui fait descendre la
/// traversée jusqu'à sa borne de profondeur.
fn camera() -> ScgCamera {
    ScgCamera {
        position: [2.0, 2.0, 2.0],
        orientation: [0.0, 0.0, 0.0, 1.0],
        fov_y: 1.5,
        near_plane: 0.1,
    }
}

/// Une sortie de balayage remplie de valeurs que l'appel doit écraser.
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

/// La traversée rend `SCG_STATUS_INCOMPLETE` quand sa profondeur est atteinte.
///
/// Une enfilade plus longue que `SCG_TRAVERSAL_DEPTH`, vue depuis son premier
/// cube : la borne tombe avant le bout. Le statut reste **positif**, donc un
/// succès — la cellule du fond est dessinée entière, seuls ses portails ne sont
/// pas dépliés.
#[test]
fn la_traversee_rend_le_statut_quand_sa_profondeur_est_atteinte() {
    let mut ctx = ptr::null_mut();
    let config = config();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    assert_eq!(unsafe { scg_create(&config, &mut ctx) }, SCG_OK);

    let camera = camera();
    // SAFETY: contexte vivant, caméra locale.
    assert_eq!(unsafe { scg_set_camera(ctx, &camera) }, SCG_OK);

    let world = load(&chain(SCG_TRAVERSAL_DEPTH + 8));
    let slots: [*const ScgTexture; 1] = [ptr::null()];
    let model = identity();
    // SAFETY: contexte et carte vivants, compte des matériaux exact, pas de
    // lightmaps, et la première cellule porte l'identifiant 1.
    let code =
        unsafe { scg_submit_world_visible(ctx, &model, world, slots.as_ptr(), 1, ptr::null(), 1) };
    assert_eq!(
        code,
        SCG_STATUS_INCOMPLETE,
        "la traversée n'a pas atteint sa borne : {}",
        context_error(ctx)
    );
    assert!(code > 0, "un statut est un succès, pas un échec");

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_world_destroy(world);
        scg_destroy(ctx);
    }
}

/// Le balayage rend `SCG_STATUS_INCOMPLETE` et tronque le déplacement.
///
/// **Le statut ne suffit pas à juger l'appel**, et c'est pourquoi la fraction est
/// lue avec lui : posée à `1`, elle annoncerait un déplacement libre que la borne
/// démentirait, et un hôte qui lit le statut comme un avertissement traverserait
/// le mur qu'on n'a pas eu le temps de regarder.
#[test]
fn le_balayage_rend_le_statut_et_tronque() {
    let count = SCG_SWEEP_CELLS + 8;
    let world = load(&chain(count));
    let mut hit = dirty_hit();
    let half = [0.5f32; 3];
    let from = [2.0f32, 2.0, 2.0];
    let to = [(count * 4) as f32 - 2.0, 2.0, 2.0];

    // SAFETY: carte vivante, trois tableaux de trois flottants, sortie
    // inscriptible.
    let code = unsafe {
        scg_world_sweep(
            world,
            1,
            half.as_ptr(),
            from.as_ptr(),
            to.as_ptr(),
            &mut hit,
        )
    };
    assert_eq!(
        code,
        SCG_STATUS_INCOMPLETE,
        "le balayage n'a pas épuisé son budget : {}",
        last_error()
    );
    assert!(code > 0, "un statut est un succès, pas un échec");
    assert!(
        hit.fraction < 1.0,
        "le déplacement est tronqué, fraction {}",
        hit.fraction
    );
    assert_eq!(hit.surface_id, 0, "aucune surface n'arrête le trajet");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// La sélection rend le même statut, et jamais `SCG_STATUS_NO_GAP`.
///
/// Elle partage le budget du balayage, donc la même enfilade le lui épuise. Ce
/// qu'elle ne peut pas rendre est le jeu perdu : la dilatation d'un rayon est
/// nulle par construction, donc il n'a aucun jeu à perdre.
#[test]
fn la_selection_rend_le_statut_du_balayage() {
    let count = SCG_SWEEP_CELLS + 8;
    let world = load(&chain(count));
    let mut hit = dirty_hit();
    let from = [2.0f32, 2.0, 2.0];
    let to = [(count * 4) as f32 - 2.0, 2.0, 2.0];

    // SAFETY: carte vivante, deux tableaux de trois flottants, sortie
    // inscriptible.
    let code = unsafe {
        scg_world_pick(
            world,
            1,
            from.as_ptr(),
            to.as_ptr(),
            SCG_PICK_SOLID,
            &mut hit,
        )
    };
    assert_eq!(
        code,
        SCG_STATUS_INCOMPLETE,
        "la sélection n'a pas épuisé son budget : {}",
        last_error()
    );
    assert!(
        hit.fraction < 1.0,
        "le rayon est tronqué, fraction {}",
        hit.fraction
    );

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}
