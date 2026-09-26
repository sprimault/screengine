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
use crate::math::fixed::UV_BITS;

/// Comment une texture s'échantillonne.
///
/// Les deux modes **s'excluent** : le tramage existe pour masquer l'escalier
/// que laisse la troncature d'une coordonnée, et le bilinéaire ne tronque
/// rien. Cumulés, le premier n'ajouterait que du bruit au second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Filter {
    /// Un texel par pixel, coordonnées décalées par la table de tramage.
    ///
    /// Le défaut, et le filtrage de la classe visée : une indirection de table
    /// et une addition, là où le bilinéaire lit quatre texels et en mélange
    /// trois paires.
    #[default]
    Dither,
    /// Les quatre texels voisins, mélangés selon les bits fractionnaires.
    ///
    /// **Dans un seul niveau de mipmap**, jamais entre deux : le trilinéaire
    /// double les lectures pour un gain qui ne se voit qu'en mouvement lent, et
    /// il appartient au matériel dédié, pas aux rasteriseurs logiciels de cette
    /// génération.
    Bilinear,
}

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
    /// Vrai si ses texels portent une transparence binaire.
    ///
    /// Porté par la texture et non par la soumission : c'est au chargement que
    /// la chaîne de mipmaps se construit, et une transparence décidée au dessin
    /// obligerait à tenir deux chaînes, ou à en tenir une fausse.
    masked: bool,
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
        Self::load_format(width, height, pixels, false)
    }

    /// Charge une texture à transparence binaire, et engendre ses mipmaps.
    ///
    /// Mêmes octets que [`Texture::load`], mais l'alpha de chaque texel y
    /// décide : ramené à tout ou rien au chargement, seuil à 128. Un texel
    /// transparent n'est ni peint ni inscrit dans la profondeur.
    ///
    /// Deux façades plutôt qu'un paramètre de format : le masquage est une
    /// variante et non un axe, et `load_masked` se lit à l'appel là où un
    /// argument de format se répéterait sans rien dire sur la trentaine de
    /// chargements opaques du dépôt.
    pub fn load_masked(width: u32, height: u32, pixels: &[u8]) -> Result<Self> {
        Self::load_format(width, height, pixels, true)
    }

    /// Le corps commun aux deux chargements.
    fn load_format(width: u32, height: u32, pixels: &[u8], masked: bool) -> Result<Self> {
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

        if masked {
            threshold(&mut texels[..levels[0].len()]);
            dilate(&mut texels[..levels[0].len()], levels[0]);
        }

        for i in 1..count {
            let (src, dst) = texels.split_at_mut(levels[i].offset as usize);
            reduce(
                &src[levels[i - 1].offset as usize..],
                levels[i - 1],
                &mut dst[..levels[i].len()],
                levels[i],
                masked,
            );
        }

        Ok(Self {
            texels,
            levels,
            count,
            masked,
        })
    }

    /// Vrai si un texel transparent ne doit pas être peint.
    pub fn masked(&self) -> bool {
        self.masked
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

    /// Les quatre texels voisins d'un point, mélangés selon ses bits
    /// fractionnaires.
    ///
    /// `u` et `v` sont en 16.16, dans le niveau demandé. **Un demi-texel se
    /// retranche d'abord** : le centre du texel `(0, 0)` est en `(0,5, 0,5)`,
    /// et sans ce recentrage l'image glisserait d'un demi-texel vers le haut et
    /// la gauche par rapport à ce que rend [`Texture::texel`] — un décalage
    /// qu'on ne voit pas sur une image fixe, mais qui fait sauter la surface au
    /// changement de filtre.
    ///
    /// Le repli reste celui du masque, appliqué à chacun des quatre voisins
    /// séparément : le point qui tombe sur le dernier texel d'une ligne mélange
    /// avec le premier, ce qui est exactement ce qu'une surface pavée demande.
    pub fn bilinear(&self, level: usize, u: i32, v: i32) -> u32 {
        let (u, v) = (u - HALF_TEXEL, v - HALF_TEXEL);
        let (x, y) = (u >> UV_BITS, v >> UV_BITS);
        // Le décalage est arithmétique : un point négatif descend vers le texel
        // inférieur comme les autres, là où une division tronquerait vers zéro
        // et doublerait un texel de part et d'autre de l'origine.
        let weight = |c: i32| ((c >> (UV_BITS - WEIGHT_BITS)) as u32) & 0xFF;
        let (wu, wv) = (weight(u), weight(v));
        let row = |y: i32| mix(self.texel(level, x, y), self.texel(level, x + 1, y), wu);
        mix(row(y), row(y + 1), wv)
    }

    /// Les texels d'un niveau, lignes jointives, même bornage que
    /// [`Texture::level_size`].
    pub fn level_texels(&self, level: usize) -> &[u32] {
        let level = self.levels[level.min(self.count - 1)];
        let start = level.offset as usize;
        &self.texels[start..start + level.len()]
    }
}

