// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Les tampons de message, y compris l'emplacement par thread.
//!
//! Le chemin d'erreur ne doit rien allouer et ne doit jamais paniquer : c'est la
//! seule fonction de l'ABI qui n'a pas de code de retour pour porter un échec.

use super::*;

/// Relit un message comme le ferait une liaison : jusqu'au premier nul.
fn read(message: &Message) -> &str {
    let bytes = &message.bytes;
    let end = bytes.iter().position(|b| *b == 0).expect("terminateur nul");
    core::str::from_utf8(&bytes[..end]).expect("UTF-8 valide")
}

/// Sans erreur, le header promet la chaîne vide et jamais un pointeur nul.
#[test]
fn un_message_neuf_est_vide() {
    assert_eq!(read(&Message::new()), "");
}

/// Le cas courant, qui garde le test de troncature honnête : une
/// implémentation qui tronquerait tout le passerait aussi.
#[test]
fn conserve_un_message_court() {
    let mut message = Message::new();
    message.set("invalid argument");
    assert_eq!(read(&message), "invalid argument");
}

/// Couper au milieu d'une séquence UTF-8 rendrait une chaîne qu'une liaison
/// ne peut pas décoder, et le texte d'une panique n'est pas garanti ASCII.
#[test]
fn tronque_un_message_trop_long_sans_couper_un_caractere() {
    let mut message = Message::new();
    // Des caractères de deux octets : une troncature à l'octet tomberait au
    // milieu de l'un d'eux et rendrait une chaîne indécodable.
    message.set(&"é".repeat(CAPACITY));
    let text = read(&message);
    assert!(text.len() < CAPACITY);
    assert!(text.chars().all(|c| c == 'é'));
}

/// Vider n'efface que le terminateur : le reste des octets survit, et c'est
/// voulu — ce qui compte est ce qu'une liaison lit, pas ce qui traîne après.
#[test]
fn vider_rend_la_chaine_vide() {
    let mut message = Message::new();
    message.set("out of memory");
    message.clear();
    assert_eq!(read(&message), "");
}

/// L'emplacement sans destructeur se lit par pointeur brut, comme le fera
/// l'hôte. C'est aussi ce qui vérifie qu'y accéder ne panique pas, alors
/// qu'un thread-local à `Drop` le ferait en fin de vie du thread.
#[test]
fn l_emplacement_par_thread_se_lit_et_se_vide() {
    set_orphan("invalid argument");
    // SAFETY: le pointeur vise le stockage local du thread courant, vivant
    // pour toute la durée du test, et rien n'écrit entre-temps.
    let text = unsafe { core::ffi::CStr::from_ptr(orphan_ptr()) };
    assert_eq!(text.to_str().expect("UTF-8 valide"), "invalid argument");

    clear_orphan();
    // SAFETY: même raisonnement.
    let text = unsafe { core::ffi::CStr::from_ptr(orphan_ptr()) };
    assert_eq!(text.to_bytes(), b"");
}
