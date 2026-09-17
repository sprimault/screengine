// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que les tests du noyau partagent.

/// Le générateur des tests aléatoires : xorshift64*, écrit ici parce que le
/// noyau n'a aucune dépendance, et parce qu'un échec qui ne se rejoue pas
/// n'a pas été trouvé.
pub(crate) struct Rng(u64);

impl Rng {
    /// La graine ne peut pas être nulle : la suite y resterait bloquée.
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    /// L'entier suivant.
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Un entier entre `lo` et `hi`, bornes comprises.
    pub(crate) fn coord(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }
}
