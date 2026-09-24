// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Un couloir éclairé qu'on parcourt, avec des caisses posées au sol.
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
//! **L'éclairage vient de lightmaps que cet exemple calcule lui-même.** À cette
//! étape le moteur n'en produit aucune — il n'a ni carte ni cellule pour cela —
//! et l'ABI en fait une ressource que l'hôte fournit : les cuire ici montre
//! donc ce que l'étape du monde fera dans le noyau, et ce qu'un intégrateur
//! peut faire dès aujourd'hui. Quatre lampes espacées de neuf mètres, et le
//! noir entre deux — c'est lui qui donne son registre au couloir, pas la
//! lumière.
//!
//! **La géométrie est subdivisée par l'éclairage, pas par la forme.**
//! L'atténuation se calcule par sommet : un mur d'un seul quadrilatère ne
//! porterait la lumière qu'à ses quatre coins. Les faces se pavent donc en
//! panneaux d'un demi-mètre — sauf ce qu'aucune lampe n'atteint, comme le ciel
//! au bout du couloir, qui se pose d'un seul tenant et l'a d'abord payé de
//! cinquante mille triangles.
//!
//! **Une caisse n'a pas de lightmap à elle**, elle échantillonne celle du
//! décor à sa position : un texel unique, ce qu'un objet mobile reçoit dans un
//! moteur de cette famille.
//!
//! C'est aussi là que le filtrage se juge, et il ne se juge qu'à l'œil : le
//! pavage du sol scintille si le niveau de mipmap est mal choisi, et le tramage
//! des coordonnées se voit en se collant à un mur, là où un texel couvre
//! plusieurs pixels. Aucune empreinte ne dit ces deux choses.
//!
//! `F` bascule le filtrage des textures. **C'est en marchant qu'il se juge** :
//! côte à côte sur une image fixe, le tramage et le bilinéaire ne montrent
//! qu'un grain contre un flou, alors que ce qui les sépare vraiment — un motif
//! qui fourmille contre une surface qui glisse — ne se voit qu'en mouvement.
//!
//! `Z` `Q` `S` `D` ou les flèches pour se déplacer, la souris pour regarder.
//! Le clic gauche prend la souris, le clic du milieu la rend, Échap ferme.

use std::sync::Arc;

