// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les codes de retour de l'ABI.
//!
//! Les plages sont réservées par étape, et c'est une règle arithmétique : la
//! catégorie d'un code est `(-code) / 100`. Une liaison qui rencontre un code
//! qu'elle ne connaît pas l'y ramène, au lieu de le traiter en erreur
//! générique. Voir `docs/abi.md`.

use screengine::Error;

/// Success.
pub const SCG_OK: i32 = 0;

/// A pointer argument was null where the call requires one.
pub const SCG_ERR_NULL: i32 = -1;

/// An argument is outside what the engine accepts.
pub const SCG_ERR_INVALID_ARGUMENT: i32 = -2;

/// An allocation failed.
pub const SCG_ERR_OUT_OF_MEMORY: i32 = -3;

/// The call came out of sequence, such as rendering a tile before the frame began.
pub const SCG_ERR_INVALID_STATE: i32 = -4;

/// The engine panicked. The object is now faulted; see `SCG_ERR_FAULTED`.
///
/// A panic is an engine defect, never a reaction to invalid input. Read the
/// message with `scg_last_error` and report it.
pub const SCG_ERR_PANIC: i32 = -5;

/// A previous call on this object panicked and left it in an unspecified state.
///
/// Every call on a faulted object returns this code, except `scg_last_error`
/// and the destructor, which stay available so the host can read the cause and
/// release the object.
pub const SCG_ERR_FAULTED: i32 = -6;

/// Deprecated name of `SCG_ERR_FAULTED`, same value. Kept so that hosts written
/// against an earlier header still compile; it will not be removed before 1.0.
// Écrit en littéral et non par renvoi à `SCG_ERR_FAULTED` : les liaisons qui
// lisent les `#define` du header, JavaScript et PHP, n'évaluent pas un nom.
pub const SCG_ERR_POISONED: i32 = -6;

/// A lookup by stable identifier found nothing.
///
/// Returned by `scg_submit_world_visible` when no cell carries the given
/// identifier. The file is well formed and the call is well shaped: it is the
/// reference that finds no target, which is neither a malformation nor a bad
/// argument.
pub const SCG_ERR_UNKNOWN_RESOURCE: i32 = -100;

/// Success, and traversal stopped at one of its bounds.
///
/// **A positive code is a success carrying a status.** Judge a call by the sign
/// of its code, never by "not `SCG_OK`": a status a binding does not know is
/// handled as success, because ignoring one is always correct.
///
/// The image holds everything that was reached, and the far cell is drawn whole —
/// only its portals were not unfolded, so what is missing begins one cell
/// further. Neither bound is configurable, so there is nothing for the host to
/// adjust: this is a property of the level, not a fault in the call.
pub const SCG_STATUS_INCOMPLETE: i32 = 1;

/// Success, and no cell was given, so nothing was submitted.
///
/// The background, the alpha and the output curve are written as they are for an
/// empty scene. Unlike [`SCG_STATUS_INCOMPLETE`], the host has something to do:
/// place the camera in a cell again. The engine never relocates it on its own.
pub const SCG_STATUS_NO_CELL: i32 = 2;

/// Success, and the box was already inside solid geometry when the sweep began.
///
/// `fraction` is `0` and the normal is that of the least deeply penetrated
/// surface. **The engine does not push out**: there is no push-out vector defined
/// against a set of non-convex surfaces, so it reports and leaves the response to
/// the host.
///
/// **This is a status and not just `fraction == 0`**, which is ambiguous: an
/// immediate legitimate contact returns the same fraction — box exactly against a
/// wall, moving into it. The two call for opposite responses, sliding or getting
/// out, and this is the one thing the host cannot reconstruct.
///
/// A sweep that stays inside solid geometry the whole way returns this same
/// status, and `fraction` says the rest.
pub const SCG_STATUS_START_SOLID: i32 = 3;

/// Success, and the box is too small, where it moves, to keep any gap.
///
/// The contact returned is still correct: what is lost is the **re-placing**. A
/// sweep leaves the box a half margin short of the solid, and putting it back at
/// the returned fraction goes through `float` positions whose step is `|p| *
/// 2^-24`. Once that step covers the gap, the body lands inside the solid and the
/// next sweep starts penetrating — which is why this is a status and not an error.
///
/// **A ray never carries it**: its margin is zero by construction, so it has no
/// gap to lose.
///
/// **It is the last of the four, and it is the one to ask about rather than wait
/// for.** Where two statuses apply the call returns the most actionable, so a box
/// in this state is reported as [`SCG_STATUS_START_SOLID`] as soon as it has landed
/// in the solid — this status would then be masked in the very case it describes.
/// Call `scg_sweep_reach` with your box once, compare it to how far your level
/// reaches, and widen the box or move the level closer to the origin before any of
/// this happens.
pub const SCG_STATUS_NO_GAP: i32 = 4;

