// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le brouillard par la distance, et la table qui le rend abordable.
//!
//! Il s'applique **pendant la recopie d'une tuile**, qui a déjà sa profondeur
//! sous la main : rien n'entre dans la boucle de remplissage, et un pixel que
//! rien n'a peint — profondeur nulle, donc infiniment lointaine — prend le
//! brouillard plein sans qu'on ait à le traiter à part. C'est ce qui supprime
//! la couture d'horizon, là où un fond effacé séparément la dessine.

use alloc::vec::Vec;

use crate::error::{Argument, Error, Result};

/// Le facteur de brouillard plein.
///
/// **Deux cent cinquante-six, et non deux cent cinquante-cinq** : le mélange
/// doit être exact **aux deux bouts**. À zéro, une surface non embrumée sort
/// identique au rendu sans brouillard ; au plein, la géométrie lointaine sort
/// exactement de la couleur du fond. Un seul niveau d'écart entre les deux
/// *est* la couture qu'on cherche à éviter, et il faut neuf bits pour ne pas
/// l'avoir.
pub const FULL: u16 = 256;

/// Entrées de la table, et donc bits de l'index.
///
/// Onze : cinq d'exposant et six de mantisse. Indexer linéairement une
/// profondeur en 0.32 serait inutilisable — tout le monde visible vit sous
/// 2²⁶, si bien qu'une table linéaire y consacrerait une entrée sur
/// soixante-quatre. L'index se prend donc comme celui du mipmap, sur la
/// représentation flottante de l'entier.
const TABLE_LEN: usize = 2048;

/// Bits de mantisse retenus dans l'index.
///
/// **Les tranches d'exposant élevé ne sont jamais lues**, et c'est sans
/// conséquence : une profondeur de six bits ne peut pas en distinguer
/// soixante-quatre, mais `to_depth` la borne à `DEPTH_MARGIN`, si bien que
/// l'exposant ne dépasse jamais 25. Les trois cent quatre-vingts dernières
/// entrées coûtent sept cent soixante octets et ne servent qu'à garder l'index
/// en deux décalages, sans comparaison ni bornage.
const MANTISSA_BITS: u32 = 6;

/// Le brouillard d'un contexte : sa couleur, et le facteur de chaque tranche
/// de profondeur.
///
/// La table se remplit à chaque réglage — un appel nommé —, jamais par image.
/// Sa capacité est réservée à la création du contexte, si bien que le réglage
/// n'alloue pas non plus.
#[derive(Debug)]
pub struct Fog {
    /// La couleur du brouillard, dans l'ordre mémoire des pixels de sortie.
    color: u32,
    /// Le facteur par tranche de profondeur, **vide tant que rien n'est
    /// réglé**.
    ///
    /// Sa vacuité est l'état « éteint » : un `Option` autour d'une table déjà
    /// réservée aurait fait deux façons de dire la même chose, qui finissent
    /// par se contredire.
    table: Vec<u16>,
}

impl Fog {
    /// Un brouillard éteint, dont la table est déjà réservée.
    pub fn new() -> Result<Self> {
        let mut table = Vec::new();
        table
            .try_reserve_exact(TABLE_LEN)
            .map_err(|_| Error::OutOfMemory)?;
        Ok(Self { color: 0, table })
    }

    /// Vrai si le brouillard est réglé.
    pub fn is_set(&self) -> bool {
        !self.table.is_empty()
    }

    /// La couleur du brouillard.
    pub fn color(&self) -> u32 {
        self.color
    }

    /// Éteint le brouillard, sans rendre sa table.
    pub fn clear(&mut self) {
        self.table.clear();
        self.color = 0;
    }

    /// Règle le brouillard : sa couleur, et les distances de vue entre
    /// lesquelles il s'épaissit.
    ///
    /// **La table se remplit linéairement en distance**, et non en profondeur.
    /// Un brouillard linéaire en `near/w` serait gratuit — la profondeur est
    /// déjà là — mais il atteint cinquante-six pour cent de son épaisseur au
    /// dixième de sa rampe : il cesse alors d'être un indice de profondeur pour
    /// devenir un voile uniforme.
    ///
    /// `near` est le plan proche de la caméra, dont dépend la conversion d'une
    /// profondeur en distance : **la table est à refaire quand il change**.
    pub fn set(&mut self, color: u32, near: f32, start: f32, end: f32) -> Result<()> {
        let finite = start.is_finite() && end.is_finite();
        if !finite || start < 0.0 || end <= start {
            return Err(Error::InvalidArgument(Argument::Fog));
        }
        self.table.clear();
        for index in 0..TABLE_LEN {
            self.table
                .push(factor_at(depth_of(index), near, start, end));
        }
        self.color = color;
        Ok(())
    }

    /// Le facteur de brouillard à la profondeur `depth`.
    ///
    /// Éteint, il rend zéro : le mélange laisse alors le pixel intact, et
    /// l'appelant n'a pas de cas à distinguer.
    #[inline]
    pub fn factor(&self, depth: u32) -> u16 {
        match self.table.get(index_of(depth)) {
            Some(factor) => *factor,
            None => 0,
        }
    }
}

