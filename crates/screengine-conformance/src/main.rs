// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Suite de conformance de Screengine.
//!
//! Rejoue les scènes de référence sans fenêtre, hache chaque tampon rendu et
//! compare l'empreinte à celle de `references/`. Les références sont
//! versionnées : chaque plateforme se compare aux mêmes fichiers, donc toutes
//! les plateformes entre elles — et, puisque chaque hôte compare la sienne au
//! chemin Rust de sa plateforme, tous les hôtes entre eux.
//!
//! Une scène se rend dans chaque configuration qui ne doit pas changer l'image,
//! et toutes se comparent à une seule référence : une couture de tuile ne se
//! voit que dans une configuration.
//!
//! `--print` rend une scène et écrit son empreinte, sans rien comparer : c'est
//! le chemin Rust natif auquel les hôtes comparent la leur.

mod crate_file;
mod hash;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{fs, io};

use std::sync::Arc;

use screengine::{
    Affine3, Angle, BYTES_PER_PIXEL, Color, Config, Context, Filter, Frame, Light, MAX_OVERBRIGHT,
    Mesh, Quat, Rect, Rows, Texture, Triangle, Vec3, VertexUv, VertexUv2,
};

/// Une façon de rendre une scène qui ne doit pas changer l'image.
///
/// Une couture de tuile ne se voit que dans une configuration : chaque scène
/// passe par toutes, et toutes se comparent à une seule référence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// Tuiles de 32, rendues par la seule fin d'image, dans l'ordre.
    Tiles32,
    /// Tuiles de 64, de même. C'est la configuration des hôtes.
    Tiles64,
    /// L'image entière d'un bloc, sans répartition : la référence des tuiles.
    Whole,
    /// Tuiles de 32, une moitié dans un ordre mélangé à graine fixe, l'autre
    /// laissée à la fin d'image.
    Shuffled,
    /// Tuiles de 32, une bande de tuiles par thread.
    Threads,
    /// Tuiles de 64, dans un contexte ouvert au maximum puis redimensionné à
    /// la résolution de la vue.
    ///
    /// Les autres passes ouvrent leur contexte à la résolution de la vue, qui
    /// est aussi son maximum : le cas « sous le maximum » n'y est jamais joué,
    /// et un changement de résolution encore moins. Celle-ci rend la même
    /// image par un contexte dont l'histoire diffère.
    Resized,
}

impl Pass {
    /// Toutes les passes, dans l'ordre où la suite les rejoue.
    const ALL: [Self; 6] = [
        Self::Tiles32,
        Self::Tiles64,
        Self::Whole,
        Self::Shuffled,
        Self::Threads,
        Self::Resized,
    ];

    /// Le nom de la passe, dans un message de divergence.
    fn name(self) -> &'static str {
        match self {
            Self::Tiles32 => "tuiles de 32",
            Self::Tiles64 => "tuiles de 64",
            Self::Whole => "image entière",
            Self::Shuffled => "ordre mélangé",
            Self::Threads => "threads",
            Self::Resized => "redimensionné",
        }
    }

    /// Le côté de tuile du contexte.
    fn tile_size(self) -> u32 {
        match self {
            Self::Tiles64 | Self::Whole | Self::Resized => 64,
            Self::Tiles32 | Self::Shuffled | Self::Threads => 32,
        }
    }

    /// Ouvre le contexte d'une vue, réglages de scène exclus.
    ///
    /// Toutes les passes ouvrent à la résolution de la vue, qui leur sert
    /// aussi de maximum. [`Pass::Resized`] prend pour maximum celui de la
    /// suite et **ouvre en 1×1**, puis redimensionne.
    ///
    /// Ouvrir ailleurs que sur la vue n'est pas un détail : c'est ce qui rend
    /// la passe capable d'attraper une projection laissée périmée. Ouvrir
    /// dessus ferait coïncider la projection d'ouverture et celle qu'on
    /// attend, et le défaut ne se verrait que sur les vues d'une autre
    /// résolution — c'est-à-dire sur la seule scène qui en a plusieurs. 1×1
    /// ne ressemble à aucune vue, et éprouve au passage la plus petite
    /// résolution que l'ABI accepte.
    fn open(self, view: View) -> Result<Context, screengine::Error> {
        let (width, height) = (view.width, view.height);
        let resized = self == Self::Resized;
        let (max_width, max_height) = if resized {
            Scene::RESOLUTION
        } else {
            (width, height)
        };
        let mut context = Context::new(Config {
            max_width,
            max_height,
            width: if resized { 1 } else { max_width },
            height: if resized { 1 } else { max_height },
            tile_size: self.tile_size(),
            max_triangles: 0,
        })?;
        if resized {
            context.set_resolution(width, height)?;
        }
        Ok(context)
    }

    /// Rend une image déjà commencée dans `pixels`, rangé par lignes de `width`.
    fn render(
        self,
        frame: Frame<'_>,
        pixels: &mut [u8],
        width: u32,
        height: u32,
    ) -> screengine::Result<()> {
        match self {
            Self::Tiles32 | Self::Tiles64 | Self::Resized => {
                frame.end(&mut Rows::new(pixels, width))
            }
            Self::Whole => {
                let mut color = vec![0u32; width as usize * height as usize];
                let mut depth = color.clone();
                let image = Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                };
                let mut out = Rows::new(pixels, width);
                frame.region(image, &mut color, &mut depth, &mut out)
            }
            Self::Shuffled => {
                let mut order: Vec<u32> = (0..frame.tile_count()).collect();
                // xorshift à graine fixe : un ordre qui change d'un passage à
                // l'autre ferait d'une divergence un échec qu'on ne rejoue pas.
                let mut state = 0x9E37_79B9_7F4A_7C15u64;
                for i in (1..order.len()).rev() {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    order.swap(i, (state % (i as u64 + 1)) as usize);
                }
                let mut rows = Rows::new(pixels, width);
                for &index in &order[..order.len() / 2] {
                    frame.tile(index, &mut rows)?;
                }
                frame.end(&mut rows)
            }
            Self::Threads => {
                let tile = self.tile_size();
                let columns = width.div_ceil(tile);
                let band = tile as usize * width as usize * BYTES_PER_PIXEL;
                std::thread::scope(|scope| {
                    let workers: Vec<_> = pixels
                        .chunks_mut(band)
                        .enumerate()
                        .map(|(row, chunk)| {
                            let frame = &frame;
                            scope.spawn(move || {
                                let mut rows = Rows::band(chunk, width, row as u32 * tile);
                                (0..columns).try_for_each(|column| {
                                    frame.tile(row as u32 * columns + column, &mut rows)
                                })
                            })
                        })
                        .collect();
                    workers.into_iter().try_for_each(|worker| {
                        worker
                            .join()
                            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                    })
                })?;
                frame.end(&mut Rows::new(pixels, width))
            }
        }
    }
}

