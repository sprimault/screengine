// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que le processeur x86 répond de lui-même.
//!
//! **Trois questions et non une, et les sauter est le défaut classique.** Qu'un
//! processeur sache exécuter AVX2 ne suffit pas : ses registres larges ne sont
//! utilisables que si le **système** les sauvegarde au changement de contexte.
//! Un noyau qui ne le fait pas laisse une tâche écraser les registres d'une
//! autre, et le symptôme est un calcul faux par intermittence, sur une machine
//! qui annonce pourtant le jeu d'instructions. L'ordre ci-dessous est donc
//! contraint, et chaque étape garde la suivante :
//!
//! 1. `OSXSAVE` dit que `XGETBV` est lisible — l'interroger sans l'avoir demandé
//!    est une instruction illégale, pas un zéro ;
//! 2. `XGETBV(0)` dit que le système sauvegarde bien les états SSE **et** AVX ;
//! 3. la feuille 7 du `cpuid` dit enfin qu'AVX2 existe.
//!
//! **Et la feuille 7 ne se lit qu'après avoir demandé jusqu'où les feuilles
//! vont.** Sur un processeur dont la feuille maximale est inférieure à sept, la
//! lire rend le contenu de la plus haute au lieu d'une erreur : les bits sont
//! alors ceux d'une autre question, et ils répondent n'importe quoi.

#![allow(unsafe_code)]

use core::sync::atomic::{AtomicU8, Ordering};

#[cfg(target_arch = "x86")]
use core::arch::x86 as arch;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64 as arch;

/// Le bit `OSXSAVE` de `CPUID.1:ECX`.
const OSXSAVE: u32 = 1 << 27;

/// Le bit `AVX2` de `CPUID.7.0:EBX`.
const AVX2: u32 = 1 << 5;

/// Les deux bits de `XCR0` qui disent que SSE et AVX sont sauvegardés.
///
/// Le premier est l'état `XMM`, le second l'état `YMM` ; AVX2 a besoin des deux,
/// et un système qui n'en sauvegarde qu'un laisserait l'autre se perdre.
const XCR0_SSE_AVX: u64 = 0b110;

/// AVX2 est-il utilisable, processeur **et** système compris ?
///
/// **La réponse est retenue après la première question**, et ce cache n'est pas
/// une optimisation de confort : sans lui, le chemin AVX2 est **treize fois plus
/// lent que le scalaire**, mesuré le 2026-10-07 — 3,97 ms contre 0,30 sur un
/// plein cadre uni. `cpuid` sérialise l'exécution et vide le pipeline, et le
/// choix du chemin a lieu une fois par **ligne de triangle**, pas une fois par
/// image.
///
/// **C'est une fréquence que ce fichier annonçait fausse.** Il disait « une fois
/// par image et non par pixel » pour justifier l'absence de cache ; la mesure a
/// montré des centaines d'appels par image, et la régression touchait toute
/// scène rendue par défaut, `SCG_SIMD_AUTO` résolvant vers AVX2 sur un poste
/// récent.
///
/// **Un statique partagé entre contextes et entre threads est ici sans danger**,
/// là où il ne le serait pas pour un réglage : ce qu'il retient est une
/// propriété de la machine, immuable pour la vie du processus. Deux threads qui
/// le peupleraient en même temps y écriraient la même valeur, et `Relaxed`
/// suffit — il n'y a aucune autre écriture à ordonner avec celle-ci.
pub fn has_avx2() -> bool {
    // Trois états : pas encore demandé, absent, présent. Un `AtomicBool` ne les
    // distinguerait pas du premier, et un second drapeau ouvrirait une course
    // entre les deux.
    const UNKNOWN: u8 = 0;
    const ABSENT: u8 = 1;
    const PRESENT: u8 = 2;
    static CACHED: AtomicU8 = AtomicU8::new(UNKNOWN);

    match CACHED.load(Ordering::Relaxed) {
        ABSENT => false,
        PRESENT => true,
        _ => {
            let present = probe_avx2();
            CACHED.store(if present { PRESENT } else { ABSENT }, Ordering::Relaxed);
            present
        }
    }
}

/// La question posée au processeur, sans cache.
///
/// Séparée de [`has_avx2`] pour que le cache tienne en quatre lignes lisibles et
/// que l'ordre des trois interrogations reste au premier plan.
// **Le bloc `unsafe` autour de `__cpuid` est exigé par le plancher, pas par la
// chaîne courante**, qui l'a marquée sûre depuis — l'instruction est dans la base
// de toute cible x86, n'écrit rien et ne lit aucune mémoire, donc elle n'a aucune
// précondition à confier à l'appelant. Sans ce bloc, `make msrv` ne compile plus ;
// avec lui, une chaîne récente le signale inutile.
//
// **`allow` et non `expect`**, qui serait la forme juste partout ailleurs : il
// rougirait sur le plancher, là où le bloc est bel et bien nécessaire et où le
// lint ne se déclenche donc pas.
//
// **À retirer le jour où `make msrv` passe sans lui**, et cette condition se
// vérifie mécaniquement plutôt que de se chercher dans des notes de version.
#[allow(unused_unsafe)]
fn probe_avx2() -> bool {
    // SAFETY: `__cpuid` ne lit ni n'écrit de mémoire et existe sur toute cible
    // x86 ; l'appelant n'a aucune précondition à tenir. Seul `_xgetbv`, plus bas,
    // en a une véritable.
    let (max_leaf, features) = unsafe { (arch::__cpuid(0).eax, arch::__cpuid(1).ecx) };

    if features & OSXSAVE == 0 {
        return false;
    }

    // SAFETY: `OSXSAVE` vient d'être lu à 1, et c'est exactement la précondition
    // de `_xgetbv` : sans lui, l'instruction est illégale et le programme tombe.
    // L'argument zéro désigne `XCR0`, le seul registre dont cette question a
    // besoin.
    let state = unsafe { arch::_xgetbv(0) };
    if state & XCR0_SSE_AVX != XCR0_SSE_AVX {
        return false;
    }

    // La feuille 7 n'existe pas partout, et la lire au-delà du maximum rend la
    // plus haute feuille disponible — donc des bits qui répondent à une autre
    // question.
    if max_leaf < 7 {
        return false;
    }

    // SAFETY: même garantie que plus haut, et la feuille 7 vient d'être annoncée
    // atteignable par la feuille zéro.
    unsafe { arch::__cpuid_count(7, 0).ebx & AVX2 != 0 }
}