/// Mélange un pixel vers la couleur du brouillard.
///
/// `factor` va de zéro à [`FULL`], et `bias` est l'arrondi — cent vingt-huit
/// pour l'arrondi au plus proche, ou une valeur de tramage.
///
/// **Deux voies dans `0x00FF00FF`, sans retenue entre elles.** Chaque voie vaut
/// au plus `255 · 256 + 255`, soit 65 535 : elle tient dans ses seize bits, et
/// deux canaux ne peuvent pas déborder l'un sur l'autre. Aux deux bouts le
/// résultat est exact — `factor` nul rend le pixel tel quel, `factor` plein
/// rend la couleur du brouillard —, ce que huit bits de facteur n'auraient pas
/// permis.
#[inline]
pub fn blend(pixel: u32, fog: u32, factor: u16, bias: u32) -> u32 {
    let (f, g) = (u32::from(factor), u32::from(FULL - factor));
    let bias = bias * 0x0001_0001;
    let mix = |shift: u32| {
        let (p, q) = ((pixel >> shift) & 0x00FF_00FF, (fog >> shift) & 0x00FF_00FF);
        ((p * g + q * f + bias) >> 8) & 0x00FF_00FF
    };
    mix(0) | (mix(8) << 8)
}

/// L'arrondi du mélange, tramé, pour le pixel `(x, y)` de l'image.
///
/// **La matrice de Bayer du rasteriseur, transposée.** Le brouillard quantifie
/// à un deux-cent-cinquante-sixième, et un arrondi constant y dessine des
/// bandes concentriques autour de la caméra — le défaut le plus reconnaissable
/// d'un brouillard en couleurs directes. Un arrondi qui varie avec la position
/// les remplace par une frange d'un niveau.
///
/// Transposée, et non recopiée : le tramage des coordonnées de texture emploie
/// déjà la même matrice, et deux motifs identiques superposés se renforcent au
/// lieu de se disperser. La transposition les décorrèle sans table de plus.
///
/// **L'index se prend sur la position dans l'image**, jamais dans la tuile,
/// pour la raison qui vaut partout ici : le motif se décalerait d'une tuile à
/// l'autre.
#[inline]
pub fn dither(x: u32, y: u32) -> u32 {
    DITHER[((y & 3) * 4 + (x & 3)) as usize]
}

/// Les arrondis du tramage, de 8 à 248 par pas de 16.
///
/// Leur moyenne est exactement 128, l'arrondi au plus proche : le tramage ne
/// déplace donc pas la rampe, il en disperse la marche.
///
/// **Dérivée de la matrice du rasteriseur, et non recopiée.** Deux tables
/// écrites à la main auraient exigé un test pour les tenir accordées, et ce
/// test aurait fini par être le seul à savoir qu'elles devaient l'être.
const DITHER: [u32; 16] = transposed_ranks();

/// Construit les arrondis à la compilation, en transposant la matrice du
/// rasteriseur et en la portant à l'échelle d'un arrondi sur huit bits.
const fn transposed_ranks() -> [u32; 16] {
    let mut table = [0u32; 16];
    let mut i = 0;
    while i < 16 {
        // La transposition est ici : l'entrée `(x, y)` lit `(y, x)`.
        let (x, y) = (i % 4, i / 4);
        // Le rasteriseur porte `(2·M − 15) << 11` ; on en retire le rang.
        let rank = ((crate::raster::DITHER[x * 4 + y] >> 11) + 15) / 2;
        // Seize niveaux répartis sur 256, centrés sur 128.
        table[i] = rank as u32 * 16 + 8;
        i += 1;
    }
    table
}

/// L'index de table d'une profondeur : exposant, puis mantisse.
///
/// Une profondeur nulle est le fond — infiniment loin —, et prend la dernière
/// tranche. Elle se traite à part parce que décaler de trente-deux bits n'est
/// pas défini, et non par prudence.
#[inline]
fn index_of(depth: u32) -> usize {
    if depth == 0 {
        return TABLE_LEN - 1;
    }
    let exponent = depth.leading_zeros();
    // Le bit de poids fort est implicite une fois la valeur normalisée : on
    // garde les six suivants, et on le retranche.
    let bits = (depth << exponent) >> (31 - MANTISSA_BITS);
    let mantissa = bits as usize - (1 << MANTISSA_BITS);
    ((exponent as usize) << MANTISSA_BITS) | mantissa
}

/// Une profondeur représentative de la tranche `index`.
///
/// La réciproque de [`index_of`] au bit près de sa mantisse : c'est la borne
/// haute de la tranche, celle dont l'index rend exactement `index`.
fn depth_of(index: usize) -> u32 {
    if index == TABLE_LEN - 1 {
        return 0;
    }
    let exponent = (index >> MANTISSA_BITS) as u32;
    let mantissa = (index & ((1 << MANTISSA_BITS) - 1)) as u32;
    let normalised = ((1 << MANTISSA_BITS) | mantissa) << (31 - MANTISSA_BITS);
    normalised >> exponent
}

/// Le facteur à une profondeur donnée, sur une rampe linéaire en distance.
fn factor_at(depth: u32, near: f32, start: f32, end: f32) -> u16 {
    if depth == 0 {
        return FULL;
    }
    // `depth` vaut `near/w` en 0.32, donc `w = near · 2³² / depth`.
    let distance = near * 4_294_967_296.0 / depth as f32;
    if distance <= start {
        return 0;
    }
    if distance >= end {
        return FULL;
    }
    // `end > start` est vérifié au réglage : la rampe est strictement positive.
    let ratio = (distance - start) / (end - start);
    // Arrondi au plus proche, puis borné : un arrondi flottant ne doit pas
    // pouvoir dépasser le plein, que le mélange suppose.
    let scaled = (ratio * f32::from(FULL) + 0.5) as u32;
    scaled.min(u32::from(FULL)) as u16
}

#[cfg(test)]
mod tests;