use screengine_play::{
    Affine3, Color, Filter, FreeCamera, KeyCode, MouseButton, Play, Texture, Triangle, Vec3,
    VertexUv, VertexUv2, load_png,
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

/// Côté d'un panneau de mur, de sol ou de plafond.
///
/// **L'éclairage décide de cette valeur, pas la géométrie.** L'atténuation
/// d'une lumière dynamique se calcule par sommet : un panneau de deux mètres
/// ne rendrait que le dégradé entre ses quatre coins, et une source posée en
/// son milieu n'y ferait aucun halo. À un demi-mètre, la lumière retrouve une
/// forme — au prix de quelques milliers de triangles, loin de la capacité de
/// seize mille.
const PANEL: f32 = 0.5;

/// À quelle distance derrière le bout du couloir le ciel se tient.
///
/// Assez loin pour qu'il ne bouge pas quand on avance — une paroi de ciel qui
/// défilerait comme un mur trahirait qu'elle en est un.
const SKY_DISTANCE: f32 = 50.0;

/// Demi-côté du panneau de ciel.
const SKY_SIDE: f32 = 40.0;

/// Texels de ciel par unité de monde : une répétition tous les vingt mètres,
/// assez lâche pour qu'aucun raccord ne se compte à l'œil.
const SKY_DENSITY: f32 = 512.0 / 20.0;

/// Texels de lightmap par unité de monde.
///
/// Grossière par construction : une lightmap s'agrandit d'un facteur dix ou
/// vingt sur la surface, et c'est le bilinéaire inconditionnel qui lui rend sa
/// douceur. Plus fine, elle coûterait de la mémoire pour un dégradé que rien
/// ne distingue.
const LIGHTMAP_DENSITY: f32 = 2.0;

/// Côté d'une caisse.
const CRATE: f32 = 0.7;

/// Texels de pierre par unité de monde : la texture couvre deux mètres, ce qui
/// donne des blocs d'une trentaine de centimètres.
///
/// C'est la densité qui décide de ce qu'on voit, bien avant la matière. Plus
/// haute, le niveau 0 ne s'atteindrait jamais et le tramage n'aurait rien à
/// masquer ; plus basse, un mur entier tiendrait dans un bloc.
const STONE_DENSITY: f32 = 256.0;

/// Texels de pavage par unité de monde : la texture couvre quatre mètres, soit
/// des pavés d'une quinzaine de centimètres.
///
/// Deux fois plus serré, le pavage se moyennait en un gris uniforme dès le
/// milieu du couloir : un motif dont le détail descend sous le texel ne rend
/// plus que sa moyenne, et le mipmap a raison de le faire. La densité décide
/// donc de ce qui reste lisible au loin, pas le filtrage.
const COBBLE_DENSITY: f32 = 128.0;

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

/// Les lampes du couloir : leur abscisse, et la teinte qu'elles y posent.
///
/// Espacées de neuf mètres pour que leurs flaques ne se rejoignent pas : ce
/// qui fait le registre n'est pas la lumière, c'est **le noir entre deux**.
/// Une lampe tous les trois mètres rendrait un couloir uniformément éclairé,
/// c'est-à-dire un couloir sans intention.
const LAMPS: [(f32, [f32; 3]); 4] = [
    (3.0, [1.00, 0.95, 0.80]),
    (12.0, [0.85, 0.95, 1.00]),
    (21.0, [1.00, 0.90, 0.75]),
    (30.0, [0.80, 0.95, 1.00]),
];

/// Portée d'une lampe, au-delà de laquelle elle n'éclaire plus rien.
const LAMP_REACH: f32 = 6.5;

/// Ce qu'une surface reçoit là où aucune lampe ne porte.
///
/// Pas zéro : un noir absolu effacerait la texture au lieu de l'assombrir, et
/// un couloir dont on ne devine plus les murs n'est pas lugubre, il est vide.
const AMBIENT: f32 = 0.06;

/// Calcule l'éclairement d'un point, toutes lampes confondues.
///
/// **C'est ce que l'étape 5 fera dans le noyau**, cellule par cellule. Ici
/// l'hôte s'en charge, comme l'ABI le prévoit à cette étape : une lightmap est
/// une ressource qu'il fournit, le moteur ne fait que l'échantillonner.
///
/// L'atténuation est celle du moteur pour ses lumières dynamiques —
/// `(1 - d²/r²)²`, nulle et de dérivée nulle à la portée —, de sorte que le
/// néon de la suite se fondra dans le même éclairage plutôt que de s'y
/// superposer comme un corps étranger.
fn lit_at(point: Vec3) -> [f32; 3] {
    let mut sum = [AMBIENT; 3];
    for (x, tint) in LAMPS {
        // La lampe est au plafond, au milieu du couloir.
        let lamp = Vec3::new(x, 0.0, HEIGHT - 0.1);
        let (dx, dy, dz) = (point.x - lamp.x, point.y - lamp.y, point.z - lamp.z);
        let square = dx * dx + dy * dy + dz * dz;
        let reach = LAMP_REACH * LAMP_REACH;
        if square >= reach {
            continue;
        }
        let falloff = 1.0 - square / reach;
        let falloff = falloff * falloff;
        for (channel, value) in sum.iter_mut().zip(tint) {
            *channel += value * falloff;
        }
    }
    sum
}

/// Un rectangle du décor, de quoi lui cuire sa lightmap et l'habiller.
///
/// Les deux axes portent leur longueur : la surface va de `origin` à
/// `origin + along + across`, et c'est la seule description dont le calcul de
/// la lightmap et le tracé des panneaux aient besoin.
#[derive(Clone, Copy)]
struct Face {
    /// Un coin, celui d'où partent les deux axes.
    origin: Vec3,
    /// Le premier axe, longueur comprise.
    along: Vec3,
    /// Le second, de même.
    across: Vec3,
}

impl Face {
    /// Le point de la surface aux coordonnées relatives `(s, t)`, entre 0 et 1.
    fn point(self, s: f32, t: f32) -> Vec3 {
        Vec3::new(
            self.origin.x + self.along.x * s + self.across.x * t,
            self.origin.y + self.along.y * s + self.across.y * t,
            self.origin.z + self.along.z * s + self.across.z * t,
        )
    }

    /// Les deux côtés de sa lightmap, en texels, chacun une puissance de deux.
    ///
    /// Le moteur exige des puissances de deux : son repli de coordonnées est
    /// un masque. On arrondit donc **vers le haut**, quitte à ce qu'un texel
    /// couvre un peu moins que prévu — l'inverse ouvrirait des trous de
    /// lumière sur les grandes faces.
    fn lightmap_size(self) -> (u32, u32) {
        let side = |axis: Vec3| {
            let length = (axis.x * axis.x + axis.y * axis.y + axis.z * axis.z).sqrt();
            let wanted = (length * LIGHTMAP_DENSITY).max(2.0) as u32;
            wanted.next_power_of_two().min(256)
        };
        (side(self.along), side(self.across))
    }

    /// Cuit la lightmap de la surface, un texel après l'autre.
    ///
    /// Chaque texel est pris **au centre de sa cellule** et non à son coin :
    /// sur un bord, un texel pris au coin déborderait de la surface d'un
    /// demi-texel une fois interpolé, et la lumière y baverait.
    fn bake(self) -> Result<Texture, screengine_play::screengine::Error> {
        let (width, height) = self.lightmap_size();
        let mut texels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height {
            for column in 0..width {
                let s = (column as f32 + 0.5) / width as f32;
                let t = (row as f32 + 0.5) / height as f32;
                let light = lit_at(self.point(s, t));
                for channel in light {
                    let scaled = channel * 255.0 + 0.5;
                    let clamped = if scaled > 255.0 { 255.0 } else { scaled };
                    texels.push(clamped as u8);
                }
                texels.push(0xFF);
            }
        }
        Texture::load(width, height, &texels)
    }
}

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
}

