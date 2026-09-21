// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Un couloir qu'on parcourt, avec des caisses posées au sol.
//!
//! **La caméra est à l'intérieur d'une géométrie fermée**, ce qu'elle sera
//! toujours dans un monde de cellules. Les deux orientations de face se mêlent
//! dans la même image, les murs passent derrière le plan proche à chaque pas et
//! débordent de l'écran : le découpage et la bande de garde travaillent en
//! permanence, ce qu'un objet regardé de loin ne déclenche jamais. Le sol qui
//! fuit vers l'horizon vient avec.
//!
//! Les caisses portent ce que le couloir ne montre pas : l'une traverse le sol,
//! et seules les profondeurs départagent les deux surfaces qui s'y croisent.
//!
//! `Z` `Q` `S` `D` ou les flèches pour se déplacer, la souris pour regarder.
//! Le clic prend la souris, Échap la rend, un second Échap ferme.

use screengine_play::{
    Affine3, Color, FreeCamera, KeyCode, MouseButton, Play, Triangle, Vec3, Vec3 as V,
};

/// Demi-largeur du couloir, en unités de monde.
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

/// Un quadrilatère plan, en deux triangles qui partagent une diagonale.
///
/// Les coins se donnent en sens antihoraire vu de la face avant. Les deux
/// triangles portent la **même** teinte : une couture le long de leur diagonale
/// laisserait voir le fond, qui se remarque sur un aplat aussi bien que sur un
/// dégradé, et deux teintes par quadrilatère ne feraient que brouiller
/// l'alternance des panneaux.
fn quad(vertices: &mut Vec<Vec3>, faces: &mut Vec<Triangle>, corners: [Vec3; 4], color: Color) {
    let base = vertices.len() as u32;
    vertices.extend_from_slice(&corners);
    faces.push(Triangle {
        indices: [base, base + 1, base + 2],
        color,
    });
    faces.push(Triangle {
        indices: [base, base + 2, base + 3],
        color,
    });
}

/// Pose une face longitudinale en panneaux, deux teintes en alternance.
///
/// Un mur d'un seul aplat ne montre presque rien quand on avance : ce sont les
/// arêtes qui défilent qui rendent la fuite lisible, et c'est sur elles qu'une
/// couture se remarque. Les panneaux en donnent une tous les deux mètres, sans
/// rien demander au moteur qu'une couleur par triangle.
fn strip<F>(vertices: &mut Vec<Vec3>, faces: &mut Vec<Triangle>, colors: [Color; 2], corners: F)
where
    F: Fn(f32, f32) -> [Vec3; 4],
{
    let mut x = START;
    let mut panel = 0;
    while x < LENGTH {
        let next = (x + PANEL).min(LENGTH);
        quad(vertices, faces, corners(x, next), colors[panel % 2]);
        x = next;
        panel += 1;
    }
}

/// Une boîte posée en `(x, y)`, de base `z`, vue de l'extérieur.
fn box_at(vertices: &mut Vec<Vec3>, faces: &mut Vec<Triangle>, x: f32, y: f32, z: f32) {
    let h = CRATE / 2.0;
    let (x0, x1) = (x - h, x + h);
    let (y0, y1) = (y - h, y + h);
    let (z0, z1) = (z, z + CRATE);
    // Une teinte par orientation, comme si la lumière venait d'en haut. Rien
    // d'un éclairage — les lightmaps sont trois étapes plus loin —, mais sans
    // cela les faces d'une caisse se confondent et on ne voit plus son volume.
    let top = Color::new(0xC8, 0x96, 0x4E, 0xFF);
    let side = Color::new(0xA2, 0x74, 0x3A, 0xFF);
    let front = Color::new(0x86, 0x5E, 0x2E, 0xFF);

    // Dessus, vu d'en haut.
    quad(
        vertices,
        faces,
        [
            V::new(x0, y0, z1),
            V::new(x1, y0, z1),
            V::new(x1, y1, z1),
            V::new(x0, y1, z1),
        ],
        top,
    );
    // Les quatre côtés, chacun tourné vers l'extérieur. L'ordre des coins se
    // lit par le produit vectoriel des deux premières arêtes : il doit rendre
    // la normale sortante, faute de quoi la face est un dos et disparaît.
    quad(
        vertices,
        faces,
        [
            V::new(x0, y1, z0),
            V::new(x0, y0, z0),
            V::new(x0, y0, z1),
            V::new(x0, y1, z1),
        ],
        front,
    );
    quad(
        vertices,
        faces,
        [
            V::new(x1, y0, z0),
            V::new(x1, y1, z0),
            V::new(x1, y1, z1),
            V::new(x1, y0, z1),
        ],
        front,
    );
    quad(
        vertices,
        faces,
        [
            V::new(x0, y0, z0),
            V::new(x1, y0, z0),
            V::new(x1, y0, z1),
            V::new(x0, y0, z1),
        ],
        side,
    );
    quad(
        vertices,
        faces,
        [
            V::new(x1, y1, z0),
            V::new(x0, y1, z0),
            V::new(x0, y1, z1),
            V::new(x1, y1, z1),
        ],
        side,
    );
}

