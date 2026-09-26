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
//! **Les deux sources de lumière du moteur s'y partagent le travail, et le
//! partage suit ce qui bouge.** La lightmap, cuite par l'exemple, porte le jour
//! qui tombe par les trouées du plafond — il ne bougera jamais. Les douze tubes
//! sont des lumières dynamiques, parce qu'ils battent. Cuire ce qui clignote
//! donnerait une flaque peinte sur le mur, calculer par sommet ce qui ne bouge
//! pas paierait une atténuation par image pour une valeur constante.
//!
//! À cette étape le moteur ne produit aucune lightmap — il n'a ni carte ni
//! cellule pour cela — et l'ABI en fait une ressource que l'hôte fournit : les
//! cuire ici montre donc ce que l'étape du monde fera dans le noyau, et ce
//! qu'un intégrateur peut faire dès aujourd'hui.
//!
//! **La géométrie est subdivisée par l'éclairage, pas par la forme.**
//! L'atténuation se calcule par sommet : un mur d'un seul quadrilatère ne
//! porterait la lumière qu'à ses quatre coins. Les faces se pavent donc en
//! panneaux d'un demi-mètre — sauf ce qu'aucune source n'atteint, comme les
//! panneaux de ciel, qui se posent d'un seul tenant et l'ont d'abord payé de
//! cinquante mille triangles.
//!
//! **Une malle n'a pas de lightmap à elle**, elle échantillonne celle du
//! décor à sa position : un texel unique, ce qu'un objet mobile reçoit dans un
//! moteur de cette famille.
//!
//! **Les tubes battent, chacun pour soi.** Un tube fatigué ne clignote pas
//! régulièrement : il tient, lâche deux ou trois fois de suite, se rétablit. Le
//! battement est une somme de trois ondes de périodes incommensurables,
//! seuillée — aucun générateur pseudo-aléatoire, et la même séquence d'une
//! machine à l'autre, puisqu'elle suit le rang du pas et non une horloge. Une
//! phase par tube les décorrèle, une usure décide de la fréquence de ses
//! extinctions, et **le moteur n'en accepte que huit par image** : l'exemple
//! retient les huit plus proches de la caméra. Éteint, un tube est **retiré**
//! plutôt que mis à rayon nul, que le moteur refuse.
//!
//! **Le sur-éclairement reste à zéro et la courbe assombrit**, ce qui est
//! l'inverse de ce qu'on croit devoir faire sur un décor sombre. À un, le
//! sur-éclairement double une lightmap qui atteint déjà l'unité sous une
//! trouée : le couloir se lave, la mousse vire au vert vif et les flaques
//! d'ombre disparaissent. Ce qui fait le registre est le contraste entre les
//! flaques et le noir, jamais la quantité de lumière.
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
    Affine3, Color, Filter, FreeCamera, KeyCode, Light, MouseButton, Play, Texture, Triangle, Vec3,
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
/// défilerait comme un mur trahirait qu'elle en est un —, assez près pour que
/// sortir du couloir ne donne pas sur du vide : à cinquante mètres, le
/// brouillard l'effaçait entièrement et le bout du couloir était un trou noir.
const SKY_DISTANCE: f32 = 25.0;

/// À quelle hauteur au-dessus du plafond se tient le ciel qu'on voit par les
/// trous.
///
/// **C'est lui qui porte vraiment la texture de ciel**, et non celui du fond :
/// à huit mètres il n'est presque pas embrumé, là où l'autre est à soixante-
/// cinq et s'y noie. Le fond donne une ouverture pâle, les trous donnent le
/// ciel.
const SKY_ABOVE: f32 = 8.0;

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

