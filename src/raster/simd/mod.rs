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

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod avx2;
#[cfg(target_arch = "aarch64")]
pub mod neon;
#[cfg(target_feature = "simd128")]
pub mod simd128;
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

/// Une ligne d'une surface unie, telle qu'une variante la reçoit.
///
/// **Les deux tranches sont celles de la ligne, déjà découpées par le puits** :
/// une variante ne sait rien de la largeur d'une région ni du rectangle d'une
/// tuile, et n'a donc aucun indice à calculer. Ce qu'elle reçoit est exactement
/// ce qu'elle a le droit d'écrire.
// **Sur une cible sans variante, aucun champ n'est lu**, et c'est le cas normal
// plutôt qu'un oubli : wasm, armv7 et la cible sans `std` n'ont que le chemin
// scalaire, si bien que la structure traverse `fill_flat_row` pour être jetée
// par sa branche par défaut. Le lint le signale sur ces cibles seulement, que
// `make lint` passe précisément parce que leurs `cfg` ne sont vérifiés nulle
// part ailleurs.
#[allow(dead_code)]
pub struct FlatRow<'a> {
    /// Les couleurs de la ligne, à écrire là où la profondeur passe.
    pub color: &'a mut [u32],
    /// Les profondeurs de la ligne, lues puis écrites de même.
    pub depth: &'a mut [u32],
    /// La profondeur au premier pixel, avant son décalage de gradient.
    pub start: i64,
    /// Ce que la profondeur gagne d'un pixel au suivant.
    pub step: i64,
    /// La couleur à écrire, constante sur toute la ligne.
    pub fill: u32,
}

/// Remplit une ligne unie par le chemin demandé, test de profondeur compris.
///
/// Rend **vrai** quand une variante a traité la ligne, **faux** quand ce chemin
/// n'a rien à offrir ici — à l'appelant de retomber alors sur
/// [`crate::raster::span_scalar`], qui est la référence.
///
/// **Le point unique où les `cfg` de cible vivent.** Un puits qui appelle cette
/// fonction n'a pas à savoir sur quelle architecture il tourne ni quels modules
/// existent : la question « ce chemin sait-il faire » se pose une fois, ici, et
/// les branches absentes se compilent en « non ».
///
/// **Le test et l'écriture sont dedans, et c'est ce que la mesure a imposé.**
/// La première forme ne calculait que les profondeurs, à charge pour le puits de
/// les relire pixel par pixel : l'aller-retour en mémoire coûtait plus que
/// l'addition épargnée, et les variantes étaient **plus lentes** que le
/// scalaire — 0,34 ms contre 0,29 sur un plein cadre uni. Ce qui se gagne ici
/// est la comparaison et l'écriture masquée de plusieurs pixels à la fois, pas
/// l'interpolation.
pub fn fill_flat_row(path: SimdPath, row: FlatRow<'_>) -> bool {
    match path {
        #[cfg(all(
            target_feature = "sse2",
            any(target_arch = "x86", target_arch = "x86_64")
        ))]
        SimdPath::Sse2 => {
            sse2::fill_flat_row(row);
            true
        }
        // **AVX2 interroge le processeur, là où SSE2 se décide à la
        // compilation**, et c'est toute la différence entre les deux branches.
        // La vérification et l'appel `unsafe` vivent dans le module de la
        // variante, pour que la précondition et sa garantie ne soient jamais à
        // deux étages l'une de l'autre — ce qui laisse ce fichier-ci entièrement
        // sûr, sous le `deny(unsafe_code)` du crate.
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        SimdPath::Avx2 => avx2::fill_flat_row_if_available(row),
        // NEON se décide à la compilation comme SSE2, la cible le portant dans
        // sa base : rien à demander au processeur, et rien à activer.
        #[cfg(target_arch = "aarch64")]
        SimdPath::Neon => {
            neon::fill_flat_row(row);
            true
        }
        // `simd128` se demande au compilateur, donc `target_feature` le dit —
        // et un module compilé sans lui n'a pas cette branche, exactement comme
        // une machine sans le jeu n'aurait pas chargé celui qui l'a.
        #[cfg(target_feature = "simd128")]
        SimdPath::Simd128 => {
            simd128::fill_flat_row(row);
            true
        }
        // Tout le reste retombe sur la référence : les variantes qui n'existent
        // pas encore, celles que cette cible ne porte pas, et le scalaire, qui
        // **est** la référence.
        _ => {
            let _ = row;
            false
        }
    }
}

