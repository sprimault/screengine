// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! L'enveloppe que traverse chaque point d'entrée.
//!
//! Elle fixe l'environnement flottant, rattrape les paniques, traduit l'erreur
//! en code et retient le message. Elle s'écrit une fois : un point d'entrée
//! ajouté sans elle est le défaut le plus discret du projet, puisqu'il ne se
//! manifeste que le jour où quelque chose panique, chez quelqu'un d'autre.

use std::any::Any;
use std::cell::UnsafeCell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use screengine::{Context, Error};

use crate::context::ScgContext;
use crate::fpenv::FpEnv;
use crate::message;
use crate::status::{
    SCG_ERR_FAULTED, SCG_ERR_INVALID_ARGUMENT, SCG_ERR_NULL, SCG_ERR_PANIC, SCG_OK, code_of,
    message_of,
};

/// Une erreur qui porte son code d'ABI et son texte.
///
/// Elle réunit ce que rend le noyau et ce que la frontière refuse elle-même —
/// un pointeur nul n'a pas de sens pour le noyau, qui ne manipule jamais de
/// pointeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AbiError {
    code: i32,
    message: &'static str,
}

impl AbiError {
    /// Un argument obligatoire est nul.
    pub(crate) const NULL: Self = Self {
        code: SCG_ERR_NULL,
        message: "null pointer argument",
    };

    /// Un champ réservé d'une structure n'est pas nul.
    ///
    /// Refusé ici et non par le noyau : les champs réservés n'existent que parce
    /// que l'ABI est figée, et le noyau n'a pas à savoir qu'elle l'est.
    pub(crate) const RESERVED: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "reserved fields must be zero",
    };

    /// L'emplacement de texture demandé n'existe pas dans la ressource.
    ///
    /// Une faute dans l'appel et non dans le contenu : le fichier est bon, c'est
    /// l'indice qui sort de ce que la ressource déclare. Un code de la plage des
    /// données enverrait l'hôte chercher un mauvais fichier.
    pub(crate) const TEXTURE_SLOT: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "texture slot beyond those the resource declares",
    };

    /// Un index au-delà de ce que la carte porte.
    ///
    /// Une faute dans l'appel et non dans le contenu : le fichier est bon, c'est
    /// l'index qui sort de ce qu'un compte a rendu.
    pub(crate) const WORLD_INDEX: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "index beyond what the map declares",
    };

    /// Le tableau de textures n'a pas le nombre d'emplacements de la ressource.
    ///
    /// Une égalité et non un minimum : un tableau plus long est le signe que
    /// l'hôte s'est trompé de ressource, et le laisser passer ferait dessiner un
    /// maillage avec l'habillage d'un autre.
    pub(crate) const TEXTURE_COUNT: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "texture count must equal the number of slots the resource declares",
    };

    /// Le tampon d'un nom ne peut pas porter le nom et son terminateur.
    ///
    /// Rien n'est écrit dans ce cas, `out_len` compris : la mesure a son propre
    /// appel — tampon nul, capacité nulle —, et remplir un paramètre de sortie
    /// sur un chemin d'erreur serait la seule exception de toute l'ABI.
    pub(crate) const NAME_CAPACITY: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "name buffer too short: measure the name first with a null buffer and zero capacity",
    };

    /// Le tampon du cache ne peut pas porter le bloc.
    ///
    /// Rien n'est écrit dans ce cas, `out_len` compris : la mesure a son propre
    /// appel — tampon nul, capacité nulle —, comme pour le tampon d'un nom. Un
    /// tampon plus long que nécessaire est accepté et n'est rempli que de ce que
    /// le bloc occupe ; sa longueur est dans son en-tête, donc un hôte qui le
    /// relit n'a pas à se souvenir de la capacité qu'il avait donnée.
    pub(crate) const CACHE_CAPACITY: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "cache buffer too short: measure the cache first with a null buffer and zero capacity",
    };

    /// Une coordonnée de sommet n'est pas un nombre fini.
    ///
    /// Le noyau ferait disparaître le triangle sans erreur, ce qui est le bon
    /// comportement pour une donnée qu'il a lui-même transformée. Reçue telle
    /// quelle d'un hôte, elle est une erreur d'appel, et le dire vaut mieux que
    /// laisser un mur manquer dans l'image.
    pub(crate) const VERTEX_NOT_FINITE: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "vertex coordinates must be finite numbers",
    };

    /// La position ou l'orientation de la caméra n'est pas finie.
    pub(crate) const CAMERA_NOT_FINITE: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "camera position and orientation must be finite numbers",
    };

    /// La matrice du modèle n'est pas une transformation affine.
    ///
    /// Refusée ici parce que le noyau ne voit qu'une 3×4, où la question ne se
    /// pose plus : c'est la frontière qui reçoit une 4×4 et doit vérifier que
    /// sa dernière ligne n'y cache pas une perspective.
    pub(crate) const MATRIX: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "model matrix must be finite, with last row exactly 0, 0, 0, 1",
    };

    /// Le format de pixels d'une texture n'est pas celui que le moteur lit.
    ///
    /// Refusé ici parce que le noyau n'a pas de format : il reçoit des texels
    /// déjà convertis. C'est la frontière qui porte l'énumération, et une
    /// description laissée à zéro tombe dessus plutôt que d'être lue comme
    /// valide — la raison pour laquelle le format RGBA8 vaut un et non zéro.
    pub(crate) const TEXTURE_FORMAT: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "texture format must be SCG_TEXTURE_FORMAT_RGBA8 \
                  or SCG_TEXTURE_FORMAT_RGBA8_MASKED",
    };

    /// Le mode de mélange demandé n'existe pas dans cette bibliothèque.
    ///
    /// Zéro y tombe aussi, et c'est voulu : un mode passé à une soumission est
    /// une description, pas un réglage de contexte. Zéro ne vaut défaut que
    /// pour les seconds — le filtrage —, et une description laissée à zéro se
    /// refuse plutôt que s'interprète.
    pub(crate) const BLEND_MODE: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "blend mode must be SCG_BLEND_MODULATE",
    };

    /// Le niveau de filtrage demandé n'existe pas dans cette bibliothèque.
    ///
    /// Refusé plutôt que rabattu sur le défaut, et c'est ce qui rend l'ajout
    /// d'un filtrage compatible : une liaison écrite contre une version
    /// ultérieure recevra une erreur franche ici, au lieu d'une image filtrée
    /// autrement qu'elle ne le croit.
    pub(crate) const FILTER: Self = Self {
        code: SCG_ERR_INVALID_ARGUMENT,
        message: "filter must be SCG_FILTER_DITHER or SCG_FILTER_BILINEAR",
    };
}

