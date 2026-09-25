// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le chargement d'un maillage, appelé comme le ferait un hôte.
//!
//! Un fichier à part de `boundary.rs`, qui porte déjà les points d'entrée du
//! rendu : ce qui est éprouvé ici est une ressource et ses accesseurs, sans
//! contexte, sans image et sans tampon de sortie.
//!
//! Les octets s'écrivent à la main, sans rien partager avec les tests du
//! décodeur : ce sont deux crates, et ce qu'on veut savoir ici est qu'un bloc
//! quelconque franchit la frontière et revient en handle — pas que le décodeur
//! est juste, ce dont le noyau répond.

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

/// Lit le message d'un contexte, où vont les refus d'une soumission.
fn context_error(ctx: *const ScgContext) -> String {
    // SAFETY: `ctx` est un handle vivant, et le pointeur rendu reste valide
    // jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ctx)) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

/// Un fichier de maillage bien formé : en-tête de vingt octets, table de
/// sections, puis les sections dans l'ordre reçu.
fn mesh_bytes(sections: &[([u8; 4], Vec<u8>)], kind: &[u8; 4], version: u32) -> Vec<u8> {
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

/// Un maillage d'un triangle, trois sommets, un emplacement nommé.
fn one_triangle_mesh() -> Vec<u8> {
    let mut vertices = Vec::new();
    for (x, y) in [(0.0f32, 0.0f32), (1.0, 0.0), (0.0, 1.0)] {
        for value in [x, y, -2.0, 0.0, 0.0] {
            vertices.extend_from_slice(&value.to_le_bytes());
        }
    }

    let mut triangles = Vec::new();
    for index in [0u32, 1, 2] {
        triangles.extend_from_slice(&index.to_le_bytes());
    }
    triangles.extend_from_slice(&[255, 255, 255, 255]);

    let mut groups = Vec::new();
    for value in [3u32, 0, 1, 0] {
        groups.extend_from_slice(&value.to_le_bytes());
    }

    let mut names = 3u16.to_le_bytes().to_vec();
    names.extend_from_slice(b"mur");

    mesh_bytes(
        &[
            (*b"SURF", groups),
            (*b"TEXN", names),
            (*b"TRIS", triangles),
            (*b"VTXS", vertices),
        ],
        b"MESH",
        1,
    )
}

/// Charge un maillage, ou échoue en disant pourquoi.
fn load(bytes: &[u8]) -> *mut ScgMesh {
    let mut out = ptr::null_mut();
    // SAFETY: le bloc et le paramètre de sortie sont des valeurs locales
    // vivantes, et la longueur est celle de la tranche.
    let code = unsafe { scg_mesh_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_OK, "chargement refusé : {}", last_error());
    assert!(!out.is_null());
    out
}

/// Un maillage se charge et se détruit sans contexte : c'est ce qui permettra à
/// la collision de le faire sans jamais allouer de tampon d'image.
#[test]
fn charge_et_detruit_un_maillage_sans_contexte() {
    let mesh = load(&one_triangle_mesh());
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_mesh_destroy(mesh) };
}

/// Un maillage vide est un fichier valide, et son handle s'interroge comme les
/// autres.
#[test]
fn un_maillage_vide_franchit_la_frontiere() {
    let mesh = load(&mesh_bytes(&[], b"MESH", 1));
    let mut count = 1;
    // SAFETY: handle vivant, paramètre de sortie local.
    let code = unsafe { scg_mesh_triangle_count(mesh, &mut count) };
    assert_eq!(code, SCG_OK);
    assert_eq!(count, 0);
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_mesh_destroy(mesh) };
}

/// Détruire un pointeur nul ne fait rien, comme `free`.
#[test]
fn detruire_un_maillage_nul_ne_fait_rien() {
    // SAFETY: le pointeur nul est admis, et la fonction ne fait rien.
    unsafe { scg_mesh_destroy(ptr::null_mut()) };
}

/// Un bloc qui n'est pas un fichier du projet est refusé par le code des
/// données et non par celui des arguments.
///
/// C'est toute la raison d'être de la plage : la faute est dans le contenu, que
/// l'hôte n'a pas écrit, et c'est ce qu'il rapporte à son utilisateur comme
/// « ce fichier est mauvais ».
#[test]
fn refuse_un_bloc_qui_n_est_pas_un_maillage() {
    let bytes = *b"pas un fichier de ce projet";
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_mesh_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_INVALID_FORMAT);
    assert!(out.is_null(), "rien ne doit être écrit en cas d'échec");
    assert!(last_error().contains("signature"));
}

