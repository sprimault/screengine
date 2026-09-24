// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le post-traitement de sortie : un gain par canal, puis le gamma.
//!
//! Les deux se composent en **une table de 256 entrées par canal**, remplie au
//! réglage et lue par indexation pendant la recopie d'une tuile. Le pixel ne
//! paie donc que trois lectures, et rien du calcul qui les a produites.
//!
//! **Le gain vient avant le gamma**, et ce n'est pas indifférent pour qui règle
//! : les deux commutent à reparamétrisation près — `(a·x)^γ = a^γ·x^γ` —, si
//! bien qu'aucune image n'est perdue d'un côté ou de l'autre, mais le nombre
//! qu'un hôte écrit ne veut pas dire la même chose. Le gain corrige la source,
//! le gamma encode pour l'écran : c'est l'ordre dans lequel on les lit.
//!
//! Écarté : un contraste. Sur huit bits il n'apporte rien que le gain et le
//! gamma ne donnent déjà, il ne commute avec aucun des deux — une affine autour
//! d'un pivot et une puissance engendrent des familles de courbes distinctes —
//! et il faudrait graver ce pivot dans l'ABI. Il s'ajoutera le jour où une
//! capture montrera qu'il manque.

use alloc::vec::Vec;

use crate::error::{Argument, Error, Result};
use crate::math::exp2::powf;

/// Entrées par canal : une par valeur d'un octet.
const LEVELS: usize = 256;

/// Les trois canaux d'une table, R, G puis B.
const CHANNELS: usize = 3;

/// Le gamma le plus fort qu'un contexte accepte.
///
/// Au-delà, la courbe écrase tout dans les premiers niveaux et la table perd
/// son sens : seize valeurs distinctes sur deux cent cinquante-six. La borne
/// est large — un réglage d'écran vit entre 1,8 et 2,6 — et n'existe que pour
/// que l'absurde soit refusé plutôt que rendu.
const MAX_GAMMA: f32 = 8.0;

/// Le gain le plus fort qu'un canal accepte.
///
/// Quatre, soit deux diaphragmes. Au-delà, la saturation emporte la dynamique
/// haute et le canal devient un aplat — la même raison qui borne le
/// sur-éclairement.
const MAX_GAIN: f32 = 4.0;

/// Le décalage le plus fort qu'un canal accepte, en valeur absolue.
///
/// Un, c'est-à-dire l'échelle entière : au-delà, le canal est un aplat noir ou
/// blanc quoi que fasse son gain, et la borne n'a plus rien à border.
const MAX_OFFSET: f32 = 1.0;

/// Le post-traitement d'un contexte.
#[derive(Debug)]
pub struct Grade {
    /// Les trois courbes bout à bout, **vides tant que rien n'est réglé**.
    ///
    /// Sa vacuité est l'état neutre, et c'est ce qui rend l'identité exacte par
    /// construction plutôt que par un calcul qui devrait retomber juste : un
    /// contexte qu'on ne configure pas ne traverse aucune table.
    tables: Vec<u8>,
}

impl Grade {
    /// Un post-traitement neutre, dont les tables sont déjà réservées.
    pub fn new() -> Result<Self> {
        let mut tables = Vec::new();
        tables
            .try_reserve_exact(CHANNELS * LEVELS)
            .map_err(|_| Error::OutOfMemory)?;
        Ok(Self { tables })
    }

    /// Vrai si un post-traitement est réglé.
    pub fn is_set(&self) -> bool {
        !self.tables.is_empty()
    }

    /// Vide les tables : le rendu repasse par le chemin sans post-traitement.
    pub fn clear(&mut self) {
        self.tables.clear();
    }

