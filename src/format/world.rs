// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le format de carte : la géométrie du décor.
//!
//! **Ce que le fichier porte est une source, jamais un cache.** Les liens de
//! portails, les triangles, les coordonnées de texture se dérivent ici, au
//! chargement ; aucun n'est stocké. C'est ce qui rend l'édition à chaud possible
//! et ce qui a rendu la chaîne d'outils des moteurs de cette famille
//! impraticable pour un projet seul.
//!
//! Les dispositions font foi dans `docs/rust.md`, section « Formats de
//! fichier ». Les quatre sections sont lues, et ce que la carte dit d'elle-même
//! passe par des accesseurs scalaires plutôt que par une structure rendue.

use alloc::string::String;
use alloc::vec::Vec;

use super::ears::{MAX_POLYGON, triangulate};
use super::{Cursor, decode};
use crate::buffer::{owned, reserved};
use crate::collide::{Hit, Surfaces};
use crate::error::{Error, Malformation, Result};
use crate::math::{MAX_TEXEL_COORD, Quat, Vec3};
use crate::scene::{Color, Light, VertexUv};
use crate::texture::MAX_TEXTURE_SIZE;

/// Le genre que porte l'en-tête d'une carte.
const KIND: [u8; 4] = *b"WRLD";

/// La seule version de format que cette construction lit.
const VERSION: u32 = 1;

/// Les genres de sections, croissants comme l'en-tête l'exige.
const TAGS: [[u8; 4]; 4] = [*b"CELL", *b"ENTS", *b"LGTS", *b"MATS"];

/// Rang de la section des cellules.
const CELL: usize = 0;
/// Rang de la section des entités.
const ENTS: usize = 1;
/// Rang de la section des lumières statiques.
const LGTS: usize = 2;
/// Rang de la section des matériaux.
const MATS: usize = 3;

/// Taille d'une lumière statique dans le fichier, en octets.
const LIGHT_LEN: usize = 24;

/// Taille d'un sommet de cellule, en octets.
const VERTEX_LEN: usize = 12;

/// Les bits de drapeaux qu'une surface définit : deux faces, ne reçoit pas de
/// lightmap, non solide.
///
/// Tous les autres sont nuls obligatoires, même règle que les champs réservés
/// de l'ABI : c'est ce qui permettra d'en employer un sans casser les cartes
/// déjà écrites.
const SURFACE_FLAGS: u32 = 0b111;

/// Le bit qui dit qu'une surface n'arrête aucun volume qui la balaie.
///
/// Défini par le format depuis l'étape des données, sans lecteur jusqu'ici :
/// c'est le balayage qui lui en donne un. Il décrit la géométrie et non
/// l'appelant — la surface compte toujours dans la parité qui localise un point,
/// et occulte toujours la cuisson.
const SURFACE_NON_SOLID: u32 = 0b100;

/// Le bit qui dit qu'une surface ne reçoit pas de lightmap.
///
/// Défini par le format depuis l'étape des données, sans lecteur jusqu'ici : c'est
/// le calcul d'éclairage qui lui en donne un. Une surface qui le porte garde son
/// habillage et reste soumise sans lightmap, comme si sa cellule n'était pas
/// encore cuite.
const SURFACE_NO_LIGHTMAP: u32 = 0b010;

/// L'étendue maximale d'une surface dans son repère de lightmap, en luxels sur
/// un côté.
///
/// Refusée ici plutôt qu'au calcul : un grand mur à pas de lightmap fin produit
/// un atlas qui ne tient pas, et le vérifier au chargement le fait découvrir à
/// l'ouverture de la carte, une fois, au lieu de le faire remonter d'un appel de
/// cuisson pour une seule cellule.
const MAX_LUXELS: f32 = 256.0;

/// L'écart relatif toléré sur l'orthogonalité des axes de lightmap et sur leur
/// appartenance au plan de la surface.
///
/// Les deux autres propriétés du repère — carrés puissances de deux, origine sur
/// la grille — sont exactes ou ne sont pas. Ces deux-ci portent le
/// résidu de la construction d'un repère oblique sur une surface oblique, et une
/// tolérance y est admissible là où elle serait interdite sur l'appariement des
/// portails : celui-ci est une relation, qu'un epsilon rendrait non transitive,
/// alors que ceci est un prédicat, qui rend les mêmes bits sur toutes les cibles.
const SQUARE_TOLERANCE: f64 = 1.0 / 1048576.0;

/// Un repère de plaquage : une origine et deux axes dont la longueur porte
/// l'échelle.
///
/// **Le fichier porte le repère, jamais les coordonnées par sommet.** Déplacer
/// un sommet d'un mur dont on ne connaîtrait que ses coordonnées obligerait
/// l'éditeur à reconstruire le repère par moindres carrés à chaque opération, et
/// deux éditeurs le reconstruiraient différemment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mapping {
    /// Le point de coordonnées nulles.
    pub(crate) origin: Vec3,
    /// L'axe des abscisses, en texels par unité de monde.
    pub(crate) u: Vec3,
    /// L'axe des ordonnées.
    pub(crate) v: Vec3,
}

impl Mapping {
    /// Les coordonnées d'un point dans ce repère.
    ///
    /// Une projection et rien d'autre : la longueur des axes porte déjà
    /// l'échelle, si bien qu'aucune normalisation n'intervient — elle
    /// demanderait une racine, donc une table, pour une valeur que l'éditeur a
    /// déjà écrite.
    pub(crate) fn project(&self, point: Vec3) -> (f32, f32) {
        let d = Vec3::new(
            point.x - self.origin.x,
            point.y - self.origin.y,
            point.z - self.origin.z,
        );
        (d.dot(self.u), d.dot(self.v))
    }
}

/// Une surface d'une cellule, une fois triangulée.
// **Plus `Copy` depuis que la surface garde son polygone** : ses indices sont un
// `Vec`, et c'est voulu — un tableau de taille fixe y coûterait le plafond de
// soixante-quatre sommets sur chaque surface d'un décor qui en a surtout quatre.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Surface {
    /// Son identifiant stable, attribué par l'éditeur, jamais nul.
    // Lu par l'interrogation de la scène, à l'étape 8 : le décodage le valide
    // dès maintenant, l'unicité d'un identifiant n'étant vérifiable qu'ici.
    id: u32,
    /// Ses drapeaux, dont les bits non définis sont nuls.
    flags: u32,
    /// Le rang du matériau qui l'habille, dans la table de la carte.
    ///
    /// **Un rang et non l'identifiant que le fichier porte** : la soumission lie
    /// les textures dans l'ordre des matériaux, et résoudre l'identifiant à
    /// chaque image ferait payer une recherche par surface et par image. La
    /// résolution a lieu au chargement, comme toute valeur dérivée ; les
    /// identifiants restent dans la table, où l'interrogation de la scène les
    /// retrouvera.
    pub(crate) material: u32,
    /// Son premier triangle dans la cellule.
    pub(crate) first_triangle: u32,
    /// Combien de triangles sa découpe a produits.
    pub(crate) triangle_count: u32,
    /// Son repère de lightmap, vérifié aligné sur une grille de puissance de
    /// deux.
    pub(crate) lightmap: Mapping,
    /// Ses sommets dans la cellule, dans l'ordre du fichier.
    ///
    /// **Le polygone et non sa triangulation**, et c'est une clause du calcul
    /// d'éclairage, pas un confort : un rayon qui passe exactement par une arête
    /// interne de la découpe d'oreilles peut être manqué par les deux triangles
    /// qui la partagent, et la lumière traverse alors le mur par un trou
    /// d'épingle — invisible à l'arrêt, visible en mouchetures sur une lightmap
    /// cuite, et tout le calculateur serait déjà construit dessus.
    pub(crate) corners: Vec<u32>,
    /// Le rang de son premier sommet dérivé dans la cellule.
    ///
    /// Le chargement dédouble les sommets par surface — chacun porte les
    /// coordonnées de son propre repère —, si bien que les coins d'une surface y
    /// sont consécutifs à partir de celui-ci. C'est par lui que le calcul
    /// d'éclairage retrouve le polygone qu'il doit interroger.
    pub(crate) first_vertex: u32,
    /// Le coin de la grille de luxels, et ses deux étendues.
    ///
    /// Dérivé au chargement comme tout le reste : c'est ce qui dimensionne le
    /// rectangle de la surface dans l'atlas de sa cellule, et le faire au calcul
    /// obligerait à reparcourir les sommets une seconde fois.
    pub(crate) luxels: Extent,
    /// Sa normale de Newell **brute**, dans le sens de son enroulement.
    ///
    /// Brute, donc pas encore orientée vers l'intérieur de la cellule : le format
    /// fixe la face visible d'une surface, ce qui n'est pas la même chose que le
    /// côté intérieur, et seul le signe du volume de la cellule le dit. C'est
    /// [`Cell::inward`] qui compose les deux.
    ///
    /// Sur le polygone entier et non sur un de ses triangles : une surface
    /// concave a des triangles dont l'orientation ne dit rien de la sienne.
    /// Elle n'est pas unitaire — sa longueur vaut le double de l'aire.
    normal: Vec3,
    /// Lesquelles de ses arêtes portent un volume propre pour le balayage.
    ///
    /// Le bit `i` désigne l'arête qui va du coin `i` au suivant, et vaut 1 quand
    /// cette arête est **exposée** : soit elle n'est partagée avec aucune autre
    /// surface solide, soit le dièdre qu'elles forment est saillant. Un `u64`
    /// suffit, une surface étant plafonnée à [`MAX_POLYGON`] coins.
    ///
    /// Une arête **rentrante ou coplanaire** vaut 0, et c'est ce qui tue la
    /// collision fantôme : la face de l'une ou l'autre surface couvre déjà
    /// l'instant du contact, et lui laisser en plus un volume d'arête ferait
    /// accrocher une boîte qui glisse sur un sol plan, à la couture de la
    /// triangulation comme à la jointure de deux dalles. Voir `docs/rust.md`,
    /// « La règle de l'arête partagée ».
    edges: u64,
}

impl Surface {
    /// Vrai si cette surface refuse la lightmap de sa cellule.
    ///
    /// Elle reste soumise et garde son habillage : c'est le chemin non éclairé qui
    /// la prend, exactement comme si sa cellule n'était pas encore cuite.
    pub(crate) fn skips_lightmap(&self) -> bool {
        self.flags & SURFACE_NO_LIGHTMAP != 0
    }

