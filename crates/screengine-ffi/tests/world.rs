// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le chargement d'une carte, appelé comme le ferait un hôte.
//!
//! Les octets s'écrivent à la main, sans rien partager avec les tests du
//! décodeur : ce qu'on veut savoir ici est qu'un bloc quelconque franchit la
//! frontière et revient en handle, pas que le décodeur est juste — le noyau en
//! répond.

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

/// Une carte d'une cellule, un carré, deux matériaux.
fn one_cell_world() -> Vec<u8> {
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

    let mut mats = Vec::new();
    for (id, name) in [(1u32, "mur"), (2, "plafond")] {
        mats.extend_from_slice(&words(&[id]));
        mats.extend_from_slice(&(name.len() as u16).to_le_bytes());
        mats.extend_from_slice(name.as_bytes());
    }

    file(&cells, &mats, b"WRLD", 1)
}

/// Une carte d'une cellule, une lumière et une entité.
fn peopled_world() -> Vec<u8> {
    let mut light = words(&[41]);
    light.extend_from_slice(&floats(&[4.0, 5.0, 6.0, 8.0]));
    light.extend_from_slice(&[0xF0, 0x80, 0x40, 0]);

    let mut body = words(&[31, 7]);
    body.extend_from_slice(&6u16.to_le_bytes());
    body.extend_from_slice(b"depart");
    body.extend_from_slice(&floats(&[1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 2.0]));
    body.extend_from_slice(&words(&[2]));
    body.extend_from_slice(&[0xDE, 0xAD]);
    let mut entity = words(&[body.len() as u32]);
    entity.extend_from_slice(&body);

    let cells = one_cell_world_cells();
    let mut mats = words(&[1]);
    mats.extend_from_slice(&3u16.to_le_bytes());
    mats.extend_from_slice(b"mur");

    sections(&cells, &entity, &light, &mats, b"WRLD", 1)
}

/// La section des cellules de la carte d'épreuve.
fn one_cell_world_cells() -> Vec<u8> {
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
    cells
}

/// Un fichier de carte, ses quatre sections.
fn sections(
    cells: &[u8],
    ents: &[u8],
    lgts: &[u8],
    mats: &[u8],
    kind: &[u8; 4],
    version: u32,
) -> Vec<u8> {
    let sections: Vec<([u8; 4], &[u8])> = [
        (*b"CELL", cells),
        (*b"ENTS", ents),
        (*b"LGTS", lgts),
        (*b"MATS", mats),
    ]
    .into_iter()
    .filter(|(_, body)| !body.is_empty())
    .collect();

    build(&sections, kind, version)
}

/// Un fichier de carte : en-tête, table de sections, sections.
fn file(cells: &[u8], mats: &[u8], kind: &[u8; 4], version: u32) -> Vec<u8> {
    let sections: Vec<([u8; 4], &[u8])> = [(*b"CELL", cells), (*b"MATS", mats)]
        .into_iter()
        .filter(|(_, body)| !body.is_empty())
        .collect();

    build(&sections, kind, version)
}