/// Une version de format inconnue a son propre code, le seul des trois qui dise
/// à l'hôte quoi faire.
#[test]
fn refuse_une_version_de_format_inconnue() {
    let bytes = mesh_bytes(&[], b"MESH", 2);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_mesh_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_UNSUPPORTED_FORMAT_VERSION);
    assert!(out.is_null());
    assert!(last_error().contains("version"));
}

/// Une carte là où un maillage est attendu est refusée par le genre, et le
/// message le dit.
#[test]
fn refuse_une_carte_au_chargement_d_un_maillage() {
    let bytes = mesh_bytes(&[], b"WRLD", 1);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_mesh_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_INVALID_FORMAT);
    assert!(last_error().contains("kind"));
}

/// Un bloc plus long que ce que son en-tête annonce est refusé.
///
/// C'est la clause qui garde l'empreinte d'intégrité de l'hôte accordée à celle
/// du moteur : tolérés, deux fichiers d'octets différents rendraient la même
/// ressource.
#[test]
fn refuse_un_bloc_plus_long_que_sa_longueur_annoncee() {
    let mut bytes = one_triangle_mesh();
    bytes.push(0);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_mesh_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_INVALID_FORMAT);
    assert!(out.is_null());
}

/// Les pointeurs nuls du chargement sont refusés, et un bloc vide reste
/// admissible avec un pointeur nul.
#[test]
fn refuse_les_pointeurs_nuls_du_chargement() {
    let bytes = one_triangle_mesh();
    // SAFETY: bloc local vivant ; le paramètre de sortie est nul, ce que la
    // fonction refuse avant de l'écrire.
    let code = unsafe { scg_mesh_load(bytes.as_ptr(), bytes.len(), ptr::null_mut()) };
    assert_eq!(code, SCG_ERR_NULL, "paramètre de sortie nul");

    let mut out = ptr::null_mut();
    // SAFETY: le bloc est nul avec une longueur non nulle, ce que la fonction
    // refuse avant de le lire.
    let code = unsafe { scg_mesh_load(ptr::null(), 20, &mut out) };
    assert_eq!(code, SCG_ERR_NULL, "bloc nul de longueur non nulle");
    assert!(out.is_null());

    // SAFETY: un bloc nul de longueur nulle est admis, et refusé pour son
    // contenu — il n'a pas même de signature.
    let code = unsafe { scg_mesh_load(ptr::null(), 0, &mut out) };
    assert_eq!(code, SCG_ERR_INVALID_FORMAT, "bloc vide");
}

/// Les deux comptes viennent du fichier, et l'hôte s'en sert avant de créer le
/// contexte auquel il soumettra.
#[test]
fn rend_les_comptes_du_maillage() {
    let mesh = load(&one_triangle_mesh());

    let mut triangles = 0;
    let mut textures = 0;
    // SAFETY: handle vivant, paramètres de sortie locaux.
    unsafe {
        assert_eq!(scg_mesh_triangle_count(mesh, &mut triangles), SCG_OK);
        assert_eq!(scg_mesh_texture_count(mesh, &mut textures), SCG_OK);
        scg_mesh_destroy(mesh);
    }
    assert_eq!(triangles, 1);
    assert_eq!(textures, 1);
}

/// Un pointeur nul aux accesseurs est refusé, du côté du handle comme du côté
/// du paramètre de sortie.
#[test]
fn refuse_un_pointeur_nul_aux_accesseurs() {
    let mesh = load(&one_triangle_mesh());
    let mut count = 0;
    // SAFETY: handle vivant, et les pointeurs nuls sont refusés avant lecture.
    unsafe {
        assert_eq!(
            scg_mesh_triangle_count(ptr::null(), &mut count),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_mesh_texture_count(ptr::null(), &mut count),
            SCG_ERR_NULL
        );
        assert_eq!(scg_mesh_triangle_count(mesh, ptr::null_mut()), SCG_ERR_NULL);
        assert_eq!(scg_mesh_texture_count(mesh, ptr::null_mut()), SCG_ERR_NULL);
        scg_mesh_destroy(mesh);
    }
}

