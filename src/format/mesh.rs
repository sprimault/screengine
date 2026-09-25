// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le format de maillage.
//!
//! Un accessoire, un objet mobile, une caisse : de la géométrie qu'une matrice
//! de modèle place dans le monde, par opposition au décor de la carte. Quatre
//! sections — les groupes de surface, les noms d'emplacements de texture, les
//! triangles, les sommets —, dont `docs/rust.md` fixe les dispositions.
//!
//! **Tout est vérifié une fois ici, et rien ne se revérifie à la soumission.**
//! C'est le premier endroit du projet où une revalidation par image serait
//! invisible : elle ne ferait rougir aucun test, ne changerait aucune empreinte,
//! et coûterait un parcours complet par image.

use alloc::string::String;
use alloc::vec::Vec;

use super::{Cursor, decode};
use crate::buffer::{owned, reserved};
use crate::error::{Error, Malformation, Result};
use crate::math::Vec3;
use crate::scene::{Color, Triangle, VertexUv};

/// Le genre que porte l'en-tête d'un maillage.
const KIND: [u8; 4] = *b"MESH";

/// La seule version de format que cette construction lit.
///
/// Elle cassera à l'étape 6, qui tranche la normale par sommet, et c'est prévu :
/// le numéro existe pour écrire une migration plutôt que jeter les maillages de
/// test.
const VERSION: u32 = 1;

/// Les genres de sections, croissants comme l'en-tête l'exige.
const TAGS: [[u8; 4]; 4] = [*b"SURF", *b"TEXN", *b"TRIS", *b"VTXS"];

/// Rang de la section des groupes de surface dans ce que rend le socle.
const SURF: usize = 0;
/// Rang de la section des noms d'emplacements.
const TEXN: usize = 1;
/// Rang de la section des triangles.
const TRIS: usize = 2;
/// Rang de la section des sommets.
const VTXS: usize = 3;

/// Taille d'un sommet dans le fichier, en octets.
const VERTEX_LEN: usize = 20;
/// Taille d'un triangle, en octets.
const TRIANGLE_LEN: usize = 16;
/// Taille d'un groupe de surface, en octets.
const GROUP_LEN: usize = 16;

/// Un groupe de surface : les triangles contigus qu'une seule soumission dessine
/// avec un même habillage.
///
/// Interne au noyau. L'ABI n'expose aucun accesseur de groupe à cette étape, et
/// un type que le chemin Rust rendrait public sans que le chemin C l'atteigne
/// casserait la règle des deux chemins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Group {
    /// Son identifiant stable, attribué par l'éditeur, jamais nul.
    pub(crate) id: u32,
    /// Son premier triangle.
    pub(crate) first_triangle: u32,
    /// Combien de triangles il porte.
    pub(crate) triangle_count: u32,
    /// L'emplacement de texture qu'il réclame, indice dans les noms.
    pub(crate) texture_slot: u32,
}

/// Un maillage chargé depuis un bloc d'octets.
///
/// Immuable, indépendante de tout contexte : la même ressource se soumet à
/// plusieurs contextes, depuis plusieurs threads, et l'étape de collision
/// pourra la charger sans jamais allouer de tampon d'image. Ses octets sont
/// copiés au chargement, si bien que l'hôte peut libérer son bloc au retour.
#[derive(Debug)]
pub struct Mesh {
    /// Les sommets, que les triangles indexent.
    vertices: Vec<VertexUv>,
    /// Les triangles, dans l'ordre du fichier — qui est celui de la soumission,
    /// et donc ce qui départage deux surfaces coplanaires.
    triangles: Vec<Triangle>,
    /// Les groupes, pavant les triangles dans l'ordre.
    groups: Vec<Group>,
    /// Les noms d'emplacements de texture, dans l'ordre du fichier.
    names: Vec<String>,
    /// Les deux coins de la boîte englobante, calculée au chargement.
    // Aucune élimination ne la lit encore : la soumission dessine tous les
    // groupes, et c'est la traversée par portails qui aura de quoi s'en servir.
    #[allow(dead_code)]
    bounds: [Vec3; 2],
}