/// Ce que la suite fait des empreintes calculées.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    /// Compare aux références et échoue sur toute divergence.
    Check,
    /// Réécrit les références. Une évolution voulue du rendu, jamais une
    /// régression qu'on ferait taire : le commit qui les met à jour est distinct.
    Update,
    /// Rend une scène et écrit son empreinte.
    Print(Scene),
    /// Écrit chaque scène en image, dans un répertoire, pour la regarder.
    ///
    /// Une empreinte ne dit pas ce qu'elle fixe. Figer une référence sans avoir
    /// vu l'image, c'est graver un fond noir ou un mur retourné et s'en
    /// apercevoir à l'étape qui s'en sert.
    Dump(PathBuf),
}

/// Les scènes que la suite sait rendre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scene {
    /// Deux triangles qui partagent une arête, vus de biais.
    ///
    /// La scène que les quatre hôtes décrivent aussi, chacun dans son langage,
    /// en tuiles de 64 : c'est elle qui relie leurs empreintes à celle du
    /// chemin Rust, donc la changer ici sans eux les ferait toutes diverger à
    /// la fois.
    ///
    /// Son arête commune éprouve la propriété la plus coûteuse du moteur, et
    /// ses deux couleurs la rendent visible à l'œil : un trou ou un
    /// recouvrement s'y verrait sans attendre qu'une empreinte le dise.
    Edge,
    /// Un mur si près et si large que ses sommets sortent de la bande de garde.
    ///
    /// C'est le seul chemin qui exerce les quatre plans de garde : sans lui,
    /// une erreur dans leur découpe ne se verrait qu'à l'étape où un couloir se
    /// parcourt, c'est-à-dire trop tard pour savoir d'où elle vient.
    Guard,
    /// Un mur qui déborde largement de l'image, mais reste dans la bande de
    /// garde.
    ///
    /// Le cas complémentaire du précédent : rien n'est découpé, et ce sont les
    /// rectangles de parcours et la répartition par tuile qui bornent le
    /// remplissage. Une tuile qui rejetterait un triangle à tort troue l'image
    /// ici, et nulle part ailleurs.
    Lateral,
    /// Deux murs qui se traversent, et un triangle soumis deux fois.
    ///
    /// L'interpénétration ne se rend juste que par le tampon de profondeur ;
    /// le doublon fixe l'autre moitié du contrat, le test strict qui laisse le
    /// premier soumis gagner à profondeur égale.
    Depth,
    /// Un sol qui commence derrière la caméra et fuit vers l'horizon.
    ///
    /// Le plan proche coupe chacun de ses triangles, et c'est la découpe qui
    /// engendre le plus de sommets par triangle.
    Near,
    /// La scène à arêtes partagées, tournée sur un tour complet et rendue à
    /// trois résolutions internes.
    ///
    /// Le critère de franchissement de l'étape : aucune couture, en rotation,
    /// à toutes les résolutions prévues. Une arête immobile ne prouve rien —
    /// la règle top-left départage quatre cas selon son orientation, et une
    /// scène fixe n'en éprouve qu'un.
    ///
    /// Ses images se hachent toutes dans la même empreinte, dans l'ordre des
    /// vues : une référence par angle désignerait l'angle fautif, mais
    /// porterait quarante-huit fichiers pour une seule scène, et c'est la
    /// comparaison entre passes qui donne déjà cette information.
    Rotation,
    /// Un sol texturé qui fuit vers l'horizon.
    ///
    /// La seule scène où la perspective des coordonnées de texture se voit : un
    /// sol donne le plus fort rapport de profondeur d'un bord à l'autre, donc
    /// c'est là qu'une interpolation affine au lieu de perspective, un segment
    /// mal aligné ou une division au mauvais endroit se lisent à l'œil comme
    /// une déformation du damier.
    ///
    /// **Son motif est écrit ici, pas chargé.** Une empreinte figée sur une
    /// image produite ailleurs serait irreproductible le jour où il faut la
    /// refaire, et un damier serré révèle mieux qu'une photographie ce que ce
    /// chemin peut casser.
    Textured,
    /// Le même sol, échantillonné en bilinéaire.
    ///
    /// **Une scène et non une passe de plus**, et la distinction n'est pas de
    /// forme : les passes d'une scène doivent toutes rendre la même empreinte,
    /// puisqu'elles ne diffèrent que par le découpage. Un filtrage qui rend
    /// délibérément une autre image n'y a donc pas sa place — il lui faut sa
    /// propre référence, elle-même vérifiée dans les cinq passes.
    ///
    /// La géométrie est celle de `texture`, au texel près : c'est ce qui rend
    /// les deux empreintes comparables, et une divergence attribuable au seul
    /// filtrage.
    TexturedBilinear,
    /// Le même sol, éclairé par les seules lumières dynamiques.
    ///
    /// Le bras du remplissage qu'aucune autre scène n'atteint : une surface
    /// **texturée** que des lumières éclairent **sans lightmap**. `lumieres`
    /// pose les lumières sur des surfaces unies, `lumiere` pose une lightmap
    /// sans lumière ; entre les deux, la combinaison la plus courante d'un
    /// décor — un mur texturé sous une torche — n'était rendue nulle part.
    ///
    /// **Le quadrilatère reste entier**, celui de `texture` au texel près, là où
    /// `lumieres` découpe son sol en seize panneaux. L'atténuation étant par
    /// sommet, l'éclairage n'y est donc qu'un dégradé entre quatre coins : c'est
    /// assumé, parce que ce que cette scène doit fixer est le bras de
    /// remplissage, et non ce que le découpage change — `lumieres` en répond
    /// déjà. Découper introduirait une seconde différence avec la scène
    /// doublée, et une divergence ne serait plus attribuable aux lumières.
    ///
    /// La contrepartie est que les rayons doivent couvrir le quadrilatère
    /// entier : voir `Scene::lights`.
    TexturedDynamic,
    /// La même, passée par une courbe de sortie : gamma et gains de canal.
    ///
    /// Elle double `texture` pour la même raison que la précédente — le
    /// post-traitement rend délibérément une autre image, donc il lui faut sa
    /// référence, et la géométrie partagée au texel près rend la divergence
    /// attribuable à lui seul.
    ///
    /// Les gains sont **distincts d'un canal à l'autre** : trois tables
    /// écrites dans le mauvais ordre rendraient une image aux teintes
    /// permutées, qu'un réglage gris laisserait passer.
    Graded,
    /// Un sol texturé et un mur uni, tous deux éclairés par une lightmap.
    ///
    /// Les **deux chemins éclairés** dans la même image : `texel × lightmap`
    /// au sol, `couleur × lightmap` au mur. Le second est le cas qu'on oublie,
    /// parce qu'un décor de démonstration est toujours texturé alors qu'un
    /// décor réel ne l'est pas partout.
    ///
    /// La lightmap est un dégradé **asymétrique en ses deux axes**, et elle
    /// s'étire une fois sur chaque surface là où la texture s'y répète : c'est
    /// la seule disposition où un axe échangé entre les deux jeux de
    /// coordonnées, ou un jeu recopié à la place de l'autre, change l'image.
    Lit,
    /// Un sol qui fuit jusqu'à l'horizon, noyé de brouillard.
    ///
    /// **Le fond est le sujet, autant que le sol.** Un pixel qu'aucun triangle
    /// n'a peint doit sortir de la couleur du brouillard, sans qu'on l'ait
    /// effacé avec : c'est ce qui fait que la géométrie lointaine rejoint le
    /// ciel sans ligne de démarcation. Une couture d'un seul niveau s'y verrait
    /// à l'œil, et l'empreinte la fige.
    ///
    /// La rampe est courte devant la longueur du sol, pour que la moitié
    /// lointaine soit pleinement embrumée et la moitié proche à peine : les
    /// deux bouts du mélange sont alors dans la même image.
    Fog,
    /// Un sol uni et un mur, éclairés par trois lumières de couleurs
    /// différentes.
    ///
    /// **Sans lightmap**, et c'est le point : les lumières doivent éclairer
    /// une surface qui n'en a pas, faute de quoi elles resteraient
    /// inutilisables jusqu'à ce que les cartes en apportent.
    ///
    /// Le sol est découpé en panneaux, parce que l'atténuation se calcule par
    /// sommet : un sol d'un seul quadrilatère ne montrerait qu'un dégradé
    /// entre ses quatre coins, et cette scène doit rendre visible ce que le
    /// découpage change. Trois teintes qui se recouvrent, pour que le mélange
    /// des couleurs soit dans l'image et pas seulement leur somme.
    Lights,
    /// La même, avec le sur-éclairement au maximum.
    ///
    /// Une scène et non une passe, pour la raison qui sépare déjà les deux
    /// scènes texturées : le réglage rend délibérément une autre image, donc
    /// il lui faut sa propre référence. C'est aussi le seul endroit où la
    /// saturation de la combinaison s'exerce sur une image entière.
    LitOverbright,
    /// La même, avec des lumières dynamiques par-dessus la lightmap.
    ///
    /// Le second bras que rien n'atteignait : lightmap **et** lumière dynamique
    /// sur la même surface, ce que fait tout décor cuit où passe une source
    /// mobile. Ses deux surfaces le prennent chacune par son chemin — le sol
    /// texturé, le mur uni —, et c'est la seule image du dépôt où les deux
    /// sources de lumière s'additionnent avant la combinaison au texel.
    ///
    /// La géométrie et la lightmap sont celles de `lumiere`, au texel près :
    /// une divergence n'est donc attribuable qu'aux lumières.
    LitDynamic,
    /// Une caisse **chargée depuis un fichier de maillage**, placée par une
    /// matrice de modèle.
    ///
    /// La seule scène du dépôt dont la géométrie ne soit pas écrite en Rust.
    /// C'est ce qui fait entrer le décodage dans la comparaison entre cibles :
    /// la conformance prouvait que le même rendu donne les mêmes bits, pas qu'un
    /// même fichier décodé sur wasm32 et sur x86_64 donne le même maillage.
    ///
    /// Ses **deux groupes de surface** prennent les deux chemins qu'un maillage
    /// peut prendre : les quatre faces latérales portent un damier, le dessus et
    /// le dessous n'ont pas de texture et se dessinent à la couleur de leurs
    /// triangles. Une soumission qui lierait la même texture à tous les
    /// emplacements rendrait une autre image.
    ///
    /// **La rotation n'est pas décorative** : trois faces sont visibles à la
    /// fois, si bien qu'une matrice de modèle ignorée, ou appliquée après la
    /// vue, ne rendrait pas cette image.
    Crate,
}