/// Le couloir et ses caisses, en coordonnées de monde.
///
/// Construit une fois : la géométrie ne change pas, seule la caméra bouge. Ce
/// que le moteur reçoit chaque image, c'est la même scène et une caméra
/// différente.
fn corridor() -> (Vec<Vec3>, Vec<Triangle>) {
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let (w, h, l) = (HALF_WIDTH, HEIGHT, LENGTH);

    // Le sol, vu d'en haut.
    strip(
        &mut vertices,
        &mut faces,
        [
            Color::new(0x52, 0x5A, 0x62, 0xFF),
            Color::new(0x3E, 0x45, 0x4C, 0xFF),
        ],
        |a, b| {
            [
                V::new(a, -w, 0.0),
                V::new(b, -w, 0.0),
                V::new(b, w, 0.0),
                V::new(a, w, 0.0),
            ]
        },
    );
    // Le plafond, vu d'en bas : l'ordre des coins est inversé.
    strip(
        &mut vertices,
        &mut faces,
        [
            Color::new(0x32, 0x36, 0x3E, 0xFF),
            Color::new(0x26, 0x2A, 0x31, 0xFF),
        ],
        |a, b| {
            [
                V::new(a, w, h),
                V::new(b, w, h),
                V::new(b, -w, h),
                V::new(a, -w, h),
            ]
        },
    );
    // Les deux murs, chacun tourné vers l'intérieur. Ils ne portent pas la même
    // paire de teintes : sans cela, on ne distingue pas lequel on longe.
    strip(
        &mut vertices,
        &mut faces,
        [
            Color::new(0x86, 0x78, 0x60, 0xFF),
            Color::new(0x6A, 0x5F, 0x4C, 0xFF),
        ],
        |a, b| {
            [
                V::new(a, -w, 0.0),
                V::new(a, -w, h),
                V::new(b, -w, h),
                V::new(b, -w, 0.0),
            ]
        },
    );
    strip(
        &mut vertices,
        &mut faces,
        [
            Color::new(0x74, 0x68, 0x54, 0xFF),
            Color::new(0x5C, 0x53, 0x42, 0xFF),
        ],
        |a, b| {
            [
                V::new(b, w, 0.0),
                V::new(b, w, h),
                V::new(a, w, h),
                V::new(a, w, 0.0),
            ]
        },
    );
    // Le fond, pour que le couloir soit fermé.
    quad(
        &mut vertices,
        &mut faces,
        [
            V::new(l, -w, 0.0),
            V::new(l, -w, h),
            V::new(l, w, h),
            V::new(l, w, 0.0),
        ],
        Color::new(0x9A, 0x86, 0x60, 0xFF),
    );

    for (x, y, z) in CRATES {
        box_at(&mut vertices, &mut faces, x, y, z);
    }
    (vertices, faces)
}

/// Ce que la mise à jour fait avancer d'un pas à l'autre.
struct World {
    /// La caméra, à hauteur d'œil.
    camera: FreeCamera,
}

/// Ouvre la fenêtre et parcourt le couloir.
fn main() -> Result<(), screengine_play::Error> {
    let (vertices, faces) = corridor();
    let world = World {
        camera: FreeCamera::new(Vec3::new(0.0, 0.0, 1.6)),
    };
    // L'indication vit dans la barre de titre et non dans la console : c'est
    // là qu'on la cherche quand on ne sait plus comment récupérer sa souris,
    // et elle y reste visible pendant que le curseur est pris.
    Play::new()
        .title(
            "Couloir — Z Q S D / flèches · clic : souris · Échap : rendre la souris, puis quitter",
        )
        .run(
            world,
            |world, tick| {
                // Le curseur n'est **pas** pris au démarrage, et c'est délibéré :
                // une fenêtre qui s'empare de la souris à l'ouverture laisse
                // chercher comment la récupérer. Le clic la prend, Échap la rend —
                // la convention de tous les jeux qui font ça bien.
                if tick.input().button_pressed(MouseButton::Left) {
                    tick.capture_cursor(true);
                }
                if tick.input().pressed(KeyCode::Escape) {
                    // Échap rend d'abord la souris, et ne ferme qu'ensuite : sans
                    // cela, on quitte sans jamais pouvoir la récupérer.
                    if tick.cursor_captured() {
                        tick.capture_cursor(false);
                    } else {
                        tick.exit();
                    }
                }
                world.camera.update(tick);
            },
            move |world, context| {
                // Un refus ne peut venir que de la capacité, que cette scène
                // n'approche pas ; le laisser passer vaut mieux qu'arrêter la
                // boucle sur une image manquante.
                let _ = context.set_camera(world.camera.camera());
                let _ = context.submit(Affine3::IDENTITY, &vertices, &faces);
            },
        )
}