    /// Son identifiant stable.
    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    /// Ses drapeaux, tels que le fichier les porte.
    ///
    /// L'empreinte du cache les prend en entier plutôt que le seul bit qu'elle
    /// sait lire : un drapeau ajouté plus tard qui changerait la cuisson périmerait
    /// alors les entrées de lui-même, là où une lecture sélective l'oublierait.
    pub(crate) fn flags(&self) -> u32 {
        self.flags
    }

    /// Sa normale de Newell brute, dans le sens de son enroulement.
    pub(crate) fn normal(&self) -> Vec3 {
        self.normal
    }

    /// Vrai si cette surface arrête un volume qui la balaie.
    ///
    /// L'inverse du bit « non solide » du format, qui décrit la géométrie et non
    /// l'appelant : une surface non solide **compte toujours** dans la parité qui
    /// dit si un point est dans la cellule, et **occulte toujours** la cuisson.
    /// Seul le balayage l'ignore.
    pub(crate) fn is_solid(&self) -> bool {
        self.flags & SURFACE_NON_SOLID == 0
    }

    /// Vrai si l'arête partant du coin `index` porte un volume propre.
    pub(crate) fn edge_is_exposed(&self, index: usize) -> bool {
        self.edges & (1 << index) != 0
    }
}

/// L'étendue d'une surface dans son repère de lightmap, en luxels.
///
/// Les bornes sont celles des **nœuds** de la grille, pas des sommets : la
/// surface commence au nœud qui la précède et finit à celui qui la suit, si bien
/// qu'aucun de ses points ne tombe hors de la grille qui l'éclaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Extent {
    /// Le nœud de départ le long de l'axe `u`.
    pub(crate) min_u: i32,
    /// Celui le long de l'axe `v`.
    pub(crate) min_v: i32,
    /// Le nombre de luxels le long de `u`, au moins un.
    pub(crate) width: u32,
    /// Celui le long de `v`.
    pub(crate) height: u32,
}

/// Un portail : le polygone plan convexe par lequel deux cellules se voient.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Portal {
    /// Son identifiant stable, jamais nul.
    id: u32,
    /// Ses sommets, en propre.
    ///
    /// Recopiés et non indexés : l'appariement compare des positions, et deux
    /// portails appariés appartiennent à deux cellules qui ne partagent aucun
    /// sommet. La duplication est la condition du mécanisme, pas son coût.
    pub(crate) points: Vec<Vec3>,
    /// La cellule et le portail qu'il rejoint, ou `None` pour un mur.
    ///
    /// Des **index**, et non des identifiants : c'est la traversée qui les lit,
    /// et elle dépile des cellules sans avoir à interroger une table à chaque
    /// portail franchi. L'appariement a lieu au chargement parce que c'est là
    /// qu'on a les deux côtés sous la main.
    pub(crate) link: Option<(u32, u32)>,
}

impl Portal {
    /// Son identifiant stable.
    pub(crate) fn id(&self) -> u32 {
        self.id
    }
}

/// Une cellule : l'unité d'édition, un volume fermé quelconque.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cell {
    /// Son identifiant stable, jamais nul.
    id: u32,
    /// Ses drapeaux, dont aucun bit n'est défini : tous nuls.
    flags: u32,
    /// Les sommets dérivés, un par sommet et par surface qui l'emploie.
    ///
    /// Un sommet partagé par deux murs porte deux coordonnées de texture, et
    /// c'est le repère de chaque surface qui les décide : le fichier garde un
    /// sommet, le rendu en voit deux.
    pub(crate) vertices: Vec<VertexUv>,
    /// Les triangles de toutes ses surfaces, dans l'ordre des surfaces.
    pub(crate) triangles: Vec<[u32; 3]>,
    /// Ses surfaces.
    pub(crate) surfaces: Vec<Surface>,
    /// Ses portails.
    pub(crate) portals: Vec<Portal>,
    /// Combien de luxels sa cuisson calcule, gouttières exclues.
    ///
    /// Ce que coûte sa lightmap, et la seule mesure qui permette à un hôte de
    /// pondérer une cuisson cellule par cellule : le nombre de surfaces ne dit
    /// rien, un mur de vingt mètres et une marche en comptant chacun une.
    luxel_count: u32,
    /// Les deux coins de sa boîte englobante, le plus bas puis le plus haut.
    ///
    /// Sur les sommets **dérivés**, donc sur ce que ses surfaces couvrent : un
    /// sommet du fichier qu'aucune surface n'emploie n'y entre pas. C'est ce qui
    /// la rend bonne pour ce qui lit des surfaces, et ce qui interdit de la
    /// prendre pour la boîte de la cellule au sens du fichier.
    ///
    /// Une cellule sans surface garde une boîte **inversée**, `f32::MAX` en bas
    /// et `f32::MIN` en haut. C'est voulu : tout point est alors hors d'elle, si
    /// bien qu'une sélection par cette boîte ne retient rien plutôt que tout.
    bounds: [Vec3; 2],
    /// Ses normales de Newell pointent-elles vers l'extérieur ?
    ///
    /// Six fois le volume signé, par le théorème de la divergence : la somme, sur
    /// les faces qui ferment la cellule, du produit scalaire d'un de leurs points
    /// par leur normale brute. **Les portails en sont**, sans quoi la cellule
    /// n'est pas fermée et la somme ne vaut rien. Un volume nul ne peut venir que
    /// d'une cellule dégénérée, qu'aucune orientation ne sauverait : le sens brut
    /// est alors gardé.
    ///
    /// **Calculé ici, une fois, et rien ne peut l'invalider ensuite** : une carte
    /// chargée est immuable, ni ses sommets ni ses portails ne bougent. C'est ce
    /// qui autorise à le mémoriser plutôt qu'à reparcourir la cellule à chaque
    /// appel — un parcours complet rend le même résultat quel que soit l'ordre
    /// des appels, et c'est cette propriété-là qu'un champ doit conserver.
    outward: bool,
}

impl Cell {
    /// Son identifiant stable.
    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    /// Ses drapeaux, tels que le fichier les porte.
    pub(crate) fn flags(&self) -> u32 {
        self.flags
    }

    /// Les deux coins de sa boîte englobante, le plus bas puis le plus haut.
    pub(crate) fn bounds(&self) -> (Vec3, Vec3) {
        (self.bounds[0], self.bounds[1])
    }

    /// La normale d'une de ses surfaces, orientée vers l'intérieur de la cellule.
    ///
    /// **L'enroulement seul ne suffit pas à l'orienter.** La formule de Newell
    /// rend une normale dont le sens suit l'ordre des sommets, et le format ne
    /// dit pas lequel des deux sens est l'intérieur — il fixe la face visible, ce
    /// qui n'est pas la même chose. Une normale prise à l'envers donne un terme
    /// de Lambert négatif et la surface reste noire : constaté en écrivant la
    /// cuisson, où le plafond du décor de validation restait éteint à deux unités
    /// sous une lampe qui lui faisait face.
    ///
    /// Le sens vient donc du **signe du volume de la cellule**, calculé une fois
    /// pour toutes ses faces. L'enroulement du format est cohérent d'une face à
    /// l'autre — sans quoi le rendu montrerait déjà des trous —, donc un seul
    /// signe les oriente toutes.
    ///
    /// **Le barycentre des sommets a été essayé et il est faux.** Comparer la
    /// normale à la direction du barycentre décide juste tant que celui-ci n'est
    /// pas dans le plan de la face ; il suffit qu'il y soit pour que le produit
    /// scalaire s'annule, que rien ne tranche et que le signe brut passe tel quel.
    /// Il ne faut pas une cellule tordue pour cela : le barycentre d'une salle en
    /// L de huit unités tombe exactement sur son coin rentrant, donc sur le plan
    /// de deux de ses murs, dont l'un ressortait noir.
    pub(crate) fn inward(&self, surface: &Surface) -> Vec3 {
        let raw = surface.normal();
        if self.outward { -raw } else { raw }
    }
}

/// Une lumière statique du décor.
///
/// **Une section propre, et ce n'est pas une exception à l'invariant du jeu.**
/// Si les sources étaient des entités opaques, le calcul de lightmap devrait
/// recevoir un tableau produit par l'hôte, et deux hôtes donneraient deux
/// éclairages pour la même carte — ce qui retirerait au projet « la même image
/// sur toutes les cibles ». La lumière est déjà dans le vocabulaire du moteur ;
/// les points de départ et les objets ramassables restent des entités opaques.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StaticLight {
    /// Son identifiant stable, jamais nul.
    ///
    /// **Lu par [`World::light_id`] et non porté par [`Light`]**, et c'est le
    /// genre de décision qu'on propose de nouveau dans six mois : `Light`
    /// traverse la frontière C **par valeur**, si bien que lui ajouter un champ
    /// déplacerait les décalages d'une structure publiée — donc casserait l'ABI.
    /// Un accesseur de plus ne grave rien.
    id: u32,
    /// Ce que le moteur connaît d'une lumière, et rien de plus.
    light: Light,
}

/// Une entité : ce que la carte pose et que le moteur ne comprend pas.
///
/// **Un identifiant, une classe jamais interprétée, une pose, une cellule et un
/// bloc d'octets que le moteur copie et ne lit pas.** Pas de modèle de
/// propriétés typé : ce serait un langage de jeu qu'il faudrait faire évoluer
/// avec les jeux.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entity {
    /// Son identifiant stable, jamais nul.
    id: u32,
    /// L'identifiant de la cellule où elle se trouve, vérifié existant.
    cell: u32,
    /// Sa classe, que le moteur transmet sans jamais la comparer à rien.
    class: String,
    /// Sa position dans le monde.
    position: Vec3,
    /// Son orientation, normalisée au chargement.
    orientation: Quat,
    /// Ses octets, copiés tels quels.
    data: Vec<u8>,
}