impl From<Error> for AbiError {
    fn from(error: Error) -> Self {
        Self {
            code: code_of(error),
            message: message_of(error),
        }
    }
}

/// Ce qui empêche un appel d'aboutir.
enum Failure {
    /// Un refus, avec son code.
    Abi(AbiError),
    /// Une panique, et sa charge utile telle que la bibliothèque standard la
    /// rend.
    Panic(Box<dyn Any + Send>),
}

/// Le texte d'une charge utile de panique, sans rien allouer.
///
/// La bibliothèque standard produit un `&'static str` pour `panic!("…")` sans
/// argument et une `String` sinon ; un autre type vient d'un `panic_any`, que
/// le projet n'utilise pas.
fn panic_text(payload: &(dyn Any + Send)) -> &str {
    if let Some(text) = payload.downcast_ref::<&str>() {
        text
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text
    } else {
        "panic with a non-string payload"
    }
}

/// Installe, une fois, le crochet qui garde le texte d'une panique sur wasm.
///
/// Sans dépliage sur une chaîne stable, une panique y est un trap :
/// `catch_unwind` ne rattrape rien, et le texte serait perdu. Le crochet
/// s'exécute avant l'arrêt et l'écrit dans l'emplacement sans contexte, dont
/// l'adresse ne change pas pendant la vie de l'instance — faute de threads, le
/// stockage local y est un statique. L'hôte le lit dans la mémoire linéaire
/// après avoir attrapé l'erreur, sans rappeler un module dont la pile est dans
/// un état inconnu.
///
/// Une fois et non à chaque appel : `set_hook` alloue, et une allocation par
/// image est ce que l'invariant interdit.
#[cfg(target_arch = "wasm32")]
fn install_panic_hook() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            message::set_orphan(panic_text(info.payload()));
        }));
    });
}

