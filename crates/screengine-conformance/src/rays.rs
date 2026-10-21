// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le passage d'un pixel du curseur à une direction de monde, et son empreinte.
//!
//! **La seule scène qui remonte de l'image vers le monde.** Toutes les autres
//! descendent — des sommets vers des pixels, ou d'un segment vers un contact —,
//! et aucune n'éprouvait donc l'inverse de la projection. C'est le chemin
//! d'`scg_pixel_ray`, et un hôte qui en inverse deux arguments ou qui lit trois
//! flottants dans le mauvais ordre n'était rattrapé par rien.
//!
//! **Tout y est exactement représentable en binaire**, position, orientation,
//! ouverture et plan proche compris. C'est une contrainte de l'hôte web et non
//! une coquetterie : il écrit ses flottants depuis des nombres à double
//! précision, et une décimale quelconque y subirait deux arrondis successifs —
//! vers le double, puis vers le simple — là où les quatre autres hôtes n'en
//! subissent qu'un. Des fractions dyadiques ne s'arrondissent nulle part.
//!
//! **Le quaternion n'est pas unitaire, et c'est voulu** : la normalisation fait
//! partie de ce que la scène fige, et elle passe par la table du noyau. Un hôte
//! qui normaliserait lui-même avant d'appeler obtiendrait d'autres bits.
//!
//! **Les refus entrent dans l'empreinte** avec les succès : deux des pixels
//! débordent de la résolution, et ce que la scène fige alors est que les cinq
//! hôtes les refusent tous — pas seulement que le calcul concorde quand il a
//! lieu.

use screengine::{Camera, Quat, Vec3};

use crate::hash;

/// La largeur de l'image sur laquelle les pixels se lisent.
///
/// Autre que la résolution par défaut du moteur : un hôte qui oublierait de
/// régler la sienne rendrait des directions plausibles et une empreinte fausse.
pub const WIDTH: u32 = 320;

/// La hauteur de l'image.
pub const HEIGHT: u32 = 180;

/// La caméra de la scène, dont chaque composante est une fraction dyadique.
///
/// L'orientation combine un lacet et un roulis — deux composantes non nulles sur
/// trois axes après normalisation —, parce qu'une rotation alignée sur un axe
/// laisse passer trois erreurs de signe sur quatre : la base de vue permute déjà
/// les axes, et une caméra neutre rend cette permutation seule.
pub fn camera() -> Camera {
    Camera {
        position: Vec3::new(1.5, -2.25, 0.75),
        orientation: Quat::new(0.25, 0.0, 0.5, 1.0),
        fov_y: 1.0,
        near: 0.125,
    }
}

/// Les pixels que l'empreinte interroge, dans l'ordre contractuel.
///
/// Les quatre coins et le centre, trois points asymétriques dont aucune
/// coordonnée n'est un multiple de l'autre, puis **deux qui débordent** — un sur
/// chaque axe, au premier pixel refusé. L'ordre est contractuel comme celui des
/// balayages : le changer déplacerait la référence sans que rien n'ait bougé du
/// moteur.
pub const PIXELS: [(u32, u32); 10] = [
    (0, 0),
    (319, 0),
    (0, 179),
    (319, 179),
    (160, 90),
    (37, 151),
    (283, 22),
    (111, 64),
    (320, 179),
    (319, 180),
];

/// L'empreinte de la scène du rayon de pixel.
///
/// Elle ne compare pas deux chemins entre eux, à la différence des deux autres
/// scènes d'interrogation : il n'y a pas de force brute dont l'inverse de la
/// projection serait l'accélération. Ce qu'elle fige est l'accord des cinq hôtes
/// avec le chemin Rust, et c'est le seul contrôle qui l'obtienne.
pub fn digest() -> Result<u64, String> {
    let camera = camera();
    let mut bytes = Vec::new();
    for (x, y) in PIXELS {
        absorb(camera.pixel_ray(WIDTH, HEIGHT, x, y).ok(), &mut bytes);
    }
    Ok(hash::of(&bytes))
}

/// Absorbe une direction dans l'empreinte, son acceptation comprise.
///
/// **Des zéros écrits plutôt que le tampon relu** sur un refus : la frontière
/// n'écrit rien quand elle refuse — c'est ce que `sortie.rs` garde pour tout
/// point d'entrée —, donc un hôte qui hacherait son tampon hacherait ce qu'il y
/// avait avant, et son empreinte dépendrait de la mémoire qu'il a reçue.
fn absorb(ray: Option<Vec3>, bytes: &mut Vec<u8>) {
    bytes.push(u8::from(ray.is_none()));
    let ray = ray.unwrap_or(Vec3::ZERO);
    for value in [ray.x, ray.y, ray.z] {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
}

/// Le rapport texte, une direction par ligne.
pub fn report() -> Result<String, String> {
    let camera = camera();
    let mut text = String::from("# pixel : x, y | direction de monde | etat\n");
    for (index, (x, y)) in PIXELS.into_iter().enumerate() {
        match camera.pixel_ray(WIDTH, HEIGHT, x, y) {
            Ok(ray) => text.push_str(&format!(
                "{index:4} : {x:4}, {y:4} | ({:.6}, {:.6}, {:.6}) | -\n",
                ray.x, ray.y, ray.z
            )),
            Err(_) => text.push_str(&format!("{index:4} : {x:4}, {y:4} | - | refuse\n")),
        }
    }
    Ok(text)
}