/// Une carte chargée depuis un bloc d'octets.
///
/// Immuable, indépendante de tout contexte, comme un maillage. Ses octets sont
/// copiés au chargement : l'hôte peut libérer son bloc au retour.
#[derive(Debug)]
pub struct World {
    /// Les cellules, dans l'ordre du fichier — qui est celui de la soumission,
    /// donc ce qui départage deux surfaces coplanaires.
    cells: Vec<Cell>,
    /// Les identifiants de cellules avec leur index, triés par identifiant.
    ///
    /// Une table plutôt qu'une recherche linéaire dans les cellules : ce n'est
    /// pas le coût d'une image qui le commande — une seule cellule se désigne par
    /// image — mais l'étape 8, qui remplacera une cellule nommée par son
    /// identifiant à chaque opération d'éditeur.
    cell_index: Vec<(u32, u32)>,
    /// Les identifiants de surfaces avec leur place, triés par identifiant.
    ///
    /// `(surface, cellule, rang dans la cellule)`. Sans elle, traduire
    /// l'identifiant qu'une interrogation rend demandait un **balayage linéaire
    /// de toute la carte** — acceptable pour un contact de collision, que le jeu
    /// lit une fois par déplacement, mais pas pour une sélection qui suit le
    /// mouvement d'une souris.
    ///
    /// C'est la table qu'une famille d'identifiants doit avoir, comme celle des
    /// cellules, et elle se dérive au même endroit.
    surface_index: Vec<(u32, u32, u32)>,
    /// Les identifiants de matériaux, dans l'ordre du fichier.
    ///
    /// Le seul des six espaces d'identifiants que rien ne lit encore : les
    /// accesseurs publiés désignent un matériau par son rang, et c'est la
    /// modification d'une surface, à l'étape 8, qui aura besoin de le désigner par
    /// son identifiant. Gardé plutôt que reconstruit alors, parce que la table des
    /// identifiants d'une famille est ce que le chargement dérive.
    #[allow(dead_code)]
    material_ids: Vec<u32>,
    /// Leurs noms, dans le même ordre.
    material_names: Vec<String>,
    /// Le nombre total de triangles, pour que l'hôte dimensionne son contexte.
    triangle_count: u32,
    /// Les lumières statiques, dans l'ordre du fichier.
    lights: Vec<StaticLight>,
    /// Les entités, dans l'ordre du fichier.
    entities: Vec<Entity>,
}

impl World {
    /// Décode une carte, ou dit ce qui l'a fait refuser.
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let sections = decode(bytes, KIND, VERSION, TAGS)?;
        let (material_ids, material_names) = materials(sections[MATS])?;
        let mut cells = cells(sections[CELL], &material_ids)?;
        link_portals(&mut cells)?;
        // Après l'appariement, et pas avant : c'est lui qui dit quelles cellules
        // se touchent, donc quelles arêtes sont intérieures au bord solide bien
        // qu'elles appartiennent à deux cellules.
        classify_edges_across_cells(&mut cells)?;

        let mut triangle_count: u32 = 0;
        for cell in &cells {
            triangle_count = triangle_count
                .checked_add(cell.triangles.len() as u32)
                .ok_or(Error::InvalidFormat(Malformation::Count))?;
        }

        let lights = lights(sections[LGTS])?;
        let cell_ids: Vec<u32> = cells.iter().map(|cell| cell.id).collect();
        let entities = entities(sections[ENTS], &cell_ids)?;

        // La table des identifiants de cellules, triée, que toute désignation
        // par identifiant interroge par dichotomie : la cellule où commence une
        // traversée, celle dont on calcule les lightmaps, celle qu'une entrée de
        // cache nomme. Le fichier n'exige pas ses identifiants triés — l'exiger
        // obligerait l'éditeur à réécrire la carte entière pour un ajout —, donc
        // le tri est ici.
        let mut cell_index = reserved(cells.len())?;
        for (index, cell) in cells.iter().enumerate() {
            cell_index.push((cell.id, index as u32));
        }
        cell_index.sort_unstable();

        // La même table pour les surfaces, et pour la même raison : une
        // désignation par identifiant stable se résout par dichotomie, jamais
        // par un parcours. Les identifiants sont déjà vérifiés uniques, donc le
        // tri suffit à rendre la recherche exacte.
        let mut surface_index = reserved(cells.iter().map(|c| c.surfaces.len()).sum())?;
        for (index, cell) in cells.iter().enumerate() {
            for (rank, surface) in cell.surfaces.iter().enumerate() {
                surface_index.push((surface.id(), index as u32, rank as u32));
            }
        }
        surface_index.sort_unstable();

        Ok(Self {
            cells,
            cell_index,
            surface_index,
            material_ids,
            material_names,
            triangle_count,
            lights,
            entities,
        })
    }

    /// Combien de triangles la carte porte, toutes cellules confondues.
    pub fn triangle_count(&self) -> u32 {
        self.triangle_count
    }

    /// Combien de cellules la carte porte.
    pub fn cell_count(&self) -> u32 {
        self.cells.len() as u32
    }

    /// L'identifiant de la cellule de ce rang, ou `None` au-delà du compte.
    ///
    /// Le rang est celui du fichier, et il n'est pas stable d'un chargement à
    /// l'autre : il sert à énumérer, jamais à désigner. C'est l'identifiant qui
    /// désigne.
    pub fn cell_id(&self, index: u32) -> Option<u32> {
        self.cells.get(index as usize).map(|cell| cell.id)
    }

    /// Combien de luxels la cuisson de cette cellule calcule, ou `None` si
    /// l'identifiant n'en désigne aucune.
    ///
    /// **La mesure du coût d'une cuisson**, que l'hôte appelle avant elle : le
    /// calcul est par cellule et synchrone, et sans cela une barre de
    /// progression avancerait par cellules, donc par pas de tailles sans
    /// rapport entre eux.
    pub fn cell_luxel_count(&self, id: u32) -> Option<u32> {
        let index = self.cell_of(id)?;
        self.cells.get(index as usize).map(|cell| cell.luxel_count)
    }

    /// L'index de la cellule que cet identifiant désigne, s'il en désigne une.
    ///
    /// Par dichotomie sur la table triée au chargement. `0` ne désigne aucune
    /// cellule — l'éditeur le réserve à « aucun » —, et c'est un identifiant
    /// inconnu comme un autre ici : c'est à l'appelant de distinguer « aucune
    /// cellule » de « cette cellule n'existe pas », les deux n'ayant pas la même
    /// réponse.
    pub(crate) fn cell_of(&self, id: u32) -> Option<u32> {
        self.cell_index
            .binary_search_by_key(&id, |(key, _)| *key)
            .ok()
            .map(|rank| self.cell_index[rank].1)
    }

    /// L'identifiant de la cellule qui contient ce point, ou `0` s'il n'en est
    /// dans aucune.
    ///
    /// **Une interrogation, pas un état.** Le moteur ne retient pas où est la
    /// caméra : l'hôte garde l'identifiant et le redonne à chaque soumission, ce
    /// qui laisse le suivi d'une caméra là où il appartient — dans le jeu.
    ///
    /// Le coût est celui d'un parcours de toutes les cellules et de toutes leurs
    /// faces. C'est un appel nommé, pour le chargement ou pour reprendre le fil ;
    /// entre deux images, [`World::track`] suffit.
    pub fn locate(&self, position: Vec3) -> u32 {
        crate::world::locate::locate(self, position)
            .and_then(|index| self.cell_id(index))
            .unwrap_or(0)
    }

    /// L'identifiant de la cellule où un déplacement aboutit, ou `0` s'il sort de
    /// toute cellule.
    ///
    /// `from_cell` est celui d'où le déplacement part. Un identifiant qui ne
    /// désigne aucune cellule rend `0`, comme une sortie : il n'y a pas de fil à
    /// reprendre depuis une cellule qui n'existe pas.
    ///
    /// **Plusieurs cellules peuvent être traversées en un seul déplacement**, et
    /// le suivi les enchaîne. Le moteur ne se relocalise jamais de lui-même :
    /// quand ceci rend `0`, c'est à l'hôte de rappeler [`World::locate`].
    pub fn track(&self, from_cell: u32, from: Vec3, to: Vec3) -> u32 {
        let Some(index) = self.cell_of(from_cell) else {
            return 0;
        };
        crate::world::locate::track(self, index, from, to)
            .and_then(|found| self.cell_id(found))
            .unwrap_or(0)
    }

    /// Balaie une boîte axiale de `from` à `to`, depuis la cellule `from_cell`.
    ///
    /// Rend `None` quand `from_cell` ne désigne aucune cellule : c'est à
    /// l'appelant d'en faire une erreur de ressource inconnue, le noyau ne
    /// connaissant pas les codes de l'ABI. `0` n'arrive pas ici — l'absence de
    /// cellule se traite avant, et elle n'est pas une erreur.
    ///
    /// **Appelable depuis plusieurs threads sur la même carte.** Rien n'y est
    /// muté, rien n'y est retenu, et le résultat ne dépend que des arguments :
    /// c'est ce qui rend un serveur possible sans contexte de rendu.
    /// **Les arguments et le résultat sont en `f32`** ; le balayage travaille en
    /// `f64` derrière cette frontière, qui est son unique point de passage. La
    /// règle d'usage est dans `docs/rust.md`, au même endroit que la clause du
    /// `f64`.
    pub fn sweep(&self, from_cell: u32, half: Vec3, from: Vec3, to: Vec3) -> Option<Hit> {
        crate::collide::sweep(
            self,
            from_cell,
            half.into(),
            from.into(),
            to.into(),
            Surfaces::Solid,
        )
    }

    /// Interroge la scène par un rayon, et rend la surface touchée.
    ///
    /// **C'est le balayage d'une boîte d'étendue nulle**, et une seule fonction
    /// pour les deux : un rayon est ce cas-là, et deux parcours auraient été le
    /// même code à valider l'un contre l'autre. La dilatation de sécurité suit —
    /// elle est relative à la plus grande demi-étendue, donc **nulle pour un
    /// rayon**, qui touche exactement ce qu'il croise et non ce qui l'approche.
    ///
    /// Ce qu'elle ajoute au balayage est `surfaces`, et rien d'autre : une
    /// sélection doit pouvoir attraper une grille ou une vitre, que la collision
    /// ignore par construction.
    ///
    /// Rend `None` quand la cellule de départ n'existe pas, comme le balayage.
    pub fn pick(&self, from_cell: u32, from: Vec3, to: Vec3, surfaces: Surfaces) -> Option<Hit> {
        crate::collide::sweep(
            self,
            from_cell,
            Vec3::ZERO.into(),
            from.into(),
            to.into(),
            surfaces,
        )
    }

    /// Le même rayon, contre **toutes** les cellules et sans traversée.
    ///
    /// La moitié d'un oracle, comme [`World::sweep_brute`] l'est du balayage :
    /// sur une carte bien formée, les deux chemins rendent les mêmes bits, et
    /// c'est ce qui attrape une traversée trop étroite.
    pub fn pick_brute(&self, from: Vec3, to: Vec3, surfaces: Surfaces) -> Hit {
        crate::collide::sweep_brute(self, Vec3::ZERO.into(), from.into(), to.into(), surfaces)
    }

    /// Le même balayage, contre **toutes** les cellules et sans traversée.
    ///
    /// **Ce n'est pas un outil de test, c'est la moitié d'un oracle.** Sur une
    /// carte bien formée, les deux chemins doivent rendre exactement les mêmes
    /// bits, et cette égalité est le seul contrôle qui attrape une traversée trop
    /// étroite — l'analogue du chemin brut de la traversée de rendu.
    ///
    /// Il ne prend pas de cellule de départ : n'en connaissant aucune, il ne peut
    /// pas se tromper de cellule, et c'est ce qui en fait une référence.
    pub fn sweep_brute(&self, half: Vec3, from: Vec3, to: Vec3) -> Hit {
        crate::collide::sweep_brute(self, half.into(), from.into(), to.into(), Surfaces::Solid)
    }

    /// Le matériau d'une surface, par son identifiant stable.
    ///
    /// `None` quand aucune surface ne le porte. C'est le pendant de
    /// l'identifiant que le balayage rend : sans lui, l'hôte recevrait un
    /// identifiant qu'aucune fonction ne traduit.
    pub fn surface_material(&self, surface_id: u32) -> Option<u32> {
        let (cell, rank) = self.surface_at(surface_id)?;
        Some(self.cells[cell as usize].surfaces[rank as usize].material)
    }

    /// Où vit une surface, `(index de cellule, rang dans la cellule)`.
    ///
    /// Par dichotomie sur la table triée au chargement, comme pour une cellule.
    /// `0` ne désigne aucune surface — l'éditeur le réserve à « aucun » —, et
    /// c'est ici un identifiant inconnu comme un autre.
    pub(crate) fn surface_at(&self, surface_id: u32) -> Option<(u32, u32)> {
        self.surface_index
            .binary_search_by_key(&surface_id, |(key, _, _)| *key)
            .ok()
            .map(|rank| {
                let (_, cell, place) = self.surface_index[rank];
                (cell, place)
            })
    }

    /// Combien de matériaux elle réclame.
    pub fn material_count(&self) -> u32 {
        // Borné : chaque entrée occupe au moins six octets du fichier.
        self.material_names.len() as u32
    }

    /// Le nom d'un matériau, ou `None` au-delà du dernier.
    pub fn material_name(&self, index: u32) -> Option<&str> {
        self.material_names.get(index as usize).map(String::as_str)
    }

    /// Les cellules, dans l'ordre du fichier.
    pub(crate) fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Combien de lumières statiques la carte porte.
    pub fn light_count(&self) -> u32 {
        // Borné : chaque lumière occupe vingt-quatre octets du fichier.
        self.lights.len() as u32
    }

    /// Une lumière statique, ou `None` au-delà de la dernière.
    ///
    /// Rendue telle que le moteur la reçoit : l'hôte la repasse à son contexte
    /// sans rien reconstruire, ce qui est tout ce qu'il en fait à cette étape.
    pub fn light(&self, index: u32) -> Option<Light> {
        self.lights.get(index as usize).map(|light| light.light)
    }

    /// L'identifiant stable d'une lumière, ou `None` au-delà de la dernière.
    ///
    /// **Par rang, celui que [`World::light`] prend.** Sans lui, un hôte qui a
    /// écrit la carte ne peut relier une lumière relue à celle qu'il a posée que
    /// par l'ordre d'écriture, et cet ordre n'est garanti par rien.
    ///
    /// Une lumière n'a pas de cellule, d'où le singulier là où une entité rend
    /// une paire.
    pub fn light_id(&self, index: u32) -> Option<u32> {
        self.lights.get(index as usize).map(|light| light.id)
    }

    /// Combien d'entités la carte porte.
    pub fn entity_count(&self) -> u32 {
        // Borné : chaque entité occupe au moins vingt-deux octets du fichier.
        self.entities.len() as u32
    }

    /// L'identifiant d'une entité et celui de sa cellule.
    pub fn entity_ids(&self, index: u32) -> Option<(u32, u32)> {
        self.entities
            .get(index as usize)
            .map(|entity| (entity.id, entity.cell))
    }

    /// La classe d'une entité, que le moteur n'interprète jamais.
    pub fn entity_class(&self, index: u32) -> Option<&str> {
        self.entities
            .get(index as usize)
            .map(|entity| entity.class.as_str())
    }

    /// La pose d'une entité : sa position et son orientation normalisée.
    pub fn entity_pose(&self, index: u32) -> Option<(Vec3, Quat)> {
        self.entities
            .get(index as usize)
            .map(|entity| (entity.position, entity.orientation))
    }

    /// Les octets d'une entité, que le moteur a copiés et ne lit pas.
    pub fn entity_data(&self, index: u32) -> Option<&[u8]> {
        self.entities
            .get(index as usize)
            .map(|entity| entity.data.as_slice())
    }
}