/// Bits du poids d'un mélange bilinéaire, et le demi-texel du recentrage.
///
/// Huit bits : de quoi mélanger deux octets sans perdre de marche, et le
/// produit de deux canaux empaquetés tient encore dans un `u32`.
const WEIGHT_BITS: u32 = 8;
const HALF_TEXEL: i32 = 1 << (UV_BITS - 1);

/// Mélange deux texels, `t` sur huit bits pour le second.
///
/// **Deux multiplications par mélange et non quatre** : R et B tiennent
/// ensemble dans `0x00FF00FF`, G et A dans le même masque une fois décalés, et
/// chaque produit en traite deux à la fois. Les canaux ne se marchent pas
/// dessus — un octet multiplié par 256 en occupe seize, et il y en a seize
/// entre eux.
///
/// L'arrondi est celui que `docs/rust.md` fixe, `(… + 128) >> 8`, posé sur les
/// deux canaux du mot en une addition.
fn mix(a: u32, b: u32, t: u32) -> u32 {
    const MASK: u32 = 0x00FF_00FF;
    const ROUND: u32 = 0x0080_0080;
    let blend = |a: u32, b: u32| ((a * (256 - t) + b * t + ROUND) >> WEIGHT_BITS) & MASK;
    blend(a & MASK, b & MASK) | (blend((a >> 8) & MASK, (b >> 8) & MASK) << 8)
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
/// Au-delà, un texel est opaque ; en deçà, il ne s'écrit pas.
///
/// La valeur vit ici et nulle part ailleurs : elle sert au chargement, sur le
/// niveau zéro, et au dessin, sur la couverture d'un niveau réduit.
pub const ALPHA_THRESHOLD: u32 = 128;

/// Ramène l'alpha de chaque texel à tout ou rien.
///
/// Sur le niveau zéro seulement : les niveaux suivants portent une
/// **couverture**, et la reseuiller à chaque réduction calculerait un niveau
/// depuis un niveau déjà seuillé. Un détail fin — une grille, une antenne —
/// disparaîtrait alors d'un coup dès que sa couverture passe sous la moitié, et
/// reviendrait en approchant : c'est le clignotement qu'on cherche à éviter.
fn threshold(texels: &mut [u32]) {
    for texel in texels {
        *texel = if (*texel >> 24) >= ALPHA_THRESHOLD {
            *texel | 0xFF00_0000
        } else {
            *texel & 0x00FF_FFFF
        };
    }
}

/// Étend la couleur des texels opaques d'un cran dans les zones transparentes.
///
/// Sans elle, le bilinéaire mêle au bord d'une silhouette la couleur que l'hôte
/// a laissée sous ses texels invisibles — souvent du noir — et la cerne d'un
/// liseré. Un cran suffit au niveau zéro, qui est le seul que le bilinéaire lit
/// texel par texel ; au-delà, c'est la réduction pondérée qui porte des valeurs
/// plausibles de niveau en niveau.
///
/// **Aucun double tampon, et l'ordre de parcours n'a pourtant aucune
/// influence** : on ne lit que des texels opaques et on n'écrit que dans des
/// transparents, et l'alpha ne bouge pas. Aucune écriture ne peut donc devenir
/// la source d'une lecture ultérieure.
fn dilate(texels: &mut [u32], level: Level) {
    let (mask_x, mask_y) = (level.width - 1, level.height - 1);
    // Les décalages d'un côté de moins de quatre texels se recouvrent une fois
    // repliés — `−1` et `+1` désignent le même voisin sur deux —, et un voisin
    // compté deux fois pèserait double dans la moyenne.
    let steps = |mask: u32| -> &'static [u32] {
        match mask {
            0 => &[0],
            1 => &[0, 1],
            _ => &[u32::MAX, 0, 1],
        }
    };
    for y in 0..level.height {
        for x in 0..level.width {
            let here = (y * level.width + x) as usize;
            if texels[here] >> 24 != 0 {
                continue;
            }
            let mut sum = [0u32; 3];
            let mut count = 0;
            for &dy in steps(mask_y) {
                for &dx in steps(mask_x) {
                    // `wrapping_add` parce que le pas vers la gauche est
                    // `u32::MAX` : le repli par masque le ramène au bon texel,
                    // la somme seule déborde.
                    let sx = x.wrapping_add(dx) & mask_x;
                    let sy = y.wrapping_add(dy) & mask_y;
                    let neighbour = texels[(sy * level.width + sx) as usize];
                    if neighbour >> 24 == 0 {
                        continue;
                    }
                    let bytes = neighbour.to_le_bytes();
                    for (channel, byte) in sum.iter_mut().zip(bytes) {
                        *channel += u32::from(byte);
                    }
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let mut bytes = [0u8; 4];
            for (byte, channel) in bytes.iter_mut().zip(sum) {
                *byte = ((channel + count / 2) / count) as u8;
            }
            texels[here] = u32::from_le_bytes(bytes);
        }
    }
}

fn reduce(src: &[u32], src_level: Level, dst: &mut [u32], dst_level: Level, masked: bool) {
    let shift_x = u32::from(src_level.width > dst_level.width);
    let shift_y = u32::from(src_level.height > dst_level.height);
    let shift = shift_x + shift_y;
    let half = 1 << (shift - 1);

    for y in 0..dst_level.height {
        for x in 0..dst_level.width {
            let mut sum = [0u32; 4];
            // La somme des alphas des sources : elle sert de poids aux trois
            // autres canaux quand la texture est masquée, et elle n'est lue
            // que dans ce cas.
            let mut weighted = [0u32; 3];
            for dy in 0..=shift_y {
                for dx in 0..=shift_x {
                    let sx = (x << shift_x) + dx;
                    let sy = (y << shift_y) + dy;
                    let texel = src[(sy * src_level.width + sx) as usize].to_le_bytes();
                    for (channel, byte) in sum.iter_mut().zip(texel) {
                        *channel += u32::from(byte);
                    }
                    for (channel, byte) in weighted.iter_mut().zip(texel) {
                        *channel += u32::from(byte) * u32::from(texel[3]);
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
            // **Le RGB d'une texture masquée est pondéré par l'alpha**, sans
            // quoi un texel transparent teinte ses voisins de la couleur qu'il
            // porte sous son invisibilité. L'alpha, lui, reste la moyenne
            // ordinaire : c'est la couverture du texel réduit.
            //
            // Quand les quatre sources sont transparentes il n'y a rien à
            // pondérer, et la moyenne ordinaire tient lieu de valeur plausible
            // — elle vient de texels déjà dilatés, et cette valeur ne sera lue
            // que par le bilinéaire du niveau suivant.
            if masked && sum[3] != 0 {
                for (byte, channel) in bytes.iter_mut().zip(weighted) {
                    *byte = ((channel + sum[3] / 2) / sum[3]) as u8;
                }
            }
            dst[(y * dst_level.width + x) as usize] = u32::from_le_bytes(bytes);
        }
    }
}

#[cfg(test)]
mod tests;
