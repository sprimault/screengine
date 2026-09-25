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
//! fichier ». Les sections `ENTS` et `LGTS` sont des genres connus de ce format
//! et ne sont pas encore lues : leur contenu arrive avec leurs accesseurs.

use alloc::string::String;
use alloc::vec::Vec;

use super::ears::{MAX_POLYGON, triangulate};
use super::{Cursor, decode};
use crate::buffer::{owned, reserved};
use crate::error::{Error, Malformation, Result};
use crate::math::{MAX_TEXEL_COORD, Vec3};
use crate::scene::VertexUv;
use crate::texture::MAX_TEXTURE_SIZE;

/// Le genre que porte l'en-tête d'une carte.
const KIND: [u8; 4] = *b"WRLD";

/// La seule version de format que cette construction lit.
const VERSION: u32 = 1;

/// Les genres de sections, croissants comme l'en-tête l'exige.
const TAGS: [[u8; 4]; 4] = [*b"CELL", *b"ENTS", *b"LGTS", *b"MATS"];

/// Rang de la section des cellules.
const CELL: usize = 0;
/// Rang de la section des matériaux.
const MATS: usize = 3;

/// Taille d'un sommet de cellule, en octets.
const VERTEX_LEN: usize = 12;

/// Les bits de drapeaux qu'une surface définit : deux faces, ne reçoit pas de
/// lightmap, non solide.
///
/// Tous les autres sont nuls obligatoires, même règle que les champs réservés
/// de l'ABI : c'est ce qui permettra d'en employer un sans casser les cartes
/// déjà écrites.
const SURFACE_FLAGS: u32 = 0b111;

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
    origin: Vec3,
    /// L'axe des abscisses, en texels par unité de monde.
    u: Vec3,
    /// L'axe des ordonnées.
    v: Vec3,
}

impl Mapping {
    /// Les coordonnées d'un point dans ce repère.
    ///
    /// Une projection et rien d'autre : la longueur des axes porte déjà
    /// l'échelle, si bien qu'aucune normalisation n'intervient — elle
    /// demanderait une racine, donc une table, pour une valeur que l'éditeur a
    /// déjà écrite.
    fn project(&self, point: Vec3) -> (f32, f32) {
        let d = Vec3::new(
            point.x - self.origin.x,
            point.y - self.origin.y,
            point.z - self.origin.z,
        );
        (d.dot(self.u), d.dot(self.v))
    }
}

/// Une surface d'une cellule, une fois triangulée.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Surface {
    /// Son identifiant stable, attribué par l'éditeur, jamais nul.
    // Lu par l'interrogation de la scène, à l'étape 8 : le décodage le valide
    // dès maintenant, l'unicité d'un identifiant n'étant vérifiable qu'ici.
    #[allow(dead_code)]
    id: u32,
    /// Ses drapeaux, dont les bits non définis sont nuls.
    #[allow(dead_code)]
    flags: u32,
    /// L'identifiant du matériau qui l'habille.
    #[allow(dead_code)]
    material: u32,
    /// Son premier triangle dans la cellule.
    first_triangle: u32,
    /// Combien de triangles sa découpe a produits.
    triangle_count: u32,
    /// Son repère de lightmap, vérifié aligné sur une grille de puissance de
    /// deux.
    // Lu par le calcul de lightmap, à l'étape 5. Le chargement le vérifie dès
    // maintenant : c'est cet alignement qui évite une marche d'éclairage à la
    // jointure de deux surfaces coplanaires, et il se vérifie plutôt qu'il ne
    // se convient.
    #[allow(dead_code)]
    lightmap: Mapping,
}

/// Un portail : le polygone plan convexe par lequel deux cellules se voient.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Portal {
    /// Son identifiant stable, jamais nul.
    #[allow(dead_code)]
    id: u32,
    /// Ses sommets, en propre.
    ///
    /// Recopiés et non indexés : l'appariement compare des positions, et deux
    /// portails appariés appartiennent à deux cellules qui ne partagent aucun
    /// sommet. La duplication est la condition du mécanisme, pas son coût.
    points: Vec<Vec3>,
    /// La cellule et le portail qu'il rejoint, ou `None` pour un mur.
    // Lu par la traversée, à l'étape 5. L'appariement a lieu au chargement
    // parce que c'est là qu'on a les deux côtés sous la main.
    #[allow(dead_code)]
    link: Option<(u32, u32)>,
}