/// Un damier de `side` texels de côté, ses cases de `cell`.
///
/// Deux teintes franches et une case qui ne divise pas la texture en deux :
/// c'est le contraste qui rend un décalage d'un texel visible, et l'asymétrie
/// qui empêche un repli fautif de passer pour correct.
fn checker(side: u32, cell: u32) -> Arc<Texture> {
    let mut bytes = Vec::with_capacity((side * side) as usize * 4);
    for v in 0..side {
        for u in 0..side {
            let dark = ((u / cell) + (v / cell)) % 2 == 0;
            // Un liseré sur la première colonne et la première ligne de chaque
            // case : sans lui, un damier reste lisible même décalé d'une case
            // entière, et le raccord ne se verrait pas.
            let edge = u % cell == 0 || v % cell == 0;
            bytes.extend_from_slice(&match (dark, edge) {
                (_, true) => [0xF0, 0xE0, 0xA0, 0xFF],
                (true, false) => [0x30, 0x38, 0x50, 0xFF],
                (false, false) => [0x90, 0x70, 0x50, 0xFF],
            });
        }
    }
    Arc::new(Texture::load(side, side, &bytes).unwrap_or_else(|_| unreachable!()))
}

/// Un quadrilatère texturé, deux triangles partageant une diagonale.
///
/// Les coordonnées de texture sont prises sur les coordonnées de monde
/// multipliées par `density`, en texels : c'est ce qui donne au damier un pas
/// constant au sol, quelle que soit la distance.
fn textured_quad(
    context: &mut Context,
    corners: [Vec3; 4],
    density: f32,
    texture: &Arc<Texture>,
) -> screengine::Result<()> {
    let vertices: Vec<VertexUv> = corners
        .iter()
        .map(|&position| VertexUv {
            position,
            u: position.x * density,
            v: position.y * density,
        })
        .collect();
    let white = Color::new(0xFF, 0xFF, 0xFF, 0xFF);
    let triangles = [
        Triangle {
            indices: [0, 1, 2],
            color: white,
        },
        Triangle {
            indices: [0, 2, 3],
            color: white,
        },
    ];
    context.submit_textured(Affine3::IDENTITY, &vertices, &triangles, texture)
}

/// Un quadrilatère éclairé par une lightmap, texturé ou uni selon `texture`.
///
/// **La lightmap s'étire une fois sur le quadrilatère**, là où la texture s'y
/// répète : c'est le cas d'usage qui justifie un second jeu de coordonnées, et
/// le seul qui rende visible une confusion entre les deux. Les coordonnées
/// vont d'un demi-texel à un demi-texel du bord opposé, pour que le bilinéaire
/// n'aille jamais chercher son voisin par le repli — une lightmap ne se pave
/// pas, contrairement à une texture.
fn lit_quad(
    context: &mut Context,
    corners: [Vec3; 4],
    density: f32,
    texture: Option<&Arc<Texture>>,
    lightmap: &Arc<Texture>,
    color: Color,
) -> screengine::Result<()> {
    let (lo, hi) = (0.5, lightmap.width() as f32 - 0.5);
    let lightmap_uv = [(lo, lo), (hi, lo), (hi, hi), (lo, hi)];
    let vertices: Vec<VertexUv2> = corners
        .iter()
        .zip(lightmap_uv)
        .map(|(&position, (u2, v2))| VertexUv2 {
            position,
            u: position.x * density,
            v: position.y * density,
            u2,
            v2,
        })
        .collect();
    let triangles = [
        Triangle {
            indices: [0, 1, 2],
            color,
        },
        Triangle {
            indices: [0, 2, 3],
            color,
        },
    ];
    context.submit_lit(Affine3::IDENTITY, &vertices, &triangles, texture, lightmap)
}

