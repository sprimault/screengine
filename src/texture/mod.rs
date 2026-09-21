// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les textures et leur chaîne de mipmaps.
//!
//! Une texture est copiée à son chargement et n'est plus jamais modifiée. C'est
//! ce qui la rend partageable en lecture entre contextes et entre threads sans
//! verrou, et c'est surtout ce qui garde une image reproductible : un bloc
//! emprunté à l'hôte pourrait changer entre deux images, et le déterminisme ne
//! survivrait pas à un décodeur qui réutilise son tampon.
//!
//! **Toute la chaîne de mipmaps est engendrée ici**, dans l'appel de
//! chargement. Un niveau produit à la demande, au premier affichage, est le
//! défaut type de « zéro allocation par image » : il ne se manifeste qu'une
//! fois, sur une image quelconque, et rien ne le signale.

use alloc::vec::Vec;
use core::fmt;

use crate::buffer::reserved;
use crate::error::{Argument, Error, Result};

/// Le plus grand côté qu'une texture accepte, en texels.
///
/// La même borne que la résolution interne, pour n'avoir qu'un plafond à
/// retenir. À 2048 de côté, une texture et sa chaîne de mipmaps pèsent déjà
/// vingt-deux mégaoctets, soit davantage que tout ce qu'un contexte réserve.
pub const MAX_TEXTURE_SIZE: u32 = 2048;

/// Les niveaux qu'une chaîne peut compter, du plus grand côté jusqu'à 1×1.
const MAX_LEVELS: usize = MAX_TEXTURE_SIZE.trailing_zeros() as usize + 1;

/// Un niveau de la chaîne : où il commence, et ce qu'il mesure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Level {
    /// Indice de son premier texel dans le tampon commun.
    offset: u32,
    /// Largeur en texels, puissance de deux.
    width: u32,
    /// Hauteur en texels, puissance de deux.
    height: u32,
}

impl Level {
    /// La place de remplissage du tableau de niveaux.
    const EMPTY: Self = Self {
        offset: 0,
        width: 0,
        height: 0,
    };

    /// Le nombre de texels du niveau.
    fn len(&self) -> usize {
        (self.width * self.height) as usize
    }
}

/// Une texture chargée, avec tous ses niveaux de mipmap.
///
/// Les niveaux vivent dans une seule allocation, du plus grand au plus petit :
/// c'est ce qui permet de la dimensionner une fois, et ce qui garde les niveaux
/// voisins proches en mémoire, le rendu passant de l'un à l'autre d'un segment
/// de seize pixels au suivant.
pub struct Texture {
    /// Tous les niveaux bout à bout, chacun en lignes jointives, un texel par
    /// `u32` dans l'ordre mémoire des pixels de sortie.
    texels: Vec<u32>,
    levels: [Level; MAX_LEVELS],
    count: usize,
}

/// Les dimensions plutôt que les texels : un `Vec` de cinq millions d'entrées
/// n'a rien à faire dans un message de débogage, et c'est ce qu'un `derive`
/// imprimerait dès qu'un contexte porte une texture.
impl fmt::Debug for Texture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Texture")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("levels", &self.count)
            .finish()
    }
}

impl Texture {
    /// Charge une texture depuis un bloc de pixels, et engendre ses mipmaps.
    ///
    /// `pixels` porte `width × height` texels, lignes jointives, quatre octets
    /// chacun dans l'ordre R, G, B, A — celui des pixels de sortie, pour qu'un
    /// hôte n'ait jamais deux ordres à tenir. Le bloc est copié : l'hôte peut
    /// le libérer au retour.
    ///
    /// **Les deux côtés sont des puissances de deux**, indépendamment l'une de
    /// l'autre, entre 1 et [`MAX_TEXTURE_SIZE`]. Le repli des coordonnées se
    /// fait alors par masque, sans division ni comparaison par texel, et c'est
    /// ce qui rend le remplissage texturé abordable en logiciel.
    ///
    /// C'est l'un des appels nommés où l'allocation est permise, et le seul que
    /// la scène impose.
    pub fn load(width: u32, height: u32, pixels: &[u8]) -> Result<Self> {
        // `is_power_of_two` écarte zéro de lui-même : le minorant n'a pas à
        // être écrit une seconde fois.
        let valid = |side: u32| side.is_power_of_two() && side <= MAX_TEXTURE_SIZE;
        if !valid(width) || !valid(height) {
            return Err(Error::InvalidArgument(Argument::TextureSize));
        }
        // Le produit tient dans un `usize` de 32 bits : seize mégaoctets au
        // pire, borné par la validation qui précède.
        if pixels.len() != (width * height) as usize * 4 {
            return Err(Error::InvalidArgument(Argument::TextureLength));
        }

        let mut levels = [Level::EMPTY; MAX_LEVELS];
        let mut count = 0;
        let (mut w, mut h, mut offset) = (width, height, 0);
        loop {
            levels[count] = Level {
                offset,
                width: w,
                height: h,
            };
            count += 1;
            offset += w * h;
            if w == 1 && h == 1 {
                break;
            }
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }

        let mut texels = reserved(offset as usize)?;
        texels.resize(offset as usize, 0);

        for (texel, bytes) in texels[..levels[0].len()]
            .iter_mut()
            .zip(pixels.chunks_exact(4))
        {
            *texel = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }

        for i in 1..count {
            let (src, dst) = texels.split_at_mut(levels[i].offset as usize);
            reduce(
                &src[levels[i - 1].offset as usize..],
                levels[i - 1],
                &mut dst[..levels[i].len()],
                levels[i],
            );
        }

        Ok(Self {
            texels,
            levels,
            count,
        })
    }