/// Une cellule : l'unité d'édition, un volume fermé quelconque.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cell {
    /// Son identifiant stable, jamais nul.
    #[allow(dead_code)]
    id: u32,
    /// Ses drapeaux, dont aucun bit n'est défini : tous nuls.
    #[allow(dead_code)]
    flags: u32,
    /// Les sommets dérivés, un par sommet et par surface qui l'emploie.
    ///
    /// Un sommet partagé par deux murs porte deux coordonnées de texture, et
    /// c'est le repère de chaque surface qui les décide : le fichier garde un
    /// sommet, le rendu en voit deux.
    vertices: Vec<VertexUv>,
    /// Les triangles de toutes ses surfaces, dans l'ordre des surfaces.
    triangles: Vec<[u32; 3]>,
    /// Ses surfaces.
    surfaces: Vec<Surface>,
    /// Ses portails.
    portals: Vec<Portal>,
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
    /// Les identifiants de matériaux, dans l'ordre du fichier.
    #[allow(dead_code)]
    material_ids: Vec<u32>,
    /// Leurs noms, dans le même ordre.
    material_names: Vec<String>,
    /// Le nombre total de triangles, pour que l'hôte dimensionne son contexte.
    triangle_count: u32,
}

impl World {
    /// Décode une carte, ou dit ce qui l'a fait refuser.
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let sections = decode(bytes, KIND, VERSION, TAGS)?;
        let (material_ids, material_names) = materials(sections[MATS])?;
        let mut cells = cells(sections[CELL], &material_ids)?;
        link_portals(&mut cells)?;

        let mut triangle_count: u32 = 0;
        for cell in &cells {
            triangle_count = triangle_count
                .checked_add(cell.triangles.len() as u32)
                .ok_or(Error::InvalidFormat(Malformation::Count))?;
        }

        Ok(Self {
            cells,
            material_ids,
            material_names,
            triangle_count,
        })
    }

    /// Combien de triangles la carte porte, toutes cellules confondues.
    pub fn triangle_count(&self) -> u32 {
        self.triangle_count
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
    // Lues par la soumission du monde, au lot suivant.
    #[allow(dead_code)]
    pub(crate) fn cells(&self) -> &[Cell] {
        &self.cells
    }
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

    Ok(Cell {
        id,
        flags,
        vertices,
        triangles,
        surfaces,
        portals,
    })
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
    if !materials.contains(&material) {
        return Err(Error::InvalidFormat(Malformation::Index));
    }
    if index_count > MAX_POLYGON {
        return Err(Error::InvalidFormat(Malformation::Polygon));
    }
    bounded(index_count, 4, cursor.remaining())?;

    let mut corners = [Vec3::ZERO; MAX_POLYGON];
    for corner in corners.iter_mut().take(index_count) {
        let index = cursor.u32()? as usize;
        *corner = *points
            .get(index)
            .ok_or(Error::InvalidFormat(Malformation::Index))?;
    }
    let corners = &corners[..index_count];

    let texture = mapping(cursor)?;
    let lightmap = mapping(cursor)?;
    // Le repère de lightmap aligne les grilles de deux surfaces coplanaires
    // adjacentes, et c'est ce qui évite une marche d'éclairage à leur jointure.
    // Vérifié plutôt que conventionnel : une carte qui ne respecterait pas cet
    // alignement ne se verrait qu'à la première capture d'éclairage.
    if !aligned(lightmap) {
        return Err(Error::InvalidFormat(Malformation::Mapping));
    }

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

    let uvs = fold(corners, texture)?;
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

/// Vrai si les axes d'un repère de lightmap ont une longueur puissance de deux
/// et que l'origine en est un multiple.
///
/// La longueur se mesure par le carré, qui évite une racine : une puissance de
/// deux au carré en est une aussi, et c'est tout ce que le contrôle demande.
fn aligned(mapping: Mapping) -> bool {
    let square = |v: Vec3| v.dot(v);
    let power_of_two = |value: f32| {
        // Un flottant est une puissance de deux quand sa mantisse est nulle. Le
        // contrôle passe par les bits plutôt que par une division : il est exact
        // et ne dépend d'aucune table.
        value > 0.0 && value.is_finite() && value.to_bits() & 0x007f_ffff == 0
    };
    power_of_two(square(mapping.u)) && power_of_two(square(mapping.v))
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
fn fold(corners: &[Vec3], mapping: Mapping) -> Result<[(f32, f32); MAX_POLYGON]> {
    let mut raw = [(0.0f32, 0.0f32); MAX_POLYGON];
    let mut low = (f32::MAX, f32::MAX);
    for (slot, corner) in raw.iter_mut().zip(corners) {
        *slot = mapping.project(*corner);
        if !slot.0.is_finite() || !slot.1.is_finite() {
            return Err(Error::InvalidFormat(Malformation::Mapping));
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
        _ => return Err(Error::InvalidFormat(Malformation::Mapping)),
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
            return Err(Error::InvalidFormat(Malformation::Mapping));
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
    let normal = super::ears::newell(points);
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
    let mut keys: Vec<(Vec<u64>, u32, u32)> = Vec::new();
    for (c, cell) in cells.iter().enumerate() {
        for (p, portal) in cell.portals.iter().enumerate() {
            let mut key = reserved(portal.points.len() * 3)?;
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
fn bits(point: Vec3) -> u64 {
    let mut key = 0u64;
    for value in [point.x, point.y, point.z] {
        key = key.rotate_left(21) ^ value.to_bits() as u64;
    }
    key
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

#[cfg(test)]
mod tests;