/// Lit un nom d'emplacement en deux temps, comme le ferait un hôte.
fn slot_name(mesh: *const ScgMesh, slot: u32) -> String {
    let mut len = usize::MAX;
    // SAFETY: handle vivant ; le tampon nul avec une capacité nulle est le
    // premier temps documenté, et `out_len` est une valeur locale.
    let code = unsafe { scg_mesh_texture_name(mesh, slot, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_OK, "mesure refusée : {}", last_error());

    let mut buf = vec![0u8; len + 1];
    // SAFETY: handle vivant ; le tampon couvre `len + 1` octets inscriptibles,
    // ce que la mesure vient de demander.
    let code =
        unsafe { scg_mesh_texture_name(mesh, slot, buf.as_mut_ptr().cast(), buf.len(), &mut len) };
    assert_eq!(code, SCG_OK, "lecture refusée : {}", last_error());
    assert_eq!(buf[len], 0, "le tampon rendu est terminé par un octet nul");

    String::from_utf8(buf[..len].to_vec()).expect("UTF-8 valide")
}

/// Un nom se lit en deux temps, et la longueur rendue ne compte pas le
/// terminateur.
#[test]
fn lit_un_nom_d_emplacement_en_deux_temps() {
    let mesh = load(&one_triangle_mesh());
    assert_eq!(slot_name(mesh, 0), "mur");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_mesh_destroy(mesh) };
}

/// Un tampon trop court est refusé sans rien écrire, `out_len` compris.
///
/// C'est la clause qui fait de la mesure le seul chemin vers la longueur : un
/// `out_len` rempli sur le chemin d'erreur serait la seule exception de toute
/// l'ABI, et une liaison finirait par en dépendre.
#[test]
fn refuse_un_tampon_de_nom_trop_court() {
    let mesh = load(&one_triangle_mesh());

    // « mur » et son terminateur en demandent quatre.
    for cap in 1..4 {
        let mut buf = [0xaau8; 8];
        let mut len = usize::MAX;
        // SAFETY: handle vivant, tampon local couvrant la capacité annoncée.
        let code =
            unsafe { scg_mesh_texture_name(mesh, 0, buf.as_mut_ptr().cast(), cap, &mut len) };
        assert_eq!(code, SCG_ERR_INVALID_ARGUMENT, "capacité {cap}");
        assert_eq!(len, usize::MAX, "rien n'est écrit dans out_len");
        assert_eq!(buf, [0xaa; 8], "rien n'est écrit dans le tampon");
        assert!(last_error().contains("measure"));
    }

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_mesh_destroy(mesh) };
}

/// Un emplacement au-delà du dernier est une faute d'appel, pas un mauvais
/// fichier.
#[test]
fn refuse_un_emplacement_au_dela_du_dernier() {
    let mesh = load(&one_triangle_mesh());
    let mut len = usize::MAX;
    // SAFETY: handle vivant, paramètre de sortie local.
    let code = unsafe { scg_mesh_texture_name(mesh, 1, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(len, usize::MAX);
    assert!(last_error().contains("slot"));
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_mesh_destroy(mesh) };
}

/// Les pointeurs nuls de la lecture d'un nom sont refusés.
#[test]
fn refuse_les_pointeurs_nuls_de_la_lecture_d_un_nom() {
    let mesh = load(&one_triangle_mesh());
    let mut len = 0;
    let mut buf = [0u8; 8];
    // SAFETY: handle vivant ; chaque pointeur nul est refusé avant lecture.
    unsafe {
        assert_eq!(
            scg_mesh_texture_name(mesh, 0, ptr::null_mut(), 0, ptr::null_mut()),
            SCG_ERR_NULL,
            "out_len est obligatoire dans les deux temps"
        );
        assert_eq!(
            scg_mesh_texture_name(ptr::null(), 0, ptr::null_mut(), 0, &mut len),
            SCG_ERR_NULL,
            "handle nul"
        );
        assert_eq!(
            scg_mesh_texture_name(mesh, 0, ptr::null_mut(), buf.len(), &mut len),
            SCG_ERR_NULL,
            "tampon nul avec une capacité non nulle"
        );
        // Un tampon plus grand que nécessaire passe : la capacité est un
        // minimum, pas une égalité.
        assert_eq!(
            scg_mesh_texture_name(mesh, 0, buf.as_mut_ptr().cast(), buf.len(), &mut len),
            SCG_OK
        );
        scg_mesh_destroy(mesh);
    }
    assert_eq!(len, 3);
    assert_eq!(&buf[..4], b"mur\0");
}

/// Un nom vide se lit comme les autres, et son tampon d'un octet porte le seul
/// terminateur.
#[test]
fn lit_un_nom_vide() {
    let mut names = 0u16.to_le_bytes().to_vec();
    names.extend_from_slice(b"");
    let mesh = load(&mesh_bytes(&[(*b"TEXN", names)], b"MESH", 1));

    let mut len = usize::MAX;
    let mut buf = [0xaau8; 2];
    // SAFETY: handle vivant, tampon local d'un octet utile.
    let code = unsafe { scg_mesh_texture_name(mesh, 0, buf.as_mut_ptr().cast(), 1, &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(len, 0);
    assert_eq!(buf, [0, 0xaa], "le terminateur, et rien de plus");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_mesh_destroy(mesh) };
}

/// Un nom accentué traverse la frontière intact, octet pour octet.
#[test]
fn un_nom_accentue_traverse_la_frontiere() {
    let mut names = (b"b\xc3\xa9ton".len() as u16).to_le_bytes().to_vec();
    names.extend_from_slice("béton".as_bytes());
    let mesh = load(&mesh_bytes(&[(*b"TEXN", names)], b"MESH", 1));

    assert_eq!(slot_name(mesh, 0), "béton");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_mesh_destroy(mesh) };
}

/// Une configuration de contexte qui passe, pour les soumissions.
fn config(max_triangles: u32) -> ScgContextConfig {
    ScgContextConfig {
        max_width: 64,
        max_height: 64,
        width: 64,
        height: 64,
        tile_size: 32,
        max_triangles,
        reserved1: 0,
        reserved2: 0,
    }
}

/// Un contexte, ou l'échec de sa création.
fn context(max_triangles: u32) -> *mut ScgContext {
    let mut ctx = ptr::null_mut();
    let config = config(max_triangles);
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    let code = unsafe { scg_create(&config, &mut ctx) };
    assert_eq!(code, SCG_OK, "contexte refusé : {}", last_error());
    ctx
}

/// La matrice identité, telle que l'ABI l'attend.
fn identity() -> ScgMat4 {
    ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    }
}

