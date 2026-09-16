// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! L'environnement flottant, fixé à l'entrée et rendu à l'hôte au retour.
//!
//! Le noyau suppose l'environnement par défaut — arrondi au plus proche, ni DAZ
//! ni FTZ, exceptions masquées — et ne touche jamais ces registres. C'est la frontière qui le lui
//! garantit, parce qu'une bibliothèque audio ou un moteur de jeu du même
//! processus peut les avoir changés, et que l'image en dépendrait.
//!
//! Les intrinsèques `_mm_getcsr` et `_mm_setcsr` ne conviennent pas : elles sont
//! dépréciées, et leur documentation qualifie de comportement indéfini le fait
//! de s'en servir pour modifier l'arrondi ou DAZ. `asm!`, stable bien avant la
//! version minimale du projet, est le seul recours.

#[cfg(target_arch = "x86_64")]
mod arch {
    use core::arch::asm;

    /// Le mot de contrôle SSE de l'hôte.
    pub(super) type Saved = u32;

    /// Efface FZ (bit 15), l'arrondi (14:13) et DAZ (bit 6), et masque les six
    /// exceptions (12:7).
    ///
    /// Les masques ne sont pas l'affaire de l'hôte seul : le moteur produit des
    /// résultats inexacts à chaque calcul de sommet, et un hôte qui a démasqué
    /// l'inexactitude — pour traquer un défaut chez lui — tomberait dès la
    /// première image. Ses drapeaux cumulés (5:0) restent, puisque masquer
    /// n'empêche pas de les lever.
    pub(super) fn normalize(control: Saved) -> Saved {
        (control & !0xE040) | 0x1F80
    }

    /// Lit le mot de contrôle.
    pub(super) fn read() -> Saved {
        let mut control = 0u32;
        // SAFETY: `stmxcsr` écrit quatre octets à l'adresse donnée, et
        // `control` est un `u32` vivant et aligné. Pas de `nomem` : le
        // compilateur ne doit pas déplacer de calcul flottant à travers cette
        // lecture.
        unsafe {
            asm!("stmxcsr [{}]", in(reg) &mut control, options(nostack, preserves_flags));
        }
        control
    }

    /// Écrit le mot de contrôle.
    pub(super) fn write(control: Saved) {
        // SAFETY: `ldmxcsr` relit les quatre octets d'un `u32` vivant et
        // aligné. La valeur écrite est toujours dérivée de celle que l'hôte
        // avait, donc jamais une combinaison de bits qu'il ne tolérerait pas.
        unsafe {
            asm!("ldmxcsr [{}]", in(reg) &control, options(nostack, preserves_flags));
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    use core::arch::asm;

    /// Le registre de contrôle flottant de l'hôte.
    pub(super) type Saved = u64;

    /// Efface AHP (26), DN (25), FZ (24), l'arrondi (23:22) et FZ16 (19), et les
    /// six autorisations de piège — IDE (15), IXE (12), UFE (11), OFE (10),
    /// DZE (9), IOE (8).
    ///
    /// DN est dans le lot alors que x86 n'a pas d'équivalent : le laisser à 1
    /// ferait propager les NaN autrement sur ARM que sur x86, et l'empreinte de
    /// conformance divergerait entre deux cibles pour cette seule raison. Les
    /// pièges, pour la même raison qu'en x86 : le moteur produit des résultats
    /// inexacts, et un piège autorisé par l'hôte tuerait l'appel.
    pub(super) fn normalize(control: Saved) -> Saved {
        control & !0x07C8_9F00
    }

    /// Lit le registre de contrôle.
    pub(super) fn read() -> Saved {
        let control;
        // SAFETY: FPCR est lisible depuis EL0, propre au thread courant, et
        // `mrs` ne touche pas la mémoire.
        unsafe {
            asm!("mrs {}, fpcr", out(reg) control, options(nostack, preserves_flags));
        }
        control
    }

    /// Écrit le registre de contrôle.
    pub(super) fn write(control: Saved) {
        // SAFETY: FPCR est inscriptible depuis EL0 et n'affecte que le thread
        // courant. La valeur est dérivée de celle de l'hôte.
        unsafe {
            asm!("msr fpcr, {}", in(reg) control, options(nostack, preserves_flags));
        }
    }
}

#[cfg(all(target_arch = "arm", target_feature = "vfp2"))]
mod arch {
    use core::arch::asm;

    /// Le registre d'état et de contrôle flottant de l'hôte.
    pub(super) type Saved = u32;

    /// Mêmes champs qu'en 64 bits, aux mêmes positions, autorisations de piège
    /// comprises.
    ///
    /// FPSCR porte en plus NZCV (31:28) et les drapeaux cumulés, qui sont
    /// l'état de l'hôte et se restaurent avec le reste.
    pub(super) fn normalize(control: Saved) -> Saved {
        control & !0x07C8_9F00
    }

    /// Lit le registre.
    pub(super) fn read() -> Saved {
        let control;
        // SAFETY: `vmrs` exige VFP, que le `cfg` de ce module garantit.
        unsafe {
            asm!("vmrs {}, fpscr", out(reg) control, options(nostack, preserves_flags));
        }
        control
    }

    /// Écrit le registre.
    pub(super) fn write(control: Saved) {
        // SAFETY: même garantie que pour la lecture.
        unsafe {
            asm!("vmsr fpscr, {}", in(reg) control, options(nostack, preserves_flags));
        }
    }
}

// wasm n'a aucun registre de contrôle flottant : sa spécification impose
// l'arrondi au plus proche et interdit d'écraser les sous-normaux, et `asm!`
// n'y existe pas. Le module neutre garde une enveloppe unique qui se compile
// partout et disparaît à l'optimisation. Il couvre aussi une cible ARM sans
// VFP, où les instructions ci-dessus seraient indéfinies.
#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    all(target_arch = "arm", target_feature = "vfp2")
)))]
mod arch {
    /// Rien à sauvegarder.
    pub(super) type Saved = ();

    /// Sans objet.
    pub(super) fn normalize(_control: Saved) -> Saved {}

    /// Sans objet.
    pub(super) fn read() -> Saved {}

    /// Sans objet.
    pub(super) fn write(_control: Saved) {}
}

/// Impose l'environnement par défaut le temps d'un appel.
///
/// `None` quand l'hôte l'avait déjà : l'écriture draine le pipeline flottant —
/// une poignée de cycles sur un cœur récent, une trentaine sur un ancien — et la
/// comparaison qui l'évite en coûte un.
pub(crate) struct FpEnv(Option<arch::Saved>);

impl FpEnv {
    /// Fixe l'environnement, et retient celui de l'hôte s'il différait.
    pub(crate) fn enter() -> Self {
        let host = arch::read();
        let ours = arch::normalize(host);
        if ours != host {
            arch::write(ours);
            Self(Some(host))
        } else {
            Self(None)
        }
    }
}

impl Drop for FpEnv {
    /// Rend à l'hôte l'environnement qu'il avait.
    ///
    /// Cette garde enveloppe `catch_unwind`, qui arrête tout dépliage avant
    /// elle : sa destruction a donc toujours lieu sur un retour normal, et
    /// l'interdiction de paniquer dans un `Drop` exporté n'est même pas
    /// sollicitée ici.
    fn drop(&mut self) {
        if let Some(host) = self.0 {
            arch::write(host);
        }
    }
}

#[cfg(test)]
mod tests;
