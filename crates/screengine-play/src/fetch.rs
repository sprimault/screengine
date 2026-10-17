// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Lire un fichier par le réseau, ce que seul un navigateur impose.
//!
//! Ailleurs, un exemple intègre ses planches par `include_bytes!` et le binaire
//! les porte. Sur wasm, elles font du module quelque chose que le navigateur
//! doit **télécharger et compiler** avant la première image : l'exemple
//! `couloir` pesait trois mégaoctets et demi, contre moins de six cents
//! kilo-octets sans ses planches.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::Error;

/// Lit un fichier servi à côté de la page, et rend ses octets.
///
/// L'adresse se résout contre celle du **document**, jamais contre celle du
/// module : `assets/mur.png` va chercher le fichier à côté de la page, où la
/// construction les dépose, et une page déplacée dans un sous-répertoire
/// casserait sans rien dire. C'est pour cela qu'elle est relative et non
/// absolue.
///
/// **Une réponse d'erreur n'est pas un échec de `fetch`** : le navigateur ne
/// rejette que sur un défaut de réseau, et rend un 404 comme un succès. C'est le
/// piège de cette API, et l'appel le referme en lisant le statut — sans quoi le
/// décodeur de PNG recevrait le corps d'une page d'erreur et parlerait de
/// fichier tronqué.
pub async fn fetch(url: &str) -> Result<Vec<u8>, Error> {
    let faute = |cause: JsValue| Error::Fetch(format!("{url}: {cause:?}"));

    let window =
        web_sys::window().ok_or_else(|| Error::Fetch(String::from("no window to fetch from")))?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(faute)?
        .dyn_into()
        .map_err(|_| Error::Fetch(format!("{url}: not a response")))?;
    if !response.ok() {
        return Err(Error::Fetch(format!("{url}: HTTP {}", response.status())));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(faute)?)
        .await
        .map_err(faute)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