/// Les lumières statiques, de taille fixe.
fn lights(section: &[u8]) -> Result<Vec<StaticLight>> {
    let mut cursor = Cursor::new(section);
    let mut lights = reserved(section.len() / LIGHT_LEN)?;
    let mut ids = reserved(section.len() / LIGHT_LEN)?;

    while cursor.remaining() != 0 {
        let id = cursor.u32()?;
        let x = cursor.f32()?;
        let y = cursor.f32()?;
        let z = cursor.f32()?;
        let radius = cursor.f32()?;
        let r = cursor.u8()?;
        let g = cursor.u8()?;
        let b = cursor.u8()?;
        let reserved = cursor.u8()?;

        if id == 0 {
            return Err(Error::InvalidFormat(Malformation::Identifier));
        }
        // L'alpha d'une lumière est réservé et nul, comme dans la structure que
        // l'hôte passe au moteur : c'est ce qui permettra de l'employer sans
        // casser les cartes déjà écrites.
        if reserved != 0 {
            return Err(Error::InvalidFormat(Malformation::Flags));
        }
        // Un rayon nul n'éclaire rien et ferait diviser par zéro le jour où
        // l'étape 5 calcule une atténuation. La comparaison est franche et non
        // niée : le curseur a déjà refusé les non-finis, donc aucun `NaN` ne
        // passe ici.
        if radius <= 0.0 {
            return Err(Error::InvalidFormat(Malformation::Light));
        }

        ids.push(id);
        lights.push(StaticLight {
            id,
            light: Light {
                position: Vec3::new(x, y, z),
                radius,
                color: Color::new(r, g, b, 0xFF),
            },
        });
    }
    unique(&ids)?;
    Ok(lights)
}

/// Les entités, chacune un enregistrement longueur-préfixé.
fn entities(section: &[u8], cells: &[u32]) -> Result<Vec<Entity>> {
    let mut cursor = Cursor::new(section);
    let mut entities = Vec::new();
    let mut ids = Vec::new();

    while cursor.remaining() != 0 {
        let len = cursor.u32()? as usize;
        let record = cursor.take(len)?;
        let entity = entity(record, cells)?;

        entities.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        ids.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        ids.push(entity.id);
        entities.push(entity);
    }
    unique(&ids)?;
    Ok(entities)
}

/// Une entité : ses deux identifiants, sa classe, sa pose, ses octets.
fn entity(record: &[u8], cells: &[u32]) -> Result<Entity> {
    let mut cursor = Cursor::new(record);
    let id = cursor.u32()?;
    let cell = cursor.u32()?;
    if id == 0 {
        return Err(Error::InvalidFormat(Malformation::Identifier));
    }
    // Par identifiant et jamais par index : sans cela, l'annulation et la
    // sauvegarde partielle de l'éditeur deviennent impraticables dès la
    // première suppression au milieu.
    if !cells.contains(&cell) {
        return Err(Error::InvalidFormat(Malformation::Index));
    }

    let class_len = cursor.u16()? as usize;
    let class = core::str::from_utf8(cursor.take(class_len)?)
        .map_err(|_| Error::InvalidFormat(Malformation::NonUtf8))?;
    let class = owned(class)?;

    let x = cursor.f32()?;
    let y = cursor.f32()?;
    let z = cursor.f32()?;
    let orientation = Quat::new(cursor.f32()?, cursor.f32()?, cursor.f32()?, cursor.f32()?);
    // Un quaternion nul n'a pas de direction à porter, et `normalize` rendrait
    // l'identité en silence : une entité posée de travers se retrouverait droite
    // sans que rien ne le signale.
    if orientation.dot(orientation) == 0.0 {
        return Err(Error::InvalidFormat(Malformation::Pose));
    }

    let data_len = cursor.u32()? as usize;
    let bytes = cursor.take(data_len)?;
    let mut data = reserved(data_len)?;
    data.extend_from_slice(bytes);

    if cursor.remaining() != 0 {
        return Err(Error::InvalidFormat(Malformation::Count));
    }

    Ok(Entity {
        id,
        cell,
        class,
        position: Vec3::new(x, y, z),
        orientation: orientation.normalize(),
        data,
    })
}

/// La table des matériaux : un identifiant et un nom par entrée.
fn materials(section: &[u8]) -> Result<(Vec<u32>, Vec<String>)> {
    let mut count = 0;
    let mut cursor = Cursor::new(section);
    while cursor.remaining() != 0 {
        cursor.u32()?;
        let len = cursor.u16()? as usize;
        cursor.take(len)?;
        count += 1;
    }

    let mut ids = reserved(count)?;
    let mut names = reserved(count)?;
    let mut cursor = Cursor::new(section);
    while cursor.remaining() != 0 {
        let id = cursor.u32()?;
        if id == 0 {
            return Err(Error::InvalidFormat(Malformation::Identifier));
        }
        let len = cursor.u16()? as usize;
        let bytes = cursor.take(len)?;
        let name =
            core::str::from_utf8(bytes).map_err(|_| Error::InvalidFormat(Malformation::NonUtf8))?;
        ids.push(id);
        names.push(owned(name)?);
    }
    unique(&ids)?;
    Ok((ids, names))
}