    /// Règle le gamma, les trois gains et les trois décalages de canal.
    ///
    /// Chaque canal subit **une affine puis le gamma** : `x·gain + offset`,
    /// ramené dans `[0, 1]`, puis `^(1/gamma)`. L'affine est la transformation
    /// la plus générale qui reste locale au pixel et propre à un canal : elle
    /// contient le contraste, le niveau de noir et la teinte, là où un gain
    /// seul ne peut ni relever un noir ni abaisser un blanc — le gamma non
    /// plus, qui fixe les deux extrêmes.
    ///
    /// `gamma` est le gamma d'écran, dans le sens où **2,2 éclaircit** : c'est
    /// la convention qu'un intégrateur attend quand il lit ce mot. Il tient
    /// dans `]0, 8]`, les gains dans `[0, 4]`, les décalages dans `[-1, 1]`, et
    /// tout ce qui sort de là — ou n'est pas fini — rend
    /// [`Argument::Grade`](crate::error::Argument::Grade) sans rien changer.
    pub fn set(
        &mut self,
        gamma: f32,
        gains: [f32; CHANNELS],
        offsets: [f32; CHANNELS],
    ) -> Result<()> {
        // Le fini se teste d'abord et nommément : un NaN rend fausse toute
        // comparaison, et les bornes ci-dessous le laisseraient passer.
        let sane = gamma.is_finite() && gamma > 0.0 && gamma <= MAX_GAMMA;
        let gain_ok = |g: &f32| g.is_finite() && *g >= 0.0 && *g <= MAX_GAIN;
        let offset_ok = |o: &f32| o.is_finite() && *o >= -MAX_OFFSET && *o <= MAX_OFFSET;
        let sane = sane && gains.iter().all(gain_ok) && offsets.iter().all(offset_ok);
        if !sane {
            return Err(Error::InvalidArgument(Argument::Grade));
        }

        let exponent = 1.0 / gamma;
        self.tables.clear();
        for channel in 0..CHANNELS {
            for level in 0..LEVELS {
                let value = level as f32 / (LEVELS - 1) as f32;
                // L'affine sature avant le gamma : hors de `[0, 1]` il n'y a
                // pas de puissance qui tienne dans l'octet, et borner après
                // reviendrait à écrêter deux fois.
                let value = value * gains[channel] + offsets[channel];
                let value = if value > 1.0 { 1.0 } else { value };
                let value = if value < 0.0 { 0.0 } else { value };
                self.tables.push(quantize(powf(value, exponent)));
            }
        }
        Ok(())
    }
}

/// Ce qu'une recopie de tuile applique à chaque pixel avant de l'écrire.
///
/// Deux implémentations, choisies **une fois par tuile** et monomorphisées.
/// Sans ce détour, la recopie testerait à chaque pixel s'il y a un
/// post-traitement, et le paierait sur toute scène — y compris celles qui n'en
/// ont aucun, c'est-à-dire le cas nominal. C'est le défaut qu'un lot précédent
/// a déjà payé un dixième du remplissage pour corriger, au même endroit et pour
/// la même raison.
pub trait Transfer {
    /// La couleur écrite pour celle qu'on a trouvée, alpha écarté.
    fn apply(&self, pixel: u32) -> u32;
}

/// L'absence de post-traitement, qui ne coûte pas une instruction.
#[derive(Debug)]
pub struct Identity;

impl Transfer for Identity {
    fn apply(&self, pixel: u32) -> u32 {
        pixel
    }
}

impl Transfer for Grade {
    /// L'alpha n'est pas repris : la recopie le force à l'opacité au seul
    /// endroit que les deux chemins de sortie traversent, et une courbe qui y
    /// toucherait romprait le contrat d'ABI.
    fn apply(&self, pixel: u32) -> u32 {
        let channel = |index: usize, shift: u32| {
            let level = ((pixel >> shift) & 0xFF) as usize;
            u32::from(self.tables[index * LEVELS + level]) << shift
        };
        channel(0, 0) | channel(1, 8) | channel(2, 16)
    }
}

/// Ramène une valeur de `[0, 1]` sur un octet, arrondie au plus proche.
///
/// Le bornage est écrit avant la conversion : `as` sature, mais la saturation
/// ne sert jamais de garde-fou — la règle vaut ici comme partout où un flottant
/// devient entier.
fn quantize(value: f32) -> u8 {
    let scaled = value * 255.0 + 0.5;
    if scaled <= 0.0 {
        return 0;
    }
    if scaled >= 255.0 {
        return 255;
    }
    scaled as u8
}

#[cfg(test)]
mod tests;
