// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Un décor **chargé depuis un fichier**, parcouru à la souris et au clavier.
//!
//! **Ce que `couloir.rs` ne montre pas.** Celui-là décrit sa géométrie en Rust,
//! panneau par panneau, et cuit ses propres lightmaps : c'est ce qu'il faut pour
//! éprouver l'éclairage, et ce n'est pas ainsi qu'un décor arrive dans un jeu.
//! Ici, la carte et les caisses viennent de deux fichiers, exactement ceux que
//! les hôtes C, C++, wasm et Android chargent — **la même scène des cinq
//! côtés**, ce qui est la seule façon de vérifier qu'aucun des deux chemins vers
//! le moteur n'est de seconde classe.
//!
//! Les deux fichiers sont intégrés par `include_bytes!` plutôt qu'ouverts :
//! un exemple qui lirait un chemin relatif ne marcherait que lancé depuis la
//! racine du dépôt, et l'hôte qui lit les fichiers est déjà montré ailleurs.
//!
//! Les textures, elles, restent celles de l'hôte : la carte nomme ses matériaux
//! — `mur` et `sol` —, l'hôte décide de ce qu'il met derrière chaque nom. Ce
//! sont celles de `couloir.rs`, et c'est le sujet : **un même décor change
//! entièrement d'aspect sans qu'un octet du fichier bouge**, parce que rien de
//! ce qui habille n'est dedans. Les autres hôtes y mettent des damiers, faute de
//! décodeur PNG.
//!
//! **Le décor est celui à quatre cellules**, et c'est ce qui donne à cet exemple sa
//! seconde raison d'être : il est parcouru par la traversée de portails, avec une
//! cellule courante que l'hôte suit lui-même, et ses lightmaps sont cuites avant la
//! première image. Un décor à deux cellules dont tout est visible ne montrerait ni
//! l'un ni l'autre.
//!
//! **Le calque d'éditeur**, que `G` éteint et rallume, est la troisième : il
//! montre ce qu'aucune empreinte ne décrit. Le curseur désigne, le rayon
//! répond, le tracé marque le point touché ; les boîtes des caisses se voient,
//! occultées par les murs comme le reste. Il ne change rien à l'image
//! au-dessous, et c'est en l'éteignant qu'on s'en assure.
//!
//! Le curseur n'est pas capturé ici — on tourne aux flèches ou à `A` et `D` —,
//! si bien qu'il désigne librement, comme dans un éditeur. C'est pour ce cas que
//! `mouse_position` rend des pixels de la résolution interne : il n'y a aucune
//! mise à l'échelle de fenêtre à défaire pour retrouver le rayon.

use std::sync::Arc;

use screengine_play::screengine::Angle;
use screengine_play::{
    Affine3, Camera, Color, DepthMode, FreeCamera, KeyCode, Lightmaps, Line, Mesh, Play, Point,
    Surfaces, Texture, Vec3, World, load_png,
};

/// La carte du dépôt, celle que les cinq autres hôtes chargent.
const SALLES: &[u8] = include_bytes!("../../../hosts/salles.world");

/// Le maillage des caisses, de la même provenance.
const CAISSE: &[u8] = include_bytes!("../../../hosts/caisse.mesh");

/// Les murs du couloir, la texture de `couloir.rs`.
const MUR: &[u8] = include_bytes!("../assets/mur-mousse.png");

/// Le sol et le plafond, de la même provenance.
const SOL: &[u8] = include_bytes!("../assets/sol-pave-mousse.png");

/// Les côtés d'une caisse.
const MALLE: &[u8] = include_bytes!("../assets/malle-rouillee.png");

/// Où sont posées les caisses : abscisse, ordonnée, et l'angle qui les tourne.
///
/// Une par cellule que la traversée peut atteindre — la salle en L, le couloir, la
/// salle du bout —, et aucune à l'étage, qu'aucun portail ne relie au reste : une
/// caisse qu'on ne peut pas aller voir ne prouve rien.
const CRATES: [(f32, f32, f32); 3] = [(3.0, 2.0, 0.4), (10.0, 2.0, -0.7), (16.0, 4.0, 1.1)];