/// The greatest number of cells one sweep visits.
///
/// Reaching it returns [`SCG_STATUS_INCOMPLETE`] and **truncates** the move to
/// where the examined region stops — the only conservative answer, since
/// reporting a free move would send an entity through a wall the engine never
/// looked at. It is not configurable: a result that depended on a configuration
/// field would escape the conformance suite.
pub const SCG_SWEEP_CELLS: u32 = 64;

/// A cell has no lightmap yet.
pub const SCG_LIGHTMAP_ABSENT: u32 = 0;

/// A cell's lightmap is ready.
pub const SCG_LIGHTMAP_READY: u32 = 1;

/// A cell has a lightmap, but has changed since it was computed.
///
/// Never returned while nothing modifies a loaded map; the value is published now
/// because editing will produce it, and a published state does not change meaning.
pub const SCG_LIGHTMAP_STALE: u32 = 2;

/// How deep portal traversal follows a line of sight.
///
/// Exposed so a host can tell why an image came back incomplete. It is a constant
/// of the engine and cannot be configured: reaching it truncates the image, and
/// an image that depended on a configuration field would escape the conformance
/// suite.
pub const SCG_TRAVERSAL_DEPTH: u32 = 64;

/// How many cells one image may retain.
///
/// A second bound, which does not follow from the first: depth limits the length
/// of a path, this one the number of cells a single image can keep a window for.
pub const SCG_TRAVERSAL_CELLS: u32 = 4096;

/// The largest side of a cell's lightmap atlas, in luxels.
///
/// What a cell can cost in lightmap memory, which the host sizes its cache on:
/// four bytes per luxel, mipmap chain included. A map whose surfaces would not
/// pack into an atlas this size is refused at load, not at build time — the
/// error then names the map rather than one cell of it.
pub const SCG_MAX_LIGHTMAP_SIZE: u32 = 1024;

// **Ces valeurs sont écrites en littéral, et concordent par assertion.**
// `cbindgen` analyse la source syntaxiquement et n'interroge jamais `rustc` : une
// constante définie depuis un chemin du noyau n'entre pas dans le header, elle y
// disparaît en silence. Les recopier les y fait entrer ; l'assertion refuse la
// compilation le jour où le noyau change la sienne, ce qu'un commentaire ne
// ferait pas.
const _: () = assert!(SCG_TRAVERSAL_DEPTH as usize == screengine::TRAVERSAL_DEPTH);
const _: () = assert!(SCG_TRAVERSAL_CELLS as usize == screengine::TRAVERSAL_CELLS);
const _: () = assert!(SCG_MAX_LIGHTMAP_SIZE == screengine::MAX_LIGHTMAP_SIZE);
// Celle-ci manquait, seule des quatre bornes publiées : le noyau pouvait changer
// sa valeur sans que rien ne refuse la compilation, et le header aurait publié
// l'ancienne.
const _: () = assert!(SCG_SWEEP_CELLS as usize == screengine::SWEEP_CELLS);

/// The block is not a data file this library can read.
///
/// The fault is in the content, not in the call: report it to the user as a bad
/// asset, not to the developer as a misuse. Read the message with
/// `scg_last_error(NULL)` for what was rejected. Unknown section kinds are
/// rejected too, deliberately: silently skipping one would render a different
/// image from the same file, without an error.
pub const SCG_ERR_INVALID_FORMAT: i32 = -101;

/// The signature and kind are right, but this library does not read that format
/// version.
///
/// The only one of the three data codes that says what to do: take a newer
/// library, or export the data again.
pub const SCG_ERR_UNSUPPORTED_FORMAT_VERSION: i32 = -102;

/// Traduit une erreur du noyau en code d'ABI.
///
/// Sans bras générique : c'est ce qui fait échouer la compilation le jour où le
/// noyau gagne une variante que personne n'a pensé à traduire.
pub(crate) fn code_of(error: Error) -> i32 {
    match error {
        Error::InvalidArgument(_) => SCG_ERR_INVALID_ARGUMENT,
        Error::OutOfMemory => SCG_ERR_OUT_OF_MEMORY,
        Error::InvalidState => SCG_ERR_INVALID_STATE,
        // Le contrat d'ABI promet ce code quand une tuile a paniqué, et c'est
        // exactement ce que le noyau signale ici : il n'a pas vu la panique,
        // seulement une tuile entrée sans ressortir.
        Error::Faulted => SCG_ERR_FAULTED,
        Error::InvalidFormat(_) => SCG_ERR_INVALID_FORMAT,
        Error::UnsupportedFormatVersion => SCG_ERR_UNSUPPORTED_FORMAT_VERSION,
        Error::UnknownResource => SCG_ERR_UNKNOWN_RESOURCE,
    }
}

#[cfg(test)]
mod tests;