/// Une lightmap en dégradé, `side` texels de côté.
///
/// **Les deux axes n'y font pas la même chose**, et c'est tout l'objet : le
/// rouge croît avec `u`, le bleu avec `v`, le vert avec leur somme. Un dégradé
/// symétrique — ou pire, un aplat — laisserait passer un axe échangé, une
/// transposition, ou les coordonnées de texture lues à la place des siennes.
///
/// Elle ne descend pas jusqu'au noir : une zone éteinte ne dirait rien de la
/// combinaison, qui y rend zéro quelle que soit sa forme.
fn gradient(side: u32) -> Arc<Texture> {
    let mut bytes = Vec::with_capacity((side * side) as usize * 4);
    let scale = |c: u32| (32 + c * 223 / (side - 1).max(1)) as u8;
    for v in 0..side {
        for u in 0..side {
            bytes.extend_from_slice(&[scale(u), scale((u + v) / 2), scale(v), 0xFF]);
        }
    }
    Arc::new(Texture::load(side, side, &bytes).expect("lightmap valide"))
}

/// Le quadrilatère à arêtes partagées, en coordonnées de monde.
///
/// Les quatre hôtes le décrivent aussi, chacun dans son langage : c'est lui qui
/// relie leurs empreintes à celle du chemin Rust. Son centre est sur l'axe de
/// visée, ce dont la scène en rotation se sert pour le faire tourner sans le
/// sortir du champ.
const EDGE_VERTICES: [Vec3; 4] = [
    Vec3::new(2.0, 2.5, 1.6),
    Vec3::new(3.5, -2.5, 1.6),
    Vec3::new(3.5, -2.5, -1.6),
    Vec3::new(2.0, 2.5, -1.6),
];

/// Ses deux triangles, dont l'arête commune est parcourue dans un sens par le
/// premier et dans l'autre par le second : le cas que la règle top-left doit
/// trancher.
const EDGE_TRIANGLES: [Triangle; 2] = [
    Triangle {
        indices: [0, 2, 1],
        color: Color::new(0xE0, 0xA0, 0x30, 0xFF),
    },
    Triangle {
        indices: [0, 3, 2],
        color: Color::new(0xA0, 0xE0, 0x30, 0xFF),
    },
];

/// Deux gris francs, pour les scènes où la couleur ne porte rien d'autre que
/// la diagonale.
const PLAIN: [Color; 2] = [
    Color::new(0xC0, 0xC0, 0xC0, 0xFF),
    Color::new(0x70, 0x70, 0x70, 0xFF),
];

/// Soumet un quadrilatère plan, en deux triangles qui partagent une diagonale.
///
/// Les quatre coins se donnent dans l'ordre du contour, en sens antihoraire vu
/// de la face avant : un ordre inverse en fait un dos de face, que le moteur
/// élimine, et la scène rend alors un fond noir sans se plaindre.
///
/// Une couleur par triangle, et non une pour le quadrilatère : sans la
/// diagonale, deux quadrilatères qui couvrent l'écran rendent la même image
/// quelles que soient leurs tailles, et deux scènes cessent de se distinguer.
fn quad(context: &mut Context, corners: [Vec3; 4], colors: [Color; 2]) -> screengine::Result<()> {
    context.submit(
        Affine3::IDENTITY,
        &corners,
        &[
            Triangle {
                indices: [0, 1, 2],
                color: colors[0],
            },
            Triangle {
                indices: [0, 2, 3],
                color: colors[1],
            },
        ],
    )
}

/// Soumet un mur perpendiculaire au regard, à `distance` de la caméra, large de
/// `half_y` et haut de `half_z`.
///
/// Sa face avant regarde la caméra. Deux scènes n'en diffèrent que par ces
/// nombres, et c'est voulu : ce qui les sépare est la seule chose qu'elles
/// éprouvent, le franchissement de la bande de garde. Elles laissent l'une
/// comme l'autre du fond visible au-dessus et au-dessous — un mur qui
/// couvrirait tout rendrait la même image, qu'il ait été découpé correctement
/// ou pas du tout.
fn wall(context: &mut Context, distance: f32, half_y: f32, half_z: f32) -> screengine::Result<()> {
    quad(
        context,
        [
            Vec3::new(distance, half_y, -half_z),
            Vec3::new(distance, -half_y, -half_z),
            Vec3::new(distance, -half_y, half_z),
            Vec3::new(distance, half_y, half_z),
        ],
        PLAIN,
    )
}

/// Une image à rendre d'une scène : sa résolution, et l'angle de rotation.
///
/// Une scène fixe n'a qu'une vue ; celle qui tourne en a autant que d'angles
/// et de résolutions, et toutes entrent dans la même empreinte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct View {
    /// Largeur interne, en pixels.
    width: u32,
    /// Hauteur interne, en pixels.
    height: u32,
    /// Le rang de l'angle, sur [`Scene::ANGLES`].
    angle: u32,
}

impl View {
    /// La désignation d'une vue dans un message de divergence.
    fn label(self) -> String {
        format!("{}×{} angle {}", self.width, self.height, self.angle)
    }
}

impl Scene {
    /// Toutes les scènes, dans l'ordre où `--check` les rejoue.
    const ALL: [Self; 16] = [
        Self::Edge,
        Self::Guard,
        Self::Lateral,
        Self::Depth,
        Self::Near,
        Self::Rotation,
        Self::Textured,
        Self::TexturedBilinear,
        Self::TexturedDynamic,
        Self::Graded,
        Self::Lit,
        Self::LitOverbright,
        Self::LitDynamic,
        Self::Fog,
        Self::Lights,
        Self::Crate,
    ];

    /// La passe que `--print` utilise, celle des hôtes.
    const HOST_PASS: Pass = Pass::Tiles64;

    /// La résolution des scènes fixes, qui est celle des hôtes.
    const RESOLUTION: (u32, u32) = (640, 360);

    /// Les résolutions internes que la scène en rotation parcourt.
    ///
    /// Trois formats 16:9, dont celui des hôtes. Une couture qui ne se
    /// montrerait qu'à une résolution donnée — un arrondi qui tombe juste en
    /// 640 de large et faux en 320 — resterait invisible sans elles.
    const RESOLUTIONS: [(u32, u32); 3] = [(640, 360), (480, 270), (320, 180)];

    /// Les angles que la scène en rotation parcourt, sur un tour complet.
    ///
    /// Seize : assez pour que chaque arête passe par l'horizontale, la
    /// verticale et les deux diagonales, qui sont les quatre cas que la règle
    /// top-left départage différemment.
    const ANGLES: u32 = 16;