/// Texels de tôle par unité de monde : une répétition exacte par face de
/// malle, soit des rivets espacés de quelques centimètres.
///
/// La malle a remplacé une caisse de bois, et c'est une mesure qui l'a décidé :
/// la planche avait une luminance moyenne de 180 dans un couloir dont le mur
/// est à 33, si bien qu'elle ressortait comme une lampe. La tôle est à 41 —
/// assez proche du décor pour lui appartenir, assez au-dessus pour s'en
/// détacher là où la lumière l'atteint. Ses rivets et ses coulures de rouille
/// sont en outre des formes à grande échelle, qui survivent au mipmap là où le
/// grain du bois disparaissait au deuxième niveau.
const PLATE_DENSITY: f32 = 512.0 / CRATE;

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

/// Un tube du couloir.
struct Tube {
    /// Son abscisse. Il est au plafond, au milieu de la largeur.
    x: f32,
    /// Sa teinte à pleine intensité, qui porte aussi son intensité : c'est
    /// elle qui traverse l'ABI, et le moteur n'a pas de facteur à côté.
    tint: [u8; 3],
    /// Le décalage de son battement. Sans lui, tout le couloir lâcherait
    /// ensemble, ce qui se lit comme une coupure de courant et non comme une
    /// rangée de tubes fatigués.
    phase: f32,
    /// Son usure, de zéro — un tube qui ne bronche qu'à peine — à un, un tube
    /// qui ne s'allume plus que par saccades.
    wear: f32,
}

impl Tube {
    /// Un tube, dans l'ordre de la déclaration.
    ///
    /// Un constructeur pour ce qui pourrait s'écrire en littéral : `rustfmt`
    /// éclate un littéral de structure sur six lignes, et la rangée de tubes
    /// cesse alors de se lire comme la table qu'elle est.
    const fn new(x: f32, tint: [u8; 3], phase: f32, wear: f32) -> Self {
        Self {
            x,
            tint,
            phase,
            wear,
        }
    }
}

/// Les tubes du couloir.
///
/// **Tout ce qui éclaire le couloir est dynamique**, et tout bat. Les lampes
/// cuites que la lightmap portait ont disparu : elles donnaient des flaques
/// fixes, et une flaque fixe au milieu de tubes qui clignotent se voit
/// immédiatement comme peinte sur le mur. Ce que la lightmap porte désormais
/// est le jour qui tombe des trouées, qui lui ne bouge pas — et c'est la seule
/// lumière dont l'immobilité se justifie.
///
/// Un tube tous les trois mètres et demi : assez pour que le couloir soit
/// équipé sur toute sa longueur, et ce sont **les tubes morts** qui rendent le
/// noir entre deux, pas l'espacement. C'est la même image, obtenue par ce qui
/// la cause plutôt que par ce qui la simule.
const TUBES: [Tube; 12] = [
    Tube::new(-1.0, [0x62, 0x86, 0x9E], 0.0, 0.20),
    Tube::new(2.5, [0x70, 0x8C, 0xA4], 1.7, 0.90),
    Tube::new(6.0, [0x5C, 0x84, 0xA0], 0.6, 0.05),
    Tube::new(9.5, [0x6E, 0x92, 0xA8], 2.3, 1.00),
    Tube::new(13.0, [0x64, 0x88, 0x9C], 3.1, 0.30),
    Tube::new(16.5, [0x78, 0x90, 0x9A], 0.9, 1.00),
    Tube::new(20.0, [0x5E, 0x82, 0xA2], 4.2, 0.10),
    Tube::new(23.5, [0x6A, 0x8E, 0xA6], 1.2, 0.70),
    Tube::new(27.0, [0x72, 0x8A, 0x96], 2.8, 1.00),
    Tube::new(30.5, [0x60, 0x86, 0xA0], 3.7, 0.25),
    Tube::new(34.0, [0x6C, 0x90, 0xA4], 0.3, 0.45),
    Tube::new(37.5, [0x66, 0x88, 0x9E], 4.9, 0.15),
];

/// Portée d'un tube.
///
/// Un peu plus que l'espacement : deux tubes voisins se rejoignent, de sorte
/// qu'un tube qui lâche laisse une pénombre et non un trou net, qui trahirait
/// la portée sphérique.
const TUBE_REACH: f32 = 5.5;