    /// La largeur du niveau 0, en texels.
    pub fn width(&self) -> u32 {
        self.levels[0].width
    }

    /// La hauteur du niveau 0, en texels.
    pub fn height(&self) -> u32 {
        self.levels[0].height
    }

    /// Les niveaux de la chaîne, le plus petit étant toujours 1×1.
    pub fn level_count(&self) -> usize {
        self.count
    }

    /// Les dimensions d'un niveau, en texels.
    ///
    /// Un niveau au-delà de la chaîne rend le dernier, et c'est voulu : le
    /// choix du mipmap se fait par segment de seize pixels, sur une dérivée qui
    /// peut désigner un niveau plus fin que la chaîne n'en porte. Borner ici
    /// évite de le borner dans la boucle qui échantillonne.
    pub fn level_size(&self, level: usize) -> (u32, u32) {
        let level = self.levels[level.min(self.count - 1)];
        (level.width, level.height)
    }

    /// Le texel d'un niveau aux coordonnées `(u, v)`, repliées par masque.
    ///
    /// **Le repli est un `ET` binaire**, et c'est toute la raison d'exiger des
    /// côtés en puissance de deux : une coordonnée qui sort de la texture y
    /// rentre sans division ni comparaison, pour une surface qui répète son
    /// habillage des milliers de fois. Sur un entier signé, le masque replie
    /// aussi les négatifs du bon côté, là où un reste de division les
    /// renverrait de l'autre.
    ///
    /// Les coordonnées sont en texels entiers, le niveau borné comme dans
    /// [`Texture::level_size`].
    pub fn texel(&self, level: usize, u: i32, v: i32) -> u32 {
        let level = self.levels[level.min(self.count - 1)];
        let x = (u as u32) & (level.width - 1);
        let y = (v as u32) & (level.height - 1);
        self.texels[(level.offset + y * level.width + x) as usize]
    }

    /// Les texels d'un niveau, lignes jointives, même bornage que
    /// [`Texture::level_size`].
    pub fn level_texels(&self, level: usize) -> &[u32] {
        let level = self.levels[level.min(self.count - 1)];
        let start = level.offset as usize;
        &self.texels[start..start + level.len()]
    }
}

/// Réduit un niveau dans le suivant, par moyenne des texels qu'il recouvre.
///
/// **Par moyenne et jamais par échantillonnage** : au plus proche voisin, un
/// motif dont le détail descend sous deux pixels devient du bruit, c'est-à-dire
/// exactement le fourmillement qu'un mipmap existe pour supprimer. Constaté sur
/// une texture de test au pavage serré.
///
/// Une texture non carrée finit par avoir un côté à 1, que la division par deux
/// ne réduit plus : la moyenne porte alors sur deux texels et non quatre, d'où
/// le nombre de sources déduit des dimensions plutôt que fixé à quatre.
///
/// La moyenne se fait sur les valeurs telles qu'elles sont stockées, sans
/// repasser en linéaire. C'est le comportement de la classe de moteurs visée,
/// et le rendu est étalonné là-dessus ; linéariser ici éclaircirait chaque
/// niveau par rapport au précédent.
fn reduce(src: &[u32], src_level: Level, dst: &mut [u32], dst_level: Level) {
    let shift_x = u32::from(src_level.width > dst_level.width);
    let shift_y = u32::from(src_level.height > dst_level.height);
    let shift = shift_x + shift_y;
    let half = 1 << (shift - 1);

    for y in 0..dst_level.height {
        for x in 0..dst_level.width {
            let mut sum = [0u32; 4];
            for dy in 0..=shift_y {
                for dx in 0..=shift_x {
                    let sx = (x << shift_x) + dx;
                    let sy = (y << shift_y) + dy;
                    let texel = src[(sy * src_level.width + sx) as usize].to_le_bytes();
                    for (channel, byte) in sum.iter_mut().zip(texel) {
                        *channel += u32::from(byte);
                    }
                }
            }
            // Quatre octets sommés font au plus 1020 : le décalage après ajout
            // de la demi-unité rend toujours un octet, et la conversion ne
            // tronque rien.
            let mut bytes = [0u8; 4];
            for (byte, channel) in bytes.iter_mut().zip(sum) {
                *byte = ((channel + half) >> shift) as u8;
            }
            dst[(y * dst_level.width + x) as usize] = u32::from_le_bytes(bytes);
        }
    }
}

#[cfg(test)]
mod tests;