/// Les cellules, chacune un enregistrement longueur-préfixé.
fn cells(section: &[u8], materials: &[u32]) -> Result<Vec<Cell>> {
    let mut cursor = Cursor::new(section);
    let mut cells = Vec::new();
    let mut ids = Vec::new();

    while cursor.remaining() != 0 {
        let len = cursor.u32()? as usize;
        // La longueur de l'enregistrement borne tout ce qu'il contient : les
        // comptes s'y recoupent, jamais avec la longueur de la section. C'est ce
        // qui permettra de remplacer une cellule sans toucher aux autres.
        let record = cursor.take(len)?;
        let cell = cell(record, materials)?;

        // Le vecteur grandit ici, faute d'un compte de cellules déclaré : le
        // chargement est un appel nommé, et réserver sur un nombre que le
        // fichier annoncerait serait la bombe d'allocation que le format ferme.
        cells.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        ids.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        ids.push(cell.id);
        cells.push(cell);
    }
    unique(&ids)?;

    let mut portal_ids = Vec::new();
    for cell in &cells {
        for portal in &cell.portals {
            portal_ids.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
            portal_ids.push(portal.id);
        }
    }
    unique(&portal_ids)?;

    let mut surface_ids = Vec::new();
    for cell in &cells {
        for surface in &cell.surfaces {
            surface_ids.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
            surface_ids.push(surface.id);
        }
    }
    unique(&surface_ids)?;

    Ok(cells)
}

/// Une cellule : son en-tête, ses sommets, ses surfaces, ses portails.
fn cell(record: &[u8], materials: &[u32]) -> Result<Cell> {
    let mut cursor = Cursor::new(record);
    let id = cursor.u32()?;
    let flags = cursor.u32()?;
    let vertex_count = cursor.u32()? as usize;
    let surface_count = cursor.u32()? as usize;
    let portal_count = cursor.u32()? as usize;

    if id == 0 {
        return Err(Error::InvalidFormat(Malformation::Identifier));
    }
    // Aucun bit de drapeau de cellule n'est défini : tous nuls, ce qui permettra
    // d'en employer un sans casser les cartes déjà écrites.
    if flags != 0 {
        return Err(Error::InvalidFormat(Malformation::Flags));
    }

    let mut points = reserved(bounded(vertex_count, VERTEX_LEN, cursor.remaining())?)?;
    for _ in 0..vertex_count {
        let x = cursor.f32()?;
        let y = cursor.f32()?;
        let z = cursor.f32()?;
        points.push(Vec3::new(x, y, z));
    }

    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut surfaces = reserved(surface_count.min(cursor.remaining()))?;
    for _ in 0..surface_count {
        let surface = surface(
            &mut cursor,
            &points,
            materials,
            &mut vertices,
            &mut triangles,
        )?;
        surfaces.push(surface);
    }

    let mut portals = reserved(portal_count.min(cursor.remaining()))?;
    for _ in 0..portal_count {
        portals.push(portal(&mut cursor, &points)?);
    }

    if cursor.remaining() != 0 {
        return Err(Error::InvalidFormat(Malformation::Count));
    }

    // **Dérivé ici**, comme l'étendue dont il vient : l'hôte s'en sert pour
    // pondérer une barre de progression avant d'appeler la cuisson, et le
    // recalculer à chaque appel ferait parcourir les surfaces pour une valeur
    // que le chargement connaît déjà. Les surfaces qui refusent la lightmap n'y
    // comptent pas — la cuisson ne les visite pas.
    let luxel_count = surfaces
        .iter()
        .filter(|surface| !surface.skips_lightmap())
        .map(|surface| u64::from(surface.luxels.width) * u64::from(surface.luxels.height))
        .sum::<u64>()
        .min(u64::from(u32::MAX)) as u32;

    // Dérivée ici pour la même raison que le compte de luxels, et parce que
    // `docs/rust.md` la range depuis l'origine parmi ce que le chargement dérive.
    // La recalculer à chaque appel qui en a besoin ferait reparcourir les sommets
    // d'une cellule pour une valeur qui ne change jamais.
    let bounds = bounds_of(&vertices);
    let outward = outward_of(&vertices, &surfaces, &portals);
    classify_edges(&vertices, &mut surfaces, outward);

    Ok(Cell {
        id,
        flags,
        vertices,
        triangles,
        surfaces,
        portals,
        luxel_count,
        bounds,
        outward,
    })
}

/// Marque, sur chaque surface solide, les arêtes qui portent un volume propre.
///
/// **La règle de l'arête partagée, dont `docs/rust.md` porte les trois clauses et
/// leur raison.** Une arête est partagée quand une autre surface solide de la
/// même cellule porte les deux mêmes positions, comparées **au bit près** ; le
/// dièdre est alors rentrant, coplanaire ou saillant, et seul le saillant garde
/// son volume.
///
/// **Exacte et dérivée ici, jamais stockée ni approchée.** L'usage est de marquer
/// les arêtes internes dans les données, ou de poser un seuil de recalage de la
/// normale au contact. Le premier est un lien stocké, donc une occasion
/// d'incohérence que l'éditeur devrait maintenir à chaque opération — l'argument
/// même qui interdit de stocker les liens de portails. Le second n'est juste que
/// sur les décors où on l'a réglé. Ici le verdict est un **signe**, et un epsilon
/// rendrait la relation non transitive, donc dépendante de l'ordre de parcours.
///
/// Les surfaces non solides n'entrent pas dans l'appariement : elles n'arrêtent
/// rien, donc l'arête que l'une d'elles borde reste exposée pour sa voisine.
fn classify_edges(vertices: &[VertexUv], surfaces: &mut [Surface], outward: bool) {
    // Toutes exposées par défaut : l'appariement ne fait qu'éteindre des bits, ce
    // qui rend une surface isolée correcte sans cas particulier.
    for surface in surfaces.iter_mut() {
        surface.edges = mask_of(surface.corners.len());
    }

    for a in 0..surfaces.len() {
        if !surfaces[a].is_solid() {
            continue;
        }
        let count_a = surfaces[a].corners.len();
        for i in 0..count_a {
            let (p, q) = edge_of(vertices, &surfaces[a], i);
            for b in 0..surfaces.len() {
                if b == a || !surfaces[b].is_solid() {
                    continue;
                }
                let Some(j) = shares_edge(vertices, &surfaces[b], p, q) else {
                    continue;
                };
                // Le sommet voisin de l'arête dans l'autre surface, et non un
                // sommet quelconque : c'est le dièdre **local** qui décide, et
                // sur un polygone concave un sommet lointain donnerait un autre
                // verdict que la géométrie du coin.
                let count_b = surfaces[b].corners.len();
                let neighbour = corner_of(vertices, &surfaces[b], (j + 2) % count_b);
                let inward_a = if outward {
                    -surfaces[a].normal
                } else {
                    surfaces[a].normal
                };
                if inward_a.dot(neighbour - p) >= 0.0 {
                    surfaces[a].edges &= !(1 << i);
                }
                break;
            }
        }
    }
}

/// Éteint les arêtes partagées **au travers d'un portail apparié**.
///
/// **Une seconde passe, et elle ne peut pas être la première.** [`classify_edges`]
/// tourne au chargement d'une cellule, donc avant que les portails soient
/// appariés : à ce moment, rien ne dit encore quelles cellules se touchent. Et
/// chaque cellule possède ses sommets en propre — c'est la condition de
/// l'appariement —, si bien que la comparaison se fait sur les **positions** et
/// jamais sur des indices.
///
/// Ce que son absence coûtait : le sol d'une cellule et celui de la voisine sont
/// coplanaires et partagent l'arête du seuil, mais chacun la voyait exposée. Elle
/// portait donc un prisme de part et d'autre, et une boîte posée **dans la bande
/// de dilatation** — ce qu'un balayage précédent produit par construction — s'y
/// arrêtait net : un mur invisible à chaque jointure, sur un sol horizontal qui ne
/// peut pas arrêter un mouvement horizontal.
///
/// Le coût est quadratique en surfaces d'un **couple** de cellules voisines, pas
/// de la carte : quelques milliers de produits scalaires au chargement d'un décor
/// de plusieurs milliers de cellules, une fois.
fn classify_edges_across_cells(cells: &mut [Cell]) -> Result<()> {
    // Les couples voisins, dédupliqués : un portail apparié lie ses deux cellules
    // des deux côtés, et le même couple reviendrait autant de fois qu'il a de
    // portails communs.
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    for (a, cell) in cells.iter().enumerate() {
        for portal in &cell.portals {
            let Some((b, _)) = portal.link else {
                continue;
            };
            let a = a as u32;
            pairs.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
            pairs.push(if a < b { (a, b) } else { (b, a) });
        }
    }
    pairs.sort_unstable();
    pairs.dedup();

    // Les extinctions se collectent avant d'être appliquées : les deux cellules
    // d'un couple se lisent en même temps, et aucune ne peut l'être en exclusif
    // pendant ce temps.
    let mut off: Vec<(u32, u32, u32)> = Vec::new();
    for (a, b) in pairs {
        collect_shared_edges(cells, a, b, &mut off)?;
        collect_shared_edges(cells, b, a, &mut off)?;
    }
    for (cell, surface, edge) in off {
        cells[cell as usize].surfaces[surface as usize].edges &= !(1u64 << edge);
    }
    Ok(())
}

/// Les arêtes de `a` qu'une surface solide de `b` rend intérieures au bord.
///
/// Le verdict est celui de [`classify_edges`], au mot : la normale **intérieure**
/// de la surface de `a`, le sommet voisin de l'arête dans celle de `b`, et un
/// signe — jamais une tolérance, qui rendrait la relation non transitive.
fn collect_shared_edges(
    cells: &[Cell],
    a: u32,
    b: u32,
    out: &mut Vec<(u32, u32, u32)>,
) -> Result<()> {
    let (left, right) = (&cells[a as usize], &cells[b as usize]);
    for (sa, surface) in left.surfaces.iter().enumerate() {
        if !surface.is_solid() {
            continue;
        }
        for i in 0..surface.corners.len() {
            // Une arête déjà éteinte dans sa propre cellule n'a rien à gagner ici.
            if !surface.edge_is_exposed(i) {
                continue;
            }
            let (p, q) = edge_of(&left.vertices, surface, i);
            for other in &right.surfaces {
                if !other.is_solid() {
                    continue;
                }
                let Some(j) = shares_edge(&right.vertices, other, p, q) else {
                    continue;
                };
                let count = other.corners.len();
                let neighbour = corner_of(&right.vertices, other, (j + 2) % count);
                if left.inward(surface).dot(neighbour - p) >= 0.0 {
                    out.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
                    out.push((a, sa as u32, i as u32));
                }
                break;
            }
        }
    }
    Ok(())
}

