// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La rotation que le moteur construit, et son empreinte.
//!
//! **La seule scène qui ne touche ni au monde ni au tampon.** Elle ne rend
//! aucune image et ne charge aucun fichier : elle compose des poses et hache ce
//! qu'elles donnent. C'est le chemin des quatre fonctions de rotation, qu'aucune
//! autre scène n'emprunte — un hôte les appelait jusqu'ici avec la
//! trigonométrie de son langage, donc sans obtenir les mêmes bits qu'un autre.
//!
//! **L'angle est l'entier que l'ABI publie**, et c'est ce que la scène fige en
//! premier : une liaison qui convertirait des radians au passage obtiendrait
//! d'autres bits, et c'est précisément ce que l'angle binaire existe pour
//! empêcher. Les angles choisis ne sont donc ni ronds en degrés ni ronds en
//! radians — ils sont ronds en unités de tour.
//!
//! **Les axes ne sont pas unitaires**, et deux d'entre eux ne sont pas alignés
//! sur un axe du monde : la normalisation fait partie de ce que la scène fige,
//! et un axe aligné laisserait passer trois erreurs de signe sur quatre. Le
//! troisième est nul, parce que l'identité qu'il doit rendre est une clause du
//! contrat et non un hasard.
//!
//! **Tout y est exactement représentable en binaire** — fractions dyadiques —,
//! pour la raison de la scène des rayons : l'hôte web écrit ses flottants depuis
//! des nombres à double précision, et une décimale quelconque y subirait deux
//! arrondis là où les autres n'en subissent qu'un.

use screengine::{Affine3, Angle, Quat, Vec3};

use crate::hash;

/// Les axes de rotation, dont aucun n'est unitaire.
///
/// Le dernier est nul : le contrat promet l'identité plutôt qu'un refus, et
/// c'est une promesse qu'un hôte ne peut pas vérifier autrement qu'en la
/// hachant avec les autres.
pub const AXES: [Vec3; 3] = [
    Vec3::new(0.0, 0.0, 2.0),
    Vec3::new(0.5, -1.5, 0.25),
    Vec3::ZERO,
];

/// Les angles, en unités de tour sur 2³².
///
/// Un quart de tour, un huitième, et une valeur qui ne tombe sur aucune
/// fraction simple — la table de sinus du noyau interpole entre ses entrées, et
/// trois angles tabulés ne diraient rien de l'interpolation.
pub const ANGLES: [u32; 3] = [1 << 30, 1 << 29, 0x1234_5678];

/// La translation composée avec chaque rotation.
pub const TRANSLATION: Vec3 = Vec3::new(1.5, -2.25, 0.75);

/// L'empreinte de la scène : les quaternions, leurs produits, et les matrices
/// qui en sortent.
///
/// Chaque étage entre dans l'empreinte, et non le seul résultat final : un hôte
/// qui se tromperait d'ordre dans le produit rendrait une empreinte fausse, mais
/// une empreinte qui ne hacherait que la fin ne dirait pas lequel des quatre
/// appels a dévié.
pub fn digest() -> Result<u64, String> {
    let mut bytes = Vec::new();

    let mut rotations = Vec::new();
    for (axis, angle) in AXES.iter().zip(ANGLES) {
        let q = Quat::from_axis_angle(*axis, Angle(angle));
        absorb_quat(q, &mut bytes);
        rotations.push(q);
    }

    // Les produits deux à deux, dans l'ordre que le contrat annonce : `a` puis
    // `b`. Pris dans les deux sens, parce que le produit n'est pas commutatif et
    // qu'un hôte qui inverserait ses arguments passerait sans cela.
    for a in &rotations {
        for b in &rotations {
            absorb_quat(product(*a, *b), &mut bytes);
        }
    }

    let mut matrices = Vec::new();
    for q in &rotations {
        let m = Affine3::from_rotation_translation(*q, TRANSLATION);
        absorb_mat(m, &mut bytes);
        matrices.push(m);
    }

    for a in &matrices {
        for b in &matrices {
            absorb_mat(b.product(*a), &mut bytes);
        }
    }

    Ok(hash::of(&bytes))
}

/// Le rapport texte : ce que l'empreinte hache, lisible.
///
/// Les quaternions seuls et les matrices seules, sans les produits : un écart
/// se cherche d'abord sur l'étage qui l'a produit, et les neuf produits de
/// chaque famille le noieraient.
pub fn report() -> Result<String, String> {
    let mut text = String::from("# axe | angle | quaternion\n");
    for (axis, angle) in AXES.iter().zip(ANGLES) {
        let q = Quat::from_axis_angle(*axis, Angle(angle));
        text.push_str(&format!(
            "({:.4}, {:.4}, {:.4}) | {angle:#010x} | ({:.6}, {:.6}, {:.6}, {:.6})\n",
            axis.x, axis.y, axis.z, q.x, q.y, q.z, q.w
        ));
    }

    text.push_str("\n# matrice, par colonnes, translation comprise\n");
    for (axis, angle) in AXES.iter().zip(ANGLES) {
        let q = Quat::from_axis_angle(*axis, Angle(angle));
        let m = Affine3::from_rotation_translation(q, TRANSLATION);
        text.push_str(&format!("{angle:#010x} |"));
        for value in m.m {
            text.push_str(&format!(" {value:.6}"));
        }
        text.push('\n');
    }
    Ok(text)
}

/// Le produit tel que l'ABI le lit : `a` d'abord, puis `b`.
///
/// L'ordre de lecture n'est pas celui de l'algèbre, et c'est une clause du
/// contrat : une fonction par-dessus la méthode du noyau, plutôt que l'ordre
/// inversé recopié à chaque site, parce que c'est lui qu'un hôte reproduit.
fn product(a: Quat, b: Quat) -> Quat {
    b.product(a)
}

/// Absorbe un quaternion dans l'empreinte, composante par composante.
fn absorb_quat(q: Quat, bytes: &mut Vec<u8>) {
    for value in [q.x, q.y, q.z, q.w] {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
}

/// Absorbe une matrice dans l'empreinte, **sa dernière ligne comprise**.
///
/// Les seize coefficients et non les douze du noyau : c'est la 4×4 que l'hôte
/// relit, et une dernière ligne mal écrite est exactement ce qu'une soumission
/// refuserait plus tard, loin de sa cause.
fn absorb_mat(m: Affine3, bytes: &mut Vec<u8>) {
    let c = &m.m;
    let full = [
        c[0], c[1], c[2], 0.0, //
        c[3], c[4], c[5], 0.0, //
        c[6], c[7], c[8], 0.0, //
        c[9], c[10], c[11], 1.0,
    ];
    for value in full {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
}

#[cfg(test)]
mod tests;