/// Assemble l'en-tête, la table et les sections.
fn build(sections: &[([u8; 4], &[u8])], kind: &[u8; 4], version: u32) -> Vec<u8> {
    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(sections.len() as u32).to_le_bytes());

    let mut offset = first;
    for (tag, body) in sections {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        offset += body.len();
    }
    for (_, body) in sections {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// Lit le message d'un contexte, où vont les refus d'une soumission.
fn context_error(ctx: *const ScgContext) -> String {
    // SAFETY: `ctx` est un handle vivant, et le pointeur rendu reste valide
    // jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ctx)) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

/// Lit le message de l'emplacement par thread, où vont les refus des appels qui
/// n'ont pas de contexte auquel se rattacher.
fn orphan_error() -> String {
    // SAFETY: le contrat admet un contexte nul, et le pointeur rendu reste valide
    // jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ptr::null())) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

/// Une configuration de contexte qui passe.
fn config() -> ScgContextConfig {
    ScgContextConfig {
        max_width: 64,
        max_height: 64,
        width: 64,
        height: 64,
        tile_size: 32,
        max_triangles: 0,
        reserved1: 0,
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

/// Une carte se soumet à travers la frontière, avec un matériau sans texture.
#[test]
fn soumet_une_carte() {
    let mut ctx = ptr::null_mut();
    let config = config();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    assert_eq!(unsafe { scg_create(&config, &mut ctx) }, SCG_OK);

    let world = load(&one_cell_world());
    let slots: [*const ScgTexture; 2] = [ptr::null(), ptr::null()];
    let model = identity();
    // SAFETY: contexte et carte vivants, matrice et tableau locaux, le compte
    // étant celui des matériaux de la carte.
    let code = unsafe { scg_submit_world(ctx, &model, world, slots.as_ptr(), 2) };
    assert_eq!(code, SCG_OK, "soumission refusée : {}", context_error(ctx));

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_world_destroy(world);
        scg_destroy(ctx);
    }
}

/// La carte dit combien de cellules elle porte, et l'identifiant de chacune.
#[test]
fn enumere_ses_cellules() {
    let world = load(&one_cell_world());
    let mut count = 0u32;
    // SAFETY: carte vivante, pointeur de sortie local.
    assert_eq!(unsafe { scg_world_cell_count(world, &mut count) }, SCG_OK);
    assert_eq!(count, 1);

    let mut id = 0u32;
    // SAFETY: idem, rang sous le compte.
    assert_eq!(unsafe { scg_world_cell_id(world, 0, &mut id) }, SCG_OK);
    assert_eq!(id, 7);

    // SAFETY: idem, rang au-delà du compte.
    let beyond = unsafe { scg_world_cell_id(world, 1, &mut id) };
    assert_eq!(beyond, SCG_ERR_INVALID_ARGUMENT);

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un point hors de toute cellule rend zéro, qui vaut « nulle part ».
///
/// La carte d'épreuve n'a qu'une face : elle ne délimite aucun volume, donc aucun
/// point n'y est dedans. C'est exactement ce que doit rendre une carte en cours
/// d'édition, et c'est une clause, pas une erreur.
#[test]
fn un_point_hors_de_toute_cellule_rend_zero() {
    let world = load(&one_cell_world());
    let position = [100.0f32, 100.0, 100.0];
    let mut cell = 7u32;
    // SAFETY: carte vivante, trois flottants lisibles, sortie locale.
    let code = unsafe { scg_world_locate(world, position.as_ptr(), &mut cell) };
    assert_eq!(code, SCG_OK, "la localisation n'est pas un échec");
    assert_eq!(cell, 0);

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Une position non finie est refusée.
///
/// Le refus est à la frontière : une comparaison avec `NaN` est fausse dans les
/// deux sens, et le comptage de traversées déclarerait la caméra nulle part sans
/// qu'on sache pourquoi.
#[test]
fn une_position_non_finie_est_refusee() {
    let world = load(&one_cell_world());
    let position = [f32::NAN, 0.0, 0.0];
    let mut cell = 0u32;
    // SAFETY: carte vivante, trois flottants lisibles, sortie locale.
    let code = unsafe { scg_world_locate(world, position.as_ptr(), &mut cell) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un suivi depuis une cellule qui n'existe pas rend zéro.
#[test]
fn un_suivi_depuis_une_cellule_inconnue_rend_zero() {
    let world = load(&one_cell_world());
    let point = [0.0f32, 0.0, 0.0];
    let mut cell = 7u32;
    // SAFETY: carte vivante, deux triplets lisibles, sortie locale.
    let code = unsafe { scg_world_track(world, 99, point.as_ptr(), point.as_ptr(), &mut cell) };
    assert_eq!(code, SCG_OK);
    assert_eq!(cell, 0, "il n'y a pas de fil à reprendre");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Une traversée se lance à travers la frontière et rend un succès.
///
/// La carte n'a qu'une cellule et aucun portail : la traversée la soumet entière,
/// sans atteindre aucune de ses bornes.
#[test]
fn soumet_ce_qu_une_cellule_laisse_voir() {
    let mut ctx = ptr::null_mut();
    let config = config();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    assert_eq!(unsafe { scg_create(&config, &mut ctx) }, SCG_OK);

    let world = load(&one_cell_world());
    let slots: [*const ScgTexture; 2] = [ptr::null(), ptr::null()];
    let model = identity();
    // SAFETY: contexte et carte vivants, matrice et tableau locaux, compte des
    // matériaux exact, et pas de handle de lightmaps.
    let code =
        unsafe { scg_submit_world_visible(ctx, &model, world, slots.as_ptr(), 2, ptr::null(), 7) };
    assert_eq!(code, SCG_OK, "traversée refusée : {}", context_error(ctx));

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_world_destroy(world);
        scg_destroy(ctx);
    }
}

/// Sans cellule de départ, la traversée rend un statut et ne soumet rien.
///
/// **Un code positif est un succès**, et c'est le premier du projet à franchir la
/// frontière : une liaison qui jugerait par « différent de `SCG_OK` » y verrait un
/// échec.
#[test]
fn sans_cellule_la_traversee_rend_un_statut() {
    let mut ctx = ptr::null_mut();
    let config = config();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    assert_eq!(unsafe { scg_create(&config, &mut ctx) }, SCG_OK);

    let world = load(&one_cell_world());
    let slots: [*const ScgTexture; 2] = [ptr::null(), ptr::null()];
    let model = identity();
    // SAFETY: mêmes préconditions ; seule la cellule est nulle.
    let code =
        unsafe { scg_submit_world_visible(ctx, &model, world, slots.as_ptr(), 2, ptr::null(), 0) };
    assert_eq!(code, SCG_STATUS_NO_CELL);
    assert!(code > 0, "un statut est un succès, pas un échec");

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_world_destroy(world);
        scg_destroy(ctx);
    }
}

/// Une cellule que la carte ne porte pas est refusée.
///
/// C'est le premier appel de tout le projet à rendre ce code, réservé depuis
/// l'étape des formats sans qu'aucune fonction ne le produise.
#[test]
fn une_cellule_inconnue_est_refusee() {
    let mut ctx = ptr::null_mut();
    let config = config();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    assert_eq!(unsafe { scg_create(&config, &mut ctx) }, SCG_OK);

    let world = load(&one_cell_world());
    let slots: [*const ScgTexture; 2] = [ptr::null(), ptr::null()];
    let model = identity();
    // SAFETY: mêmes préconditions ; seul l'identifiant ne désigne rien.
    let code =
        unsafe { scg_submit_world_visible(ctx, &model, world, slots.as_ptr(), 2, ptr::null(), 99) };
    assert_eq!(code, SCG_ERR_UNKNOWN_RESOURCE);
    assert!(context_error(ctx).contains("unknown identifier"));

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_world_destroy(world);
        scg_destroy(ctx);
    }
}

/// Les lightmaps se créent, se calculent et se soumettent à travers la frontière.
///
/// **La carte se détruit avant le porteur**, exprès : le handle la garde vivante
/// de son côté, et c'est ce qui permet à un hôte de ne pas avoir à ordonner ses
/// destructions. Sans cette garantie, l'ordre choisi ici rendrait la carte
/// pendante et le calcul lirait de la mémoire libérée.
#[test]
fn les_lightmaps_franchissent_la_frontiere() {
    let world = load(&one_cell_world());
    let mut lighting = ptr::null_mut();
    // SAFETY: carte vivante, pointeur de sortie local.
    let created = unsafe { scg_lighting_create(world, &mut lighting) };
    assert_eq!(created, SCG_OK, "création refusée : {}", orphan_error());
    assert!(!lighting.is_null());

    let mut state = 9u32;
    // SAFETY: handle vivant, sortie locale.
    let read = unsafe { scg_lighting_state(lighting, 7, &mut state) };
    assert_eq!(read, SCG_OK);
    assert_eq!(state, SCG_LIGHTMAP_ABSENT);

    // SAFETY: handle vivant.
    let built = unsafe { scg_lighting_build(lighting, 7) };
    assert_eq!(built, SCG_OK, "cuisson refusée : {}", orphan_error());
    // SAFETY: handle vivant, sortie locale.
    let read = unsafe { scg_lighting_state(lighting, 7, &mut state) };
    assert_eq!(read, SCG_OK);
    assert_eq!(state, SCG_LIGHTMAP_READY);

    // La carte part la première : le porteur en garde une référence.
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_lighting_destroy(lighting) };
    // SAFETY: le contrat dit qu'un pointeur nul ne fait rien.
    unsafe { scg_lighting_destroy(ptr::null_mut()) };
}

/// Une cellule que la carte ne porte pas est refusée au calcul comme à la lecture.
#[test]
fn une_cellule_inconnue_ne_se_cuit_pas() {
    let world = load(&one_cell_world());
    let mut lighting = ptr::null_mut();
    // SAFETY: carte vivante, pointeur de sortie local.
    assert_eq!(unsafe { scg_lighting_create(world, &mut lighting) }, SCG_OK);

    // SAFETY: handle vivant.
    let built = unsafe { scg_lighting_build(lighting, 99) };
    assert_eq!(built, SCG_ERR_UNKNOWN_RESOURCE);

    let mut state = 0u32;
    // SAFETY: handle vivant, sortie locale.
    let read = unsafe { scg_lighting_state(lighting, 99, &mut state) };
    assert_eq!(read, SCG_ERR_UNKNOWN_RESOURCE);

    // SAFETY: handles vivants, détruits une seule fois chacun.
    unsafe {
        scg_lighting_destroy(lighting);
        scg_world_destroy(world);
    }
}

/// Le cache se mesure, s'écrit, puis se reprend.
///
/// **Le patron en deux temps est ce qu'un auteur de liaison lira le plus souvent
/// de travers** : mesurer avec un tampon nul, allouer, remplir. Le test le suit
/// exactement comme un hôte le ferait, et vérifie que la seconde reprise retrouve
/// l'entrée que la première avait cuite.
#[test]
fn le_cache_se_mesure_s_ecrit_et_se_reprend() {
    let world = load(&one_cell_world());
    let mut lighting = ptr::null_mut();
    // SAFETY: carte vivante, pointeur de sortie local.
    let created = unsafe { scg_lighting_create(world, &mut lighting) };
    assert_eq!(created, SCG_OK, "création refusée : {}", orphan_error());

    // SAFETY: handle vivant.
    let built = unsafe { scg_lighting_build(lighting, 7) };
    assert_eq!(built, SCG_OK, "cuisson refusée : {}", orphan_error());

    let mut len = 0usize;
    // SAFETY: handle vivant, tampon nul avec capacité nulle, sortie locale.
    let measured = unsafe { scg_lighting_save(lighting, ptr::null_mut(), 0, &mut len) };
    assert_eq!(measured, SCG_OK, "mesure refusée : {}", orphan_error());
    assert!(len > 0, "un cache d'une cellule cuite n'est pas vide");

    let mut block = vec![0u8; len];
    let mut written = 0usize;
    // SAFETY: handle vivant, tampon local de `len` octets, sortie locale.
    let saved = unsafe { scg_lighting_save(lighting, block.as_mut_ptr(), len, &mut written) };
    assert_eq!(saved, SCG_OK, "écriture refusée : {}", orphan_error());
    assert_eq!(written, len);

    let mut other = ptr::null_mut();
    // SAFETY: carte vivante, pointeur de sortie local.
    let created = unsafe { scg_lighting_create(world, &mut other) };
    assert_eq!(created, SCG_OK);

    let mut accepted = 0u32;
    // SAFETY: handle vivant, bloc local de `len` octets, sortie locale.
    let restored = unsafe { scg_lighting_restore(other, block.as_ptr(), len, &mut accepted) };
    assert_eq!(restored, SCG_OK, "reprise refusée : {}", orphan_error());
    assert_eq!(accepted, 1);

    let mut state = 0u32;
    // SAFETY: handle vivant, sortie locale.
    let read = unsafe { scg_lighting_state(other, 7, &mut state) };
    assert_eq!(read, SCG_OK);
    assert_eq!(state, SCG_LIGHTMAP_READY);

    // SAFETY: handles vivants, détruits une seule fois chacun.
    unsafe {
        scg_lighting_destroy(other);
        scg_lighting_destroy(lighting);
        scg_world_destroy(world);
    }
}

/// Un tampon qui n'a pas la longueur mesurée est refusé, et rien n'est écrit.
///
/// **`out_len` compris** : remplir un paramètre de sortie sur un chemin d'erreur
/// serait la seule exception de toute l'ABI, et une liaison qui s'y fierait lirait
/// une longueur d'un appel qui a échoué.
#[test]
fn un_tampon_de_cache_trop_court_est_refuse_sans_rien_ecrire() {
    let world = load(&one_cell_world());
    let mut lighting = ptr::null_mut();
    // SAFETY: carte vivante, pointeur de sortie local.
    assert_eq!(unsafe { scg_lighting_create(world, &mut lighting) }, SCG_OK);
    // SAFETY: handle vivant.
    assert_eq!(unsafe { scg_lighting_build(lighting, 7) }, SCG_OK);

    let mut len = 0usize;
    // SAFETY: handle vivant, tampon nul avec capacité nulle, sortie locale.
    let measured = unsafe { scg_lighting_save(lighting, ptr::null_mut(), 0, &mut len) };
    assert_eq!(measured, SCG_OK);

    let mut block = vec![0u8; len];
    let mut written = 9usize;
    // SAFETY: handle vivant, tampon local dont on annonce une longueur plus
    // courte que sa vraie taille : l'appel ne doit pas y écrire.
    let short = unsafe { scg_lighting_save(lighting, block.as_mut_ptr(), len - 1, &mut written) };
    assert_eq!(short, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(written, 9, "out_len a été écrit sur un chemin d'erreur");
    assert!(
        block.iter().all(|byte| *byte == 0),
        "le tampon a été touché"
    );

    // Un tampon plus long est en revanche accepté, et seul le bloc est écrit : sa
    // longueur est dans son en-tête, l'hôte n'a pas à retenir sa capacité.
    let mut roomy = vec![0u8; len + 64];
    // SAFETY: handle vivant, tampon local de `len + 64` octets, sortie locale.
    let long = unsafe { scg_lighting_save(lighting, roomy.as_mut_ptr(), len + 64, &mut written) };
    assert_eq!(long, SCG_OK, "tampon plus long refusé : {}", orphan_error());
    assert_eq!(written, len);
    assert!(
        roomy[len..].iter().all(|byte| *byte == 0),
        "l'écriture a débordé du bloc"
    );

    // SAFETY: handles vivants, détruits une seule fois chacun.
    unsafe {
        scg_lighting_destroy(lighting);
        scg_world_destroy(world);
    }
}

/// Un bloc de cache malformé est une erreur de format.
#[test]
fn un_bloc_de_cache_malforme_est_refuse() {
    let world = load(&one_cell_world());
    let mut lighting = ptr::null_mut();
    // SAFETY: carte vivante, pointeur de sortie local.
    assert_eq!(unsafe { scg_lighting_create(world, &mut lighting) }, SCG_OK);

    let junk = *b"pas un bloc";
    let mut accepted = 7u32;
    // SAFETY: handle vivant, tranche locale, sortie locale.
    let refused =
        unsafe { scg_lighting_restore(lighting, junk.as_ptr(), junk.len(), &mut accepted) };
    assert_eq!(refused, SCG_ERR_INVALID_FORMAT);

    // SAFETY: handles vivants, détruits une seule fois chacun.
    unsafe {
        scg_lighting_destroy(lighting);
        scg_world_destroy(world);
    }
}

/// Un compte de textures qui n'est pas celui des matériaux est refusé.
#[test]
fn refuse_un_compte_de_textures_qui_n_est_pas_celui_des_materiaux() {
    let mut ctx = ptr::null_mut();
    let config = config();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    assert_eq!(unsafe { scg_create(&config, &mut ctx) }, SCG_OK);

    let world = load(&one_cell_world());
    let slots: [*const ScgTexture; 2] = [ptr::null(), ptr::null()];
    let model = identity();
    // SAFETY: contexte et carte vivants ; seul le compte annoncé est faux.
    let code = unsafe { scg_submit_world(ctx, &model, world, slots.as_ptr(), 1) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert!(context_error(ctx).contains("texture count"));

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_world_destroy(world);
        scg_destroy(ctx);
    }
}

/// Les pointeurs nuls de la soumission d'une carte sont refusés.
#[test]
fn refuse_les_pointeurs_nuls_de_la_soumission_d_une_carte() {
    let mut ctx = ptr::null_mut();
    let config = config();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    assert_eq!(unsafe { scg_create(&config, &mut ctx) }, SCG_OK);

    let world = load(&one_cell_world());
    let slots: [*const ScgTexture; 2] = [ptr::null(), ptr::null()];
    let model = identity();
    // SAFETY: chaque appel passe un pointeur nul refusé avant lecture.
    unsafe {
        assert_eq!(
            scg_submit_world(ptr::null_mut(), &model, world, slots.as_ptr(), 2),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_submit_world(ctx, ptr::null(), world, slots.as_ptr(), 2),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_submit_world(ctx, &model, ptr::null(), slots.as_ptr(), 2),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_submit_world(ctx, &model, world, ptr::null(), 2),
            SCG_ERR_NULL
        );
        scg_world_destroy(world);
        scg_destroy(ctx);
    }
}

/// Une carte peuplée rend sa lumière dans la structure que l'hôte lui donne.
///
/// **La structure est celle que l'hôte repasse à `scg_set_lights`** : c'est tout
/// ce qu'il en fait à cette étape, et la lui faire reconstruire champ par champ
/// n'aurait servi qu'à respecter la lettre d'un principe qui vise la mémoire du
/// moteur, pas un tampon de l'appelant.
#[test]
fn rend_une_lumiere_dans_la_structure_de_l_hote() {
    let world = load(&peopled_world());
    let mut count = 0;
    let mut light = ScgLight {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        radius: 0.0,
        r: 0,
        g: 0,
        b: 0,
        _reserved: 0xFF,
    };

    // SAFETY: handle vivant, paramètres de sortie locaux.
    unsafe {
        assert_eq!(scg_world_light_count(world, &mut count), SCG_OK);
        assert_eq!(scg_world_light(world, 0, &mut light), SCG_OK);
    }
    assert_eq!(count, 1);
    assert_eq!((light.x, light.y, light.z), (4.0, 5.0, 6.0));
    assert_eq!(light.radius, 8.0);
    assert_eq!((light.r, light.g, light.b), (0xF0, 0x80, 0x40));
    assert_eq!(
        light._reserved, 0,
        "l'octet réservé est écrit nul, ce que le contrat exige de l'hôte"
    );

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Une carte peuplée rend son entité : identifiants, pose, classe, octets.
#[test]
fn rend_une_entite_par_ses_accesseurs() {
    let world = load(&peopled_world());
    let mut count = 0;
    let mut id = 0;
    let mut cell = 0;
    let mut pose = [0.0f32; 7];
    let mut len = 0;

    // SAFETY: handle vivant, paramètres de sortie locaux, `pose` couvrant sept
    // flottants.
    unsafe {
        assert_eq!(scg_world_entity_count(world, &mut count), SCG_OK);
        assert_eq!(scg_world_entity_ids(world, 0, &mut id, &mut cell), SCG_OK);
        assert_eq!(scg_world_entity_pose(world, 0, pose.as_mut_ptr()), SCG_OK);
    }
    assert_eq!(count, 1);
    assert_eq!((id, cell), (31, 7));
    assert_eq!(&pose[..3], &[1.0, 2.0, 3.0]);
    assert_eq!(
        &pose[3..],
        &[0.0, 0.0, 0.0, 1.0],
        "le quaternion est normalisé au chargement"
    );

    // La classe, en deux temps.
    // SAFETY: handle vivant ; le tampon nul avec une capacité nulle mesure.
    let code = unsafe { scg_world_entity_class(world, 0, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(len, 6);
    let mut class = vec![0u8; len + 1];
    // SAFETY: handle vivant, tampon couvrant ce que la mesure demande.
    let code = unsafe {
        scg_world_entity_class(world, 0, class.as_mut_ptr().cast(), class.len(), &mut len)
    };
    assert_eq!(code, SCG_OK);
    assert_eq!(&class[..len], b"depart");

    // Les octets opaques, en deux temps aussi, mais sans terminateur.
    // SAFETY: mêmes préconditions.
    let code = unsafe { scg_world_entity_data(world, 0, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(len, 2);
    let mut data = vec![0u8; len];
    // SAFETY: handle vivant, tampon de la longueur mesurée.
    let code = unsafe { scg_world_entity_data(world, 0, data.as_mut_ptr(), data.len(), &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(data, vec![0xDE, 0xAD]);

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un index au-delà de ce que la carte porte est une faute d'appel.
#[test]
fn refuse_un_index_au_dela_de_la_carte() {
    let world = load(&peopled_world());
    let mut light = ScgLight {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        radius: 0.0,
        r: 0,
        g: 0,
        b: 0,
        _reserved: 0,
    };
    let mut id = 0;
    let mut cell = 0;
    let mut pose = [0.0f32; 7];
    let mut len = usize::MAX;

    // SAFETY: handle vivant, paramètres de sortie locaux.
    unsafe {
        assert_eq!(
            scg_world_light(world, 1, &mut light),
            SCG_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            scg_world_entity_ids(world, 1, &mut id, &mut cell),
            SCG_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            scg_world_entity_pose(world, 1, pose.as_mut_ptr()),
            SCG_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            scg_world_entity_class(world, 1, ptr::null_mut(), 0, &mut len),
            SCG_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            scg_world_entity_data(world, 1, ptr::null_mut(), 0, &mut len),
            SCG_ERR_INVALID_ARGUMENT
        );
        scg_world_destroy(world);
    }
    assert_eq!(len, usize::MAX, "rien n'est écrit sur un index refusé");
}

/// Un tampon d'octets plus court que les données est refusé sans rien écrire.
#[test]
fn refuse_un_tampon_de_donnees_trop_court() {
    let world = load(&peopled_world());
    let mut buf = [0xaau8; 4];
    let mut len = usize::MAX;
    // SAFETY: handle vivant, tampon local couvrant la capacité annoncée.
    let code = unsafe { scg_world_entity_data(world, 0, buf.as_mut_ptr(), 1, &mut len) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(len, usize::MAX);
    assert_eq!(buf, [0xaa; 4]);
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Les pointeurs nuls des accesseurs de lumières et d'entités sont refusés.
#[test]
fn refuse_les_pointeurs_nuls_des_accesseurs_peuples() {
    let world = load(&peopled_world());
    let mut id = 0;
    let mut len = 0;
    // SAFETY: handle vivant ; chaque pointeur nul est refusé avant lecture.
    unsafe {
        assert_eq!(scg_world_light(world, 0, ptr::null_mut()), SCG_ERR_NULL);
        assert_eq!(
            scg_world_entity_ids(world, 0, ptr::null_mut(), &mut id),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_entity_ids(world, 0, &mut id, ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_entity_pose(world, 0, ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_entity_class(world, 0, ptr::null_mut(), 0, ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_entity_data(world, 0, ptr::null_mut(), 8, &mut len),
            SCG_ERR_NULL
        );
        assert_eq!(scg_world_entity_count(ptr::null(), &mut id), SCG_ERR_NULL);
        assert_eq!(scg_world_light_count(ptr::null(), &mut id), SCG_ERR_NULL);
        scg_world_destroy(world);
    }
}

/// Charge une carte, ou échoue en disant pourquoi.
fn load(bytes: &[u8]) -> *mut ScgWorld {
    let mut out = ptr::null_mut();
    // SAFETY: le bloc et le paramètre de sortie sont des valeurs locales
    // vivantes, et la longueur est celle de la tranche.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_OK, "chargement refusé : {}", last_error());
    assert!(!out.is_null());
    out
}

/// Une carte se charge et se détruit sans contexte, comme un maillage.
///
/// C'est ce qui permettra à la collision de charger une carte sur un serveur
/// sans jamais allouer de tampon d'image.
#[test]
fn charge_et_detruit_une_carte_sans_contexte() {
    let world = load(&one_cell_world());
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Détruire un pointeur nul ne fait rien, comme `free`.
#[test]
fn detruire_une_carte_nulle_ne_fait_rien() {
    // SAFETY: le pointeur nul est admis, et la fonction ne fait rien.
    unsafe { scg_world_destroy(ptr::null_mut()) };
}

/// Les deux comptes viennent du fichier, et l'hôte s'en sert avant de créer son
/// contexte.
#[test]
fn rend_les_comptes_de_la_carte() {
    let world = load(&one_cell_world());
    let mut triangles = 0;
    let mut materials = 0;
    // SAFETY: handle vivant, paramètres de sortie locaux.
    unsafe {
        assert_eq!(scg_world_triangle_count(world, &mut triangles), SCG_OK);
        assert_eq!(scg_world_material_count(world, &mut materials), SCG_OK);
        scg_world_destroy(world);
    }
    assert_eq!(triangles, 2, "le carré donne deux triangles");
    assert_eq!(materials, 2);
}

/// Un nom de matériau se lit en deux temps, comme un nom d'emplacement.
#[test]
fn lit_un_nom_de_materiau_en_deux_temps() {
    let world = load(&one_cell_world());
    let mut len = usize::MAX;
    // SAFETY: handle vivant ; le tampon nul avec une capacité nulle est le
    // premier temps documenté.
    let code = unsafe { scg_world_material_name(world, 1, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(len, 7, "« plafond »");

    let mut buf = vec![0u8; len + 1];
    // SAFETY: handle vivant, tampon couvrant la capacité que la mesure demande.
    let code =
        unsafe { scg_world_material_name(world, 1, buf.as_mut_ptr().cast(), buf.len(), &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(&buf[..len], b"plafond");
    assert_eq!(buf[len], 0, "le tampon rendu est terminé par un octet nul");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un tampon trop court est refusé sans rien écrire, `out_len` compris.
///
/// La même clause que pour un nom d'emplacement, et c'est le même code des deux
/// côtés : un protocole recopié qui divergerait d'un mot ferait mentir le header
/// pour l'une des deux ressources.
#[test]
fn refuse_un_tampon_de_nom_trop_court() {
    let world = load(&one_cell_world());
    let mut buf = [0xaau8; 16];
    let mut len = usize::MAX;
    // SAFETY: handle vivant, tampon local couvrant la capacité annoncée.
    let code = unsafe { scg_world_material_name(world, 1, buf.as_mut_ptr().cast(), 4, &mut len) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(len, usize::MAX, "rien n'est écrit dans out_len");
    assert_eq!(buf, [0xaa; 16], "rien n'est écrit dans le tampon");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un indice de matériau au-delà du dernier est une faute d'appel.
#[test]
fn refuse_un_materiau_au_dela_du_dernier() {
    let world = load(&one_cell_world());
    let mut len = usize::MAX;
    // SAFETY: handle vivant, paramètre de sortie local.
    let code = unsafe { scg_world_material_name(world, 2, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(len, usize::MAX);
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un maillage là où une carte est attendue est refusé par le genre.
#[test]
fn refuse_un_maillage_au_chargement_d_une_carte() {
    let bytes = file(&[], &[], b"MESH", 1);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_INVALID_FORMAT);
    assert!(out.is_null());
    assert!(last_error().contains("kind"));
}

/// Une version de format inconnue a son propre code.
#[test]
fn refuse_une_version_de_carte_inconnue() {
    let bytes = file(&[], &[], b"WRLD", 3);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_UNSUPPORTED_FORMAT_VERSION);
    assert!(out.is_null());
}

/// Les pointeurs nuls du chargement et des accesseurs sont refusés.
#[test]
fn refuse_les_pointeurs_nuls() {
    let bytes = one_cell_world();
    let world = load(&bytes);
    let mut count = 0;
    let mut len = 0;
    // SAFETY: handle vivant ; chaque pointeur nul est refusé avant lecture.
    unsafe {
        assert_eq!(
            scg_world_load(bytes.as_ptr(), bytes.len(), ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_load(ptr::null(), 20, &mut ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_triangle_count(ptr::null(), &mut count),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_material_count(world, ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_material_name(ptr::null(), 0, ptr::null_mut(), 0, &mut len),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_material_name(world, 0, ptr::null_mut(), 0, ptr::null_mut()),
            SCG_ERR_NULL
        );
        scg_world_destroy(world);
    }
}

/// Une carte malformée est refusée par le code des données, et le message dit
/// quoi.
#[test]
fn refuse_une_carte_malformee() {
    // Une cellule dont l'identifiant est nul : l'éditeur réserve zéro à
    // « aucun ».
    let mut body = words(&[0, 0, 0, 0, 0]);
    body.truncate(20);
    let mut cells = words(&[body.len() as u32]);
    cells.extend_from_slice(&body);

    let bytes = file(&cells, &[], b"WRLD", 1);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_INVALID_FORMAT);
    assert!(out.is_null());
    assert!(last_error().contains("identifier"));
}

/// La même carte se lit depuis plusieurs threads, étant immuable et sans
/// contexte.
#[test]
fn une_carte_se_lit_depuis_plusieurs_threads() {
    let world = load(&one_cell_world());
    let address = world as usize;

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(move || {
                let handle = address as *const ScgWorld;
                let mut count = 0;
                // SAFETY: le handle reste vivant pendant toute la portée, et la
                // ressource est immuable : les accesseurs ne font que lire.
                let code = unsafe { scg_world_triangle_count(handle, &mut count) };
                assert_eq!(code, SCG_OK);
                assert_eq!(count, 2);
            });
        }
    });

    // SAFETY: handle vivant, détruit une seule fois, après la portée.
    unsafe { scg_world_destroy(world) };
}
