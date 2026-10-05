// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le plus petit programme qui affiche le moteur : une fenêtre, un triangle,
//! Échap pour fermer. C'est celui que montrent les README, mot pour mot.
//!
//! Il reste petit délibérément. Ce qu'une caméra, un couloir et des entrées
//! demandent est dans `couloir.rs` ; ici, on montre qu'une fenêtre qui rend
//! quelque chose tient en vingt lignes.

use screengine_play::{Affine3, Color, KeyCode, Play, Triangle, Vec3};

/// Le triangle, à quatre unités devant la caméra par défaut.
///
/// **L'ordre des trois sommets décide de tout**, et il se lit dans le repère de
/// vue : la caméra neutre regarde le +X du monde, son axe droit est le −Y et son
/// haut le +Z. Le sommet du milieu est donc celui du haut, et les deux autres
/// vont de la gauche vers la droite de l'écran.
const VERTICES: [Vec3; 3] = [
    Vec3::new(4.0, 1.5, -1.0),
    Vec3::new(4.0, -1.5, -1.0),
    Vec3::new(4.0, 0.0, 1.5),
];

/// Sa face avant regarde la caméra ; l'autre sens en ferait un dos, éliminé —
/// et une fenêtre noire, ce que cet exemple a rendu jusqu'ici.
const TRIANGLES: [Triangle; 1] = [Triangle {
    indices: [0, 1, 2],
    color: Color::new(0xE0, 0xA0, 0x30, 0xFF),
}];

/// Ouvre la fenêtre ; Échap ferme.
fn main() -> Result<(), screengine_play::Error> {
    Play::new().run(
        (),
        |_, tick| {
            if tick.input().pressed(KeyCode::Escape) {
                tick.exit();
            }
        },
        |_, context| {
            // La scène se resoumet à chaque image : la fin de la précédente a
            // périmé sa liste de dessin.
            let _ = context.submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES);
        },
    )
}

/// Ce que la fenêtre montrerait, vérifié sans fenêtre.
///
/// **Le test vit dans l'exemple, et c'est tout son intérêt.** Il lit `VERTICES`
/// et `TRIANGLES` — les constantes mêmes que `main` soumet et que les deux
/// `README` recopient —, là où un test rangé ailleurs aurait éprouvé sa propre
/// copie. C'est exactement l'écart qui a laissé cet exemple rendre une fenêtre
/// noire pendant toute une livraison : le sens de parcours faisait de sa face un
/// dos, éliminé, et aucun contrôle ne regardait l'image.
///
/// `test = true` sur l'entrée `[[example]]` du `Cargo.toml` est ce qui fait
/// exécuter ce module : Cargo compile les exemples par défaut, il n'en lance pas
/// les tests.
#[cfg(test)]
mod tests {
    use super::{TRIANGLES, VERTICES};
    use screengine::{Affine3, Config, Context};

    /// Le cadrage du contrôle, assez petit pour tenir en une tuile.
    fn config() -> Config {
        Config {
            max_width: 64,
            max_height: 64,
            width: 64,
            height: 64,
            tile_size: 64,
            max_triangles: 0,
            max_lines: 0,
        }
    }

    /// La scène de l'exemple peint, et pas seulement sans erreur.
    ///
    /// **Le critère est le pixel, jamais le code de retour.** Une soumission
    /// acceptée puis éliminée au test de face rend `Ok` et une image vide : c'est
    /// ce qui s'est produit, et c'est pourquoi ce test compte ce qui a été écrit
    /// plutôt que de vérifier qu'aucun appel n'a échoué.
    #[test]
    fn la_scene_de_l_exemple_peint() {
        let mut context = Context::new(config()).expect("cadrage valide");
        context
            .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
            .expect("le triangle de l'exemple est accepté");

        let mut pixels = vec![0u8; 64 * 64 * 4];
        context
            .frame_end(&mut pixels, 64)
            .expect("l'image se clôt dans le tampon");

        // Le fond laisse le rouge nul, comme le contrôle de couverture de la
        // conformance s'en sert : un pixel qui en porte vient du triangle.
        let peints = pixels.chunks_exact(4).filter(|pixel| pixel[0] != 0).count();
        assert!(
            peints > 64,
            "l'exemple ne peint que {peints} pixel(s) : la fenêtre serait noire"
        );
    }
}