impl Mesh {
    /// Décode un maillage, ou dit ce qui l'a fait refuser.
    ///
    /// L'ordre des passes suit les dépendances : les sommets bornent les indices
    /// des triangles, les noms bornent les emplacements des groupes, et les
    /// triangles bornent leur pavage.
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let sections = decode(bytes, KIND, VERSION, TAGS)?;
        let vertices = vertices(sections[VTXS])?;
        let triangles = triangles(sections[TRIS], vertices.len())?;
        let names = names(sections[TEXN])?;
        let groups = groups(sections[SURF], triangles.len(), names.len())?;
        let bounds = bounds(&vertices);
        Ok(Self {
            vertices,
            triangles,
            groups,
            names,
            bounds,
        })
    }

    /// Combien de triangles le maillage porte.
    ///
    /// L'hôte en a besoin pour dimensionner la capacité de son contexte avant
    /// d'avoir rien soumis : sans ce compte, la seule façon de le connaître
    /// serait d'essayer.
    pub fn triangle_count(&self) -> u32 {
        // Le compte tient dans un `u32` : chaque triangle occupe seize octets
        // d'une section dont la longueur en est une.
        self.triangles.len() as u32
    }

    /// Combien d'emplacements de texture il réclame.
    pub fn texture_count(&self) -> u32 {
        // Même borne : un nom coûte au moins deux octets du fichier.
        self.names.len() as u32
    }

    /// Les groupes de surface, dans l'ordre du fichier.
    ///
    /// Chacun est une soumission : ses triangles sont contigus, le chargement
    /// l'a vérifié, et le rendu n'a donc rien à rassembler.
    pub(crate) fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// Les sommets, que les indices des triangles atteignent tous.
    ///
    /// **Indexables sans contrôle** : le chargement a borné chaque indice, et
    /// c'est là tout le gain — une revalidation par image ne ferait rougir aucun
    /// test et coûterait un parcours complet.
    pub(crate) fn vertices(&self) -> &[VertexUv] {
        &self.vertices
    }

    /// Les triangles, dans l'ordre du fichier.
    pub(crate) fn triangles(&self) -> &[Triangle] {
        &self.triangles
    }

    /// Le nom d'un emplacement, ou `None` au-delà du dernier.
    ///
    /// Le fichier porte des noms et non des chemins : c'est l'hôte qui décide ce
    /// qu'il charge et avec quoi, et le moteur n'ouvre rien.
    pub fn texture_name(&self, slot: u32) -> Option<&str> {
        self.names.get(slot as usize).map(String::as_str)
    }
}

/// Les sommets d'une section, cinq flottants chacun.
///
/// La capacité vient de la longueur de la section et jamais d'un nombre déclaré :
/// c'est la bombe d'allocation que le format ferme d'avance.
fn vertices(section: &[u8]) -> Result<Vec<VertexUv>> {
    let mut cursor = Cursor::new(section);
    let mut vertices = reserved(section.len() / VERTEX_LEN)?;
    while cursor.remaining() != 0 {
        let x = cursor.f32()?;
        let y = cursor.f32()?;
        let z = cursor.f32()?;
        let u = cursor.f32()?;
        let v = cursor.f32()?;
        vertices.push(VertexUv {
            position: Vec3::new(x, y, z),
            u,
            v,
        });
    }
    Ok(vertices)
}

/// Les triangles d'une section, indices bornés par le nombre de sommets.
///
/// Un reste partiel de section n'a pas de contrôle à lui : la dernière lecture
/// dépasse la borne du curseur, et c'est une troncature.
fn triangles(section: &[u8], vertex_count: usize) -> Result<Vec<Triangle>> {
    let mut cursor = Cursor::new(section);
    let mut triangles = reserved(section.len() / TRIANGLE_LEN)?;
    while cursor.remaining() != 0 {
        let indices = [cursor.u32()?, cursor.u32()?, cursor.u32()?];
        let color = Color::new(cursor.u8()?, cursor.u8()?, cursor.u8()?, cursor.u8()?);
        if indices.iter().any(|index| *index as usize >= vertex_count) {
            return Err(Error::InvalidFormat(Malformation::Index));
        }
        triangles.push(Triangle { indices, color });
    }
    Ok(triangles)
}