/// Exécute un appel sous l'environnement du moteur, paniques rattrapées.
///
/// La garde d'environnement flottant enveloppe `catch_unwind` et non l'inverse :
/// le dépliage s'arrête donc avant elle, sa destruction a toujours lieu sur un
/// retour normal, et le message de panique se formate encore sous
/// l'environnement par défaut.
fn guarded<T, F>(f: F) -> Result<T, Failure>
where
    F: FnOnce() -> Result<T, AbiError>,
{
    #[cfg(target_arch = "wasm32")]
    install_panic_hook();

    let _fpenv = FpEnv::enter();

    // `AssertUnwindSafe` ne se pose qu'ici : c'est l'état défaillant qui la rend
    // honnête, puisque l'état laissé par une panique n'est plus jamais observé.
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(Failure::Abi(error)),
        Err(payload) => Err(Failure::Panic(payload)),
    }
}

/// L'accès au contexte du noyau que reçoit un appel exclusif.
///
/// Le partage est toujours permis ; l'exclusivité ne l'est que hors du rendu
/// des tuiles, et c'est l'état atomique du noyau qui le dit avant qu'aucune
/// référence exclusive n'existe. Une tuile qui tournerait pendant ce temps
/// serait un appel hors du contrat de l'ABI.
pub(crate) struct Core<'a> {
    cell: &'a UnsafeCell<Context>,
}

impl Core<'_> {
    /// Le contexte en partage, tel que les tuiles le voient.
    pub(crate) fn shared(&self) -> &Context {
        // SAFETY: aucune référence exclusive ne vit tant que `self` est
        // emprunté en partage : `exclusive` exige `&mut self`.
        unsafe { &*self.cell.get() }
    }

    /// Le contexte en exclusivité, ou [`Error::InvalidState`] pendant le rendu
    /// des tuiles.
    pub(crate) fn exclusive(&mut self) -> Result<&mut Context, AbiError> {
        if self.shared().is_rendering() {
            return Err(Error::InvalidState.into());
        }
        // SAFETY: hors du rendu, le contrat de l'ABI interdit tout autre appel
        // sur le contexte, et `&mut self` empêche une référence partagée issue
        // de ce même accès de survivre.
        Ok(unsafe { &mut *self.cell.get() })
    }
}

/// Ce qu'un point d'entrée rend quand il réussit.
///
/// Presque tous rendent `()`, donc `SCG_OK`. Ceux de la traversée rendent un
/// **statut positif**, et c'est la seule raison de ce trait : l'enveloppe reste
/// unique. La dupliquer pour laisser passer un code serait rouvrir le défaut le
/// plus discret du projet — un point d'entrée sans `catch_unwind`, qui ne se
/// manifeste que le jour où quelque chose panique, chez quelqu'un d'autre.
pub(crate) trait Outcome {
    /// Le code que l'ABI rend pour ce succès.
    fn code(self) -> i32;
}

impl Outcome for () {
    fn code(self) -> i32 {
        SCG_OK
    }
}

impl Outcome for i32 {
    fn code(self) -> i32 {
        self
    }
}

/// Enveloppe un appel qui porte sur un contexte, hors rendu d'une tuile.
///
/// Son message va dans le contexte. Il ne s'exécute pas en même temps qu'un
/// autre appel sur le même contexte ; seule la fin d'image peut croiser des
/// tuiles, qu'elle refuse d'attendre.
///
/// # Safety
///
/// `ctx` est nul, ou un handle rendu par `scg_create` et pas encore détruit.
pub(crate) unsafe fn with_context<T, F>(ctx: *mut ScgContext, f: F) -> i32
where
    T: Outcome,
    F: FnOnce(Core<'_>) -> Result<T, AbiError>,
{
    // Vidé en entrant, pour qu'un thread recyclé ne rende jamais le message
    // d'une tâche précédente.
    message::clear_orphan();

    // SAFETY: précondition de la fonction — `ctx` est nul ou valide.
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        message::set_orphan(AbiError::NULL.message);
        return SCG_ERR_NULL;
    };

    // SAFETY: un appel exclusif est seul à écrire le message du contexte.
    unsafe { ctx.message_mut() }.clear();

    if ctx.faulted() {
        // SAFETY: même raisonnement.
        if !unsafe { ctx.take_tile_panic() } {
            // SAFETY: idem.
            unsafe { ctx.message_mut() }.set("a previous call panicked; this object is faulted");
        }
        return SCG_ERR_FAULTED;
    }

    let core = Core { cell: ctx.core() };
    match guarded(|| f(core)) {
        Ok(value) => value.code(),
        Err(Failure::Abi(error)) => {
            // SAFETY: un appel exclusif est seul à écrire le message.
            unsafe { ctx.message_mut() }.set(error.message);
            error.code
        }
        Err(Failure::Panic(payload)) => {
            ctx.mark_faulted();
            // SAFETY: idem.
            unsafe { ctx.message_mut() }.set(panic_text(&*payload));
            SCG_ERR_PANIC
        }
    }
}

