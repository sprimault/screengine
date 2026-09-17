// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les angles, et la trigonométrie sans libm.
//!
//! Un angle est un entier sur 32 bits où 2³² vaut un tour. Le tour boucle par
//! l'arithmétique modulaire, un demi-angle est un décalage exact, et les
//! symétries entre quadrants le sont aussi : `sin(-a)` rend exactement
//! `-sin(a)`. Le sinus se lit dans une table d'un quart de cercle, interpolée
//! linéairement.

use core::f64::consts::PI;

/// Un angle, 2³² valant un tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Angle(pub u32);

/// Un quart de tour.
const QUARTER: u32 = 1 << 30;

/// Intervalles de la table sur un quart de cercle.
///
/// Le pas vaut 1,5·10⁻³ radian. Sans interpolation, l'erreur ferait bouger un
/// sommet d'un demi-pixel à 640 pixels de large, visible en rotation lente ;
/// interpolée, elle tombe sous la précision du `f32`.
const STEPS: usize = 1 << 10;

/// Bits de l'angle, dans un quadrant, sous l'index de la table.
const FRACTION_BITS: u32 = 30 - 10;

/// `sin` aux `STEPS + 1` points du quart de cercle, bornes comprises.
static QUARTER_SINE: [f32; STEPS + 1] = quarter_sine();

/// Calcule la table, à la compilation.
///
/// Par une série de Taylor en `f64`, puis une conversion en `f32`. Le calcul
/// flottant à la compilation suit IEEE 754 au bit près depuis Rust 1.82 : la
/// table ne dépend ni de la machine qui construit, ni de la cible. Écartés : un
/// `build.rs`, qui appellerait la libm de la machine de construction, et une
/// table littérale, qu'aucun relecteur ne vérifierait.
///
/// Les tests rappellent cette fonction à l'exécution et comparent les bits.
const fn quarter_sine() -> [f32; STEPS + 1] {
    let mut table = [0.0f32; STEPS + 1];
    let mut i = 0;
    while i <= STEPS {
        let x = (PI / 2.0) * (i as f64 / STEPS as f64);
        // Sur [0, π/2], le terme de rang 13 tombe sous 10⁻²² : vingt termes
        // laissent la somme exacte au double près.
        let mut term = x;
        let mut sum = x;
        let mut n = 1;
        while n < 20 {
            term = -term * x * x / ((2 * n) as f64 * (2 * n + 1) as f64);
            sum += term;
            n += 1;
        }
        table[i] = sum as f32;
        i += 1;
    }
    table
}

/// Le sinus d'une position dans le premier quadrant, `0 ..= QUARTER`.
fn quarter(position: u32) -> f32 {
    let index = (position >> FRACTION_BITS) as usize;
    if index == STEPS {
        return QUARTER_SINE[STEPS];
    }
    let fraction = position & ((1 << FRACTION_BITS) - 1);
    // Exact : un entier de vingt bits et une puissance de deux.
    let weight = fraction as f32 / (1u32 << FRACTION_BITS) as f32;
    let (a, b) = (QUARTER_SINE[index], QUARTER_SINE[index + 1]);
    a + (b - a) * weight
}

/// Un tour, en radians, et son inverse.
const INV_TAU: f32 = (1.0 / (2.0 * PI)) as f32;

/// La résolution de la conversion depuis les radians : 2²⁴ pas par tour, ce
/// qu'un `f32` distingue sur un tour.
const TURN_UNITS: f32 = (1u32 << 24) as f32;

impl Angle {
    /// Un quart de tour.
    pub const QUARTER: Self = Self(QUARTER);

    /// Convertit des radians.
    ///
    /// La réduction à un tour est écrite avant la conversion vers l'entier, et
    /// c'est tout l'objet de cette fonction : une conversion flottant → entier
    /// en dépassement sature en Rust scalaire et rend `0x80000000` en SSE. Ici,
    /// aucune conversion ne reçoit une valeur hors de son intervalle, sur
    /// aucun chemin. Un NaN, un infini, ou plus de 2²⁴ tours — où un `f32` ne
    /// distingue plus les fractions de tour — donnent l'angle nul.
    pub fn from_radians(radians: f32) -> Self {
        let turns = radians * INV_TAU;
        if turns.is_nan() || turns.abs() >= TURN_UNITS {
            return Self(0);
        }
        // `turns` tient dans ±2²⁴ : la conversion vers `i32` est exacte à la
        // troncature près, et la différence l'est aussi.
        let mut fraction = turns - (turns as i32) as f32;
        if fraction < 0.0 {
            fraction += 1.0;
        }
        // `fraction` est dans [0, 1], donc le produit dans [0, 2²⁴] : pas de
        // dépassement. Le masque ramène exactement 2²⁴, un tour, à zéro.
        let units = (fraction * TURN_UNITS) as u32 & ((1 << 24) - 1);
        Self(units << 8)
    }

    /// Le sinus.
    pub fn sin(self) -> f32 {
        let position = self.0 & (QUARTER - 1);
        match self.0 >> 30 {
            0 => quarter(position),
            1 => quarter(QUARTER - position),
            2 => -quarter(position),
            _ => -quarter(QUARTER - position),
        }
    }

    /// Le cosinus, lu comme le sinus du complément.
    pub fn cos(self) -> f32 {
        Self(QUARTER.wrapping_sub(self.0)).sin()
    }

    /// La moitié de l'angle, exacte.
    ///
    /// Un angle et le même plus un tour ont des moitiés qui diffèrent d'un
    /// demi-tour : pour un quaternion, c'est l'opposé, qui décrit la même
    /// rotation.
    pub fn half(self) -> Self {
        Self(self.0 >> 1)
    }
}

#[cfg(test)]
mod tests;
