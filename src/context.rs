// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Le contexte de rendu et sa configuration.

use crate::error::{Error, Result};

/// La plus grande résolution interne qu'un contexte accepte, en pixels de côté.
///
/// Ce n'est pas une limite de confort. Les formats en virgule fixe de
/// `docs/rust.md` calculent leurs pires cas sur cette borne : au-delà, une
/// fonction de bord déborderait avant que quoi que ce soit d'autre ne le
/// signale.
pub const MAX_RESOLUTION: u32 = 2048;

/// Les deux tailles de tuile admises, en pixels de côté.
///
/// Une tuile de 32 ou 64 tient en L1 avec sa profondeur. Au-delà, l'intérêt
/// principal des tuiles — le cache — disparaît.
pub const TILE_SIZES: [u32; 2] = [32, 64];

/// Quatre octets par pixel, R, G, B puis A en mémoire.
pub const BYTES_PER_PIXEL: usize = 4;

/// Ce que reçoit la création d'un contexte.
///
/// La résolution maximale dimensionne tous les tampons propres à l'image dès la
/// création. Changer de résolution sous ce maximum n'alloue donc rien, ce qui
/// est la seule façon de tenir « zéro allocation par image » quand l'hôte
/// ajuste sa résolution en cours de partie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Largeur maximale, en pixels. Entre 1 et [`MAX_RESOLUTION`].
    pub max_width: u32,
    /// Hauteur maximale, en pixels. Entre 1 et [`MAX_RESOLUTION`].
    pub max_height: u32,
    /// Largeur initiale, en pixels. Au plus `max_width`.
    pub width: u32,
    /// Hauteur initiale, en pixels. Au plus `max_height`.
    pub height: u32,
    /// Côté d'une tuile : 32 ou 64.
    pub tile_size: u32,
}

impl Config {
    /// Refuse une configuration que le moteur ne peut pas honorer.
    fn validate(&self) -> Result<()> {
        let bounded = |v: u32, max: u32| v >= 1 && v <= max;

        if !bounded(self.max_width, MAX_RESOLUTION) || !bounded(self.max_height, MAX_RESOLUTION) {
            return Err(Error::InvalidArgument);
        }
        if !bounded(self.width, self.max_width) || !bounded(self.height, self.max_height) {
            return Err(Error::InvalidArgument);
        }
        if !TILE_SIZES.contains(&self.tile_size) {
            return Err(Error::InvalidArgument);
        }
        Ok(())
    }
}

/// Un contexte de rendu.
///
/// Il porte la configuration, la résolution courante et — à partir du lot
/// suivant — les tampons dimensionnés pour la résolution maximale.
#[derive(Debug)]
pub struct Context {
    config: Config,
    width: u32,
    height: u32,
}

impl Context {
    /// Crée un contexte, ou refuse la configuration.
    ///
    /// C'est l'un des appels nommés où l'allocation est permise : tout ce que
    /// l'image consomme se réserve ici, pour la résolution maximale.
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            width: config.width,
            height: config.height,
        })
    }

    /// La configuration reçue à la création.
    pub fn config(&self) -> Config {
        self.config
    }

    /// La résolution interne courante, en pixels.
    pub fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Termine l'image et écrit le résultat dans le tampon de l'hôte.
    ///
    /// `stride` est en pixels et vaut au moins la largeur courante. Le tampon
    /// fait au moins `stride × hauteur` pixels de quatre octets ; la frontière C
    /// ne reçoit pas sa longueur et en fait une précondition, alors qu'un
    /// appelant Rust la porte avec la tranche — c'est le seul contrôle des deux
    /// qui distingue les deux chemins.
    pub fn frame_end(&mut self, pixels: &mut [u8], stride: u32) -> Result<()> {
        if stride < self.width {
            return Err(Error::InvalidArgument);
        }

        let needed = (stride as usize)
            .checked_mul(self.height as usize)
            .and_then(|p| p.checked_mul(BYTES_PER_PIXEL))
            .ok_or(Error::InvalidArgument)?;
        if pixels.len() < needed {
            return Err(Error::InvalidArgument);
        }

        todo!("étape 0 : remplissage du triangle en dur")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une configuration qui passe, dont les tests dérivent leurs variantes.
    fn sane() -> Config {
        Config {
            max_width: 640,
            max_height: 360,
            width: 640,
            height: 360,
            tile_size: 64,
        }
    }

    #[test]
    fn accepte_une_configuration_saine() {
        let ctx = Context::new(sane()).expect("configuration saine");
        assert_eq!(ctx.resolution(), (640, 360));
    }

    #[test]
    fn refuse_une_dimension_nulle() {
        let mut config = sane();
        config.width = 0;
        assert_eq!(Context::new(config).unwrap_err(), Error::InvalidArgument);
    }

    #[test]
    fn refuse_une_resolution_initiale_au_dela_du_maximum() {
        let mut config = sane();
        config.width = config.max_width + 1;
        assert_eq!(Context::new(config).unwrap_err(), Error::InvalidArgument);
    }

    #[test]
    fn refuse_un_maximum_au_dela_de_la_borne_des_formats() {
        let mut config = sane();
        config.max_width = MAX_RESOLUTION + 1;
        config.width = MAX_RESOLUTION + 1;
        assert_eq!(Context::new(config).unwrap_err(), Error::InvalidArgument);
    }

    #[test]
    fn refuse_une_taille_de_tuile_hors_liste() {
        let mut config = sane();
        config.tile_size = 48;
        assert_eq!(Context::new(config).unwrap_err(), Error::InvalidArgument);
    }

    #[test]
    fn refuse_un_stride_plus_court_que_la_largeur() {
        let mut ctx = Context::new(sane()).expect("configuration saine");
        let mut pixels = [0u8; 4];
        assert_eq!(
            ctx.frame_end(&mut pixels, 639).unwrap_err(),
            Error::InvalidArgument
        );
    }

    #[test]
    fn refuse_un_tampon_trop_court_pour_son_stride() {
        let mut ctx = Context::new(sane()).expect("configuration saine");
        let mut pixels = [0u8; 4];
        assert_eq!(
            ctx.frame_end(&mut pixels, 640).unwrap_err(),
            Error::InvalidArgument
        );
    }
}