    /// Le nom de la scène, qui est aussi celui de sa référence.
    fn name(self) -> &'static str {
        match self {
            Self::Edge => "arete",
            Self::Guard => "garde",
            Self::Lateral => "lateral",
            Self::Depth => "profondeur",
            Self::Near => "proche",
            Self::Rotation => "rotation",
            Self::Textured => "texture",
            Self::TexturedBilinear => "texture-bilineaire",
            Self::TexturedDynamic => "texture-dynamique",
            Self::Graded => "gamma",
            Self::Lit => "lumiere",
            Self::LitOverbright => "lumiere-surbrillance",
            Self::LitDynamic => "lumiere-dynamique",
            Self::Fog => "brouillard",
            Self::Lights => "lumieres",
            Self::Crate => "maillage",
        }
    }

    /// Les lumières dynamiques que la scène règle, ou aucune.
    ///
    /// Vide partout ailleurs, et c'est ce qui garde les autres empreintes
    /// inchangées : un contexte qu'on ne configure pas n'éclaire rien.
    fn lights(self) -> Vec<Light> {
        match self {
            // Une lumière à chaque bout de la diagonale du sol, de teintes
            // opposées : leurs contributions décroissent en sens inverse, et
            // l'image porte un dégradé que ni l'une ni l'autre ne donnerait
            // seule. Deux et non trois — le mélange de teintes qui se
            // recouvrent est le sujet de `lumieres`, qui a le découpage pour le
            // montrer.
            //
            // **Les rayons couvrent le quadrilatère entier**, et c'est une
            // contrainte, pas un réglage : l'atténuation se calcule par sommet,
            // ces scènes partagent la géométrie d'un sol de plusieurs dizaines
            // d'unités, et des rayons dimensionnés pour un halo n'auraient
            // atteint aucun de ses quatre coins. La surface s'éteint alors
            // entièrement — vu à l'écriture de ces scènes, où `texture-dynamique`
            // rendait une image noire, que le contrôle de couverture refuse.
            Self::TexturedDynamic => vec![
                Light {
                    position: Vec3::new(10.0, -18.0, 6.0),
                    radius: 70.0,
                    color: Color::new(0xFF, 0xC0, 0x60, 0xFF),
                },
                Light {
                    position: Vec3::new(52.0, 18.0, 6.0),
                    radius: 70.0,
                    color: Color::new(0x40, 0x80, 0xFF, 0xFF),
                },
            ],
            // Les mêmes, ramenées sur le sol plus court de `lumiere` ; la
            // seconde est près du mur, qu'elle éclaire de biais par son bord
            // haut.
            Self::LitDynamic => vec![
                Light {
                    position: Vec3::new(6.0, -12.0, 6.0),
                    radius: 60.0,
                    color: Color::new(0xFF, 0xC0, 0x60, 0xFF),
                },
                Light {
                    position: Vec3::new(38.0, 12.0, 6.0),
                    radius: 60.0,
                    color: Color::new(0x40, 0x80, 0xFF, 0xFF),
                },
            ],
            Self::Lights => vec![
                // Trois teintes primaires, espacées le long du couloir et
                // assez larges pour se recouvrir deux à deux.
                Light {
                    position: Vec3::new(8.0, -3.0, 1.5),
                    radius: 12.0,
                    color: Color::new(0xFF, 0x30, 0x20, 0xFF),
                },
                Light {
                    position: Vec3::new(16.0, 3.0, 1.5),
                    radius: 12.0,
                    color: Color::new(0x20, 0xFF, 0x40, 0xFF),
                },
                Light {
                    position: Vec3::new(24.0, -2.0, 2.5),
                    radius: 14.0,
                    color: Color::new(0x30, 0x50, 0xFF, 0xFF),
                },
            ],
            _ => Vec::new(),
        }
    }

    /// Le brouillard que la scène demande au contexte : sa couleur et sa
    /// rampe, ou rien.
    ///
    /// Éteint partout ailleurs, et c'est ce qui garde les autres empreintes
    /// inchangées : un contexte qu'on ne configure pas ne brume rien.
    fn fog(self) -> Option<(Color, f32, f32)> {
        match self {
            Self::Fog => Some((Color::new(0x30, 0x38, 0x48, 0xFF), 3.0, 14.0)),
            _ => None,
        }
    }

    /// Le sur-éclairement que la scène demande au contexte.
    ///
    /// Zéro partout ailleurs : c'est le défaut du moteur, et une scène qui n'a
    /// rien à dire du réglage doit rendre ce que rend un contexte qu'on ne
    /// configure pas.
    fn overbright(self) -> u32 {
        match self {
            Self::LitOverbright => MAX_OVERBRIGHT,
            _ => 0,
        }
    }

    /// Le filtrage que la scène demande au contexte.
    ///
    /// Le tramage partout ailleurs : c'est le défaut du moteur, et une scène
    /// qui n'a rien à dire du filtrage doit rendre ce que rend un contexte
    /// qu'on ne configure pas.
    fn filter(self) -> Filter {
        match self {
            Self::TexturedBilinear => Filter::Bilinear,
            _ => Filter::Dither,
        }
    }

    /// La courbe de sortie que la scène demande au contexte.
    ///
    /// Neutre partout ailleurs, et c'est ce qui garde les douze autres
    /// empreintes inchangées : un contexte qu'on ne configure pas ne traverse
    /// aucune table.
    ///
    /// Les trois gains et les trois décalages sont **tous distincts** : à
    /// valeurs égales, une permutation des tables rendrait la même image et
    /// l'empreinte ne dirait rien. Le décalage du vert est négatif pour que la
    /// borne basse serve aussi.
    fn grade(self) -> Option<(f32, [f32; 3], [f32; 3])> {
        match self {
            Self::Graded => Some((2.2, [1.15, 1.0, 0.85], [0.04, -0.02, 0.08])),
            _ => None,
        }
    }

    /// Les vues que cette scène rend, dans l'ordre où elles entrent dans
    /// l'empreinte.
    fn views(self) -> Vec<View> {
        let (width, height) = Self::RESOLUTION;
        match self {
            Self::Rotation => Self::RESOLUTIONS
                .iter()
                .flat_map(|&(width, height)| {
                    (0..Self::ANGLES).map(move |angle| View {
                        width,
                        height,
                        angle,
                    })
                })
                .collect(),
            _ => vec![View {
                width,
                height,
                angle: 0,
            }],
        }
    }

    /// Soumet la scène, la caméra restant celle du contexte neuf.
    ///
    /// Les coordonnées sont celles du monde — X vers l'est, Z en haut —, et la
    /// caméra neutre regarde le +X depuis l'origine.
    fn submit(self, context: &mut Context, view: View) -> screengine::Result<()> {
        match self {
            // Le même quadrilatère que `arete`, tourné autour de l'axe de
            // visée. L'axe passe par son centre, donc il reste dans le champ et
            // garde sa fuite en perspective ; ce qui change, c'est
            // l'orientation de chacune de ses arêtes, y compris la commune.
            Self::Rotation => {
                let turn = view.angle as f32 * core::f32::consts::TAU / Self::ANGLES as f32;
                let spin =
                    Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), Angle::from_radians(turn));
                context.submit(
                    Affine3::from_rotation_translation(spin, Vec3::ZERO),
                    &EDGE_VERTICES,
                    &EDGE_TRIANGLES,
                )
            }
            // Le quadrilatère fuit vers la droite, donc chaque pixel a sa
            // propre profondeur, et il déborde à gauche pour que le parcours
            // traite des triangles plus larges que l'image. L'ordre des sommets
            // fait que l'arête commune est parcourue dans un sens par le
            // premier triangle et dans l'autre par le second : c'est le cas que
            // la règle top-left doit trancher.
            Self::Edge => context.submit(Affine3::IDENTITY, &EDGE_VERTICES, &EDGE_TRIANGLES),
            // À une demi-unité de la caméra et dix de large : ses bords partent
            // à plus de six mille pixels du centre, loin au-delà des 4096 de la
            // bande de garde.
            Self::Guard => wall(context, 0.5, 10.0, 0.15),
            // À trois unités et vingt de large : deux mille pixels, donc bien
            // hors de l'image, mais dans la bande — rien n'est découpé ici.
            Self::Lateral => wall(context, 3.0, 20.0, 1.0),
            Self::Depth => {
                // Deux murs inclinés qui se croisent en leur milieu : aucune
                // arête ne marque l'intersection, seule la profondeur la trace.
                quad(
                    context,
                    [
                        Vec3::new(6.0, -4.0, 3.0),
                        Vec3::new(14.0, 4.0, 3.0),
                        Vec3::new(14.0, 4.0, -3.0),
                        Vec3::new(6.0, -4.0, -3.0),
                    ],
                    [
                        Color::new(0xE0, 0x40, 0x40, 0xFF),
                        Color::new(0xA0, 0x30, 0x30, 0xFF),
                    ],
                )?;
                quad(
                    context,
                    [
                        Vec3::new(14.0, -4.0, 3.0),
                        Vec3::new(6.0, 4.0, 3.0),
                        Vec3::new(6.0, 4.0, -3.0),
                        Vec3::new(14.0, -4.0, -3.0),
                    ],
                    [
                        Color::new(0x40, 0x40, 0xE0, 0xFF),
                        Color::new(0x30, 0x30, 0xA0, 0xFF),
                    ],
                )?;
                // Exactement les mêmes sommets, donc exactement la même
                // profondeur : le test strict garde le premier, et un test
                // relâché ferait apparaître le second.
                let corners = [
                    Vec3::new(9.0, -1.0, -1.0),
                    Vec3::new(9.0, 1.0, -1.0),
                    Vec3::new(9.0, 0.0, 1.0),
                ];
                for color in [
                    Color::new(0x20, 0xE0, 0x20, 0xFF),
                    Color::new(0xE0, 0xE0, 0x20, 0xFF),
                ] {
                    context.submit(
                        Affine3::IDENTITY,
                        &corners,
                        &[Triangle {
                            indices: [0, 2, 1],
                            color,
                        }],
                    )?;
                }
                Ok(())
            }
            // Le sol commence derrière la caméra : chaque triangle traverse le
            // plan proche, et aucun n'en sort entier.
            // Un sol à 1,2 unité sous la caméra, de deux unités devant elle
            // jusqu'à soixante : le rapport de profondeur d'un bord à l'autre
            // est de trente, ce qui donne à la perspective de quoi se tromper.
            // Huit texels par unité et des cases de huit texels : une case fait
            // une unité au sol, donc la fuite se lit case par case.
            // Les quatre scènes texturées partagent leur géométrie : seuls le
            // filtrage, la courbe de sortie et les lumières que le contexte
            // porte les séparent, et c'est ce qui rend leurs empreintes
            // comparables.
            Self::Textured | Self::TexturedBilinear | Self::Graded | Self::TexturedDynamic => {
                textured_quad(
                    context,
                    [
                        Vec3::new(2.0, -24.0, -1.2),
                        Vec3::new(60.0, -24.0, -1.2),
                        Vec3::new(60.0, 24.0, -1.2),
                        Vec3::new(2.0, 24.0, -1.2),
                    ],
                    8.0,
                    &checker(64, 8),
                )
            }
            // Un sol et un mur découpés en panneaux, éclairés par trois
            // lumières et **sans aucune lightmap**. Le découpage est le sujet
            // autant que les lumières : l'atténuation étant par sommet, un sol
            // d'un seul quadrilatère ne rendrait qu'un dégradé entre ses
            // quatre coins, et les trois halos seraient invisibles.
            Self::Lights => {
                // Le sol va **au-delà de la portée des trois lumières** : ses
                // derniers panneaux ne sont atteints par aucune, et doivent
                // s'éteindre en continuité avec leurs voisins. Une transition
                // franche y trahirait un triangle traité autrement que les
                // autres, ce qui est exactement le défaut à prévenir.
                let panels = 16;
                let (near, far) = (2.0f32, 50.0f32);
                let step = (far - near) / panels as f32;
                for i in 0..panels {
                    let (a, b) = (near + i as f32 * step, near + (i + 1) as f32 * step);
                    quad(
                        context,
                        [
                            Vec3::new(a, -7.0, -1.2),
                            Vec3::new(b, -7.0, -1.2),
                            Vec3::new(b, 7.0, -1.2),
                            Vec3::new(a, 7.0, -1.2),
                        ],
                        [
                            Color::new(0xB0, 0xB0, 0xB0, 0xFF),
                            Color::new(0xB0, 0xB0, 0xB0, 0xFF),
                        ],
                    )?;
                    // Le mur de gauche, pour que les lumières éclairent une
                    // surface qui n'est pas parallèle au sol.
                    quad(
                        context,
                        [
                            Vec3::new(a, -7.0, 4.0),
                            Vec3::new(b, -7.0, 4.0),
                            Vec3::new(b, -7.0, -1.2),
                            Vec3::new(a, -7.0, -1.2),
                        ],
                        [
                            Color::new(0x90, 0x90, 0x98, 0xFF),
                            Color::new(0x90, 0x90, 0x98, 0xFF),
                        ],
                    )?;
                }
                Ok(())
            }
            // Un sol qui fuit bien au-delà de la fin de la rampe : sa moitié
            // lointaine se confond avec le fond, sa moitié proche garde son
            // damier. Texturé, pour que le brouillard ait un motif à effacer —
            // sur un aplat, on ne verrait pas où il agit.
            Self::Fog => textured_quad(
                context,
                [
                    Vec3::new(1.0, -20.0, -1.2),
                    Vec3::new(50.0, -20.0, -1.2),
                    Vec3::new(50.0, 20.0, -1.2),
                    Vec3::new(1.0, 20.0, -1.2),
                ],
                8.0,
                &checker(64, 8),
            ),
            // Un sol texturé qui fuit, et un mur uni au fond : les deux
            // chemins éclairés dans la même image. Le mur est en retrait du
            // bout du sol pour qu'on voie les deux se rejoindre, et sa couleur
            // est franche pour que la lightmap se lise dessus.
            Self::Lit | Self::LitOverbright | Self::LitDynamic => {
                let lightmap = gradient(16);
                lit_quad(
                    context,
                    [
                        Vec3::new(2.0, -16.0, -1.2),
                        Vec3::new(40.0, -16.0, -1.2),
                        Vec3::new(40.0, 16.0, -1.2),
                        Vec3::new(2.0, 16.0, -1.2),
                    ],
                    8.0,
                    Some(&checker(64, 8)),
                    &lightmap,
                    Color::new(0xFF, 0xFF, 0xFF, 0xFF),
                )?;
                lit_quad(
                    context,
                    [
                        Vec3::new(40.0, -16.0, -1.2),
                        Vec3::new(40.0, -16.0, 10.0),
                        Vec3::new(40.0, 16.0, 10.0),
                        Vec3::new(40.0, 16.0, -1.2),
                    ],
                    0.0,
                    None,
                    &lightmap,
                    Color::new(0xC0, 0xB0, 0x90, 0xFF),
                )
            }
            Self::Near => quad(
                context,
                [
                    Vec3::new(-6.0, -12.0, -1.0),
                    Vec3::new(40.0, -12.0, -1.0),
                    Vec3::new(40.0, 12.0, -1.0),
                    Vec3::new(-6.0, 12.0, -1.0),
                ],
                [
                    Color::new(0x60, 0x90, 0x60, 0xFF),
                    Color::new(0x40, 0x68, 0x40, 0xFF),
                ],
            ),
            // La caisse du fichier, à cinq unités devant la caméra et tournée
            // sur deux axes pour que trois de ses faces soient visibles.
            //
            // Assez près pour qu'elle couvre une bonne part de l'image : une
            // caisse lointaine tiendrait dans quelques milliers de pixels, et
            // une régression du décodage — un sommet permuté, un indice décalé —
            // y serait moins nette qu'un motif qui occupe l'écran.
            //
            // Les deux rotations se composent dans cet ordre, et l'ordre compte
            // — la composition de quaternions n'est pas commutative, et
            // l'échanger déplacerait l'image sans rien casser d'autre.
            //
            // L'emplacement des faces latérales reçoit le damier ; celui du
            // dessus et du dessous ne reçoit rien, et ses triangles sortent à
            // leur couleur.
            Self::Crate => {
                let mesh = Mesh::load(&crate_file::crate_mesh())
                    .unwrap_or_else(|_| unreachable!("le fichier de la caisse est bien formé"));
                let degrees = |d: f32| Angle::from_radians(d * core::f32::consts::PI / 180.0);
                let tilt = Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), degrees(-24.0));
                let turn = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), degrees(34.0));
                let sides = checker(64, 8);
                context.submit_mesh(
                    Affine3::from_rotation_translation(
                        turn.product(tilt),
                        Vec3::new(5.0, 0.0, 0.0),
                    ),
                    &mesh,
                    |slot| match slot {
                        0 => Some(&sides),
                        _ => None,
                    },
                )
            }
        }
    }

    /// Reconnaît une scène par son nom.
    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scene| scene.name() == name)
    }

    /// Rend une vue par `pass` et rend son empreinte.
    fn render_view(self, pass: Pass, view: View) -> Result<u64, screengine::Error> {
        let pixels = self.render_pixels(pass, view)?;
        Ok(hash::image(&pixels, view.width, view.height, view.width))
    }

    /// Rend une vue par `pass` et rend le tampon lui-même.
    ///
    /// L'empreinte ne dit pas si une image est noire, et une scène soumise à
    /// l'envers est éliminée comme dos de face sans un mot : c'est par ce
    /// chemin que les tests vérifient qu'il y a quelque chose à hacher.
    fn render_pixels(self, pass: Pass, view: View) -> Result<Vec<u8>, screengine::Error> {
        self.render_with(pass, view, true)
    }

    /// Rend le fond de la scène : ses réglages, et rien de soumis.
    ///
    /// C'est la référence contre laquelle se mesure la couverture. Un critère
    /// écrit en dur — « différent du noir » — est inerte dès qu'une scène a un
    /// fond qui ne l'est pas : le brouillard peint le sien de sa propre couleur,
    /// et une courbe de sortie fait traverser le noir. Les deux scènes
    /// concernées rendaient donc « toute l'image couverte » quoi qu'il arrive,
    /// et une scène soumise à l'envers y serait passée.
    ///
    /// Le fond vient ainsi par construction plutôt que d'une couleur déclarée
    /// scène par scène, qu'il aurait fallu tenir à jour à chaque réglage
    /// nouveau — et qu'on aurait tenue d'après le même raisonnement que celui
    /// qui a produit le critère inerte.
    ///
    /// `cfg(test)` parce que rien du binaire n'en a l'usage : ni `--check`, ni
    /// `--print`, ni `--images` ne rendent un fond seul.
    #[cfg(test)]
    fn render_background(self, pass: Pass, view: View) -> Result<Vec<u8>, screengine::Error> {
        self.render_with(pass, view, false)
    }

    /// Le corps commun aux deux, `submit` faisant seul la différence.
    fn render_with(
        self,
        pass: Pass,
        view: View,
        submit: bool,
    ) -> Result<Vec<u8>, screengine::Error> {
        let (width, height) = (view.width, view.height);
        let mut context = pass.open(view)?;
        context.set_filter(self.filter())?;
        context.set_overbright(self.overbright())?;
        if let Some((color, start, end)) = self.fog() {
            context.set_fog(color, start, end)?;
        }
        if let Some((gamma, gains, offsets)) = self.grade() {
            context.set_grade(gamma, gains, offsets)?;
        }
        let lights = self.lights();
        if !lights.is_empty() {
            context.set_lights(&lights)?;
        }
        if submit {
            self.submit(&mut context, view)?;
        }
        let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];
        pass.render(context.frame_begin()?, &mut pixels, width, height)?;
        Ok(pixels)
    }

    /// Rend la scène par chaque passe, et rend l'empreinte commune.
    ///
    /// Deux passes qui divergent sont une erreur du moteur, pas une
    /// différence à départager : l'image ne dépend ni du découpage, ni de
    /// l'ordre, ni des threads.
    /// La comparaison se fait **vue par vue** et non sur l'empreinte cumulée :
    /// c'est ce qui permet au message de nommer la résolution et l'angle où la
    /// divergence apparaît, là où l'empreinte de la scène ne dirait que « ça ne
    /// correspond plus ».
    fn render_all(self) -> Result<u64, String> {
        let views = self.views();
        let mut reference: Vec<u64> = Vec::with_capacity(views.len());

        for (rank, pass) in Pass::ALL.into_iter().enumerate() {
            for (index, view) in views.iter().enumerate() {
                let hash = self.render_view(pass, *view).map_err(|error| {
                    format!(
                        "{} ({}, {}) : le moteur a refusé la scène : {error:?}",
                        self.name(),
                        pass.name(),
                        view.label()
                    )
                })?;
                if rank == 0 {
                    reference.push(hash);
                } else if reference[index] != hash {
                    return Err(format!(
                        "{} ({}) : {} et {} divergent ({} contre {})",
                        self.name(),
                        view.label(),
                        Pass::ALL[0].name(),
                        pass.name(),
                        hash::format(reference[index]),
                        hash::format(hash)
                    ));
                }
            }
        }
        Ok(hash::chain(&reference))
    }
}

