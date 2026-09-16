// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Les points d'entrée appelés comme le ferait un hôte.
//!
//! Pointeurs nuls, séquences invalides, panique rattrapée : ce que les tests
//! unitaires ne voient pas, parce qu'ils n'ont pas de pointeur à passer. Un
//! test qui provoque une panique laisse le message par défaut sur la sortie
//! d'erreur — c'est attendu, et c'est justement ce qui est rattrapé.

use std::ffi::CStr;
use std::ptr;

use screengine_ffi::*;

/// Une configuration qui passe, dont les tests dérivent leurs variantes.
fn sane() -> ScgContextConfig {
    ScgContextConfig {
        max_width: 64,
        max_height: 32,
        width: 64,
        height: 32,
        tile_size: 32,
        reserved0: 0,
        reserved1: 0,
        reserved2: 0,
    }
}

/// Crée un contexte dont la configuration est saine.
fn create(config: &ScgContextConfig) -> *mut ScgContext {
    let mut ctx = ptr::null_mut();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    let code = unsafe { scg_create(config, &mut ctx) };
    assert_eq!(code, SCG_OK);
    assert!(!ctx.is_null());
    ctx
}

/// Lit le message d'un contexte, ou celui du thread si `ctx` est nul.
fn last_error(ctx: *const ScgContext) -> String {
    // SAFETY: `ctx` est nul ou vivant, et le pointeur rendu reste valide
    // jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ctx)) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

#[test]
fn la_version_d_abi_est_celle_du_crate() {
    assert_eq!(scg_abi_version(), SCG_ABI_VERSION);
}

#[test]
fn cree_et_detruit_un_contexte() {
    let ctx = create(&sane());
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

#[test]
fn detruire_un_pointeur_nul_ne_fait_rien() {
    // SAFETY: `scg_destroy` accepte explicitement le pointeur nul.
    unsafe { scg_destroy(ptr::null_mut()) };
}

#[test]
fn refuse_une_configuration_nulle() {
    let mut ctx = ptr::null_mut();
    // SAFETY: le premier pointeur est nul, ce que la fonction doit refuser
    // plutôt que déréférencer.
    let code = unsafe { scg_create(ptr::null(), &mut ctx) };
    assert_eq!(code, SCG_ERR_NULL);
    assert!(ctx.is_null());
    assert_eq!(last_error(ptr::null()), "null pointer argument");
}

#[test]
fn refuse_un_parametre_de_sortie_nul() {
    let config = sane();
    // SAFETY: le second pointeur est nul, ce que la fonction doit refuser.
    let code = unsafe { scg_create(&config, ptr::null_mut()) };
    assert_eq!(code, SCG_ERR_NULL);
}

#[test]
fn refuse_une_configuration_invalide_et_dit_pourquoi() {
    let mut config = sane();
    config.tile_size = 48;

    let mut ctx = ptr::null_mut();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    let code = unsafe { scg_create(&config, &mut ctx) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert!(ctx.is_null());
    assert_eq!(last_error(ptr::null()), "invalid argument");
}

#[test]
fn refuse_un_champ_reserve_non_nul() {
    let mut config = sane();
    config.reserved1 = 1;

    let mut ctx = ptr::null_mut();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    let code = unsafe { scg_create(&config, &mut ctx) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
}

#[test]
fn refuse_une_fin_d_image_sans_contexte() {
    let mut pixels = [0u8; 64 * 32 * 4];
    // SAFETY: le contexte est nul, ce que la fonction doit refuser.
    let code = unsafe { scg_frame_end(ptr::null_mut(), pixels.as_mut_ptr(), 64) };
    assert_eq!(code, SCG_ERR_NULL);
}

#[test]
fn refuse_un_tampon_nul() {
    let ctx = create(&sane());
    // SAFETY: handle vivant ; le tampon est nul, ce que la fonction refuse.
    let code = unsafe { scg_frame_end(ctx, ptr::null_mut(), 64) };
    assert_eq!(code, SCG_ERR_NULL);
    assert_eq!(last_error(ctx), "null pointer argument");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

#[test]
fn refuse_un_stride_plus_court_que_la_largeur() {
    let ctx = create(&sane());
    let mut pixels = [0u8; 64 * 32 * 4];
    // SAFETY: handle vivant, et le tampon couvre `stride × hauteur` pixels
    // puisque le stride est plus petit que la largeur.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 63) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(last_error(ctx), "invalid argument");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Le remplissage n'est pas écrit : l'appel panique, la frontière le rattrape,
/// et c'est exactement le contrat qu'on veut voir tenir avant d'avoir un moteur.
#[test]
fn une_panique_devient_un_code_puis_empoisonne_le_contexte() {
    let ctx = create(&sane());
    let mut pixels = [0u8; 64 * 32 * 4];

    // SAFETY: handle vivant, tampon d'au moins `stride × hauteur` pixels.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(code, SCG_ERR_PANIC);
    assert!(
        last_error(ctx).contains("étape 0"),
        "le message doit porter l'étape du stub"
    );

    // SAFETY: même handle, toujours vivant.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(code, SCG_ERR_POISONED);
    assert!(last_error(ctx).contains("poisoned"));

    // La destruction reste permise sur un objet empoisonné : c'est ce qui
    // laisse l'hôte lire la cause puis libérer.
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

#[test]
fn sans_erreur_le_message_est_la_chaine_vide() {
    let ctx = create(&sane());
    assert_eq!(last_error(ctx), "");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

#[test]
fn alloue_et_libere_un_tampon_aligne() {
    let len = 64 * 32 * 4;
    let ptr = scg_buffer_alloc(len);
    assert!(!ptr.is_null());
    assert_eq!(
        ptr as usize % SCG_BUFFER_ALIGNMENT,
        0,
        "l'alignement est une constante de l'ABI"
    );

    // SAFETY: le tampon vient d'être alloué, sa longueur est celle-là même.
    unsafe { ptr::write_bytes(ptr, 0, len) };
    // SAFETY: même pointeur, même longueur, libéré une seule fois.
    unsafe { scg_buffer_free(ptr, len) };
}

#[test]
fn allouer_zero_octet_rend_un_pointeur_nul() {
    assert!(scg_buffer_alloc(0).is_null());
}

#[test]
fn liberer_un_pointeur_nul_ne_fait_rien() {
    // SAFETY: `scg_buffer_free` accepte explicitement le pointeur nul.
    unsafe { scg_buffer_free(ptr::null_mut(), 0) };
}

/// L'emplacement sans contexte est vidé à chaque entrée : un appel qui aboutit
/// ne laisse pas derrière lui le message du précédent.
#[test]
fn un_appel_reussi_efface_le_message_sans_contexte() {
    let mut ctx = ptr::null_mut();
    // SAFETY: le premier pointeur est nul, ce que la fonction refuse.
    unsafe { scg_create(ptr::null(), &mut ctx) };
    assert_ne!(last_error(ptr::null()), "");

    let ctx = create(&sane());
    assert_eq!(last_error(ptr::null()), "");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}
