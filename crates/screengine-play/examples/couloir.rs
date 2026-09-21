// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Un couloir texturé qu'on parcourt, avec des caisses posées au sol.
//!
//! **La caméra est à l'intérieur d'une géométrie fermée**, ce qu'elle sera
//! toujours dans un monde de cellules. Les deux orientations de face se mêlent
//! dans la même image, les murs passent derrière le plan proche à chaque pas et
//! débordent de l'écran : le découpage et la bande de garde travaillent en
//! permanence, ce qu'un objet regardé de loin ne déclenche jamais.
//!
//! Les caisses portent ce que le couloir ne montre pas : l'une traverse le sol,
//! et seules les profondeurs départagent les deux surfaces qui s'y croisent.
//!
//! C'est aussi là que le filtrage se juge, et il ne se juge qu'à l'œil : le
//! pavage du sol scintille si le niveau de mipmap est mal choisi, et le tramage
//! des coordonnées se voit en se collant à un mur, là où un texel couvre
//! plusieurs pixels. Aucune empreinte ne dit ces deux choses.
//!
//! `Z` `Q` `S` `D` ou les flèches pour se déplacer, la souris pour regarder.
//! Le clic gauche prend la souris, le clic du milieu la rend, Échap ferme.

use std::sync::Arc;

use screengine_play::{
    Affine3, Color, FreeCamera, MouseButton, Play, Texture, Triangle, Vec3, VertexUv, load_png,
};

/// Demi-largeur du couloir, en unités de monde. Une unité vaut un mètre : la
/// caméra est à hauteur d'œil, et les textures s'échelonnent là-dessus.
const HALF_WIDTH: f32 = 1.6;

/// Hauteur sous plafond.
const HEIGHT: f32 = 2.4;

/// Longueur du couloir.
const LENGTH: f32 = 40.0;

/// Où le couloir commence, derrière la caméra.
const START: f32 = -2.0;

/// Longueur d'un panneau de mur, de sol ou de plafond.
const PANEL: f32 = 2.0;

/// Côté d'une caisse.
const CRATE: f32 = 0.7;

/// Texels de brique par unité de monde : la texture couvre un mètre, ce qui
/// donne des briques de vingt centimètres.
///
/// C'est la densité qui décide de ce qu'on voit, bien avant la matière. Plus
/// haute, le niveau 0 ne s'atteindrait jamais et le tramage n'aurait rien à
/// masquer ; plus basse, un mur entier tiendrait dans une brique.
const BRICK_DENSITY: f32 = 512.0;

/// Texels de pavage par unité de monde : la texture couvre quatre mètres, soit
/// des pavés de treize centimètres.
///
/// Deux fois plus serré, le pavage se moyennait en un gris uniforme dès le
/// milieu du couloir : un motif dont le détail descend sous le texel ne rend
/// plus que sa moyenne, et le mipmap a raison de le faire. La densité décide
/// donc de ce qui reste lisible au loin, pas le filtrage.
const STONE_DENSITY: f32 = 128.0;

/// Texels de bois par unité de monde : une répétition exacte par face de
/// caisse, soit des planches de dix-sept centimètres.
const WOOD_DENSITY: f32 = 512.0 / CRATE;

/// Les caisses, à leur abscisse le long du couloir, avec leur cote de base.
///
/// La dernière est enfoncée sous le sol : c'est elle qui fait travailler le
/// tampon de profondeur, deux surfaces s'y coupant sans qu'aucune arête ne
/// marque l'intersection.
const CRATES: [(f32, f32, f32); 5] = [
    (6.0, 0.6, 0.0),
    (12.0, -0.7, 0.0),
    (18.0, 0.0, 0.0),
    (25.0, 0.8, 0.0),
    (31.0, -0.4, -CRATE * 0.4),
];

/// Une surface et son habillage, prêtes à partir en un lot.
///
/// Un lot porte une texture et une seule : le couloir en compte donc trois, et
/// c'est ce découpage-là qui décide de la géométrie, pas l'inverse.
#[derive(Default)]
struct Mesh {
    vertices: Vec<VertexUv>,
    faces: Vec<Triangle>,
}

impl Mesh {
    /// Ajoute un quadrilatère plan, en deux triangles qui partagent une
    /// diagonale.
    ///
    /// Les coins se donnent en sens antihoraire vu de la face avant. La couleur
    /// du triangle n'est pas lue quand une texture l'habille — le remplissage
    /// écrit le texel —, et vaut blanc pour que le jour où elle modulerait la
    /// texture, elle la laisse intacte.
    fn quad(&mut self, corners: [VertexUv; 4]) {
        let base = self.vertices.len() as u32;
        let white = Color::new(0xFF, 0xFF, 0xFF, 0xFF);
        self.vertices.extend_from_slice(&corners);
        self.faces.push(Triangle {
            indices: [base, base + 1, base + 2],
            color: white,
        });
        self.faces.push(Triangle {
            indices: [base, base + 2, base + 3],
            color: white,
        });
    }