/// Un segment **texturé** d'une ligne, tel qu'une variante le reçoit.
///
/// **Le cas le plus simple qui échantillonne**, et c'est ce qui le rend
/// mesurable : une texture tramée, sans masquage, sans éclairage et sans
/// modulation. Tout ce qui s'en écarte retombe sur le chemin scalaire, qui reste
/// la référence.
///
/// **Les deux tranches sont celles du segment**, que le puits a découpées : une
/// variante ne sait rien du rectangle d'une région et n'a aucun indice à
/// calculer.
// Même raison que pour `FlatRow` : sur une cible sans variante, aucun champ
// n'est lu, et c'est le cas normal plutôt qu'un oubli.
#[allow(dead_code)]
pub struct SampledRow<'a> {
    /// Les couleurs du segment, à écrire là où la profondeur passe.
    pub color: &'a mut [u32],
    /// Les profondeurs, lues puis écrites de même.
    pub depth: &'a mut [u32],
    /// La profondeur au premier pixel, avant son décalage de gradient.
    pub start: i64,
    /// Ce que la profondeur gagne d'un pixel au suivant.
    pub step: i64,
    /// Les deux coordonnées de texture au premier pixel, **avant** le décalage
    /// de niveau.
    pub uv: [i64; 2],
    /// Ce que chacune gagne d'un pixel au suivant.
    pub uv_step: [i64; 2],
    /// Le décalage de niveau, **non borné par la hauteur de la pile**.
    ///
    /// C'est celui que le scalaire applique à la coordonnée interpolée, et le
    /// borner ici déplacerait l'image d'une surface dont la densité dépasse sa
    /// pile de mipmaps : la lecture, elle, se fait dans le dernier niveau, d'où
    /// deux valeurs et non une.
    pub shift: u32,
    /// Les texels du niveau lu, lignes jointives.
    pub texels: &'a [u32],
    /// Ses dimensions, toutes deux puissances de deux.
    pub size: (u32, u32),
    /// Les quatre décalages de tramage du bloc, en `u` puis en `v`.
    ///
    /// **Ils ne dépendent pas du bloc**, et c'est ce qui les met hors de la
    /// boucle : le motif a une période de quatre en `x`, les blocs avancent de
    /// quatre, donc `x & 3` ne change pas d'un bloc au suivant. Le tramage ne
    /// coûte alors rien par pixel, là où le scalaire lit sa table à chacun.
    pub dither: [[i32; 4]; 2],
    /// L'abscisse du premier pixel dans l'image, dont le reste scalaire a besoin.
    pub x0: i32,
    /// Son ordonnée, de même.
    pub y: i32,
}

/// Remplit un segment texturé par le chemin demandé, test de profondeur
/// compris.
///
/// Rend le **nombre de pixels traités**, toujours un multiple de la largeur du
/// chemin, ou `None` quand ce chemin n'a rien à offrir ici.
///
/// **Le reste du segment appartient à l'appelant**, et c'est voulu : il le finit
/// par son parcours scalaire, celui-là même qui fait référence. Une variante qui
/// terminerait elle-même recopierait la formule d'échantillonnage — niveau,
/// tramage, repli, adressage — dans un second endroit, et c'est exactement le
/// genre de copie dont ce projet sait qu'elle finit par diverger. Le marcheur
/// étant affine, l'avancer de ce compte est exact.
///
/// **AVX2 passe ici par le chemin SSE2**, qu'il porte toujours. Ce n'est pas un
/// oubli : lui donner son propre chemin large est un lot à lui, et sans ce
/// renvoi une machine qui a AVX2 — c'est-à-dire presque toutes — retomberait au
/// scalaire alors qu'elle sait faire mieux. Les deux rendent les mêmes bits,
/// étant le même code.
pub fn fill_sampled_row(path: SimdPath, row: SampledRow<'_>) -> Option<usize> {
    match path {
        #[cfg(all(
            target_feature = "sse2",
            any(target_arch = "x86", target_arch = "x86_64")
        ))]
        SimdPath::Sse2 | SimdPath::Avx2 => Some(sse2::fill_sampled_row(row)),
        _ => {
            let _ = row;
            None
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