/// La longueur d'un vecteur, pour dimensionner une face.
fn length(v: Vec3) -> f32 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

/// Une surface éclairée et son habillage : sommets à deux jeux de coordonnées.
///
/// Séparé de [`Mesh`] plutôt que fondu avec lui : les caisses n'ont pas de
/// lightmap, et un maillage qui en porterait une par nécessité de type
/// obligerait à en cuire une pour elles — c'est-à-dire à payer une ressource
/// pour satisfaire une signature.
#[derive(Default)]
struct LitMesh {
    vertices: Vec<VertexUv2>,
    faces: Vec<Triangle>,
}

impl LitMesh {
    /// Ajoute un quadrilatère plan, en deux triangles qui partagent une
    /// diagonale, coins en sens antihoraire vus de la face avant.
    fn quad(&mut self, corners: [VertexUv2; 4]) {
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

    /// Reprend un maillage d'objet en lui donnant des coordonnées de lightmap
    /// **constantes**, celles du texel unique de [`object_light`].
    ///
    /// L'objet reçoit ainsi l'éclairage du décor à sa position, d'un bloc,
    /// plutôt que de rester à sa couleur pleine au milieu d'un couloir sombre.
    fn from_object(mesh: Mesh) -> Self {
        let vertices = mesh
            .vertices
            .iter()
            .map(|vertex| VertexUv2 {
                position: vertex.position,
                u: vertex.u,
                v: vertex.v,
                u2: 1.0,
                v2: 1.0,
            })
            .collect();
        Self {
            vertices,
            faces: mesh.faces,
        }
    }

    /// Pose une face **d'un seul tenant**, sans la subdiviser.
    ///
    /// Réservé à ce qu'aucune lampe n'atteint. La subdivision n'existe que
    /// pour l'éclairage par sommet : une surface à éclairement uniforme n'en
    /// tire rien, et la payer se voit tout de suite — le ciel, quatre-vingts
    /// mètres de côté, faisait à lui seul cinquante mille triangles en
    /// panneaux d'un demi-mètre, soit trois fois la capacité d'une image.
    fn plane(&mut self, face: Face, density: f32) {
        let along = length(face.along);
        let across = length(face.across);
        let corner = |s: f32, t: f32| VertexUv2 {
            position: face.point(s, t),
            u: s * along * density,
            v: t * across * density,
            u2: if s > 0.0 { 1.5 } else { 0.5 },
            v2: if t > 0.0 { 1.5 } else { 0.5 },
        };
        self.quad([
            corner(0.0, 0.0),
            corner(1.0, 0.0),
            corner(1.0, 1.0),
            corner(0.0, 1.0),
        ]);
    }

    /// Pave une face de panneaux, dans **les deux directions**.
    ///
    /// C'est l'éclairage qui l'impose : l'atténuation étant par sommet, une
    /// face d'un seul quadrilatère ne porterait la lumière qu'à ses quatre
    /// coins. Chaque panneau reçoit ses coordonnées de texture — en texels,
    /// aux densités du décor — et celles de la lightmap, qui couvrent la face
    /// entière et non le panneau : c'est toute la différence entre un
    /// éclairage continu et un damier de panneaux.
    ///
    /// Le demi-texel retranché aux bords de la lightmap place les coins des
    /// panneaux **au centre** des texels extrêmes, là où `bake` les a
    /// calculés. Sans lui, l'interpolation irait chercher au-delà du bord et
    /// la face s'assombrirait sur son pourtour.
    fn pave(&mut self, face: Face, density: f32) {
        let (lw, lh) = face.lightmap_size();
        let along = length(face.along);
        let across = length(face.across);
        let steps = |size: f32| ((size / PANEL).ceil() as u32).max(1);
        let (columns, rows) = (steps(along), steps(across));

        // Les coordonnées de lightmap d'une fraction de la face, en texels.
        let lu = |s: f32| 0.5 + s * (lw as f32 - 1.0);
        let lv = |t: f32| 0.5 + t * (lh as f32 - 1.0);

        for row in 0..rows {
            for column in 0..columns {
                let s0 = column as f32 / columns as f32;
                let s1 = (column + 1) as f32 / columns as f32;
                let t0 = row as f32 / rows as f32;
                let t1 = (row + 1) as f32 / rows as f32;
                let corner = |s: f32, t: f32| VertexUv2 {
                    position: face.point(s, t),
                    u: s * along * density,
                    v: t * across * density,
                    u2: lu(s),
                    v2: lv(t),
                };
                self.quad([
                    corner(s0, t0),
                    corner(s1, t0),
                    corner(s1, t1),
                    corner(s0, t1),
                ]);
            }
        }
    }
}

/// La lightmap d'un objet : un éclairement unique, pris à sa position.
///
/// **C'est ce qu'un objet mobile reçoit** dans un moteur de cette famille : il
/// n'a pas de lightmap à lui, il échantillonne celle du décor là où il se
/// trouve. Deux texels de côté parce que le moteur lit une lightmap en
/// bilinéaire inconditionnel — un seul texel n'aurait pas de voisin — et la
/// valeur est la même partout, si bien que l'interpolation ne change rien.
fn object_light(position: Vec3) -> Result<Texture, screengine_play::screengine::Error> {
    let light = lit_at(position);
    let mut texel = [0u8; 4];
    for (slot, channel) in texel.iter_mut().zip(light) {
        let scaled = channel * 255.0 + 0.5;
        let clamped = if scaled > 255.0 { 255.0 } else { scaled };
        *slot = clamped as u8;
    }
    texel[3] = 0xFF;
    let texels: Vec<u8> = texel.iter().copied().cycle().take(4 * 4).collect();
    Texture::load(2, 2, &texels)
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

/// Le décor : la maçonnerie éclairée, le sol éclairé, les caisses.
///
/// Construit une fois, lightmaps comprises : la géométrie ne change pas, seule
/// la caméra bouge. Ce que le moteur reçoit chaque image, c'est la même scène
/// et une caméra différente.
///
/// **Les lightmaps sont cuites ici**, pas chargées. À cette étape le moteur
/// n'en calcule aucune — il n'a ni carte ni cellule pour cela, qui viennent
/// plus tard — et l'ABI en fait une ressource que l'hôte fournit. Les cuire
/// dans l'exemple montre donc exactement ce que l'étape du monde fera dans le
/// noyau, et ce que n'importe quel intégrateur peut faire aujourd'hui.
fn corridor() -> Result<Decor, screengine_play::Error> {
    let (w, h, l) = (HALF_WIDTH, HEIGHT, LENGTH);
    let (stone, cobble) = (STONE_DENSITY, COBBLE_DENSITY);
    let span = l - START;

    // Les cinq faces du couloir, chacune décrite par un coin et ses deux axes.
    // Le sens des axes décide du sens de la face : celle qu'on voit de
    // l'intérieur tourne dans l'autre sens que celle qu'on verrait de dehors.
    let floor_face = Face {
        origin: Vec3::new(START, -w, 0.0),
        along: Vec3::new(span, 0.0, 0.0),
        across: Vec3::new(0.0, 2.0 * w, 0.0),
    };
    let ceiling_face = Face {
        origin: Vec3::new(START, w, h),
        along: Vec3::new(span, 0.0, 0.0),
        across: Vec3::new(0.0, -2.0 * w, 0.0),
    };
    let left_face = Face {
        origin: Vec3::new(START, -w, h),
        along: Vec3::new(span, 0.0, 0.0),
        across: Vec3::new(0.0, 0.0, -h),
    };
    let right_face = Face {
        origin: Vec3::new(l, w, h),
        along: Vec3::new(-span, 0.0, 0.0),
        across: Vec3::new(0.0, 0.0, -h),
    };
    // Le couloir ne se ferme pas : il **débouche**. Un mur au fond arrêterait
    // le regard à quarante mètres, là où une ouverture donne la respiration
    // qui fait sentir la longueur qu'on vient de parcourir — et c'est elle qui
    // fait travailler les mipmaps de lightmap, une surface s'y éloignant
    // vraiment au-delà de ce qu'un intérieur permet.
    let sky_face = Face {
        origin: Vec3::new(l + SKY_DISTANCE, -SKY_SIDE, SKY_SIDE),
        along: Vec3::new(0.0, 2.0 * SKY_SIDE, 0.0),
        across: Vec3::new(0.0, 0.0, -2.0 * SKY_SIDE),
    };

    // Un lot par face, et non un lot par matière : chaque face porte **sa**
    // lightmap, et un lot ne traverse la frontière qu'avec une seule.
    let mut walls = Vec::new();
    for face in [ceiling_face, left_face, right_face] {
        let mut mesh = LitMesh::default();
        mesh.pave(face, stone);
        walls.push((mesh, Arc::new(face.bake()?)));
    }

    let mut floor = LitMesh::default();
    floor.pave(floor_face, cobble);

    // Une caisse, un lot : chacune porte sa propre lightmap d'un texel, prise
    // là où elle se trouve. Cinq lots de plus par image, ce qui ne se mesure
    // pas, contre des caisses qui resteraient à leur couleur pleine au milieu
    // d'un couloir noir.
    let mut crates = Vec::new();
    for (x, y, z) in CRATES {
        let mut mesh = Mesh::default();
        crate_at(&mut mesh, x, y, z);
        let center = Vec3::new(x, y, z + CRATE / 2.0);
        crates.push((LitMesh::from_object(mesh), Arc::new(object_light(center)?)));
    }

    // Le ciel ne reçoit pas les lampes : il est à cinquante mètres, hors de
    // portée de toutes. Sa lightmap est donc pleine — le texel ressort intact,
    // et c'est ce qui le distingue du décor, qui lui s'éteint.
    let mut sky = LitMesh::default();
    sky.plane(sky_face, SKY_DENSITY);

    let decor = Decor {
        walls,
        floor: (floor, Arc::new(floor_face.bake()?)),
        crates,
        sky: (sky, Arc::new(full_light()?)),
    };
    Ok(decor)
}

/// Une lightmap pleine : ce qu'une surface hors de portée de toute lampe doit
/// recevoir pour rendre sa texture intacte.
fn full_light() -> Result<Texture, screengine_play::screengine::Error> {
    Texture::load(2, 2, &[0xFF; 16])
}

/// Le décor éclairé : les faces de maçonnerie et le sol, chacun avec sa
/// lightmap.
struct Decor {
    /// Plafond, murs et fond, chacun avec la sienne.
    walls: Vec<(LitMesh, Arc<Texture>)>,
    /// Le sol, qui porte l'autre texture.
    floor: (LitMesh, Arc<Texture>),
    /// Les caisses, chacune avec son éclairement d'un texel.
    crates: Vec<(LitMesh, Arc<Texture>)>,
    /// Le ciel au bout du couloir, à lightmap pleine.
    sky: (LitMesh, Arc<Texture>),
}

/// Ce que la mise à jour fait avancer d'un pas à l'autre.
struct World {
    /// La caméra, à hauteur d'œil.
    camera: FreeCamera,
    /// Le filtrage courant, que `F` bascule.
    ///
    /// Il part du tramage, qui est le défaut du moteur : l'exemple montre
    /// d'abord ce qu'un hôte obtient sans rien configurer.
    filter: Filter,
}

/// Le titre de la fenêtre, qui porte les commandes et le filtrage actif.
///
/// Les deux ensemble, et là plutôt que dans la console : c'est le seul endroit
/// qu'on regarde sans lâcher la souris, et le seul où un jeu peut écrire tant
/// que le moteur ne dessine pas de texte.
fn title(filter: Filter) -> &'static str {
    match filter {
        Filter::Bilinear => "Couloir — F : bilinéaire · Z Q S D / flèches · clic : souris",
        _ => "Couloir — F : tramage · Z Q S D / flèches · clic : souris",
    }
}

/// Ouvre la fenêtre et parcourt le couloir.
fn main() -> Result<(), screengine_play::Error> {
    let decor = corridor()?;
    let stone = Arc::new(load_png(include_bytes!("../assets/mur-mousse.png"))?);
    let cobble = Arc::new(load_png(include_bytes!("../assets/sol-pave-mousse.png"))?);
    let sky = Arc::new(load_png(include_bytes!("../assets/ciel-orageux.png"))?);
    let wood = Arc::new(load_png(include_bytes!("../assets/wood.png"))?);
    let world = World {
        camera: FreeCamera::new(Vec3::new(0.0, 0.0, 1.6)),
        filter: Filter::Dither,
    };
    // L'indication vit dans la barre de titre et non dans la console : c'est
    // là qu'on la cherche quand on ne sait plus comment récupérer sa souris,
    // et elle y reste visible pendant que le curseur est pris.
    Play::new().title(title(world.filter)).run(
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
            if tick.input().pressed(KeyCode::KeyF) {
                world.filter = match world.filter {
                    Filter::Bilinear => Filter::Dither,
                    _ => Filter::Bilinear,
                };
                tick.set_title(title(world.filter));
            }
            world.camera.update(tick);
        },
        move |world, context| {
            // Un refus ne peut venir que de la capacité, que cette scène
            // n'approche pas ; le laisser passer vaut mieux qu'arrêter la
            // boucle sur une image manquante.
            let _ = context.set_camera(world.camera.camera());
            let _ = context.set_filter(world.filter);

            // Un lot par face : chacune porte sa lightmap, et un lot n'en
            // traverse qu'une.
            for (mesh, lightmap) in &decor.walls {
                let _ = context.submit_lit(
                    Affine3::IDENTITY,
                    &mesh.vertices,
                    &mesh.faces,
                    Some(&stone),
                    lightmap,
                );
            }
            let (mesh, lightmap) = &decor.floor;
            let _ = context.submit_lit(
                Affine3::IDENTITY,
                &mesh.vertices,
                &mesh.faces,
                Some(&cobble),
                lightmap,
            );

            // Le ciel en premier : il est au fond, et le tampon de profondeur
            // se charge du reste — l'ordre de soumission ne décide de rien
            // d'autre qu'une égalité de profondeur, qui n'arrive pas ici.
            let (mesh, lightmap) = &decor.sky;
            let _ = context.submit_lit(
                Affine3::IDENTITY,
                &mesh.vertices,
                &mesh.faces,
                Some(&sky),
                lightmap,
            );

            // Chaque caisse porte sa propre lightmap d'un texel : un objet
            // n'en a pas à lui, il échantillonne celle du décor là où il se
            // trouve.
            for (mesh, lightmap) in &decor.crates {
                let _ = context.submit_lit(
                    Affine3::IDENTITY,
                    &mesh.vertices,
                    &mesh.faces,
                    Some(&wood),
                    lightmap,
                );
            }
        },
    )
}