/// Le répertoire des références, à côté du `Cargo.toml` de la suite et non du
/// répertoire courant : `make conform` la lance depuis la racine.
fn references() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("references")
}

/// Le contenu d'un fichier de référence : l'empreinte et un saut de ligne, ce
/// que `--update` écrit et que `--check` compare octet pour octet.
fn reference_text(hash: u64) -> String {
    format!("{}\n", hash::format(hash))
}

/// Compare une scène à sa référence ; rend le compte rendu d'une ligne.
fn check(scene: Scene, dir: &Path) -> Result<String, String> {
    let rendered = scene.render_all()?;
    let path = dir.join(scene.name());
    let expected = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!(
                "{} : référence absente, {} ; make conform-update si la scène est nouvelle",
                scene.name(),
                path.display()
            ));
        }
        Err(error) => return Err(format!("{} : {error}", path.display())),
    };
    if expected == reference_text(rendered) {
        Ok(format!(
            "{} : {}, conforme",
            scene.name(),
            hash::format(rendered)
        ))
    } else {
        Err(format!(
            "{} : rendu {}, référence {}",
            scene.name(),
            hash::format(rendered),
            expected.trim_end()
        ))
    }
}

/// Réécrit la référence d'une scène.
fn update(scene: Scene, dir: &Path) -> Result<String, String> {
    let rendered = scene.render_all()?;
    let path = dir.join(scene.name());
    fs::create_dir_all(dir)
        .and_then(|()| fs::write(&path, reference_text(rendered)))
        .map_err(|error| format!("{} : {error}", path.display()))?;
    Ok(format!(
        "{} : {}, écrite",
        scene.name(),
        hash::format(rendered)
    ))
}