/// Les noms d'emplacements, chacun une longueur puis ses octets UTF-8.
///
/// Deux passes : leur nombre ne se déduit d'aucune longueur, et une seule passe
/// ferait grandir le vecteur en cours de route — une réallocation dont le
/// chargement a le droit, mais que le projet n'écrit nulle part ailleurs.
fn names(section: &[u8]) -> Result<Vec<String>> {
    let mut count = 0;
    let mut cursor = Cursor::new(section);
    while cursor.remaining() != 0 {
        let len = cursor.u16()? as usize;
        cursor.take(len)?;
        count += 1;
    }

    let mut names = reserved(count)?;
    let mut cursor = Cursor::new(section);
    while cursor.remaining() != 0 {
        let len = cursor.u16()? as usize;
        let bytes = cursor.take(len)?;
        let name =
            core::str::from_utf8(bytes).map_err(|_| Error::InvalidFormat(Malformation::NonUtf8))?;
        names.push(owned(name)?);
    }
    Ok(names)
}

/// Les groupes de surface, qui pavent les triangles dans l'ordre.
fn groups(section: &[u8], triangle_count: usize, texture_count: usize) -> Result<Vec<Group>> {
    let mut cursor = Cursor::new(section);
    let mut groups = reserved(section.len() / GROUP_LEN)?;
    let mut next: usize = 0;
    while cursor.remaining() != 0 {
        let group = Group {
            id: cursor.u32()?,
            first_triangle: cursor.u32()?,
            triangle_count: cursor.u32()?,
            texture_slot: cursor.u32()?,
        };
        if group.id == 0 {
            return Err(Error::InvalidFormat(Malformation::Identifier));
        }
        // Aucun emplacement pour « sans texture » : un groupe nomme toujours le
        // sien, et c'est l'hôte qui passe un handle nul à la soumission s'il ne
        // veut rien plaquer dessus. Un maillage qui porte des triangles déclare
        // donc au moins un nom.
        if group.texture_slot as usize >= texture_count {
            return Err(Error::InvalidFormat(Malformation::Index));
        }
        if group.first_triangle as usize != next {
            return Err(Error::InvalidFormat(Malformation::GroupBounds));
        }
        next = next
            .checked_add(group.triangle_count as usize)
            .ok_or(Error::InvalidFormat(Malformation::GroupBounds))?;
        if next > triangle_count {
            return Err(Error::InvalidFormat(Malformation::GroupBounds));
        }
        groups.push(group);
    }
    if next != triangle_count {
        return Err(Error::InvalidFormat(Malformation::GroupBounds));
    }
    unique_ids(&groups)?;
    Ok(groups)
}

/// Refuse deux groupes qui porteraient le même identifiant.
///
/// Par tri puis passe adjacente, et non par table de hachage : le noyau n'en a
/// aucune, et l'ordre d'itération d'une table n'est pas contractuel. Le tableau
/// trié ne se garde pas — le maillage n'a aucune recherche par identifiant à
/// l'étape 4, et une table dérivée qu'aucun appel n'interroge serait à maintenir
/// pour rien.
fn unique_ids(groups: &[Group]) -> Result<()> {
    let mut ids = reserved(groups.len())?;
    ids.extend(groups.iter().map(|group| group.id));
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::InvalidFormat(Malformation::Identifier));
    }
    Ok(())
}

/// Les deux coins de la boîte englobante des sommets, nuls si le maillage est
/// vide.
///
/// Par comparaison et non par `f32::min` : celui-ci ne spécifie pas le signe
/// qu'il rend de `min(-0.0, 0.0)`, et deux cibles donneraient deux boîtes pour
/// le même maillage. Les coordonnées étant finies — le curseur l'a garanti —,
/// une comparaison suffit et rend les mêmes bits partout.
fn bounds(vertices: &[VertexUv]) -> [Vec3; 2] {
    let Some(first) = vertices.first() else {
        return [Vec3::ZERO; 2];
    };

    let mut low = first.position;
    let mut high = first.position;
    for vertex in &vertices[1..] {
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

#[cfg(test)]
mod tests;