/// L'échelle des caisses.
///
/// Le maillage fait deux unités de côté, les salles quatre de haut : posée telle
/// quelle, une caisse en occupe la moitié. Le fichier ne se redimensionne pas —
/// son empreinte de conformance est figée —, donc l'échelle va dans la matrice
/// de modèle, qui est faite pour ça.
const CRATE_SCALE: f32 = 0.5;

/// La cote du centre d'une caisse, sa demi-hauteur au-dessus du sol.
const CRATE_Z: f32 = 0.5;

/// Où la caméra commence : dans la salle en L, à hauteur d'œil.
///
/// **Le sol est en zéro dans ce décor**, là où le couloir l'avait à `-1.5` : une
/// caméra laissée à l'origine serait dans le plancher, hors de toute cellule, et
/// la traversée ne rendrait rien du tout.
const START: Vec3 = Vec3::new(2.0, 2.0, 2.0);

/// Les demi-étendues du volume que le joueur occupe.
///
/// Une boîte et non un point : un point traverserait un angle rentrant sans
/// jamais toucher un mur, et c'est justement ce qu'un joueur fait quand il longe
/// une cloison.
///
/// **La verticale décrit un corps, pas une tête.** Centrée sur l'œil, une boîte
/// de cette largeur flotterait au-dessus de caisses hautes d'une unité et les
/// traverserait sans les toucher — géométriquement juste, et parfaitement faux.
/// Le volume descend donc jusqu'au sol, dont il garde un jeu : posé dessus, il
/// partirait en contact et le balayage le dirait solide.
const BODY_HALF: Vec3 = Vec3::new(0.3, 0.3, 0.9);

/// De combien l'œil est au-dessus du centre de ce volume.
///
/// La caméra porte l'œil, la collision porte le corps : l'un se déduit de
/// l'autre par cette constante, et rien d'autre ne les relie. Le départ est à
/// `2,0` et le sol à zéro, ce qui laisse le corps entre `0,1` et `1,9`.
const EYE_ABOVE: f32 = 1.0;

/// La demi-étendue de la boîte qui entoure une caisse.
///
/// **Le maillage n'est pas dans le décor, donc le balayage ne le voit pas** : une
/// caisse est un obstacle que l'exemple porte lui-même, et c'est la répartition
/// que l'étape veut montrer. Le moteur arrête sur la géométrie de cellule ;
/// l'hôte décide de ce qui n'en est pas.
///
/// Le maillage fait deux unités de côté et [`CRATE_SCALE`] le réduit de moitié,
/// d'où une demi-étendue d'une demi-unité. La rotation n'entre pas dans le
/// calcul : une boîte alignée qui enveloppe toutes les orientations est la
/// réponse la plus simple, et elle déborde d'un cinquième d'unité dans les
/// diagonales — ce qu'on ne sent pas en jouant.
const CRATE_HALF: f32 = 0.5 * core::f32::consts::SQRT_2;

/// La matrice d'une caisse : une rotation autour du zénith mise à l'échelle,
/// puis une translation.
///
/// Écrite coefficient par coefficient faute d'un constructeur qui compose les
/// trois — `Affine3` n'en a qu'un, sans échelle. Les sinus viennent des tables
/// du noyau : un exemple n'est comparé à aucune empreinte, mais appeler la libm
/// ici serait le premier pas vers l'appeler ailleurs.
fn crate_model(x: f32, y: f32, angle: f32) -> Affine3 {
    let a = Angle::from_radians(angle);
    let (c, s) = (a.cos() * CRATE_SCALE, a.sin() * CRATE_SCALE);
    Affine3 {
        m: [c, s, 0.0, -s, c, 0.0, 0.0, 0.0, CRATE_SCALE, x, y, CRATE_Z],
    }
}