/// Un maillage se soumet à travers la frontière, avec un emplacement sans
/// texture.
#[test]
fn soumet_un_maillage_sans_texture() {
    let ctx = context(0);
    let mesh = load(&one_triangle_mesh());
    let slots: [*const ScgTexture; 1] = [ptr::null()];
    let model = identity();

    // SAFETY: contexte et maillage vivants, matrice et tableau locaux, le compte
    // étant celui du tableau.
    let code = unsafe { scg_submit_mesh(ctx, &model, mesh, slots.as_ptr(), 1) };
    assert_eq!(code, SCG_OK, "soumission refusée : {}", last_error());

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_mesh_destroy(mesh);
        scg_destroy(ctx);
    }
}

/// Un compte de textures qui n'est pas celui de la ressource est refusé, dans
/// les deux sens.
///
/// Une égalité et non un minimum : un tableau plus long signale que l'hôte s'est
/// trompé de ressource, et le laisser passer ferait dessiner un maillage avec
/// l'habillage d'un autre.
#[test]
fn refuse_un_compte_de_textures_qui_n_est_pas_celui_de_la_ressource() {
    let ctx = context(0);
    let mesh = load(&one_triangle_mesh());
    let slots: [*const ScgTexture; 2] = [ptr::null(), ptr::null()];
    let model = identity();

    // SAFETY: contexte et maillage vivants ; le tableau couvre bien deux
    // entrées, seul le compte annoncé est faux.
    let trop = unsafe { scg_submit_mesh(ctx, &model, mesh, slots.as_ptr(), 2) };
    assert_eq!(trop, SCG_ERR_INVALID_ARGUMENT, "un emplacement de trop");
    assert!(context_error(ctx).contains("texture count"));

    // SAFETY: mêmes handles ; un compte nul avec un tableau nul est la forme que
    // la fonction admet, et c'est le compte qui est faux.
    let trop_peu = unsafe { scg_submit_mesh(ctx, &model, mesh, ptr::null(), 0) };
    assert_eq!(trop_peu, SCG_ERR_INVALID_ARGUMENT, "aucun emplacement");

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_mesh_destroy(mesh);
        scg_destroy(ctx);
    }
}

/// Une soumission pendant une image ouverte est refusée.
///
/// C'est le contrôle que l'API Rust ne peut pas porter : son type d'image
/// emprunte le contexte, alors qu'un hôte C garde deux handles et peut appeler
/// dans n'importe quel ordre.
#[test]
fn refuse_un_maillage_soumis_pendant_une_image() {
    let ctx = context(0);
    let mesh = load(&one_triangle_mesh());
    let slots: [*const ScgTexture; 1] = [ptr::null()];
    let model = identity();

    let mut tiles = 0;
    // SAFETY: contexte vivant, paramètre de sortie local.
    let code = unsafe { scg_frame_begin(ctx, &mut tiles) };
    assert_eq!(code, SCG_OK);
    // SAFETY: contexte et maillage vivants, tableau local d'une entrée.
    let code = unsafe { scg_submit_mesh(ctx, &model, mesh, slots.as_ptr(), 1) };
    assert_eq!(code, SCG_ERR_INVALID_STATE);

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_mesh_destroy(mesh);
        scg_destroy(ctx);
    }
}