/// Le masque de toutes les arêtes d'un polygone de `count` coins.
///
/// `count` ne dépasse jamais [`MAX_POLYGON`], que la triangulation a déjà refusé
/// au-delà ; le décalage de 64 bits, qui serait indéfini, ne peut donc pas se
/// produire — et il est écrit tout de même, parce qu'un plafond qui changerait
/// ailleurs ne ferait rougir aucun test ici.
fn mask_of(count: usize) -> u64 {
    if count >= 64 {
        u64::MAX
    } else {
        (1 << count) - 1
    }
}

/// Les deux positions de l'arête partant du coin `index`.
fn edge_of(vertices: &[VertexUv], surface: &Surface, index: usize) -> (Vec3, Vec3) {
    let count = surface.corners.len();
    (
        corner_of(vertices, surface, index),
        corner_of(vertices, surface, (index + 1) % count),
    )
}

/// La position du `index`-ième coin d'une surface, parmi les sommets dérivés.
fn corner_of(vertices: &[VertexUv], surface: &Surface, index: usize) -> Vec3 {
    vertices[surface.first_vertex as usize + index].position
}

/// Le rang, dans `surface`, de l'arête qui porte les mêmes positions que `(p, q)`.
///
/// **Au bit près et dans les deux sens** : deux surfaces adjacentes bien formées
/// parcourent leur arête commune en sens inverse, mais rien dans le format ne
/// l'impose, et une carte en cours d'édition peut porter les deux. Comparer par
/// `to_bits` plutôt que par `==` écarte le cas où un zéro négatif d'un côté
/// répondrait égal à un zéro positif de l'autre : ce sont deux écritures, et la
/// relation doit rester une égalité d'octets.
fn shares_edge(vertices: &[VertexUv], surface: &Surface, p: Vec3, q: Vec3) -> Option<usize> {
    let count = surface.corners.len();
    for j in 0..count {
        let (a, b) = edge_of(vertices, surface, j);
        if (same(a, p) && same(b, q)) || (same(a, q) && same(b, p)) {
            return Some(j);
        }
    }
    None
}

/// Deux positions écrites avec exactement les mêmes octets.
fn same(a: Vec3, b: Vec3) -> bool {
    a.x.to_bits() == b.x.to_bits()
        && a.y.to_bits() == b.y.to_bits()
        && a.z.to_bits() == b.z.to_bits()
}

/// Les normales de Newell de cette cellule pointent-elles vers l'extérieur ?
///
/// Six fois le volume signé : la somme, sur les faces qui ferment la cellule, du
/// produit scalaire d'un de leurs points par leur normale brute. **Les portails
/// en sont**, sans quoi la cellule n'est pas fermée et la somme ne vaut rien —
/// une pièce dont on retire une porte n'a plus de volume défini.
///
/// L'ordre de sommation est celui du fichier, surfaces puis portails : c'est une
/// valeur dérivée, donc son ordre d'opérations est contractuel comme les autres.
///
/// **Les points sont ramenés sur un sommet de la cellule avant d'être sommés**, et
/// c'est ce qui rend la somme invariante par translation. Elle ne l'est de
/// elle-même que si les normales se compensent exactement ; sinon le résidu croît
/// avec la distance à l'origine et finit par dominer le volume, si bien que le
/// signe **bascule selon l'endroit où la cellule est posée dans le monde**. Une
/// cellule retournée n'arrête plus rien du balayage, toutes ses normales étant à
/// l'envers — mesuré chez un intégrateur sur un décor de 64 unités de côté, où un
/// tiers des cellules, les plus éloignées, étaient muettes.
///
/// Ce rappel seul ne rend pas le signe juste, il le rend **stable** : c'est
/// l'orientation des portails ci-dessous qui le rend juste, et les deux sont
/// nécessaires — la première ferme la classe, la seconde le cas.
fn outward_of(vertices: &[VertexUv], surfaces: &[Surface], portals: &[Portal]) -> bool {
    let Some(origin) = vertices.first().map(|vertex| vertex.position) else {
        return false;
    };

    let mut volume = 0.0;
    for surface in surfaces {
        let anchor = vertices[surface.first_vertex as usize].position - origin;
        volume += anchor.dot(surface.normal);
    }
    for portal in portals {
        if let Some(&first) = portal.points.first() {
            let normal = crate::math::polygon::newell(&portal.points);
            // Remis dans le sens des surfaces, faute de quoi il compte à l'envers.
            let normal = if portal_is_reversed(vertices, surfaces, portal) {
                -normal
            } else {
                normal
            };
            volume += (first - origin).dot(normal);
        }
    }
    volume > 0.0
}

/// Ce portail tourne-t-il à l'envers des surfaces de sa cellule ?
///
/// **Le format ne le dit pas, et il faut donc le dériver.** Il ne fixe que
/// l'enroulement inverse **entre les deux portails d'une paire** ; celui d'un
/// portail par rapport aux surfaces de sa propre cellule reste libre, et un
/// générateur légitime les écrit à l'envers. Or le volume signé les somme avec
/// les surfaces : comptés à l'envers, ils en faussent le signe.
///
/// La règle vient de la géométrie et de rien d'autre : **sur une surface fermée
/// orientée, deux faces adjacentes parcourent leur arête commune en sens
/// opposé.** Un portail qui parcourt la sienne dans le même sens que la surface
/// voisine est donc retourné. La comparaison est exacte, au bit près, comme celle
/// qui apparie les portails — un epsilon rendrait la relation non transitive.
///
/// **Précondition : un portail partage au moins une arête avec une surface de sa
/// cellule.** Elle tient par construction d'un décor plein — un portail remplace
/// un mur et reprend ses sommets, et un polygone simple n'a pas de trou où
/// faire flotter une ouverture. Quand elle ne tient pas, on rend `false` : le
/// portail compte dans son sens écrit, ce qui n'est pas pire que ce qui précédait
/// ce correctif, et le rappel à l'origine garde le signe stable.
fn portal_is_reversed(vertices: &[VertexUv], surfaces: &[Surface], portal: &Portal) -> bool {
    let count = portal.points.len();
    for i in 0..count {
        let p = portal.points[i];
        let q = portal.points[(i + 1) % count];
        for surface in surfaces {
            for j in 0..surface.corners.len() {
                let (a, b) = edge_of(vertices, surface, j);
                if same(a, p) && same(b, q) {
                    return true;
                }
                if same(a, q) && same(b, p) {
                    return false;
                }
            }
        }
    }
    false
}

/// Les deux coins de la boîte englobante d'un jeu de sommets dérivés.
///
/// Par comparaisons écrites, jamais par `f32::min` : celle-ci ne spécifie pas le
/// signe qu'elle rend de `min(-0,0, 0,0)`, et deux cibles donneraient deux boîtes
/// pour la même carte. Les coordonnées sont finies, le refus des non-finis à la
/// lecture le garantit, donc une comparaison suffit et rend les mêmes bits
/// partout.
///
/// Sans sommet, la boîte sort **inversée**, et c'est ce que [`Cell::bounds`]
/// documente.
fn bounds_of(vertices: &[VertexUv]) -> [Vec3; 2] {
    let mut low = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut high = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for vertex in vertices {
        let p = vertex.position;
        if p.x < low.x {
            low.x = p.x;
        }
        if p.y < low.y {
            low.y = p.y;
        }
        if p.z < low.z {
            low.z = p.z;
        }
        if p.x > high.x {
            high.x = p.x;
        }
        if p.y > high.y {
            high.y = p.y;
        }
        if p.z > high.z {
            high.z = p.z;
        }
    }
    [low, high]
}

/// Une capacité déduite d'un compte, refusée si elle dépasse ce qui reste.
///
/// **Aucune capacité ne vient d'un nombre déclaré seul** : le produit se recoupe
/// avec les octets restants, et le `checked_mul` compte parce que `usize` fait
/// 32 bits sur deux des quatre cibles.
fn bounded(count: usize, size: usize, remaining: usize) -> Result<usize> {
    let needed = count
        .checked_mul(size)
        .ok_or(Error::InvalidFormat(Malformation::Truncated))?;
    if needed > remaining {
        return Err(Error::InvalidFormat(Malformation::Truncated));
    }
    Ok(count)
}

/// Une surface : son en-tête, ses indices, ses deux repères, puis sa découpe.
fn surface(
    cursor: &mut Cursor<'_>,
    points: &[Vec3],
    materials: &[u32],
    vertices: &mut Vec<VertexUv>,
    triangles: &mut Vec<[u32; 3]>,
) -> Result<Surface> {
    let id = cursor.u32()?;
    let flags = cursor.u32()?;
    let material = cursor.u32()?;
    let index_count = cursor.u32()? as usize;

    if id == 0 {
        return Err(Error::InvalidFormat(Malformation::Identifier));
    }
    if flags & !SURFACE_FLAGS != 0 {
        return Err(Error::InvalidFormat(Malformation::Flags));
    }
    // L'identifiant se résout en rang ici, une fois : c'est ce que la
    // soumission lira, et une recherche par surface et par image serait
    // invisible — elle ne ferait rougir aucun test et ne changerait aucune
    // empreinte.
    let material = materials
        .iter()
        .position(|known| *known == material)
        .ok_or(Error::InvalidFormat(Malformation::Index))? as u32;
    if index_count > MAX_POLYGON {
        return Err(Error::InvalidFormat(Malformation::Polygon));
    }
    bounded(index_count, 4, cursor.remaining())?;

    let mut corners = [Vec3::ZERO; MAX_POLYGON];
    // Les indices se gardent en plus des points : c'est le polygone que le calcul
    // d'éclairage interroge, et il n'est nulle part ailleurs une fois la surface
    // triangulée.
    let mut indices = reserved(index_count)?;
    for corner in corners.iter_mut().take(index_count) {
        let index = cursor.u32()? as usize;
        *corner = *points
            .get(index)
            .ok_or(Error::InvalidFormat(Malformation::Index))?;
        indices.push(index as u32);
    }
    let corners = &corners[..index_count];

    let texture = mapping(cursor)?;
    let lightmap = mapping(cursor)?;
    // Le repère de lightmap aligne les grilles de deux surfaces coplanaires
    // adjacentes, et c'est ce qui évite une marche d'éclairage à leur jointure.
    // Vérifié plutôt que conventionnel : une carte qui ne respecterait pas cet
    // alignement ne se verrait qu'à la première capture d'éclairage.
    if !aligned(lightmap, corners) {
        return Err(Error::InvalidFormat(Malformation::Mapping { surface: id }));
    }
    let luxels = luxel_extent(lightmap, corners)
        .ok_or(Error::InvalidFormat(Malformation::Mapping { surface: id }))?;

    let mut cut = [[0u32; 3]; MAX_POLYGON];
    let count =
        triangulate(corners, &mut cut).ok_or(Error::InvalidFormat(Malformation::Polygon))?;

    let first_vertex = vertices.len() as u32;
    let first_triangle = triangles.len() as u32;
    vertices
        .try_reserve(index_count)
        .map_err(|_| Error::OutOfMemory)?;
    triangles
        .try_reserve(count)
        .map_err(|_| Error::OutOfMemory)?;

    let uvs = fold(corners, texture, id)?;
    for (corner, (u, v)) in corners.iter().zip(&uvs[..index_count]) {
        vertices.push(VertexUv {
            position: *corner,
            u: *u,
            v: *v,
        });
    }
    for triangle in &cut[..count] {
        triangles.push(triangle.map(|index| index + first_vertex));
    }

    Ok(Surface {
        id,
        flags,
        material,
        first_triangle,
        triangle_count: count as u32,
        lightmap,
        corners: indices,
        first_vertex,
        luxels,
        // Sur `corners`, qui porte les positions que les sommets dérivés
        // reprennent telles quelles : c'est mot pour mot ce que la cuisson
        // recalculait à chaque appel, dans le même ordre, donc au bit près la
        // même normale.
        normal: crate::math::polygon::newell(corners),
        // Posé par `classify_edges`, une fois la cellule complète : l'appariement
        // a besoin de toutes ses surfaces, et une surface ne connaît pas ses
        // voisines au moment où elle se décode.
        edges: 0,
    })
}