/// Où le déplacement s'arrête : le décor d'abord, les caisses ensuite.
///
/// **Deux chemins, et c'est le sujet.** Les murs, le sol et le plafond viennent
/// du fichier : ils sont de la géométrie de cellule, et le moteur les balaie. Les
/// caisses sont des maillages posés par l'hôte, que le décor ne connaît pas :
/// c'est à l'exemple de les tester, en trois lignes, et le moteur n'a pas de
/// primitive pour cela — il n'en a pas besoin.
///
/// Rien ne glisse le long de ce qui arrête : le mouvement s'arrête net. La
/// glissade est une règle de jeu, trois lignes de projection sur la normale du
/// contact, et l'ajouter ici ferait passer pour un service du moteur ce qui
/// appartient à l'hôte.
fn stopped(scene: &Scene, from: Vec3, to: Vec3) -> Vec3 {
    // Hors de toute cellule, rien à balayer : la caméra est déjà sortie du
    // décor, et l'arrêter là l'y enfermerait.
    if scene.cell == 0 {
        return to;
    }
    let mut end = match scene.world.sweep(scene.cell, BODY_HALF, from, to) {
        // Un départ dans le solide ne bloque pas : il rendrait une fraction
        // nulle, et la caméra resterait collée sans moyen d'en sortir.
        Some(hit) if !hit.start_solid => from + (to - from) * hit.fraction,
        _ => to,
    };

    for &(x, y, _) in &CRATES {
        end = around_crate(Vec3::new(x, y, CRATE_Z), from, end);
    }
    end
}

/// Arrête le déplacement devant une caisse, sur l'axe où il y entre le plus tard.
///
/// Le recouvrement de deux boîtes alignées, écrit à la main : c'est la forme la
/// plus courte de ce qu'un hôte fait pour un obstacle qui n'est pas du décor, et
/// elle tient en vingt lignes parce qu'on ne demande ni le point de contact ni la
/// normale — seulement de ne pas entrer.
fn around_crate(centre: Vec3, from: Vec3, to: Vec3) -> Vec3 {
    let half = Vec3::new(
        CRATE_HALF + BODY_HALF.x,
        CRATE_HALF + BODY_HALF.y,
        CRATE_SCALE + BODY_HALF.z,
    );
    let inside = |p: Vec3| {
        (p.x - centre.x).abs() < half.x
            && (p.y - centre.y).abs() < half.y
            && (p.z - centre.z).abs() < half.z
    };
    if !inside(to) || inside(from) {
        return to;
    }

    // L'axe par lequel on est entré est celui dont on est le plus proche du
    // bord : y revenir sort de la boîte sans toucher aux deux autres, ce qui
    // laisse glisser le long d'une caisse alors que les murs, eux, arrêtent net.
    // La différence est visible en jouant, et elle est voulue : deux chemins,
    // deux réponses, décidées par l'hôte dans les deux cas.
    let mut best = to;
    let mut shortest = f32::MAX;
    for axis in 0..3 {
        let (p, c, h) = match axis {
            0 => (to.x, centre.x, half.x),
            1 => (to.y, centre.y, half.y),
            _ => (to.z, centre.z, half.z),
        };
        let edge = if p < c { c - h } else { c + h };
        let push = (edge - p).abs();
        if push < shortest {
            shortest = push;
            best = match axis {
                0 => Vec3::new(edge, to.y, to.z),
                1 => Vec3::new(to.x, edge, to.z),
                _ => Vec3::new(to.x, to.y, edge),
            };
        }
    }
    best
}

/// La résolution interne, écrite plutôt que laissée au défaut.
///
/// Le pas de mise à jour en a besoin : c'est lui qui change un pixel de curseur
/// en rayon, et il n'a pas le contexte sous la main. La valeur est celle que
/// `Play` donnait déjà, donc rien ne bouge de l'image.
const RESOLUTION: (u32, u32) = (640, 360);

/// Jusqu'où le rayon de sélection porte, en unités de monde.
///
/// Assez pour traverser le décor en diagonale. Au-delà, un rayon qui ne touche
/// rien est une réponse comme une autre, et le calque n'affiche alors rien.
const PICK_REACH: f32 = 40.0;

/// La longueur des branches de la croix qui marque le point touché.
const MARK: f32 = 0.25;

/// Les demi-étendues de la boîte d'une caisse.
///
/// **Celles de [`CRATE_HALF`] et [`CRATE_SCALE`], sans la marge du corps** : ce
/// que le calque dessine est ce que le curseur désigne, à l'unité près. La boîte
/// que `around_crate` teste est celle-là dilatée du corps du joueur, qui est une
/// autre question — on ne voit pas son propre volume.
const CRATE_BOX: Vec3 = Vec3::new(CRATE_HALF, CRATE_HALF, CRATE_SCALE);

/// Le gris froid des boîtes au repos.
const GUIDE: Color = Color::new(0x50, 0x70, 0xA0, 0xFF);

