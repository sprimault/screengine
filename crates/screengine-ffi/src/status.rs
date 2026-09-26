// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les codes de retour de l'ABI.
//!
//! Les plages sont réservées par étape, et c'est une règle arithmétique : la
//! catégorie d'un code est `(-code) / 100`. Une liaison qui rencontre un code
//! qu'elle ne connaît pas l'y ramène, au lieu de le traiter en erreur
//! générique. Voir `docs/abi.md`.

use screengine::{Argument, Error, Malformation};

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

// **Les deux valeurs sont écrites en littéral, et concordent par assertion.**
// `cbindgen` analyse la source syntaxiquement et n'interroge jamais `rustc` : une
// constante définie depuis un chemin du noyau n'entre pas dans le header, elle y
// disparaît en silence. Les recopier les y fait entrer ; l'assertion refuse la
// compilation le jour où le noyau change la sienne, ce qu'un commentaire ne
// ferait pas.
const _: () = assert!(SCG_TRAVERSAL_DEPTH as usize == screengine::TRAVERSAL_DEPTH);
const _: () = assert!(SCG_TRAVERSAL_CELLS as usize == screengine::TRAVERSAL_CELLS);

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

/// Le texte anglais que `scg_last_error` rendra pour une erreur du noyau.
///
/// Anglais parce qu'il franchit la frontière, et jamais localisé : un message
/// qui change avec l'environnement donne des journaux qu'on ne peut plus
/// rapprocher d'un poste à l'autre. Le code dit la catégorie, le message dit
/// quel argument : une liaison ne décide rien sur ce texte.
pub(crate) fn message_of(error: Error) -> &'static str {
    match error {
        Error::InvalidArgument(Argument::Resolution) => {
            "invalid resolution: each side must be between 1 and 2048, and the current resolution within the maximum"
        }
        Error::InvalidArgument(Argument::TileSize) => "invalid tile size: must be 32 or 64",
        Error::InvalidArgument(Argument::Stride) => {
            "invalid stride: must be at least the internal width"
        }
        Error::InvalidArgument(Argument::BufferLength) => {
            "pixel buffer too short: needs stride x height x 4 bytes"
        }
        Error::InvalidArgument(Argument::TileIndex) => {
            "invalid tile index: must be less than the tile count of the frame"
        }
        Error::InvalidArgument(Argument::Region) => "invalid region: must lie within the image",
        Error::InvalidArgument(Argument::ScratchLength) => {
            "scratch buffer too short: needs one word per pixel of the region"
        }
        Error::InvalidArgument(Argument::TriangleCapacity) => {
            "too many triangles submitted for the capacity reserved at creation"
        }
        Error::InvalidArgument(Argument::VertexIndex) => {
            "invalid triangle index: must be less than the vertex count of the batch"
        }
        Error::InvalidArgument(Argument::Projection) => {
            "invalid projection: the vertical field of view must be within ]0, pi[ radians, and the near plane positive and finite"
        }
        Error::InvalidArgument(Argument::VertexCoordinate) => {
            "vertex coordinate is not finite: NaN and infinities are rejected, and the whole batch with them"
        }
        Error::InvalidArgument(Argument::TextureCapacity) => {
            "too many distinct textures in one frame for the capacity reserved at creation"
        }
        Error::InvalidArgument(Argument::TextureCoordinate) => {
            "texture coordinate is not finite, or beyond 16384 texels"
        }
        Error::InvalidArgument(Argument::TextureSize) => {
            "invalid texture size: each side must be a power of two between 1 and 2048"
        }
        Error::InvalidArgument(Argument::Overbright) => {
            "invalid overbright shift: must be 0, 1 or 2"
        }
        Error::InvalidArgument(Argument::Fog) => {
            "invalid fog range: start must be finite and not negative, end finite and beyond start"
        }
        Error::InvalidArgument(Argument::LightCapacity) => "too many dynamic lights for one frame",
        Error::InvalidArgument(Argument::Light) => {
            "invalid light: position must be finite, and radius finite and positive"
        }
        Error::InvalidArgument(Argument::Grade) => {
            "invalid output curve: gamma must be within ]0, 8], each channel gain within [0, 4], \
             each channel offset within [-1, 1]"
        }
        Error::InvalidArgument(Argument::TextureLength) => {
            "pixel block of the wrong length: needs width x height x 4 bytes"
        }
        Error::OutOfMemory => "out of memory",
        // Un seul message pour toute la variante, et il ne nomme aucun cas :
        // le noyau ne distingue pas une tuile déjà rendue d'une soumission
        // pendant le rendu, et un texte qui parlerait de tuiles enverrait sur
        // une fausse piste l'hôte qui a simplement soumis trop tard.
        Error::InvalidState => {
            "call out of sequence: check the frame state and whether this tile was already rendered"
        }
        Error::Faulted => "a tile did not return from rendering; this frame is incomplete",
        Error::InvalidFormat(Malformation::Truncated) => {
            "malformed data file: a field, the section table or a section runs past the end of the block"
        }
        Error::InvalidFormat(Malformation::Signature) => {
            "not a Screengine data file: the four signature bytes do not match"
        }
        Error::InvalidFormat(Malformation::Kind) => {
            "wrong kind of data file: a mesh was given where a world was expected, or the reverse"
        }
        Error::InvalidFormat(Malformation::Length) => {
            "malformed data file: the declared total length is not the length of the block received"
        }
        Error::InvalidFormat(Malformation::SectionKind) => {
            "malformed data file: a section of a kind this version does not know"
        }
        Error::InvalidFormat(Malformation::SectionOrder) => {
            "malformed data file: sections must be in increasing kind order, at most one of each"
        }
        Error::InvalidFormat(Malformation::SectionBounds) => {
            "malformed data file: sections must pave the file, with no gap and no overlap"
        }
        Error::InvalidFormat(Malformation::NonFinite) => {
            "malformed data file: a floating-point value is not finite"
        }
        Error::InvalidFormat(Malformation::NonUtf8) => {
            "malformed data file: a name is not valid UTF-8"
        }
        Error::InvalidFormat(Malformation::Index) => {
            "malformed data file: an index is beyond what it points into"
        }
        Error::InvalidFormat(Malformation::Identifier) => {
            "malformed data file: an identifier is zero, or used twice in its family"
        }
        Error::InvalidFormat(Malformation::GroupBounds) => {
            "malformed data file: surface groups must pave the triangles in order, with no gap and no overlap"
        }
        Error::InvalidFormat(Malformation::Count) => {
            "malformed data file: a count does not match the length that bounds it"
        }
        Error::InvalidFormat(Malformation::Flags) => {
            "malformed data file: undefined flag bits must be zero"
        }
        Error::InvalidFormat(Malformation::Polygon) => {
            "malformed data file: a polygon is degenerate, too large, or not convex where convexity is required"
        }
        Error::InvalidFormat(Malformation::Mapping) => {
            "malformed data file: a mapping frame is unusable, or the coordinates it derives are not finite"
        }
        Error::InvalidFormat(Malformation::Light) => {
            "malformed data file: a static light has a radius that is not finite and positive"
        }
        Error::InvalidFormat(Malformation::Pose) => {
            "malformed data file: an orientation is a zero quaternion, which carries no direction"
        }
        Error::InvalidFormat(Malformation::Portal) => {
            "malformed data file: three portals share the same vertices"
        }
        Error::UnsupportedFormatVersion => {
            "unsupported data format version: take a newer library, or export the data again"
        }
        Error::UnknownResource => "unknown identifier: the resource holds no such cell",
    }
}

#[cfg(test)]
mod tests;
