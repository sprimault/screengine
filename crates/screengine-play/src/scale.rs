// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La mise à l'échelle de l'image interne vers la fenêtre, au CPU.
//!
//! Le moteur rend en RGBA à basse résolution ; softbuffer attend des `u32`
//! `0x00RRGGBB` à la taille de la fenêtre. Les deux se font dans la même passe,
//! sans tampon intermédiaire à la taille de la fenêtre.

/// Comment l'image interne remplit la fenêtre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scale {
    /// Le plus grand facteur entier qui tient, image centrée, bandes noires
    /// autour. Chaque pixel devient un carré net, sans aucun mélange.
    ///
    /// **Sauf sous la résolution interne**, où aucun facteur entier ne tient :
    /// l'image se replie alors sur un ajustement à proportions gardées, avec le
    /// fondu d'un pixel de `Fill`. Rendre des bandes noires autour d'une image
    /// tronquée serait pire, et refuser d'afficher ferait d'une fenêtre réduite
    /// une erreur — ce qu'elle n'est pas, la taille changeant aussi avec le
    /// facteur d'échelle de l'écran.
    #[default]
    Integer,
    /// Toute la fenêtre à proportions gardées, par un facteur quelconque.
    ///
    /// Chaque pixel reste un aplat net ; seule la frontière entre deux pixels
    /// reçoit un fondu d'un pixel de large. Un bilinéaire sur toute l'image la
    /// rendrait floue, un plus proche voisin donnerait des pixels de largeurs
    /// inégales qui ondulent en mouvement.
    Fill,
    /// Un facteur entier imposé : la fenêtre s'ouvre à la résolution interne
    /// multipliée par lui.
    ///
    /// Un écran trop petit pour l'accueillir est une erreur au lancement. Une
    /// fenêtre réduite ensuite n'en est pas une : l'image se replie sur le plus
    /// grand facteur entier qui tient, puisque la taille change aussi avec le
    /// facteur d'échelle de l'écran ou l'aimantation d'une fenêtre.
    Fixed(u32),
}

/// Le mélange de deux pixels source pour un pixel de sortie, sur un axe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tap {
    /// Le premier pixel source.
    first: usize,
    /// Le second, égal au premier au bord de l'image.
    second: usize,
    /// La part du second, sur 256.
    weight: u32,
}

/// La disposition de l'image dans la fenêtre, recalculée à chaque
/// redimensionnement.
#[derive(Debug)]
enum Layout {
    /// Facteur entier : chaque pixel source devient un carré de `factor` de côté.
    Nearest { factor: usize },
    /// Facteur quelconque, fondu d'un pixel aux frontières.
    Sharp {
        columns: Vec<Tap>,
        rows: Vec<Tap>,
        /// Deux lignes de travail à la largeur de sortie, mélangées
        /// horizontalement avant le mélange vertical.
        upper: Vec<u32>,
        lower: Vec<u32>,
    },
}

/// Recopie l'image interne vers la surface de la fenêtre.
#[derive(Debug)]
pub(crate) struct Scaler {
    mode: Scale,
    source: (usize, usize),
    target: (usize, usize),
    /// Coin haut-gauche de l'image dans la fenêtre.
    origin: (usize, usize),
    /// Taille de l'image dans la fenêtre.
    extent: (usize, usize),
    layout: Layout,
}

/// Convertit un pixel RGBA du moteur au format de softbuffer.
fn to_xrgb(rgba: &[u8]) -> u32 {
    u32::from(rgba[0]) << 16 | u32::from(rgba[1]) << 8 | u32::from(rgba[2])
}