/// Écrit une image en BMP 24 bits, non compressé.
///
/// Un format écrit à la main plutôt qu'une bibliothèque d'images : la suite de
/// conformance n'a pas de dépendance, et n'en prend pas une pour un aperçu.
/// BMP et non PPM parce qu'un poste de travail l'ouvre sans rien installer.
fn write_bmp(path: &Path, pixels: &[u8], width: u32, height: u32) -> io::Result<()> {
    // 640 × 3 est déjà un multiple de quatre ; le calcul reste écrit pour que
    // la fonction survive au jour où une scène changera de largeur.
    let row = width as usize * 3;
    let padding = (4 - row % 4) % 4;
    let data = (row + padding) * height as usize;
    let mut file = Vec::with_capacity(54 + data);

    file.extend_from_slice(b"BM");
    file.extend_from_slice(&((54 + data) as u32).to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&54u32.to_le_bytes());
    file.extend_from_slice(&40u32.to_le_bytes());
    file.extend_from_slice(&(width as i32).to_le_bytes());
    // Hauteur positive : les lignes se rangent du bas vers le haut.
    file.extend_from_slice(&(height as i32).to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&24u16.to_le_bytes());
    file.extend_from_slice(&[0u8; 24]);

    for y in (0..height as usize).rev() {
        let line = &pixels[y * width as usize * BYTES_PER_PIXEL..];
        for x in 0..width as usize {
            let p = &line[x * BYTES_PER_PIXEL..];
            file.extend_from_slice(&[p[2], p[1], p[0]]);
        }
        file.extend(core::iter::repeat_n(0u8, padding));
    }
    fs::write(path, file)
}