    /// Pose une face longitudinale en panneaux de [`PANEL`] mètres.
    ///
    /// Les panneaux ne sont pas là pour la finesse de la géométrie : ils
    /// **bornent les coordonnées de texture**. À la densité de la brique, une
    /// abscisse continue atteindrait vingt et un mille texels sur la longueur
    /// du couloir, au-delà de ce que le moteur accepte ; chaque panneau repart
    /// donc de zéro, et cela ne se voit pas parce qu'il en porte deux
    /// répétitions entières.
    ///
    /// C'est au tracé de le décider, panneau par panneau : le sol, quatre fois
    /// moins dense, garde des coordonnées continues, faute de quoi sa demi-
    /// répétition par panneau ouvrirait une couture tous les deux mètres.
    fn strip<F>(&mut self, corners: F)
    where
        F: Fn(f32, f32) -> [VertexUv; 4],
    {
        let mut x = START;
        while x < LENGTH {
            let next = (x + PANEL).min(LENGTH);
            self.quad(corners(x, next));
            x = next;
        }
    }
}

/// Un sommet et ses coordonnées de texture, en texels et non normalisées.
fn at(position: Vec3, u: f32, v: f32) -> VertexUv {
    VertexUv { position, u, v }
}

/// Pose une caisse en `(x, y)`, de base `z`, vue de l'extérieur.
///
/// **Chaque face reçoit les coordonnées de ses deux axes propres**, jamais la
/// même paire projetée six fois : les planches courent alors horizontalement
/// sur le dessus et verticalement sur les côtés, comme une caisse clouée. Les
/// six faces habillées pareil donneraient un cube dont on ne lit plus les
/// arêtes.
fn crate_at(mesh: &mut Mesh, x: f32, y: f32, z: f32) {
    let h = CRATE / 2.0;
    let (x0, x1) = (x - h, x + h);
    let (y0, y1) = (y - h, y + h);
    let (z0, z1) = (z, z + CRATE);
    let d = WOOD_DENSITY;
    // Sur les côtés, la texture descend avec la caisse : `v` se compte depuis
    // le dessus, faute de quoi les planches sont la tête en bas.
    let down = |cote: f32| (z1 - cote) * d;

    // Dessus, vu d'en haut : les planches suivent y.
    mesh.quad([
        at(Vec3::new(x0, y0, z1), 0.0, 0.0),
        at(Vec3::new(x1, y0, z1), (x1 - x0) * d, 0.0),
        at(Vec3::new(x1, y1, z1), (x1 - x0) * d, (y1 - y0) * d),
        at(Vec3::new(x0, y1, z1), 0.0, (y1 - y0) * d),
    ]);
    // Les quatre côtés, chacun tourné vers l'extérieur. L'ordre des coins se
    // lit par le produit vectoriel des deux premières arêtes : il doit rendre
    // la normale sortante, faute de quoi la face est un dos et disparaît.
    mesh.quad([
        at(Vec3::new(x0, y1, z0), 0.0, down(z0)),
        at(Vec3::new(x0, y0, z0), (y1 - y0) * d, down(z0)),
        at(Vec3::new(x0, y0, z1), (y1 - y0) * d, 0.0),
        at(Vec3::new(x0, y1, z1), 0.0, 0.0),
    ]);
    mesh.quad([
        at(Vec3::new(x1, y0, z0), 0.0, down(z0)),
        at(Vec3::new(x1, y1, z0), (y1 - y0) * d, down(z0)),
        at(Vec3::new(x1, y1, z1), (y1 - y0) * d, 0.0),
        at(Vec3::new(x1, y0, z1), 0.0, 0.0),
    ]);
    mesh.quad([
        at(Vec3::new(x0, y0, z0), 0.0, down(z0)),
        at(Vec3::new(x1, y0, z0), (x1 - x0) * d, down(z0)),
        at(Vec3::new(x1, y0, z1), (x1 - x0) * d, 0.0),
        at(Vec3::new(x0, y0, z1), 0.0, 0.0),
    ]);
    mesh.quad([
        at(Vec3::new(x1, y1, z0), 0.0, down(z0)),
        at(Vec3::new(x0, y1, z0), (x1 - x0) * d, down(z0)),
        at(Vec3::new(x0, y1, z1), (x1 - x0) * d, 0.0),
        at(Vec3::new(x1, y1, z1), 0.0, 0.0),
    ]);
}

