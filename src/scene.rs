// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce qu'une image reçoit : une caméra, des sommets, des triangles colorés.
//!
//! Rien du jeu n'entre ici. Ce sont les seules données que le moteur connaisse
//! d'une scène, et chacune a son équivalent exact dans l'ABI C : ce que l'API
//! Rust permet de décrire, une liaison le décrit aussi.

use crate::math::{Affine3, Quat, Vec3};

/// L'orientation des axes de vue que donne le quaternion identité.
///
/// Le monde est en main droite, Z en haut ; le repère de vue est X à droite, Y
/// vers le bas, Z vers l'avant. Les deux ne coïncident donc jamais, et il faut
/// dire lequel des deux l'identité aligne : ici, ses colonnes sont les images
/// des axes de vue dans le monde.
///
/// ```text
/// X_vue (droite) ↦ −Y monde
/// Y_vue (bas)    ↦ −Z monde
/// Z_vue (avant)  ↦ +X monde
/// ```
///
/// Une caméra d'orientation neutre regarde donc le +X du monde, le haut de
/// l'écran vers le zénith. Écarté : aligner naïvement les axes de vue sur ceux
/// du monde, qui ferait regarder le zénith par défaut et poserait le nord au
/// bas de l'écran. Le déterminant vaut un, donc la composition reste une
/// transformation rigide et [`Affine3::inverse_rigid`] garde sa précondition.
///
/// Une phrase — « la caméra regarde le +X » — ne suffirait pas : elle laisse le
/// roulis indéterminé. Les trois colonnes le fixent.
const VIEW_BASIS: Affine3 = Affine3 {
    m: [
        0.0, -1.0, 0.0, // X de vue
        0.0, 0.0, -1.0, // Y de vue
        1.0, 0.0, 0.0, // Z de vue
        0.0, 0.0, 0.0,
    ],
};

/// D'où l'on regarde, et avec quelle ouverture.
///
/// Position et orientation plutôt qu'une matrice de vue : une matrice laisserait
/// l'hôte composer lui-même l'inverse de la pose, donc normaliser un quaternion,
/// donc appeler sa libm — et les empreintes cesseraient d'être comparables d'une
/// liaison à l'autre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Sa position dans le monde.
    pub position: Vec3,
    /// Son orientation, normalisée à la réception. Voir [`Camera::view`] pour
    /// ce que l'identité désigne.
    pub orientation: Quat,
    /// Le champ de vision vertical, en radians, dans `]0, π[`.
    pub fov_y: f32,
    /// Le plan proche, strictement positif.
    pub near: f32,
}

impl Camera {
    /// Le champ de vision vertical par défaut, soixante degrés.
    pub const DEFAULT_FOV_Y: f32 = core::f32::consts::FRAC_PI_3;

    /// Le plan proche par défaut, en unités de monde.
    ///
    /// Assez près pour qu'un mur frôlé ne disparaisse pas, assez loin pour que
    /// la profondeur `near/w` garde de la résolution au fond d'un couloir.
    pub const DEFAULT_NEAR: f32 = 0.1;

    /// À l'origine, regardant le +X du monde.
    pub const DEFAULT: Self = Self {
        position: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        fov_y: Self::DEFAULT_FOV_Y,
        near: Self::DEFAULT_NEAR,
    };

    /// La transformation du monde vers l'espace de vue.
    ///
    /// La pose de la caméra dans le monde, composée avec [`VIEW_BASIS`], puis
    /// inversée. La composition est exacte au bit près — chaque coefficient du
    /// résultat est un coefficient de la rotation, au signe près, les autres
    /// termes étant des produits par zéro —, si bien qu'une caméra neutre rend
    /// une permutation d'axes et rien de plus.
    pub fn view(&self) -> Affine3 {
        let pose = Affine3::from_rotation_translation(self.orientation.normalize(), self.position);
        pose.product(VIEW_BASIS).inverse_rigid()
    }
}

/// Une couleur, quatre octets dans l'ordre où l'ABI les écrit en mémoire.
///
/// Quatre champs nommés et non un `u32` : l'entier que le rasteriseur manipule
/// s'écrit `0xAABBGGRR`, parce que c'est ce que sa conversion en octets
/// petit-boutistes impose. Un littéral pris pour du `0xAARRGGBB` échange le
/// rouge et le bleu sans que rien ne le signale — le défaut que la disposition
/// des pixels d'Android produit déjà chez les hôtes qui s'y trompent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    /// Rouge.
    pub r: u8,
    /// Vert.
    pub g: u8,
    /// Bleu.
    pub b: u8,
    /// Alpha. Le moteur l'écrit tel quel et n'en compose rien.
    pub a: u8,
}

impl Color {
    /// Une couleur de composantes données.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// La forme qu'en garde le rasteriseur.
    pub(crate) const fn packed(self) -> u32 {
        u32::from_le_bytes([self.r, self.g, self.b, self.a])
    }
}

/// Un triangle soumis : trois indices dans le tableau de sommets, et sa couleur.
///
/// La couleur est portée par le triangle et non par le lot : une surface entière
/// se soumet alors en un seul franchissement de la frontière, ce qui est tout
/// l'objet de la soumission par lot. Les indices ne sont pas facultatifs — un
/// hôte sans maillage indexé écrit `0, 1, 2` puis `3, 4, 5`, et le moteur n'a
/// pas deux chemins à tenir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triangle {
    /// Les trois sommets, en sens antihoraire vus de la face avant.
    pub indices: [u32; 3],
    /// Sa couleur, uniforme jusqu'à ce que les textures existent.
    pub color: Color,
}

#[cfg(test)]
mod tests;
