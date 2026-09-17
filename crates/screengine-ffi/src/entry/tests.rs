// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! L'enveloppe, éprouvée sur ce qu'elle existe pour empêcher.
//!
//! Le test de panique fait sortir un dépliage de la fermeture enveloppée : s'il
//! échoue, ce n'est pas une assertion qui rougit, c'est le processus de test qui
//! tombe — et c'est exactement ce qui arriverait à un hôte C.

use screengine::{Argument, Config};

use super::*;
use crate::status::SCG_ERR_INVALID_ARGUMENT;

/// Un contexte enveloppé, pour les tests qui ont besoin d'un handle.
fn context() -> ScgContext {
    let config = Config {
        max_width: 8,
        max_height: 8,
        width: 8,
        height: 8,
        tile_size: 32,
    };
    ScgContext::new(Context::new(config).expect("configuration saine"))
}

/// L'état défaillant, au niveau où il est décidé. Ce test vit ici et non dans
/// les tests de frontière parce que plus aucun point d'entrée ne panique : il
/// faut provoquer la panique soi-même pour éprouver ce que l'ABI promet.
#[test]
fn une_panique_rend_le_contexte_defaillant() {
    let mut ctx = context();
    let handle: *mut ScgContext = &mut ctx;

    // SAFETY: le handle vise une valeur locale vivante, utilisée par ce seul
    // thread pendant les deux appels.
    let code = unsafe { with_context(handle, |_| panic!("défaut simulé du moteur")) };
    assert_eq!(code, SCG_ERR_PANIC);

    // SAFETY: même handle, toujours vivant.
    let code = unsafe { with_context(handle, |_| Ok(())) };
    assert_eq!(
        code, SCG_ERR_FAULTED,
        "un appel qui aboutirait après une panique lirait un état inconnu"
    );
}

/// Le message survit à la défaillance : c'est ce qui laisse l'hôte apprendre
/// la cause avant de détruire l'objet.
#[test]
fn le_message_survit_a_la_defaillance() {
    let mut ctx = context();
    let handle: *mut ScgContext = &mut ctx;

    // SAFETY: le handle vise une valeur locale vivante.
    unsafe { with_context(handle, |_| panic!("défaut simulé du moteur")) };
    let message = ctx.message().as_ptr();
    // SAFETY: le message appartient au contexte, vivant jusqu'à la fin du
    // test, et rien n'écrit entre-temps.
    let text = unsafe { std::ffi::CStr::from_ptr(message) };
    assert_eq!(
        text.to_str().expect("UTF-8 valide"),
        "défaut simulé du moteur"
    );
}

/// Une panique dans une tuile rend le contexte défaillant sans toucher à son
/// message, que d'autres tuiles pourraient croiser ; la fin d'image, appel
/// exclusif, en rend le texte.
#[test]
fn une_panique_de_tuile_est_rapportee_par_la_fin() {
    let mut ctx = context();
    let handle: *mut ScgContext = &mut ctx;

    // SAFETY: le handle vise une valeur locale vivante.
    let code = unsafe { with_tile(handle, |_| panic!("défaut simulé d'une tuile")) };
    assert_eq!(code, SCG_ERR_PANIC);
    assert_eq!(read(message::orphan_ptr()), "défaut simulé d'une tuile");
    assert_eq!(read(ctx.message().as_ptr()), "", "message du contexte");

    // SAFETY: même handle, toujours vivant.
    let code = unsafe { with_tile(handle, |_| Ok(())) };
    assert_eq!(code, SCG_ERR_FAULTED, "une tuile suivante est refusée");

    // SAFETY: même handle, toujours vivant.
    let code = unsafe { with_context(handle, |_| Ok(())) };
    assert_eq!(code, SCG_ERR_FAULTED);
    assert_eq!(read(ctx.message().as_ptr()), "défaut simulé d'une tuile");
}

/// Lit une chaîne terminée par un octet nul.
fn read(text: *const std::ffi::c_char) -> String {
    // SAFETY: les messages du moteur sont terminés, et vivent au moins jusqu'au
    // prochain appel ; la copie a lieu avant.
    let text = unsafe { std::ffi::CStr::from_ptr(text) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

/// Le chemin nominal : sans lui, une enveloppe qui échouerait toujours
/// passerait les trois tests suivants.
#[test]
fn un_appel_qui_aboutit_rend_le_succes() {
    assert_eq!(without_context(|| Ok(())), SCG_OK);
}

/// La traduction du noyau vers l'ABI, dans le sens où l'hôte la voit.
#[test]
fn une_erreur_du_noyau_devient_son_code() {
    let code = without_context(|| Err(Error::InvalidArgument(Argument::TileSize).into()));
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
}

/// La raison d'être de l'enveloppe : une panique qui atteindrait l'appelant
/// C serait un comportement indéfini, pas un plantage propre. Si ce test
/// échoue, il fait tomber le processus de test avec lui.
#[test]
fn une_panique_devient_un_code_et_ne_s_echappe_pas() {
    let code = without_context(|| panic!("défaut simulé du moteur"));
    assert_eq!(code, SCG_ERR_PANIC);
}

/// Le code dit qu'il y a eu panique, le message dit laquelle — c'est tout ce
/// qu'un intégrateur aura pour rapporter le défaut, dans un langage sans
/// pile Rust à afficher.
#[test]
fn le_message_d_une_panique_est_retenu() {
    without_context(|| panic!("défaut simulé du moteur"));
    // SAFETY: le pointeur vise le stockage local du thread courant, et rien
    // n'écrit entre-temps.
    let text = unsafe { std::ffi::CStr::from_ptr(message::orphan_ptr()) };
    assert_eq!(
        text.to_str().expect("UTF-8 valide"),
        "défaut simulé du moteur"
    );
}

/// L'emplacement est par thread, pas par appel : sans ce vidage, un thread
/// recyclé rendrait le message d'une tâche précédente, et l'hôte croirait à
/// une erreur qui n'a pas eu lieu.
#[test]
fn un_appel_qui_aboutit_efface_le_message_precedent() {
    without_context(|| Err(Error::OutOfMemory.into()));
    without_context(|| Ok(()));
    // SAFETY: même raisonnement.
    let text = unsafe { std::ffi::CStr::from_ptr(message::orphan_ptr()) };
    assert_eq!(text.to_bytes(), b"");
}

/// Le texte d'une charge utile se lit sans allouer, quelle que soit la forme
/// que la bibliothèque standard lui donne.
#[test]
fn le_texte_d_une_charge_utile_se_lit_dans_les_deux_formes() {
    let statique: Box<dyn Any + Send> = Box::new("littéral");
    assert_eq!(panic_text(&*statique), "littéral");

    let formate: Box<dyn Any + Send> = Box::new(String::from("formaté"));
    assert_eq!(panic_text(&*formate), "formaté");

    let autre: Box<dyn Any + Send> = Box::new(7u8);
    assert_eq!(panic_text(&*autre), "panic with a non-string payload");
}