/// Les pointeurs nuls de la soumission sont refusés.
#[test]
fn refuse_les_pointeurs_nuls_de_la_soumission() {
    let ctx = context(0);
    let mesh = load(&one_triangle_mesh());
    let slots: [*const ScgTexture; 1] = [ptr::null()];
    let model = identity();

    // SAFETY: chaque appel passe un pointeur nul que la fonction refuse avant de
    // le lire, les autres étant vivants.
    unsafe {
        assert_eq!(
            scg_submit_mesh(ptr::null_mut(), &model, mesh, slots.as_ptr(), 1),
            SCG_ERR_NULL,
            "contexte nul"
        );
        assert_eq!(
            scg_submit_mesh(ctx, ptr::null(), mesh, slots.as_ptr(), 1),
            SCG_ERR_NULL,
            "matrice nulle"
        );
        assert_eq!(
            scg_submit_mesh(ctx, &model, ptr::null(), slots.as_ptr(), 1),
            SCG_ERR_NULL,
            "maillage nul"
        );
        assert_eq!(
            scg_submit_mesh(ctx, &model, mesh, ptr::null(), 1),
            SCG_ERR_NULL,
            "tableau nul avec un compte non nul"
        );
        scg_mesh_destroy(mesh);
        scg_destroy(ctx);
    }
}

/// Un maillage détruit juste après sa soumission ne change pas l'image en cours.
///
/// Et ce n'est pas pour la raison de la texture : le contexte ne garde aucune
/// référence sur un maillage, plus rien ne le lisant après le retour de la
/// soumission. Un hôte qui supposerait une seule règle pour les deux se
/// tromperait le jour où l'une change.
#[test]
fn un_maillage_detruit_apres_sa_soumission_ne_change_pas_l_image() {
    let ctx = context(0);
    let model = identity();
    let slots: [*const ScgTexture; 1] = [ptr::null()];
    let mut pixels = vec![0u8; 64 * 64 * 4];

    let mesh = load(&one_triangle_mesh());
    // SAFETY: contexte et maillage vivants, tableau local d'une entrée.
    unsafe { scg_submit_mesh(ctx, &model, mesh, slots.as_ptr(), 1) };
    // SAFETY: handle vivant, détruit une seule fois — avant la fin de l'image.
    unsafe { scg_mesh_destroy(mesh) };
    // SAFETY: contexte vivant, tampon local de la taille annoncée.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(code, SCG_OK);

    let garde = context(0);
    let mesh = load(&one_triangle_mesh());
    let mut reference = vec![0u8; 64 * 64 * 4];
    // SAFETY: mêmes préconditions, la destruction venant après la fin.
    unsafe {
        scg_submit_mesh(garde, &model, mesh, slots.as_ptr(), 1);
        assert_eq!(scg_frame_end(garde, reference.as_mut_ptr(), 64), SCG_OK);
        scg_mesh_destroy(mesh);
    }

    assert_eq!(pixels, reference);

    // SAFETY: contextes vivants, détruits une seule fois.
    unsafe {
        scg_destroy(ctx);
        scg_destroy(garde);
    }
}

/// Un refus laisse le message dans l'emplacement du thread, que
/// `scg_last_error(NULL)` lit — un chargement n'a pas de contexte où le ranger.
#[test]
fn le_message_d_un_refus_va_dans_l_emplacement_du_thread() {
    let bytes = mesh_bytes(&[], b"MESH", 7);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    unsafe { scg_mesh_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert!(!last_error().is_empty());

    // Un appel qui réussit vide le message : sans quoi un hôte qui lit après
    // coup rapporterait le refus précédent.
    let mesh = load(&mesh_bytes(&[], b"MESH", 1));
    assert!(last_error().is_empty());
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_mesh_destroy(mesh) };
}

/// Le même maillage se soumet depuis plusieurs threads, puisqu'il n'appartient à
/// aucun contexte et qu'il est immuable.
#[test]
fn un_maillage_se_lit_depuis_plusieurs_threads() {
    let mesh = load(&one_triangle_mesh());
    let address = mesh as usize;

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(move || {
                let handle = address as *const ScgMesh;
                let mut count = 0;
                // SAFETY: le handle reste vivant pendant toute la portée, et la
                // ressource est immuable : les accesseurs ne font que lire.
                let code = unsafe { scg_mesh_triangle_count(handle, &mut count) };
                assert_eq!(code, SCG_OK);
                assert_eq!(count, 1);
            });
        }
    });

    // SAFETY: handle vivant, détruit une seule fois, après la portée des
    // threads.
    unsafe { scg_mesh_destroy(mesh) };
}
