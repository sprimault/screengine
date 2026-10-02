// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! L'image finie, confiée à l'hôte avant qu'elle parte à la fenêtre.

use screengine::BYTES_PER_PIXEL;

/// L'image finie, avant qu'elle parte à la fenêtre.
///
/// **Ce que l'ABI C donne depuis toujours, et que cet étage ne donnait pas.** Là
/// le tampon appartient à l'appelant, qui y écrit après `scg_frame_end` ; ici la
/// boucle le possédait et le recopiait aussitôt dans la fenêtre. Une jauge, un
/// score, une arme vue en main vivent donc ici, et nulle part ailleurs : les
/// faire passer par le moteur serait la première marche du code de jeu dans le
/// rendu, et le tracé de lignes ne les sert pas — il prend des coordonnées de
/// monde, pas d'écran.
pub struct Output<'a> {
    pixels: &'a mut [u8],
    width: u32,
    height: u32,
    stride: u32,
}

impl<'a> Output<'a> {
    /// Ce que la boucle confie au rappel, le temps de l'appel.
    ///
    /// Le tampon reçu est dimensionné sur le plafond de résolution, donc plus
    /// grand que l'image courante : c'est cette structure qui en borne l'accès.
    ///
    /// **Public pour qu'un appelant puisse éprouver son propre dessin** sans
    /// ouvrir de fenêtre : en passant par la boucle, le seul chemin vers un
    /// `Output` exigeait un écran, et ce qu'on écrit par-dessus l'image ne se
    /// vérifiait qu'en le regardant.
    ///
    /// `pixels` doit couvrir `stride × height × 4` octets — c'est une
    /// précondition et non un contrôle, de la même forme que celle que la
    /// frontière C tient sur son tampon de sortie. En deçà, [`pixel`](Self::pixel)
    /// sort du tampon et panique : il borne sur la zone utile, qui est ce qu'un
    /// dessin doit respecter, pas sur la longueur reçue.
    pub fn new(pixels: &'a mut [u8], width: u32, height: u32, stride: u32) -> Self {
        Self {
            pixels,
            width,
            height,
            stride,
        }
    }

    /// Les octets de l'image, quatre par pixel, dans l'ordre R, G, B, A.
    ///
    /// **Une ligne avance de `stride` pixels et non de `width`** : un indice
    /// calculé sur la largeur décale l'image d'une ligne à l'autre dès que les
    /// deux diffèrent. La tranche rendue couvre le tampon entier, plafond
    /// compris — ce qui dépasse l'image n'est pas recopié dans la fenêtre.
    pub fn pixels(&mut self) -> &mut [u8] {
        self.pixels
    }

    /// La largeur de la zone utile, en pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Sa hauteur.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Le pas d'une ligne, en pixels.
    ///
    /// **Égal à la largeur aujourd'hui** : cet étage rend sans marge, là où un
    /// hôte C choisit son pas. S'en servir plutôt que de la largeur ne coûte rien
    /// et garde le dessin juste si cela change — c'est aussi ce qui rend un tracé
    /// d'interface transposable d'un chemin à l'autre.
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// Les quatre octets d'un pixel, ou `None` hors de la zone utile.
    ///
    /// La seule aide de cette structure, et elle existe pour la raison
    /// ci-dessus : le pas de ligne est ce qu'un hôte confond avec la largeur, et
    /// les deux hôtes C du dépôt entourent leur tampon de sentinelles pour
    /// attraper ce défaut-là.
    pub fn pixel(&mut self, x: u32, y: u32) -> Option<&mut [u8]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        // La borne porte sur la résolution courante et non sur la taille du
        // tampon : écrire entre les deux ne planterait pas et ne se verrait pas,
        // la recopie ne prenant que l'image.
        let base = (y as usize * self.stride as usize + x as usize) * BYTES_PER_PIXEL;
        Some(&mut self.pixels[base..base + BYTES_PER_PIXEL])
    }
}

#[cfg(test)]
mod tests;