/// Le couloir en trois surfaces : la maçonnerie, le sol, les caisses.
///
/// Construit une fois : la géométrie ne change pas, seule la caméra bouge. Ce
/// que le moteur reçoit chaque image, c'est la même scène et une caméra
/// différente.
fn corridor() -> [Mesh; 3] {
    let mut masonry = Mesh::default();
    let mut floor = Mesh::default();
    let mut crates = Mesh::default();
    let (w, h, l) = (HALF_WIDTH, HEIGHT, LENGTH);
    let (brick, stone) = (BRICK_DENSITY, STONE_DENSITY);

    // Le sol, vu d'en haut, en coordonnées continues d'un bout à l'autre.
    floor.strip(|a, b| {
        let (ua, ub) = ((a - START) * stone, (b - START) * stone);
        [
            at(Vec3::new(a, -w, 0.0), ua, 0.0),
            at(Vec3::new(b, -w, 0.0), ub, 0.0),
            at(Vec3::new(b, w, 0.0), ub, 2.0 * w * stone),
            at(Vec3::new(a, w, 0.0), ua, 2.0 * w * stone),
        ]
    });
    // Le plafond, vu d'en bas : l'ordre des coins est inversé. Il prend la
    // brique des murs plutôt que le pavage du sol — un plafond pavé se lit
    // comme un sol au-dessus de la tête.
    masonry.strip(|a, b| {
        [
            at(Vec3::new(a, w, h), 0.0, 0.0),
            at(Vec3::new(b, w, h), (b - a) * brick, 0.0),
            at(Vec3::new(b, -w, h), (b - a) * brick, 2.0 * w * brick),
            at(Vec3::new(a, -w, h), 0.0, 2.0 * w * brick),
        ]
    });
    // Les deux murs, chacun tourné vers l'intérieur. `v` se compte depuis le
    // plafond : les assises se posent depuis le haut de la texture, et un mur
    // retourné se verrait aux joints.
    masonry.strip(|a, b| {
        [
            at(Vec3::new(a, -w, 0.0), 0.0, h * brick),
            at(Vec3::new(a, -w, h), 0.0, 0.0),
            at(Vec3::new(b, -w, h), (b - a) * brick, 0.0),
            at(Vec3::new(b, -w, 0.0), (b - a) * brick, h * brick),
        ]
    });
    masonry.strip(|a, b| {
        [
            at(Vec3::new(b, w, 0.0), (b - a) * brick, h * brick),
            at(Vec3::new(b, w, h), (b - a) * brick, 0.0),
            at(Vec3::new(a, w, h), 0.0, 0.0),
            at(Vec3::new(a, w, 0.0), 0.0, h * brick),
        ]
    });
    // Le fond, pour que le couloir soit fermé.
    masonry.quad([
        at(Vec3::new(l, -w, 0.0), 0.0, h * brick),
        at(Vec3::new(l, -w, h), 0.0, 0.0),
        at(Vec3::new(l, w, h), 2.0 * w * brick, 0.0),
        at(Vec3::new(l, w, 0.0), 2.0 * w * brick, h * brick),
    ]);

    for (x, y, z) in CRATES {
        crate_at(&mut crates, x, y, z);
    }
    [masonry, floor, crates]
}

/// Ce que la mise à jour fait avancer d'un pas à l'autre.
struct World {
    /// La caméra, à hauteur d'œil.
    camera: FreeCamera,
}

/// Ouvre la fenêtre et parcourt le couloir.
fn main() -> Result<(), screengine_play::Error> {
    let [masonry, floor, crates] = corridor();
    let textures: [Arc<Texture>; 3] = [
        Arc::new(load_png(include_bytes!("../assets/brick.png"))?),
        Arc::new(load_png(include_bytes!("../assets/stone.png"))?),
        Arc::new(load_png(include_bytes!("../assets/wood.png"))?),
    ];
    let world = World {
        camera: FreeCamera::new(Vec3::new(0.0, 0.0, 1.6)),
    };
    // L'indication vit dans la barre de titre et non dans la console : c'est
    // là qu'on la cherche quand on ne sait plus comment récupérer sa souris,
    // et elle y reste visible pendant que le curseur est pris.
    Play::new()
        .title("Couloir — Z Q S D / flèches · clic gauche : souris · clic milieu : la rendre")
        .run(
            world,
            |world, tick| {
                // Le curseur n'est **pas** pris au démarrage, et c'est délibéré :
                // une fenêtre qui s'empare de la souris à l'ouverture laisse
                // chercher comment la récupérer. Le clic gauche la prend, celui
                // du milieu la rend — un bouton qui ne sert à rien d'autre, là
                // où Échap ferme sans détour.
                if tick.input().button_pressed(MouseButton::Left) {
                    tick.capture_cursor(true);
                }
                if tick.input().button_pressed(MouseButton::Middle) {
                    tick.capture_cursor(false);
                }
                world.camera.update(tick);
            },
            move |world, context| {
                // Un refus ne peut venir que de la capacité, que cette scène
                // n'approche pas ; le laisser passer vaut mieux qu'arrêter la
                // boucle sur une image manquante.
                let _ = context.set_camera(world.camera.camera());
                for (mesh, texture) in [&masonry, &floor, &crates].into_iter().zip(&textures) {
                    let _ = context.submit_textured(
                        Affine3::IDENTITY,
                        &mesh.vertices,
                        &mesh.faces,
                        texture,
                    );
                }
            },
        )
}