/// Enveloppe le rendu d'une tuile, appelable depuis plusieurs threads à la
/// fois sur le même contexte.
///
/// Rien ne s'y écrit dans le contexte hors de ses atomiques : le message va
/// dans l'emplacement par thread, et une panique ne laisse son texte que par
/// [`ScgContext::fault_from_tile`].
///
/// # Safety
///
/// `ctx` est nul, ou un handle rendu par `scg_create` et pas encore détruit.
pub(crate) unsafe fn with_tile<F>(ctx: *mut ScgContext, f: F) -> i32
where
    F: FnOnce(&Context) -> Result<(), AbiError>,
{
    message::clear_orphan();

    // SAFETY: précondition de la fonction — `ctx` est nul ou valide.
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        message::set_orphan(AbiError::NULL.message);
        return SCG_ERR_NULL;
    };

    if ctx.faulted() {
        message::set_orphan("a previous call panicked; this object is faulted");
        return SCG_ERR_FAULTED;
    }

    let core = Core { cell: ctx.core() };
    match guarded(|| f(core.shared())) {
        Ok(()) => SCG_OK,
        Err(Failure::Abi(error)) => {
            message::set_orphan(error.message);
            error.code
        }
        Err(Failure::Panic(payload)) => {
            let text = panic_text(&*payload);
            ctx.fault_from_tile(text);
            message::set_orphan(text);
            SCG_ERR_PANIC
        }
    }
}

/// Enveloppe un appel qui n'a pas de contexte auquel se rattacher.
///
/// Son message va dans l'emplacement par thread, que `scg_last_error(NULL)`
/// lit.
pub(crate) fn without_context<F>(f: F) -> i32
where
    F: FnOnce() -> Result<(), AbiError>,
{
    message::clear_orphan();

    match guarded(f) {
        Ok(()) => SCG_OK,
        Err(Failure::Abi(error)) => {
            message::set_orphan(error.message);
            error.code
        }
        Err(Failure::Panic(payload)) => {
            message::set_orphan(panic_text(&*payload));
            SCG_ERR_PANIC
        }
    }
}

/// Enveloppe un appel qui ne rend rien, et qu'aucun code ne peut donc porter.
///
/// C'est le cas des destructions. Une panique y est avalée : le handle est
/// rendu, l'appel n'a pas de retour, et la propager vers du C serait un
/// comportement indéfini. Son texte va tout de même dans l'emplacement par
/// thread, seule trace qu'un hôte puisse en lire — et c'est pourquoi une
/// destruction qui panique reste un défaut du moteur, pas un cas prévu.
///
/// Le vidage de l'emplacement en entrée vaut ici comme ailleurs : sans lui, un
/// thread de pool rendrait le message d'une tâche précédente.
pub(crate) fn nothing<F>(f: F)
where
    F: FnOnce(),
{
    message::clear_orphan();

    let call = || {
        f();
        Ok(())
    };
    if let Err(Failure::Panic(payload)) = guarded(call) {
        message::set_orphan(panic_text(&*payload));
    }
}

/// Enveloppe un appel qui rend une valeur plutôt qu'un code, et donne `fallback`
/// si cet appel panique.
///
/// C'est le cas de l'allocation de tampon, qui rend un pointeur nul en cas
/// d'échec comme le ferait `malloc` : l'ABI la nomme parmi les exceptions à la
/// règle du code de retour, et une panique doit donc y ressembler à un échec
/// d'allocation ordinaire.
pub(crate) fn producing<T, F>(fallback: T, f: F) -> T
where
    F: FnOnce() -> T,
{
    message::clear_orphan();

    match guarded(|| Ok(f())) {
        Ok(value) => value,
        Err(Failure::Panic(payload)) => {
            message::set_orphan(panic_text(&*payload));
            fallback
        }
        Err(Failure::Abi(error)) => {
            message::set_orphan(error.message);
            fallback
        }
    }
}

#[cfg(test)]
mod tests;