/// Un repère : origine, axe `u`, axe `v`.
fn mapping(cursor: &mut Cursor<'_>) -> Result<Mapping> {
    let mut axes = [Vec3::ZERO; 3];
    for axis in &mut axes {
        let x = cursor.f32()?;
        let y = cursor.f32()?;
        let z = cursor.f32()?;
        *axis = Vec3::new(x, y, z);
    }
    Ok(Mapping {
        origin: axes[0],
        u: axes[1],
        v: axes[2],
    })
}

/// Le produit scalaire de deux vecteurs, en double précision.
///
/// Le `f64` est permis hors image, et ces contrôles n'ont aucun budget à tenir :
/// il évite de se demander si le produit de grandes coordonnées déborde, sans
/// rien coûter à personne. IEEE 754 impose les quatre opérations au bit près en
/// `f64` comme en `f32`, donc le verdict est le même sur toutes les cibles.
fn dot64(a: Vec3, b: Vec3) -> f64 {
    f64::from(a.x) * f64::from(b.x)
        + f64::from(a.y) * f64::from(b.y)
        + f64::from(a.z) * f64::from(b.z)
}

/// Vrai si la valeur est un entier exact.
///
/// Bornée avant la conversion, comme toute conversion du projet : `as` sature, et
/// la saturation ne sert jamais de bornage.
fn whole(value: f64) -> bool {
    if !value.is_finite() || value < -9.007_199_254_740_992e15 || value > 9.007_199_254_740_992e15 {
        return false;
    }
    value == (value as i64) as f64
}

/// Vrai si `dot` est nul à la tolérance relative près, les deux carrés de
/// longueur étant donnés.
///
/// Comparé au carré pour n'appeler aucune racine : la forme `(a·b)² ≤ ε²·|a|²·|b|²`
/// dit la même chose que « l'angle est droit à ε près » sans quitter les quatre
/// opérations.
fn perpendicular(dot: f64, square_a: f64, square_b: f64) -> bool {
    dot * dot <= SQUARE_TOLERANCE * SQUARE_TOLERANCE * square_a * square_b
}

/// Vrai si le repère de lightmap d'une surface est utilisable pour la cuire.
///
/// Trois propriétés, et aucune ne remplace une autre.
///
/// La longueur au carré de chaque axe est **finie et non nulle**, et rien de
/// plus : un axe dégénéré n'a pas de direction, donc pas de réciproque.
///
/// **La puissance de deux n'est plus exigée, et l'exiger interdisait toute pente
/// autre que 45°.** Elle existait pour rendre `1/|u|²` exact, mais la cuisson
/// **divise de toute façon** — une fois par surface, jamais par luxel —, si bien
/// que le contrôle ne supprimait pas la division : il en rendait seulement le
/// quotient exact. Une division IEEE est exactement arrondie, donc elle rend les
/// mêmes bits sur toutes les cibles, et c'est tout ce que le déterminisme demande.
/// Ce qui était interdit à bon droit reste interdit : une division **par luxel**.
///
/// Ce que la clause coûtait, mesuré sur un décor réel : un repère écrit en nombres
/// ronds impose à `course² + montée²` d'être une puissance de deux, ce qui ne
/// laisse que `course = montée` ou l'axial. Une rampe 1:2, 1:3, 2:3, à 30°, à 60°,
/// ou de normale `(1, 1, 1)` faisait donc **refuser le fichier entier** — et le
/// drapeau « ne reçoit pas de lightmap » n'y échappait pas, ce contrôle étant
/// antérieur à toute lecture de drapeau.
///
/// L'origine tombe sur un nœud de sa propre grille, mesurée depuis le zéro du
/// monde. Le contrôle est local à la surface et emporte le global : deux origines
/// à coordonnée entière diffèrent d'un entier, donc leurs grilles coïncident,
/// alors que comparer les surfaces deux à deux serait quadratique et n'aurait
/// aucun sens pour une surface isolée.
///
/// Les axes sont **orthogonaux**, faute de quoi la reconstruction demande
/// l'inverse d'une 2×2 quelconque, donc une division. Et ils sont **dans le plan
/// de la surface**, faute de quoi la grille ne recouvre pas ce qu'elle éclaire.
/// Ces deux derniers contrôles tolèrent un résidu, pour la raison écrite sur
/// [`SQUARE_TOLERANCE`].
fn aligned(mapping: Mapping, corners: &[Vec3]) -> bool {
    let square_u = mapping.u.dot(mapping.u);
    let square_v = mapping.v.dot(mapping.v);
    // Un axe dégénéré n'a pas de direction à porter, et sa réciproque n'existe
    // pas : c'est tout ce que la longueur doit garantir.
    if !(square_u > 0.0 && square_u.is_finite() && square_v > 0.0 && square_v.is_finite()) {
        return false;
    }

    // **Sans diviser par le carré de la longueur**, comme [`Mapping::project`] et
    // comme [`luxel_extent`] : le produit scalaire *est* la coordonnée, donc
    // c'est lui qui doit être entier. La division rendait ce contrôle faux dans
    // les deux sens, et neutre sur un axe unitaire — ce que sont tous les décors
    // du dépôt, d'où une version entière sans que rien ne le montre. Sur un axe
    // de carré `1/4`, une origine à un quart de luxel d'un nœud donnait un
    // quotient entier et passait ; sur un axe de carré `4`, une origine
    // exactement sur un nœud était refusée.
    if !whole(dot64(mapping.origin, mapping.u)) || !whole(dot64(mapping.origin, mapping.v)) {
        return false;
    }

    if !perpendicular(
        dot64(mapping.u, mapping.v),
        f64::from(square_u),
        f64::from(square_v),
    ) {
        return false;
    }

    // Le plan vient de la normale de Newell, celle-là même dont la convexité se
    // sert : elle n'est pas normalisée, ce dont la forme au carré n'a pas besoin.
    let normal = crate::math::polygon::newell(corners);
    let square_n = dot64(normal, normal);
    perpendicular(dot64(normal, mapping.u), square_n, f64::from(square_u))
        && perpendicular(dot64(normal, mapping.v), square_n, f64::from(square_v))
}

/// L'entier immédiatement inférieur ou égal, sans passer par la bibliothèque.
///
/// `floor` est de celles que `clippy.toml` refuse : son résultat dépend de
/// l'implémentation, et une carte serait refusée ici et acceptée là. La
/// conversion `as` tronque vers zéro, ce qui n'est le plancher que pour un
/// positif — d'où la correction écrite, et le bornage qui la précède.
fn floor_i32(value: f64) -> Option<i32> {
    if !value.is_finite() || value < -1.0e9 || value > 1.0e9 {
        return None;
    }
    let truncated = value as i32;
    Some(if (truncated as f64) > value {
        truncated - 1
    } else {
        truncated
    })
}

/// L'étendue d'une surface dans son repère de lightmap, ou `None` au-delà du
/// plafond.
///
/// **Les bornes sont des nœuds de la grille, pas les sommets eux-mêmes** : le
/// plancher du minimum et le plafond du maximum, si bien qu'aucun point de la
/// surface ne tombe hors de la grille qui l'éclaire. Un luxel de plus de chaque
/// côté ne coûte rien et évite d'avoir à raisonner sur le cas où un sommet tombe
/// pile sur un nœud.
///
/// Les extrema se prennent par comparaisons écrites et non par `f32::min`, dont
/// le résultat sur `min(-0,0, 0,0)` n'est pas spécifié : deux cibles refuseraient
/// alors des cartes différentes.
fn luxel_extent(mapping: Mapping, corners: &[Vec3]) -> Option<Extent> {
    let mut bounds = [(f64::MAX, f64::MIN); 2];

    for corner in corners {
        let offset = Vec3::new(
            corner.x - mapping.origin.x,
            corner.y - mapping.origin.y,
            corner.z - mapping.origin.z,
        );
        // **Sans diviser par le carré de la longueur**, comme [`Mapping::project`] :
        // dans ce format, c'est la longueur de l'axe qui porte l'échelle, si bien
        // que le produit scalaire *est* la coordonnée. Diviser ici donnerait une
        // étendue en désaccord avec les coordonnées que la soumission calcule — et
        // rien ne le montrerait tant que les axes sont unitaires, ce qu'ils sont
        // dans tous les décors du dépôt.
        let coordinates = [dot64(offset, mapping.u), dot64(offset, mapping.v)];
        for (bound, value) in bounds.iter_mut().zip(coordinates) {
            if !value.is_finite() {
                return None;
            }
            if value < bound.0 {
                bound.0 = value;
            }
            if value > bound.1 {
                bound.1 = value;
            }
        }
    }

    let mut nodes = [(0i32, 0u32); 2];
    for (node, (low, high)) in nodes.iter_mut().zip(bounds) {
        if high - low > f64::from(MAX_LUXELS) {
            return None;
        }
        let first = floor_i32(low)?;
        // Le nœud qui suit le maximum : le plancher, plus un dès que le maximum
        // n'est pas déjà sur un nœud.
        let last = floor_i32(high)?;
        let last = if (last as f64) < high { last + 1 } else { last };
        let span = last.checked_sub(first)?;
        *node = (first, span as u32 + 1);
    }

    Some(Extent {
        min_u: nodes[0].0,
        min_v: nodes[1].0,
        width: nodes[0].1,
        height: nodes[1].1,
    })
}