/// Mélange deux pixels `0x00RRGGBB`, `weight` sur 256 pour le second.
fn mix(a: u32, b: u32, weight: u32) -> u32 {
    let channel = |shift: u32| {
        let (ca, cb) = ((a >> shift) & 0xFF, (b >> shift) & 0xFF);
        ((ca * (256 - weight) + cb * weight) >> 8) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// Les mélanges d'un axe de `source` pixels agrandi à `output` pixels.
///
/// Chaque pixel de sortie se projette au centre dans la source. Sa distance au
/// centre du pixel source, multipliée par le facteur puis bornée, ne sort de
/// 0 ou 1 que sur le pixel de sortie où passe la frontière : c'est ce qui garde
/// les aplats nets.
fn taps(source: usize, output: usize) -> Vec<Tap> {
    let scale = output as f64 / source as f64;
    let last = source - 1;
    (0..output)
        .map(|x| {
            let u = (x as f64 + 0.5) / scale - 0.5;
            let floor = u.floor();
            let fraction = ((u - floor - 0.5) * scale + 0.5).clamp(0.0, 1.0);
            let first = (floor.max(0.0) as usize).min(last);
            let second = ((floor + 1.0).max(0.0) as usize).min(last);
            Tap {
                first,
                second,
                weight: (fraction * 256.0).round() as u32,
            }
        })
        .collect()
}

impl Scaler {
    /// Un recopieur pour une image interne de `width × height`, jamais nulle.
    pub(crate) fn new(mode: Scale, width: u32, height: u32) -> Self {
        let source = (width as usize, height as usize);
        Self {
            mode,
            source,
            target: source,
            origin: (0, 0),
            extent: source,
            layout: Layout::Nearest { factor: 1 },
        }
    }

    /// Recalcule la disposition pour une fenêtre de `width × height`, non nulle.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        let (sw, sh) = self.source;
        let (tw, th) = (width as usize, height as usize);
        self.target = (tw, th);

        let fit = (tw / sw).min(th / sh);
        let factor = match self.mode {
            Scale::Integer => fit,
            Scale::Fixed(imposed) => fit.min(imposed as usize),
            // Un remplissage qui tombe sur un facteur entier est un agrandissement
            // entier : les tables ne rendraient que des poids nuls.
            Scale::Fill if sw * fit == tw || sh * fit == th => fit,
            Scale::Fill => 0,
        };

        if factor > 0 {
            self.extent = (sw * factor, sh * factor);
            self.layout = Layout::Nearest { factor };
        } else {
            // Proportions gardées : l'axe le plus contraint remplit la fenêtre.
            // Aussi le repli d'une fenêtre plus petite que l'image elle-même,
            // où aucun facteur entier ne tient.
            self.extent = if tw * sh <= th * sw {
                (tw, (sh * tw / sw).max(1))
            } else {
                ((sw * th / sh).max(1), th)
            };
            self.layout = Layout::Sharp {
                columns: taps(sw, self.extent.0),
                rows: taps(sh, self.extent.1),
                upper: vec![0; self.extent.0],
                lower: vec![0; self.extent.0],
            };
        }
        self.origin = ((tw - self.extent.0) / 2, (th - self.extent.1) / 2);
    }

    /// Le pixel de l'image interne sous un point de la fenêtre, s'il y en a un.
    pub(crate) fn to_source(&self, x: f64, y: f64) -> Option<(u32, u32)> {
        let local = |p: f64, origin: usize, extent: usize, source: usize| {
            let offset = p - origin as f64;
            (offset >= 0.0 && offset < extent as f64)
                .then(|| ((offset * source as f64 / extent as f64) as usize).min(source - 1))
        };
        let sx = local(x, self.origin.0, self.extent.0, self.source.0)?;
        let sy = local(y, self.origin.1, self.extent.1, self.source.1)?;
        Some((sx as u32, sy as u32))
    }

    /// Écrit l'image RGBA `rgba` dans la surface `out`, bandes noires comprises.
    ///
    /// `rgba` fait `largeur × hauteur × 4` octets de la résolution interne,
    /// `out` la taille de la fenêtre passée au dernier [`resize`](Self::resize).
    pub(crate) fn blit(&mut self, rgba: &[u8], out: &mut [u32]) {
        let (sw, sh) = self.source;
        let tw = self.target.0;
        let (ox, oy) = self.origin;
        let ew = self.extent.0;

        out.fill(0);

        match &mut self.layout {
            Layout::Nearest { factor } => {
                let k = *factor;
                for (y, line) in rgba.chunks_exact(sw * 4).take(sh).enumerate() {
                    let start = (oy + y * k) * tw + ox;
                    let row = &mut out[start..start + ew];
                    for (pixel, block) in line.chunks_exact(4).zip(row.chunks_exact_mut(k)) {
                        block.fill(to_xrgb(pixel));
                    }
                    // La ligne agrandie une fois, recopiée ensuite : moins de
                    // conversions, et une recopie que la bibliothèque standard
                    // vectorise.
                    for copy in 1..k {
                        out.copy_within(start..start + ew, start + copy * tw);
                    }
                }
            }
            Layout::Sharp {
                columns,
                rows,
                upper,
                lower,
            } => {
                let spread = |source_row: usize, line: &mut [u32]| {
                    let base = source_row * sw * 4;
                    for (slot, tap) in line.iter_mut().zip(columns.iter()) {
                        let a = to_xrgb(&rgba[base + tap.first * 4..]);
                        let b = to_xrgb(&rgba[base + tap.second * 4..]);
                        *slot = mix(a, b, tap.weight);
                    }
                };
                for (y, tap) in rows.iter().enumerate() {
                    spread(tap.first, upper);
                    let start = (oy + y) * tw + ox;
                    let row = &mut out[start..start + ew];
                    if tap.weight == 0 {
                        row.copy_from_slice(upper);
                        continue;
                    }
                    spread(tap.second, lower);
                    for ((slot, a), b) in row.iter_mut().zip(upper.iter()).zip(lower.iter()) {
                        *slot = mix(*a, *b, tap.weight);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