/// Écrit chaque vue d'une scène en image dans `dir` ; rend le compte rendu.
///
/// Une scène à plusieurs vues en écrit autant, suffixées de leur résolution et
/// de leur angle : c'est en les ouvrant à la suite qu'on voit tourner ce qu'une
/// empreinte ne montre pas.
fn dump(scene: Scene, dir: &Path) -> Result<String, String> {
    let views = scene.views();
    fs::create_dir_all(dir).map_err(|error| format!("{} : {error}", dir.display()))?;

    for (index, view) in views.iter().enumerate() {
        let pixels = scene
            .render_pixels(Scene::HOST_PASS, *view)
            .map_err(|error| {
                format!(
                    "{} ({}) : le moteur a refusé la scène : {error:?}",
                    scene.name(),
                    view.label()
                )
            })?;
        let name = if views.len() == 1 {
            format!("{}.bmp", scene.name())
        } else {
            format!("{}-{index:02}.bmp", scene.name())
        };
        let path = dir.join(name);
        write_bmp(&path, &pixels, view.width, view.height)
            .map_err(|error| format!("{} : {error}", path.display()))?;
    }
    Ok(format!(
        "{} : {} image(s) dans {}",
        scene.name(),
        views.len(),
        dir.display()
    ))
}

/// Lit le mode dans les arguments, programme exclu.
///
/// Exactement un mode est attendu. Sans lui, la suite ne choisit pas à la
/// place de l'appelant : réécrire des références par défaut effacerait une
/// régression au lieu de la signaler.
fn parse_mode(args: &[String]) -> Result<Mode, String> {
    let usage =
        "usage : screengine-conformance --check | --update | --print <scène> | --dump <répertoire>";
    match args {
        [only] if only == "--check" => Ok(Mode::Check),
        [only] if only == "--update" => Ok(Mode::Update),
        [print, name] if print == "--print" => Scene::named(name)
            .map(Mode::Print)
            .ok_or_else(|| format!("scène inconnue : {name}")),
        [dump, dir] if dump == "--dump" => Ok(Mode::Dump(PathBuf::from(dir))),
        _ => Err(usage.to_string()),
    }
}

/// Point d'entrée : rend 2 sur un usage invalide, 1 sur une divergence, une
/// référence absente ou un refus du moteur.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match parse_mode(&args) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    let mut dir = references();
    let step: fn(Scene, &Path) -> Result<String, String> = match mode {
        Mode::Check => check,
        Mode::Update => update,
        Mode::Dump(into) => {
            dir = into;
            dump
        }
        Mode::Print(scene) => {
            // L'empreinte de la première vue, et non celle de la scène : un
            // hôte hache une image, pas une suite d'images, et c'est à cette
            // valeur-là qu'il compare la sienne.
            let view = scene.views()[0];
            return match scene.render_view(Scene::HOST_PASS, view) {
                Ok(hash) => {
                    println!("{}", hash::format(hash));
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("le moteur a refusé la scène : {error:?}");
                    ExitCode::FAILURE
                }
            };
        }
    };

    let mut failed = false;
    for scene in Scene::ALL {
        match step(scene, &dir) {
            Ok(line) => println!("{line}"),
            Err(line) => {
                eprintln!("{line}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests;