/// Les coordonnées de texture d'une surface, repliées dans `[0, 2048)`.
///
/// **Un plaquage fin sur une grande surface dépasse trivialement la borne de
/// l'ABI**, et la soumission refuserait alors le lot entier sans dire quelle
/// surface. Le repli soustrait un multiple entier de 2048 texels, le même pour
/// tous les sommets : les côtés de texture sont des puissances de deux d'au plus
/// 2048 et le repli se fait par masque, si bien que **l'image est identique au
/// bit près** — les dérivées, donc le niveau de mipmap et le motif de tramage,
/// sont invariantes par translation.
fn fold(corners: &[Vec3], mapping: Mapping, surface: u32) -> Result<[(f32, f32); MAX_POLYGON]> {
    let mut raw = [(0.0f32, 0.0f32); MAX_POLYGON];
    let mut low = (f32::MAX, f32::MAX);
    for (slot, corner) in raw.iter_mut().zip(corners) {
        *slot = mapping.project(*corner);
        if !slot.0.is_finite() || !slot.1.is_finite() {
            return Err(Error::InvalidFormat(Malformation::Mapping { surface }));
        }
        // Par comparaison et non par `f32::min`, que le projet interdit : son
        // résultat sur deux zéros de signes opposés n'est pas spécifié, et deux
        // cibles replieraient la surface différemment.
        if slot.0 < low.0 {
            low.0 = slot.0;
        }
        if slot.1 < low.1 {
            low.1 = slot.1;
        }
    }

    let step = MAX_TEXTURE_SIZE as f32;
    let shift = |value: f32| {
        let quotient = value / step;
        // **Au-delà de 2²⁴, un `f32` ne porte plus tous les entiers**, et
        // soustraire le multiple cesserait d'être exact : le repli déchirerait
        // la surface au lieu de la déplacer. Une comparaison encadrante plutôt
        // qu'une valeur absolue, qui refuse aussi un `NaN` — un repère dont les
        // axes annulent la différence en produit un.
        if !(quotient > -16_777_216.0 && quotient < 16_777_216.0) {
            return None;
        }
        // Le multiple à retirer, arrondi vers le bas. La conversion tronque vers
        // zéro, d'où l'ajustement : un arrondi qui dépendrait du signe
        // laisserait des coordonnées négatives, que la soumission refuserait.
        let truncated = quotient as i32;
        let floor = if quotient < truncated as f32 {
            truncated - 1
        } else {
            truncated
        };
        Some(floor as f32 * step)
    };
    let offset = match (shift(low.0), shift(low.1)) {
        (Some(u), Some(v)) => (u, v),
        _ => return Err(Error::InvalidFormat(Malformation::Mapping { surface })),
    };

    for slot in raw.iter_mut().take(corners.len()) {
        slot.0 -= offset.0;
        slot.1 -= offset.1;
        // **Le repli ramène le coin le plus bas dans la fenêtre, pas la surface
        // entière** : une face immense sous un plaquage fin s'étale au-delà, et
        // c'est structurel. Ce qui reste dehors est refusé ici plutôt qu'à la
        // soumission, qui rejetterait le lot entier sans dire quelle surface —
        // et c'est tout ce que le repli existe pour éviter.
        let within = |value: f32| (0.0..=MAX_TEXEL_COORD).contains(&value);
        if !within(slot.0) || !within(slot.1) {
            return Err(Error::InvalidFormat(Malformation::Mapping { surface }));
        }
    }
    Ok(raw)
}

/// Un portail : son identifiant et ses sommets.
fn portal(cursor: &mut Cursor<'_>, points: &[Vec3]) -> Result<Portal> {
    let id = cursor.u32()?;
    let index_count = cursor.u32()? as usize;

    if id == 0 {
        return Err(Error::InvalidFormat(Malformation::Identifier));
    }
    if !(3..=MAX_POLYGON).contains(&index_count) {
        return Err(Error::InvalidFormat(Malformation::Polygon));
    }
    bounded(index_count, 4, cursor.remaining())?;

    let mut corners = reserved(index_count)?;
    for _ in 0..index_count {
        let index = cursor.u32()? as usize;
        corners.push(
            *points
                .get(index)
                .ok_or(Error::InvalidFormat(Malformation::Index))?,
        );
    }
    // **La convexité est exigée là où elle ne l'est pas pour la cellule**, parce
    // que le portail décide de ce qu'on voit : la traversée réduira la fenêtre
    // de découpe à l'intersection de la fenêtre courante et de la projection du
    // portail, et une projection concave n'a pas d'intersection exprimable
    // comme réduction de fenêtre. L'erreur se paierait en trou définitif.
    if !convex(&corners) {
        return Err(Error::InvalidFormat(Malformation::Polygon));
    }

    Ok(Portal {
        id,
        points: corners,
        link: None,
    })
}

/// Vrai si le polygone est plan, convexe et d'orientation constante.
fn convex(points: &[Vec3]) -> bool {
    let normal = crate::math::polygon::newell(points);
    if normal.dot(normal) == 0.0 {
        return false;
    }
    let mut sign = 0.0f32;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let c = points[(i + 2) % points.len()];
        let ab = Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let bc = Vec3::new(c.x - b.x, c.y - b.y, c.z - b.z);
        let turn = Vec3::new(
            ab.y * bc.z - ab.z * bc.y,
            ab.z * bc.x - ab.x * bc.z,
            ab.x * bc.y - ab.y * bc.x,
        )
        .dot(normal);
        if turn < 0.0 || (sign == 0.0 && turn == 0.0) {
            // Un virage à l'envers, ou trois sommets alignés : le second rend le
            // polygone ambigu pour la traversée, qui a besoin d'arêtes franches.
            return false;
        }
        if turn > 0.0 {
            sign = turn;
        }
    }
    // La planéité vient avec : un polygone gauche a des virages qui ne sont pas
    // tous du même côté de la normale de Newell, et l'un d'eux est négatif.
    true
}

/// Apparie les portails qui partagent tous leurs sommets.
///
/// **Au bit près, sans tolérance.** Un ε donnerait une relation non transitive —
/// `a≈b`, `b≈c`, `a≉c` —, donc un appariement dépendant de l'ordre de parcours,
/// ce que le déterminisme interdit. C'est l'éditeur qui écrit les mêmes octets
/// des deux côtés, et c'est une clause du format.
///
/// Par tri de clés canoniques et non par table de hachage : le noyau n'en a
/// aucune, et l'ordre d'itération d'une table n'est pas contractuel.
fn link_portals(cells: &mut [Cell]) -> Result<()> {
    let mut keys: Vec<(Vec<[u32; 3]>, u32, u32)> = Vec::new();
    for (c, cell) in cells.iter().enumerate() {
        for (p, portal) in cell.portals.iter().enumerate() {
            let mut key = reserved(portal.points.len())?;
            for point in &portal.points {
                key.push(bits(*point));
            }
            // Les deux portails d'une paire ont des enroulements inverses : la
            // clé se trie donc, pour qu'un même ensemble de sommets donne la
            // même suite des deux côtés.
            key.sort_unstable();
            keys.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
            keys.push((key, c as u32, p as u32));
        }
    }
    keys.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    let mut i = 0;
    while i < keys.len() {
        let mut j = i + 1;
        while j < keys.len() && keys[j].0 == keys[i].0 {
            j += 1;
        }
        match j - i {
            // Un portail non apparié est un mur, pas une erreur : une carte en
            // cours d'édition en a toujours.
            1 => {}
            2 => {
                let first = (keys[i].1, keys[i].2);
                let second = (keys[i + 1].1, keys[i + 1].2);
                // Deux portails d'une même cellule ne s'apparient pas : le lien
                // ramènerait sur la cellule courante, et la traversée tournerait
                // sur place au lieu d'avancer.
                if first.0 == second.0 {
                    return Err(Error::InvalidFormat(Malformation::Portal));
                }
                cells[first.0 as usize].portals[first.1 as usize].link = Some(second);
                cells[second.0 as usize].portals[second.1 as usize].link = Some(first);
            }
            // Trois portails sur la même clé n'ont pas de réponse à « lequel des
            // deux ».
            _ => return Err(Error::InvalidFormat(Malformation::Portal)),
        }
        i = j;
    }
    Ok(())
}

/// Les bits d'un point, en une clé comparable.
///
/// Sur `to_bits` et non sur la valeur : `-0,0` et `+0,0` sont égaux en
/// arithmétique, et deux portails que l'éditeur a écrits différents
/// s'apparieraient en silence.
///
/// **Les trois mots en entier, jamais mêlés en un seul.** Quatre-vingt-seize
/// bits ne tiennent pas dans soixante-quatre, et les réduire — par rotations et
/// `XOR`, ce que ce fichier faisait — rend l'appariement exact « à collision
/// près » : deux portails de sommets différents portent alors la même clé et se
/// lient en silence, sur des coordonnées de carte qui sont régulières par
/// construction. Le prix n'est pas un refus mais une traversée qui ouvre sur une
/// cellule non voisine, et ce défaut-là ne se rattrape pas localement.
fn bits(point: Vec3) -> [u32; 3] {
    [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()]
}

/// Refuse deux identifiants égaux dans une même famille.
///
/// Par tri puis passe adjacente. Le tableau trié ne se garde pas : la
/// correspondance par identifiant arrive avec les appels qui l'interrogent.
fn unique(ids: &[u32]) -> Result<()> {
    let mut sorted = reserved(ids.len())?;
    sorted.extend_from_slice(ids);
    sorted.sort_unstable();
    if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::InvalidFormat(Malformation::Identifier));
    }
    Ok(())
}

// `pub(crate)` pour ses constructeurs de cartes d'épreuve, que les tests de la
// traversée réemploient : ils sont le seul endroit du noyau où les décalages du
// format sont écrits une seconde fois, et deux copies divergeraient.
#[cfg(test)]
pub(crate) mod tests;
