// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le choix d'un chemin de remplissage, et les variantes vectorielles.
//!
//! **Le scalaire est la référence, et il reste compilé et testé.** Toute
//! variante se valide contre son empreinte : une divergence est une variante
//! fausse, jamais une différence acceptable. C'est aussi pourquoi le scalaire
//! vit dans [`crate::raster::triangle`] et non ici — les fondre supprimerait la
//! référence, et la duplication est voulue.
//!
//! **Ce module est le seul endroit du noyau qui autorise `unsafe`**, et il ne
//! s'en sert que pour deux choses : interroger le processeur, et appeler les
//! intrinsèques. Le reste du crate garde son `#![deny(unsafe_code)]`.
//!
//! # Ce que la sélection a vraiment à décider
//!
//! Beaucoup moins qu'il n'y paraît, et c'est ce qui donne sa forme à ce module.
//! Trois des quatre jeux d'instructions se décident **à la compilation** :
//!
//! - **SSE2** est dans la base de `x86_64`, et **NEON** dans celle d'`aarch64` ;
//! - **`simd128`** se demande au compilateur, donc `target_feature` le dit ;
//! - **armv7 et ARMv6 n'ont rien à décider** : les fonctionnalités ARM 32 bits
//!   sont instables, leur `cfg` n'est jamais vrai sur une chaîne stable, et ces
//!   cibles retombent sur le scalaire — ce que la feuille de route annonce pour
//!   le Pi 1 et le Zero.
//!
//! **Il ne reste qu'AVX2**, qui est le seul à se demander au processeur.
//! `is_x86_feature_detected!` ne peut pas servir : elle vit dans `std`, parce
//! que la détection réclame le système sur la plupart des architectures. Le
//! `cpuid` est donc écrit à la main dans [`x86`], et c'est tout ce que coûte
//! l'absence de `std` ici.
//!
//! # Pourquoi le chemin se force
//!
//! Sans levier, une variante ne se valide que sur une machine qui la porte, et
//! **SSE2 ne serait jamais comparé au scalaire sur une machine qui a AVX2** —
//! c'est-à-dire sur presque toutes. Le forçage existe pour que les trois chemins
//! se jouent sur le même processeur et rendent les mêmes bits.
//!
//! **Il est porté par le contexte, jamais par un statique du crate.** Un réglage
//! global serait partagé entre contextes et entre threads, et les tests de ce
//! dépôt tournent en parallèle : un cas qui force SSE2 déciderait de ce que voit
//! un cas qui force le scalaire, sans que rien ne le signale. Le contexte est
//! déjà l'objet dont la concurrence est écrite.

#[cfg(all(
    target_feature = "sse2",
    any(target_arch = "x86", target_arch = "x86_64")
))]
pub mod sse2;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86;

/// Un chemin de remplissage.
///
/// **L'ordre des variantes n'a aucun sens numérique** : il n'y a pas de « plus
/// rapide » à comparer, et rien n'itère dessus. Ce qui compte est que chacune
/// désigne un jeu d'instructions, et `Auto` l'absence de choix.
///
/// **`SimdPath` et non `Path`**, qui serait le nom court et juste : il entre en
/// collision avec `std::path::Path`, que tout hôte Rust qui lit un fichier
/// importe — c'est-à-dire tous. Le nom court aurait coûté une désambiguïsation à
/// chaque intégrateur, et le réexport plat de l'étage d'accueil l'aurait posé à
/// côté des types de fichiers. Celui-ci suit en outre `SCG_SIMD_*` et
/// `scg_set_simd`, qui le portent à travers la frontière.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SimdPath {
    /// Le moteur choisit, et c'est le défaut.
    ///
    /// Zéro vaut défaut pour un réglage de contexte, et celui-ci en est un.
    #[default]
    Auto,
    /// Le rasteriseur scalaire, qui est la référence.
    Scalar,
    /// SSE2, que toute cible `x86_64` porte.
    Sse2,
    /// AVX2, le seul jeu que ce module demande au processeur.
    Avx2,
    /// NEON, que toute cible `aarch64` porte.
    Neon,
    /// `simd128`, décidé à la compilation.
    Simd128,
}

impl SimdPath {
    /// Ce chemin est-il utilisable sur cette machine, telle qu'elle est compilée
    /// et telle qu'elle tourne ?
    ///
    /// **`Auto` et `Scalar` le sont toujours**, et c'est ce qui garantit qu'un
    /// hôte ne peut pas se mettre dans une situation sans chemin : le scalaire
    /// n'a besoin de rien.
    pub fn available(self) -> bool {
        match self {
            Self::Auto | Self::Scalar => true,
            Self::Sse2 => cfg!(target_feature = "sse2"),
            Self::Avx2 => avx2_available(),
            Self::Neon => cfg!(target_arch = "aarch64"),
            Self::Simd128 => cfg!(target_feature = "simd128"),
        }
    }

    /// Le chemin que `Auto` désigne sur cette machine.
    ///
    /// **Le plus large que la cible porte**, et rien d'autre à arbitrer : les
    /// variantes rendent toutes la même image, donc le choix ne porte que sur le
    /// coût. Un chemin déjà explicite se rend lui-même.
    pub fn resolve(self) -> Self {
        if self != Self::Auto {
            return self;
        }
        if Self::Avx2.available() {
            Self::Avx2
        } else if Self::Neon.available() {
            Self::Neon
        } else if Self::Simd128.available() {
            Self::Simd128
        } else if Self::Sse2.available() {
            Self::Sse2
        } else {
            Self::Scalar
        }
    }
}

/// Calcule les profondeurs d'une ligne par le chemin demandé.
///
/// Rend **vrai** quand une variante a rempli `out`, **faux** quand ce chemin n'a
/// rien à offrir ici — à l'appelant de retomber alors sur
/// [`crate::raster::span_scalar`], qui est la référence.
///
/// **Le point unique où les `cfg` de cible vivent.** Un puits qui appelle cette
/// fonction n'a pas à savoir sur quelle architecture il tourne ni quels modules
/// existent : la question « ce chemin sait-il faire » se pose une fois, ici, et
/// les branches absentes se compilent en « non ».
pub fn span_depths(path: SimdPath, depth: i64, depth_x: i64, out: &mut [u32]) -> bool {
    match path {
        #[cfg(all(
            target_feature = "sse2",
            any(target_arch = "x86", target_arch = "x86_64")
        ))]
        SimdPath::Sse2 => {
            sse2::depths(depth, depth_x, out);
            true
        }
        // Tout le reste retombe sur la référence : les variantes qui n'existent
        // pas encore, celles que cette cible ne porte pas, et le scalaire, qui
        // **est** la référence.
        _ => {
            let _ = (depth, depth_x, out);
            false
        }
    }
}

/// AVX2 est-il utilisable, processeur et système compris ?
///
/// Hors de `x86`, la question ne se pose pas et la réponse est non.
fn avx2_available() -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        x86::has_avx2()
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        false
    }
}

#[cfg(test)]
mod tests;