/// L'ambre de ce que le curseur désigne.
const PICKED: Color = Color::new(0xFF, 0xC0, 0x40, 0xFF);

/// La teinte du repère sur un mur, et celle du fil d'aplomb.
const ON_WALL: Color = Color::new(0x60, 0xE0, 0xFF, 0xFF);

/// Celle du repère sur un sol ou un plafond.
const ON_FLOOR: Color = Color::new(0xC0, 0xFF, 0x60, 0xFF);

/// Ce que le curseur désigne.
///
/// **Deux chemins, comme pour le déplacement.** Le décor vient du fichier et
/// c'est le moteur qui l'interroge ; les caisses sont des maillages que l'hôte
/// pose, et le décor ne les connaît pas — c'est donc à l'exemple de les tester.
/// La plus proche des deux réponses gagne, et le moteur n'a rien eu à savoir des
/// caisses.
enum Aim {
    /// Une surface du décor : où, sa normale, et le nom de son matériau.
    ///
    /// Le nom plutôt que le rang : c'est ce qui rend `surface_material` lisible
    /// sans une ligne de texte à l'écran, le repère prenant sa couleur de là.
    Surface(Vec3, Vec3, &'static str),
    /// Une caisse, par son rang dans [`CRATES`].
    Crate(usize),
}

/// La direction que désigne un pixel du curseur, en coordonnées de monde.
///
/// L'inverse exact de la projection du moteur, qui porte le rapport d'aspect par
/// la largeur seule : un même facteur sur les deux axes, et aucune correction
/// d'aspect à écrire ici. Le repère de vue est celui de `docs/abi.md` — X à
/// droite, Y en bas, Z devant —, ramené au monde par la base de la caméra.
fn aim_ray(camera: &Camera, pixel: (u32, u32)) -> Vec3 {
    let (width, height) = (RESOLUTION.0 as f32, RESOLUTION.1 as f32);
    let half = Angle::from_radians(camera.fov_y).half();
    let scale = (height * 0.5) * (half.cos() / half.sin());
    let x = (pixel.0 as f32 + 0.5 - width * 0.5) / scale;
    let y = (pixel.1 as f32 + 0.5 - height * 0.5) / scale;
    // X de vue ↦ −Y monde, Y de vue ↦ −Z monde, Z de vue ↦ +X monde.
    let local = Vec3::new(1.0, -x, -y);
    Affine3::from_rotation_translation(camera.orientation.normalize(), Vec3::ZERO)
        .transform_vector(local)
}

/// Où un rayon entre dans une boîte alignée, en fraction du trajet.
///
/// Les trois couples de plans, pris par leur intersection : la forme la plus
/// courte de ce qu'un hôte écrit pour désigner un objet qu'il porte lui-même.
/// Un rayon parallèle à un axe donne une division par zéro dont l'infini tombe
/// du bon côté de la comparaison, et c'est pour cela qu'il n'y a pas de cas
/// particulier.
fn ray_box(from: Vec3, span: Vec3, centre: Vec3, half: Vec3) -> Option<f32> {
    let mut near = 0.0f32;
    let mut far = 1.0f32;
    for axis in 0..3 {
        let (o, d, c, h) = match axis {
            0 => (from.x, span.x, centre.x, half.x),
            1 => (from.y, span.y, centre.y, half.y),
            _ => (from.z, span.z, centre.z, half.z),
        };
        let (mut lo, mut hi) = ((c - h - o) / d, (c + h - o) / d);
        if lo > hi {
            core::mem::swap(&mut lo, &mut hi);
        }
        near = near.max(lo);
        far = far.min(hi);
        if near > far {
            return None;
        }
    }
    Some(near)
}

/// Ce que le curseur désigne, ou rien.
fn aimed(scene: &Scene, pixel: (u32, u32)) -> Option<Aim> {
    let camera = scene.camera.camera();
    let from = camera.position;
    let span = aim_ray(&camera, pixel) * PICK_REACH;

    // **Toutes les surfaces, non solides comprises** : une sélection doit
    // pouvoir désigner une grille ou une vitre, que la collision traverse par
    // construction. Ce décor n'en porte aucune, et le filtre y rend donc la même
    // réponse que `Solid` — c'est celui de la collision qui les sépare, et
    // l'empreinte de la scène `selection` qui le fige.
    let mut best = None;
    if scene.cell != 0
        && let Some(hit) = scene
            .world
            .pick(scene.cell, from, from + span, Surfaces::All)
        && hit.surface != 0
    {
        let name = match scene.world.surface_material(hit.surface) {
            Some(rank) => match scene.world.material_name(rank) {
                Some("mur") => "mur",
                Some(_) => "sol",
                None => "sans nom",
            },
            None => "sans nom",
        };
        best = Some((hit.fraction, Aim::Surface(hit.point, hit.normal, name)));
    }

    for (rank, &(x, y, _)) in CRATES.iter().enumerate() {
        let centre = Vec3::new(x, y, CRATE_Z);
        if let Some(entry) = ray_box(from, span, centre, CRATE_BOX)
            && best.as_ref().is_none_or(|(closest, _)| entry < *closest)
        {
            best = Some((entry, Aim::Crate(rank)));
        }
    }
    best.map(|(_, aim)| aim)
}

/// Les douze arêtes d'une boîte alignée.
fn box_lines(centre: Vec3, half: Vec3, color: Color) -> [Line; 12] {
    let corner = |i: usize| {
        let sign = |bit: usize| if i & (1 << bit) == 0 { -1.0 } else { 1.0 };
        Vec3::new(
            centre.x + sign(0) * half.x,
            centre.y + sign(1) * half.y,
            centre.z + sign(2) * half.z,
        )
    };
    // Deux sommets sont voisins quand leurs rangs ne diffèrent que d'un bit :
    // les douze paires sortent de là, sans table à recopier.
    let mut lines = Vec::with_capacity(12);
    for i in 0..8usize {
        for bit in 0..3 {
            let j = i | (1 << bit);
            if j != i {
                lines.push(Line {
                    a: corner(i),
                    b: corner(j),
                    color,
                });
            }
        }
    }
    lines
        .try_into()
        .unwrap_or_else(|_| unreachable!("huit sommets donnent douze arêtes"))
}

/// Ce que la boucle garde entre deux images.
struct Scene {
    /// La caméra dirigée au clavier et à la souris.
    camera: FreeCamera,
    /// Le décor.
    world: World,
    /// Le maillage posé trois fois.
    crate_mesh: Mesh,
    /// Une texture par matériau de la carte, dans l'ordre qu'elle déclare.
    materials: Vec<Arc<Texture>>,
    /// La texture des caisses, sur leurs deux emplacements.
    crate_texture: Arc<Texture>,
    /// Les lightmaps, cuites une fois avant la première image.
    lightmaps: Lightmaps,
    /// La cellule où la caméra se trouve, suivie d'une image à l'autre.
    ///
    /// **L'hôte la garde, le moteur ne la retient pas.** C'est ce qui permet à la
    /// caméra d'être portée par autre chose que le moteur — ici une caméra libre au
    /// clavier —, et c'est aussi ce qui rend l'hôte responsable de ne pas l'écraser
    /// avec le zéro que le suivi rend quand on sort du décor.
    cell: u32,
    /// Où la caméra était à l'image précédente, ce dont le suivi a besoin.
    previous: Vec3,
    /// Le calque d'éditeur est-il allumé ?
    guides: bool,
    /// Ce que le curseur désignait au dernier pas, ou rien.
    aim: Option<Aim>,
}

/// Ouvre la fenêtre ; Échap ferme.
fn main() -> Result<(), screengine_play::Error> {
    let world = World::load(SALLES).expect("carte du dépôt valide");
    // L'hôte lit les noms et décide de ce qu'il charge : le moteur ne connaît
    // que des emplacements à remplir, dans l'ordre de la table. Un nom qu'il ne
    // reconnaît pas prend le pavé, plutôt que de laisser un trou sans texture
    // dans une image qu'on regarde.
    let mur = Arc::new(load_png(MUR)?);
    let sol = Arc::new(load_png(SOL)?);
    let materials = (0..world.material_count())
        .map(|rank| match world.material_name(rank) {
            Some("mur") => Arc::clone(&mur),
            _ => Arc::clone(&sol),
        })
        .collect();

    // **Toutes les cellules sont cuites avant la première image**, et non celles
    // que la caméra voit : une lightmap est un cache de la carte, pas de la vue, et
    // la cuire en chemin ferait allouer un atlas au milieu d'une image.
    let mut lightmaps = Lightmaps::new(&world).expect("porteur");
    for index in 0..world.cell_count() {
        if let Some(id) = world.cell_id(index) {
            lightmaps.build(&world, id).expect("cuisson possible");
        }
    }

    let cell = world.locate(START);
    let scene = Scene {
        camera: FreeCamera::new(START),
        world,
        crate_mesh: Mesh::load(CAISSE).expect("maillage du dépôt valide"),
        materials,
        crate_texture: Arc::new(load_png(MALLE)?),
        lightmaps,
        cell,
        previous: START,
        guides: true,
        aim: None,
    };

    Play::new()
        .title("Screengine — carte chargée")
        .resolution(RESOLUTION.0, RESOLUTION.1)
        .run(
            scene,
            |scene, tick| {
                if tick.input().pressed(KeyCode::Escape) {
                    tick.exit();
                }
                let before = scene.camera.position;
                scene.camera.update(tick);

                // **Le déplacement se filtre avant tout le reste.** La caméra libre
                // a déjà posé sa position ; ce qui suit la ramène à ce que le décor
                // autorise, et le suivi de cellule part alors d'un point qui est
                // réellement dedans. Ce qui se balaie est le centre du corps, l'œil
                // s'en déduisant.
                let body = Vec3::new(0.0, 0.0, EYE_ABOVE);
                scene.camera.position =
                    stopped(scene, before - body, scene.camera.position - body) + body;

                // **Le suivi se fait ici, pas au rendu** : il dépend du déplacement,
                // donc de deux positions successives, et le rendu n'en connaît qu'une.
                let position = scene.camera.camera().position;
                let found = scene.world.track(scene.cell, scene.previous, position);
                // **Zéro veut dire « sorti du décor », et ne s'écrit pas.** Une caméra
                // libre passe à travers les murs ; garder la dernière cellule connue
                // laisse voir le décor depuis dehors, là où l'écraser éteindrait
                // l'image et donnerait à croire que le moteur a lâché.
                if found != 0 {
                    scene.cell = found;
                }
                scene.previous = position;

                if tick.input().pressed(KeyCode::KeyG) {
                    scene.guides = !scene.guides;
                }

                // **L'interrogation est ici, pas au rendu** : ce que le curseur
                // désigne est un état de la partie — un éditeur en ferait la
                // sélection —, et le rendu n'en est que la mise en image. Elle ne se
                // pose pas quand le calque est éteint : une sélection qu'on ne voit
                // pas n'en est pas une.
                //
                // **Rien quand le curseur est hors de l'image** — dans une bande
                // noire, ou hors de la fenêtre : il ne désigne alors aucun pixel, et
                // inventer le centre de l'écran ferait croire à une visée qui
                // n'existe pas.
                scene.aim = match (scene.guides, tick.input().mouse_position()) {
                    (true, Some(pixel)) => aimed(scene, pixel),
                    _ => None,
                };
                // Le titre dit ce qui est visé, et c'est la seule voie de texte
                // qu'un exemple ait : le tracé prend des coordonnées de monde, et
                // une étiquette à l'écran appartient à l'hôte, après l'image.
                //
                // **Il nomme le matériau, pas le rôle de la surface**, et cette
                // carte en déclare deux : son plafond porte celui du sol, qu'il
                // annonce donc, et qu'il est pavé comme lui. Ce qui les sépare
                // dans le calque est la branche de la normale, qui sort de la
                // surface et pointe vers le bas sur un plafond.
                let title = match &scene.aim {
                    Some(Aim::Surface(_, _, name)) => {
                        format!("Screengine — carte chargée : {name}")
                    }
                    Some(Aim::Crate(rank)) => format!("Screengine — carte chargée : caisse {rank}"),
                    None => String::from("Screengine — carte chargée"),
                };
                tick.set_title(&title);
            },
            |scene, context| {
                let _ = context.set_camera(scene.camera.camera());
                // Un refus ne peut venir que de la capacité, que cette scène
                // n'approche pas ; le laisser passer vaut mieux qu'arrêter la
                // boucle sur une image manquante.
                let _ = context.submit_world_visible(
                    Affine3::IDENTITY,
                    &scene.world,
                    scene.cell,
                    Some(&scene.lightmaps),
                    |rank| scene.materials.get(rank as usize),
                );
                for &(x, y, angle) in &CRATES {
                    let model = crate_model(x, y, angle);
                    // Les deux emplacements portent la même texture. Les hôtes de
                    // conformance laissent le second vide, et la caisse y prend le
                    // bleu que son fichier donne au dessus : c'est ainsi qu'ils
                    // montrent un lot sans texture. Un exemple qu'on regarde n'a pas
                    // à porter cette démonstration.
                    let _ = context
                        .submit_mesh(model, &scene.crate_mesh, |_| Some(&scene.crate_texture));
                }
                if scene.guides {
                    overlay(scene, context);
                }
            },
        )
}

/// Le calque d'éditeur, après le décor et les caisses.
///
/// **L'ordre de soumission ne décide de rien pour le tracé** : il vient après le
/// remplissage de toute façon, et le mode de profondeur dit s'il est occulté. Ce
/// qu'il décide, c'est le rang entre deux traits qui se croisent — le tracé
/// n'écrit jamais la profondeur, et le dernier posé l'emporte. D'où la boîte
/// désignée en dernier.
fn overlay(scene: &Scene, context: &mut screengine_play::screengine::Context) {
    let picked = match scene.aim {
        Some(Aim::Crate(rank)) => Some(rank),
        _ => None,
    };

    // **Les boîtes sont occultées comme le décor** : une caisse derrière un mur
    // ne doit pas se voir à travers, sans quoi le calque mentirait sur ce qui est
    // visible. C'est l'autre moitié du contrat, et le repère plus bas en prend
    // l'autre face.
    for (rank, &(x, y, _)) in CRATES.iter().enumerate() {
        if picked == Some(rank) {
            continue;
        }
        let lines = box_lines(Vec3::new(x, y, CRATE_Z), CRATE_BOX, GUIDE);
        let _ = context.submit_lines(Affine3::IDENTITY, &lines, DepthMode::Tested);
    }

    match scene.aim {
        Some(Aim::Crate(rank)) => {
            let (x, y, _) = CRATES[rank];
            let lines = box_lines(Vec3::new(x, y, CRATE_Z), CRATE_BOX, PICKED);
            let _ = context.submit_lines(Affine3::IDENTITY, &lines, DepthMode::Tested);
        }
        // Une croix posée sur la surface, plus le fil d'aplomb jusqu'au sol.
        //
        // **La croix se dessine à travers**, et c'est le seul mode qui la rende
        // utilisable : testée, elle est à égalité de profondeur avec la surface
        // qu'elle marque, et le moindre arrondi la ferait clignoter. Le fil
        // d'aplomb l'est aussi, parce qu'il traverse le décor par construction —
        // c'est ce qu'un éditeur en attend pour placer quelque chose au sol.
        Some(Aim::Surface(at, normal, name)) => {
            let color = if name == "mur" { ON_WALL } else { ON_FLOOR };
            // Deux tangentes du plan, tirées de la normale sans trigonométrie :
            // le produit vectoriel avec l'axe dont la normale s'écarte le plus
            // ne dégénère jamais.
            let away = if normal.z.abs() < 0.5 {
                Vec3::new(0.0, 0.0, 1.0)
            } else {
                Vec3::new(1.0, 0.0, 0.0)
            };
            let u = normal.cross(away).normalize() * MARK;
            let v = normal.cross(u).normalize() * MARK;
            let marks = [
                Line {
                    a: at - u,
                    b: at + u,
                    color,
                },
                Line {
                    a: at - v,
                    b: at + v,
                    color,
                },
                // La normale, qui sort de la surface : c'est ce qui distingue un
                // sol d'un plafond quand la croix seule est ambiguë.
                Line {
                    a: at,
                    b: at + normal * (MARK * 2.0),
                    color,
                },
                Line {
                    a: at,
                    b: Vec3::new(at.x, at.y, 0.0),
                    color: GUIDE,
                },
            ];
            let _ = context.submit_lines(Affine3::IDENTITY, &marks, DepthMode::Always);
            // Le point exact, par-dessus la croix : il dit où le rayon a touché,
            // au pixel, là où la croix dit l'orientation.
            let _ = context.submit_points(
                Affine3::IDENTITY,
                &[Point { at, color: PICKED }],
                DepthMode::Always,
            );
        }
        None => {}
    }
}