/// De combien un tube pend sous le plafond.
const TUBE_DROP: f32 = 0.15;

/// Le moteur n'accepte que huit lumières dynamiques par image, et il y a douze
/// tubes : chaque image retient **les huit plus proches de la caméra**.
///
/// Le tri ne se voit pas, et ce n'est pas une approximation : au-delà de la
/// portée un tube n'ajoute rien, et un point du couloir n'est jamais à portée
/// de plus de quatre. Ce qui est écarté est ce qui ne rendait rien.
const LIVE_TUBES: usize = 8;

/// Les trouées du plafond, à leur abscisse et à leur écart de l'axe.
///
/// **C'est par elles que le ciel se voit**, et c'est elles qui donnent au
/// couloir la raison d'être dans cet état. Quatre seulement : une trouée tous
/// les dix mètres laisse de longues portions sous les seuls tubes, et c'est
/// l'alternance du jour et du néon qui fait la longueur.
///
/// La dernière n'était pas là au premier essai, et son absence se voyait : les
/// dix derniers mètres n'avaient ni trouée ni tube sain, si bien qu'on
/// traversait un noir complet avant de déboucher sur une ouverture éblouissante
/// — un contraste que l'œil lit comme un défaut d'éclairage et non comme une
/// intention.
const HOLES: [(f32, f32); 4] = [(5.5, -0.4), (14.0, 0.5), (27.0, -0.2), (35.5, 0.3)];

/// Côté d'une trouée, en mètres.
const HOLE: f32 = 1.2;

/// La teinte du jour qui tombe par une trouée.
///
/// Prise sur les éclaircies du ciel lui-même — 208, 193, 164 en moyenne, une
/// lueur d'ocre sale — et non sur un blanc froid : le jour et les tubes
/// doivent se départager à l'œil, et c'est l'écart de teinte qui le fait, pas
/// celui d'intensité.
const DAY_TINT: [f32; 3] = [1.00, 0.93, 0.79];

/// Jusqu'où le jour porte sous une trouée.
const DAY_REACH: f32 = 6.5;

/// La couleur du brouillard, et la rampe sur laquelle il s'épaissit.
///
/// **Gris pâle et froid, et non presque noir**, ce qui est l'inverse du
/// premier réglage : à six sur sept sur dix, le fond du couloir rendait 12 sur
/// un premier plan à 23, c'est-à-dire plus sombre que ce qui est près. Un fond
/// plus sombre que le reste ne se lit pas comme du brouillard mais comme un
/// trou. À soixante-dix-huit sur quatre-vingt-huit sur cent quatre, la même
/// ouverture rend 37 et les dernières malles s'y découpent en silhouette, sans
/// qu'un seul point ne bouge au premier plan.
const FOG_COLOR: Color = Color::new(0x4E, 0x58, 0x68, 0xFF);
const FOG_START: f32 = 10.0;
const FOG_END: f32 = 55.0;

/// Ce qu'une surface reçoit là où ni le jour ni un tube ne porte.
///
/// Pas zéro : un noir absolu effacerait la texture au lieu de l'assombrir, et
/// un couloir dont on ne devine plus les murs n'est pas lugubre, il est vide.
const AMBIENT: f32 = 0.06;

/// Calcule l'éclairement d'un point : l'ambiance, et le jour des trouées.
///
/// **C'est ce que l'étape 5 fera dans le noyau**, cellule par cellule. Ici
/// l'hôte s'en charge, comme l'ABI le prévoit à cette étape : une lightmap est
/// une ressource qu'il fournit, le moteur ne fait que l'échantillonner.
///
/// L'atténuation est celle du moteur pour ses lumières dynamiques —
/// `(1 - d²/r²)²`, nulle et de dérivée nulle à la portée —, de sorte que les
/// tubes se fondent dans le même éclairage plutôt que de s'y superposer comme
/// un corps étranger.
fn lit_at(point: Vec3) -> [f32; 3] {
    let mut sum = [AMBIENT; 3];
    for (x, y) in HOLES {
        let hole = Vec3::new(x, y, HEIGHT);
        let (dx, dy, dz) = (point.x - hole.x, point.y - hole.y, point.z - hole.z);
        let square = dx * dx + dy * dy + dz * dz;
        let reach = DAY_REACH * DAY_REACH;
        if square >= reach {
            continue;
        }
        let falloff = 1.0 - square / reach;
        let falloff = falloff * falloff;
        // **Le jour tombe, il ne rayonne pas.** Une source ponctuelle posée
        // dans la trouée éclairerait le plafond autour d'elle autant que le sol
        // dessous, et la trouée se lirait comme une ampoule encastrée. Le
        // facteur va de zéro dans le plan du plafond à un au sol : ce qui reste
        // est la colonne de lumière, et le plafond garde son noir jusqu'au bord
        // du trou.
        let fall = (hole.z - point.z) / HEIGHT;
        let fall = if fall < 0.0 { 0.0 } else { fall };
        for (channel, value) in sum.iter_mut().zip(DAY_TINT) {
            *channel += value * falloff * fall;
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
                normal: Vec3::ZERO,
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
            normal: Vec3::ZERO,
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
        self.pave_holed(face, density, |_| false);
    }

    /// La même, en sautant les panneaux dont le centre satisfait `hole`.
    ///
    /// C'est ainsi que le plafond se troue : rien à découper, rien à
    /// retriangulariser. Le bord d'une trouée suit donc la grille des panneaux
    /// et non un rectangle exact — ce qui convient à un plafond effondré, et ne
    /// conviendrait pas à une ouverture maçonnée.
    fn pave_holed(&mut self, face: Face, density: f32, hole: impl Fn(Vec3) -> bool) {
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
                if hole(face.point((s0 + s1) / 2.0, (t0 + t1) / 2.0)) {
                    continue;
                }
                let corner = |s: f32, t: f32| VertexUv2 {
                    position: face.point(s, t),
                    u: s * along * density,
                    v: t * across * density,
                    u2: lu(s),
                    v2: lv(t),
                    normal: Vec3::ZERO,
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

/// L'intensité d'un tube à l'instant `time`, entre zéro et un.
///
/// **Un tube fatigué ne clignote pas régulièrement.** Il tient allumé, puis
/// lâche quelques fois de suite, puis se rétablit — et c'est l'irrégularité qui
/// fait le registre. Une alternance périodique se lirait comme un gyrophare.
///
/// La forme est une somme de trois battements de périodes incommensurables,
/// seuillée : elle ne se répète jamais à l'identique sur la durée d'une
/// animation, sans qu'aucun générateur pseudo-aléatoire n'entre ici. La phase
/// décale la même forme d'un tube à l'autre, ce qui coûte une addition là où
/// douze formes distinctes coûteraient douze réglages à accorder entre eux.
///
/// **C'est le seuil que l'usure déplace, pas l'amplitude.** Un tube n'éclaire
/// pas moins en vieillissant, il s'éteint plus souvent : à usure nulle le seuil
/// est sous le minimum de la somme et le tube ne bronche qu'à peine, à usure
/// pleine il ne repasse au-dessus que par saccades.
fn neon_level(time: f32, phase: f32, wear: f32) -> f32 {
    let t = time + phase;
    let wave = |period: f32| (t * core::f32::consts::TAU / period).sin();
    let mix = wave(0.37) * 0.5 + wave(1.13) * 0.3 + wave(2.9) * 0.2;
    // Le seuil coupe franchement : un tube s'éteint, il ne s'estompe pas.
    if mix < -0.95 + wear * 1.45 {
        return 0.0;
    }
    // Au-dessus, une variation d'intensité qui reste haute — le tube vacille
    // sans jamais retrouver tout à fait sa pleine lumière.
    0.72 + mix * 0.28
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
    let d = PLATE_DENSITY;
    // Sur les côtés, la texture descend avec la malle : `v` se compte depuis
    // le dessus, faute de quoi les rangées de rivets sont la tête en bas.
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
    // Le ciel qu'on voit par les trouées, à plat au-dessus du plafond. Il
    // déborde largement le couloir : ce qu'on en voit par un trou d'un mètre
    // dépend de l'angle du regard, et un panneau juste assez grand se
    // terminerait dans le champ dès qu'on s'écarte de la verticale.
    let above_face = Face {
        origin: Vec3::new(START - SKY_SIDE, SKY_SIDE, h + SKY_ABOVE),
        along: Vec3::new(span + SKY_DISTANCE + 2.0 * SKY_SIDE, 0.0, 0.0),
        across: Vec3::new(0.0, -2.0 * SKY_SIDE, 0.0),
    };
    // Le dehors, du bout du couloir jusqu'au ciel du fond. Sans lui, sortir
    // donnait sur rien : le regard passait sous l'horizon et ne rencontrait
    // aucune surface, donc la couleur du brouillard et pas un repère.
    let outside_face = Face {
        origin: Vec3::new(l, -SKY_SIDE, 0.0),
        along: Vec3::new(SKY_DISTANCE, 0.0, 0.0),
        across: Vec3::new(0.0, 2.0 * SKY_SIDE, 0.0),
    };

    // Un lot par face, et non un lot par matière : chaque face porte **sa**
    // lightmap, et un lot ne traverse la frontière qu'avec une seule.
    let mut walls = Vec::new();
    for face in [left_face, right_face] {
        let mut mesh = LitMesh::default();
        mesh.pave(face, stone);
        walls.push((mesh, Arc::new(face.bake()?)));
    }

    // Le plafond, lui, se pave troué.
    let mut ceiling = LitMesh::default();
    ceiling.pave_holed(ceiling_face, stone, |p| {
        HOLES
            .iter()
            .any(|(x, y)| (p.x - x).abs() < HOLE / 2.0 && (p.y - y).abs() < HOLE / 2.0)
    });
    walls.push((ceiling, Arc::new(ceiling_face.bake()?)));

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

    // Ni le ciel ni le dehors ne reçoivent quoi que ce soit : ils sont hors de
    // portée des tubes comme des trouées. Leur lightmap est donc pleine — le
    // texel ressort intact, et c'est ce qui les distingue du couloir, qui lui
    // s'éteint. Un seul quadrilatère chacun : l'éclairage n'y variant pas, la
    // subdivision n'aurait rien à porter, et le ciel du dessus pavé au demi-
    // mètre coûtait à lui seul plus de triangles que tout le couloir.
    let mut sky = LitMesh::default();
    sky.plane(sky_face, SKY_DENSITY);
    sky.plane(above_face, SKY_DENSITY);
    let mut outside = LitMesh::default();
    outside.plane(outside_face, cobble);

    let decor = Decor {
        walls,
        floor: (floor, Arc::new(floor_face.bake()?)),
        crates,
        sky: (sky, Arc::new(full_light()?)),
        outside: (outside, Arc::new(full_light()?)),
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
    /// Les deux panneaux de ciel — celui du fond et celui du dessus —, à
    /// lightmap pleine.
    sky: (LitMesh, Arc<Texture>),
    /// Le sol du dehors, de même.
    outside: (LitMesh, Arc<Texture>),
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
    /// Le temps écoulé, qui fait battre le néon.
    ///
    /// Compté en pas de la boucle à pas fixe et non par une horloge : deux
    /// machines de vitesses différentes voient alors le même battement, ce
    /// qu'une mesure de temps réel ne garantirait pas.
    time: f32,
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
    let sky = Arc::new(load_png(include_bytes!("../assets/ciel-jour.png"))?);
    let plate = Arc::new(load_png(include_bytes!("../assets/malle-rouillee.png"))?);
    let world = World {
        camera: FreeCamera::new(Vec3::new(0.0, 0.0, 1.6)),
        filter: Filter::Dither,
        time: 0.0,
    };
    // Le tampon des lumières vit hors de la boucle et se vide par `clear` :
    // l'hôte n'est tenu par aucun invariant du noyau, mais réallouer douze
    // lumières soixante fois par seconde n'a pas d'excuse non plus.
    let mut lights: Vec<Light> = Vec::with_capacity(TUBES.len());
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
            // L'index du pas et non un temps cumulé : c'est lui qui rejoue une
            // partie à l'identique, et le battement du néon doit en être.
            world.time = tick.index() as f32 * tick.dt();
            world.camera.update(tick);
        },
        move |world, context| {
            // Un refus ne peut venir que de la capacité, que cette scène
            // n'approche pas ; le laisser passer vaut mieux qu'arrêter la
            // boucle sur une image manquante.
            let _ = context.set_camera(world.camera.camera());
            let _ = context.set_filter(world.filter);

            // Les tubes, réglés **avant** toute soumission : une lumière posée
            // après n'éclaire pas le lot déjà passé. Un tube éteint est
            // **retiré** plutôt que mis à rayon nul — que le moteur refuse, et
            // qui coûterait de toute façon une atténuation par sommet pour
            // rien.
            lights.clear();
            for tube in &TUBES {
                let level = neon_level(world.time, tube.phase, tube.wear);
                if level <= 0.0 {
                    continue;
                }
                let tint = |channel: u8| (f32::from(channel) * level) as u8;
                lights.push(Light {
                    position: Vec3::new(tube.x, 0.0, HEIGHT - TUBE_DROP),
                    radius: TUBE_REACH,
                    color: Color::new(
                        tint(tube.tint[0]),
                        tint(tube.tint[1]),
                        tint(tube.tint[2]),
                        0xFF,
                    ),
                });
            }
            if lights.len() > LIVE_TUBES {
                // Les tubes ne diffèrent que par leur abscisse : l'écart en x
                // suffit à les ordonner, et évite une racine par tube.
                let eye = world.camera.camera().position.x;
                lights.sort_by(|a, b| {
                    (a.position.x - eye)
                        .abs()
                        .total_cmp(&(b.position.x - eye).abs())
                });
                lights.truncate(LIVE_TUBES);
            }
            let _ = context.set_lights(&lights);

            // Le brouillard mange le fond : on ne voit pas où le couloir
            // s'arrête, et le ciel s'y fond au lieu de se découper.
            let _ = context.set_fog(FOG_COLOR, FOG_START, FOG_END);
            // **Le sur-éclairement reste à zéro**, et c'est le réglage que
            // l'image a corrigé : à un, il double une lightmap qui atteint
            // déjà l'unité sous une trouée, et le couloir entier se lave — la
            // mousse vire au vert vif, les flaques d'ombre disparaissent, il
            // ne reste qu'un couloir uniformément éclairé. Le registre tient
            // au contraste entre les flaques et le noir, pas à la quantité de
            // lumière.
            let _ = context.set_overbright(0);
            // La courbe **assombrit** au lieu d'éclaircir — gamma sous un —,
            // refroidit d'un souffle par les gains, et relève le noir d'un
            // rien pour qu'on devine encore les murs hors des flaques.
            let _ = context.set_grade(0.92, [0.92, 0.98, 1.06], [0.012, 0.014, 0.022]);

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
            let (mesh, lightmap) = &decor.outside;
            let _ = context.submit_lit(
                Affine3::IDENTITY,
                &mesh.vertices,
                &mesh.faces,
                Some(&cobble),
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
                    Some(&plate),
                    lightmap,
                );
            }
        },
    )
}
