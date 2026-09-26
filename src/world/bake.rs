// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le calcul des lightmaps d'une cellule.
//!
//! **Cellule par cellule, sur appel explicite, et jamais pendant une image.** Une
//! lightmap reste un cache dérivable : la carte est la source, et ce module ne
//! fait que relire ce que le chargement a déjà dérivé.
//!
//! Les onze clauses que ce calcul respecte sont dans `docs/rust.md`, section
//! « Lightmaps calculées ». Trois commandent tout le reste :
//!
//! - **le rayon se teste contre le polygone de la surface, jamais contre sa
//!   triangulation.** Un rayon qui passe exactement par une arête interne de la
//!   découpe d'oreilles peut être manqué par les deux triangles qui la partagent,
//!   et la lumière traverse alors le mur par un trou d'épingle — invisible à
//!   l'arrêt, visible en mouchetures sur une lightmap cuite, et tout ce module
//!   serait déjà construit dessus ;
//! - **l'ensemble d'occulteurs est la cellule, ses voisines à un portail, et les
//!   portails du bord rendus opaques.** La région est alors fermée, donc rien
//!   d'extérieur ne peut contribuer, ce qui rend l'empreinte du cache prouvée
//!   suffisante. Conséquence assumée : la lumière ne tourne pas deux coins ;
//! - **aucun appel à la libm**, ici comme partout : la racine inverse passe par la
//!   table du noyau.
//!
//! Le calcul reste **scalaire** et sort du périmètre de l'étape 9 : sur armv7, le
//! SIMD avancé n'a que la sémantique du zéro forcé là où le VFP scalaire traite
//! les sous-normaux, et une variante vectorielle y rendrait d'autres luxels.

use alloc::vec::Vec;

use crate::buffer::reserved;
use crate::error::Result;
use crate::format::World;
use crate::format::world::{Cell, Mapping, Surface};
use crate::math::Vec3;
use crate::scene::Light;

use super::atlas::{Atlas, GUTTER, Slot};

/// Le nombre d'échantillons par luxel, sur chaque axe.
///
/// **Ce n'est pas un réglage.** Configurable, il changerait l'image, devrait
/// entrer dans l'ABI et dans l'empreinte du cache ; le jour où il faut le changer,
/// c'est une constante de plus dans la révision du calcul.
const SAMPLES: u32 = 2;

/// Le décalage du point d'échantillonnage le long de la normale, en unités de
/// monde.
///
/// Une puissance de deux, pour qu'aucun sous-normal n'entre dans un calcul — même
/// clause que la racine inverse du noyau. La surface est de toute façon exclue de
/// ses propres occulteurs ; ce décalage sert à ce que le point ne soit pas
/// exactement dans le plan.
const LIFT: f32 = 0.015_625;

/// La révision du calcul, qui entre dans l'empreinte de tout cache.
///
/// **À incrémenter à tout changement de ce module** : densité effective, nombre
/// d'échantillons, forme de l'atténuation, terme de Lambert, portée de l'ensemble
/// d'occulteurs, quantification. C'est le champ qu'on oublie, et son absence
/// laisse un cache valide produire une image que la version suivante ne produit
/// plus.
///
/// Son lecteur est l'empreinte du cache, qui arrive avec lui : elle est écrite
/// dès maintenant parce qu'elle appartient au calcul, et l'ajouter au moment du
/// cache ferait oublier de l'incrémenter pour les changements déjà faits.
#[allow(dead_code)]
pub(crate) const REVISION: u32 = 1;

/// Les luxels d'une cellule, prêts à devenir une texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Baked {
    /// Le côté de l'atlas, une puissance de deux.
    pub(crate) side: u32,
    /// Les texels, quatre octets par luxel dans l'ordre mémoire des pixels.
    pub(crate) texels: Vec<u8>,
    /// Le rangement qui a produit ces texels.
    pub(crate) atlas: Atlas,
}

/// Calcule les lightmaps d'une cellule.
///
/// `index` est le rang de la cellule dans la carte. Les occulteurs viennent d'elle
/// et de ses voisines par portail apparié ; les lumières retenues sont celles dont
/// la portée atteint sa boîte englobante.
pub(crate) fn bake(world: &World, index: u32, atlas: Atlas) -> Result<Baked> {
    let cells = world.cells();
    let cell = &cells[index as usize];
    let side = atlas.side;
    let mut texels = reserved((side * side) as usize * 4)?;
    texels.resize((side * side) as usize * 4, 0);

    let lights = retained(world, cell)?;
    let occluders = region(world, index)?;

    for (surface, slot) in cell.surfaces.iter().zip(&atlas.slots) {
        let plane = normal_of(cell, surface);
        shade(
            cell,
            surface,
            plane,
            *slot,
            &lights,
            &occluders,
            side,
            &mut texels,
        );
    }

    Ok(Baked {
        side,
        texels,
        atlas,
    })
}

/// La normale du plan d'une surface, **tournée vers l'intérieur de la cellule**.
///
/// **L'enroulement seul ne suffit pas à l'orienter.** La formule de Newell rend
/// une normale dont le sens suit l'ordre des sommets, et le format ne dit pas
/// lequel des deux sens est l'intérieur — il fixe la face visible, ce qui n'est
/// pas la même chose. Une normale prise à l'envers donne un terme de Lambert
/// négatif et la surface reste noire : vu en écrivant ce module, où le plafond du
/// décor de validation restait éteint à deux unités sous une lampe qui lui faisait
/// face.
///
/// Le sens se décide donc sur le **signe du volume de la cellule**, calculé une
/// fois pour toutes ses faces. L'enroulement du format est cohérent d'une face à
/// l'autre — sans quoi le rendu montrerait déjà des trous —, donc un seul signe
/// les oriente toutes.
///
/// **Le barycentre des sommets a été essayé et il est faux.** Comparer la normale
/// à la direction du barycentre décide juste tant que celui-ci n'est pas dans le
/// plan de la face ; il suffit qu'il y soit pour que le produit scalaire s'annule,
/// que rien ne tranche et que le signe brut de Newell passe tel quel. Ce n'est pas
/// une cellule tordue qu'il faut pour cela : le barycentre d'une salle en L de huit
/// unités tombe exactement sur son coin rentrant, donc sur le plan de deux de ses
/// murs, dont l'un ressortait noir.
fn normal_of(cell: &Cell, surface: &Surface) -> Vec3 {
    let raw = newell_of(cell, surface);
    if outward(cell) {
        Vec3::new(-raw.x, -raw.y, -raw.z)
    } else {
        raw
    }
}

/// Les normales de Newell de cette cellule pointent-elles vers l'extérieur ?
///
/// Six fois le volume signé, par le théorème de la divergence : la somme, sur les
/// faces fermant la cellule, du produit scalaire d'un de leurs points par leur
/// normale de Newell. Les portails en sont, sans quoi la cellule n'est pas fermée
/// et la somme ne vaut rien.
///
/// Un volume nul ne peut venir que d'une cellule dégénérée, qu'aucune orientation
/// ne sauverait ; le sens brut est alors gardé.
fn outward(cell: &Cell) -> bool {
    let mut volume = 0.0;
    for surface in &cell.surfaces {
        let anchor = cell.vertices[point_of(cell, surface, 0)].position;
        volume += dot(anchor, newell_of(cell, surface));
    }
    for portal in &cell.portals {
        if let Some(&anchor) = portal.points.first() {
            volume += dot(anchor, newell(&portal.points));
        }
    }
    volume > 0.0
}

/// La normale de Newell d'une surface, dans le sens de son enroulement.
///
/// Sur le polygone entier, et non sur un de ses triangles : une surface concave a
/// des triangles dont l'orientation ne dit rien de la sienne.
fn newell_of(cell: &Cell, surface: &Surface) -> Vec3 {
    let mut normal = Vec3::ZERO;
    let n = surface.corners.len();
    for i in 0..n {
        let a = cell.vertices[point_of(cell, surface, i)].position;
        let b = cell.vertices[point_of(cell, surface, (i + 1) % n)].position;
        normal.x += (a.y - b.y) * (a.z + b.z);
        normal.y += (a.z - b.z) * (a.x + b.x);
        normal.z += (a.x - b.x) * (a.y + b.y);
    }
    normal
}

/// Le rang, dans les sommets dérivés de la cellule, du `i`-ième coin d'une
/// surface.
///
/// Le chargement dédouble les sommets par surface — chacun porte les coordonnées
/// de son propre repère —, et les coins d'une surface y sont donc consécutifs à
/// partir de son premier.
fn point_of(cell: &Cell, surface: &Surface, i: usize) -> usize {
    let _ = cell;
    surface.first_vertex as usize + i
}

/// Les lumières que cette cellule retient.
///
/// **Par la géométrie, jamais par appartenance à une cellule** : les lumières
/// statiques n'ont pas de champ de cellule, et un test d'appartenance à une
/// cellule non convexe est un comptage de traversées — un calcul de plus à rendre
/// déterministe pour un résultat que la boîte englobante donne gratuitement.
fn retained(world: &World, cell: &Cell) -> Result<Vec<Light>> {
    let (low, high) = bounds_of(cell);
    let mut kept = reserved(0)?;
    for index in 0..world.light_count() {
        let Some(light) = world.light(index) else {
            continue;
        };
        // La sphère de la lumière coupe-t-elle la boîte ? Le point de la boîte le
        // plus proche du centre, puis la distance au carré : aucune racine.
        let closest = Vec3::new(
            clamp(light.position.x, low.x, high.x),
            clamp(light.position.y, low.y, high.y),
            clamp(light.position.z, low.z, high.z),
        );
        let offset = Vec3::new(
            closest.x - light.position.x,
            closest.y - light.position.y,
            closest.z - light.position.z,
        );
        if dot(offset, offset) <= light.radius * light.radius {
            kept.try_reserve(1)
                .map_err(|_| crate::error::Error::OutOfMemory)?;
            kept.push(light);
        }
    }
    Ok(kept)
}

/// La valeur ramenée entre deux bornes, par comparaisons écrites.
fn clamp(value: f32, low: f32, high: f32) -> f32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// La boîte englobante d'une cellule.
fn bounds_of(cell: &Cell) -> (Vec3, Vec3) {
    let mut low = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut high = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for vertex in &cell.vertices {
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
    (low, high)
}

/// Un polygone qui arrête la lumière.
struct Occluder {
    /// Ses sommets, dans l'ordre.
    points: Vec<Vec3>,
}

/// L'ensemble d'occulteurs d'une cellule.
///
/// La cellule, ses voisines par portail apparié, et **les portails du bord rendus
/// opaques** : sans eux la région serait ouverte et une lumière lointaine y
/// entrerait, ce que l'empreinte du cache ne saurait pas voir.
fn region(world: &World, index: u32) -> Result<Vec<Occluder>> {
    let cells = world.cells();
    let mut members = reserved(1)?;
    members.push(index);
    for portal in &cells[index as usize].portals {
        if let Some((next, _)) = portal.link {
            members
                .try_reserve(1)
                .map_err(|_| crate::error::Error::OutOfMemory)?;
            members.push(next);
        }
    }

    let mut out: Vec<Occluder> = reserved(0)?;
    for &member in &members {
        let cell = &cells[member as usize];
        for surface in &cell.surfaces {
            let mut points = reserved(surface.corners.len())?;
            for i in 0..surface.corners.len() {
                points.push(cell.vertices[point_of(cell, surface, i)].position);
            }
            out.try_reserve(1)
                .map_err(|_| crate::error::Error::OutOfMemory)?;
            out.push(Occluder { points });
        }
        // Un portail qui mène hors de l'ensemble ferme la région ; un portail
        // intérieur laisse passer, comme une porte ouverte.
        for portal in &cell.portals {
            let inside = matches!(portal.link, Some((next, _)) if members.contains(&next));
            if inside {
                continue;
            }
            let mut points = reserved(portal.points.len())?;
            points.extend_from_slice(&portal.points);
            out.try_reserve(1)
                .map_err(|_| crate::error::Error::OutOfMemory)?;
            out.push(Occluder { points });
        }
    }
    Ok(out)
}

/// Remplit le rectangle d'une surface dans l'atlas.
#[allow(clippy::too_many_arguments)]
fn shade(
    cell: &Cell,
    surface: &Surface,
    normal: Vec3,
    slot: Slot,
    lights: &[Light],
    occluders: &[Occluder],
    side: u32,
    texels: &mut [u8],
) {
    let unit = normalized(normal);
    let mapping = surface.lightmap;
    let inverse_u = 1.0 / dot(mapping.u, mapping.u);
    let inverse_v = 1.0 / dot(mapping.v, mapping.v);
    let outline: Vec<Vec3> = (0..surface.corners.len())
        .map(|i| cell.vertices[point_of(cell, surface, i)].position)
        .collect();
    let anchor = outline[0];

    for j in 0..surface.luxels.height {
        for i in 0..surface.luxels.width {
            let mut total = [0.0f32; 3];
            // Suréchantillonnage 2×2 à des décalages d'un quart de luxel, sommés
            // dans un ordre écrit et moyennés par une division par quatre, qui est
            // exacte.
            for sj in 0..SAMPLES {
                for si in 0..SAMPLES {
                    let offset_u = (si as f32 * 2.0 + 1.0) / (SAMPLES as f32 * 2.0);
                    let offset_v = (sj as f32 * 2.0 + 1.0) / (SAMPLES as f32 * 2.0);
                    let point = luxel_point(
                        mapping,
                        surface.luxels.min_u as f32 + i as f32 + offset_u,
                        surface.luxels.min_v as f32 + j as f32 + offset_v,
                        inverse_u,
                        inverse_v,
                        unit,
                        anchor,
                    );
                    let point = clamped(&outline, normal, point);
                    let lifted = Vec3::new(
                        point.x + unit.x * LIFT,
                        point.y + unit.y * LIFT,
                        point.z + unit.z * LIFT,
                    );
                    let sample = gather(lifted, unit, lights, occluders, cell, surface);
                    for (slot, value) in total.iter_mut().zip(sample) {
                        *slot += value;
                    }
                }
            }

            let x = slot.x + GUTTER + i;
            let y = slot.y + GUTTER + j;
            let base = ((y * side + x) * 4) as usize;
            for channel in 0..3 {
                let averaged = total[channel] / (SAMPLES * SAMPLES) as f32;
                texels[base + channel] = quantize(averaged);
            }
            texels[base + 3] = 0xFF;
        }
    }

    fill_gutter(slot, side, texels);
}

/// Le point ramené sur la surface, s'il en était sorti.
///
/// **Le rectangle d'une lightmap déborde toujours du polygone qu'elle habille**, et
/// pour une surface concave il en déborde largement : le plafond d'une salle en L
/// occupe le carré qui la contient, dont un quart n'est pas la salle. Ces luxels-là
/// sont calculés — le rectangle est plein —, et les laisser où ils tombent les met
/// hors de la cellule, où plus rien ne les occulte : au-dessus d'un plancher opaque,
/// ils reçoivent une lampe que le plancher cache, et le bilinéaire ramène cette
/// fuite sur le bord visible de la surface.
///
/// Le point le plus proche du contour est donc pris à la place. Ce n'est pas un
/// pis-aller : c'est la même valeur que le bord porte déjà, prolongée vers le
/// dehors, exactement ce que la gouttière fait au bord du rectangle.
fn clamped(outline: &[Vec3], normal: Vec3, point: Vec3) -> Vec3 {
    if contains(outline, normal, point) {
        return point;
    }

    let mut best = point;
    let mut nearest = f32::MAX;
    for i in 0..outline.len() {
        let candidate = on_segment(outline[i], outline[(i + 1) % outline.len()], point);
        let offset = sub(candidate, point);
        let square = dot(offset, offset);
        if square < nearest {
            nearest = square;
            best = candidate;
        }
    }
    best
}

/// Le point d'un segment le plus proche d'un point donné.
fn on_segment(a: Vec3, b: Vec3, point: Vec3) -> Vec3 {
    let edge = sub(b, a);
    let square = dot(edge, edge);
    if square <= 0.0 {
        return a;
    }
    let t = clamp(dot(sub(point, a), edge) / square, 0.0, 1.0);
    Vec3::new(a.x + edge.x * t, a.y + edge.y * t, a.z + edge.z * t)
}

/// Le point du monde d'un luxel, ramené dans le plan de sa surface.
///
/// `inverse_u` et `inverse_v` sont exacts : le chargement a vérifié que les carrés
/// des longueurs d'axe sont des puissances de deux, et l'inverse d'une puissance de
/// deux en est une.
///
/// **L'origine du repère n'a aucune raison d'appartenir au plan de la surface**, et
/// le format ne l'exige pas : un éditeur pose un repère par matériau et le partage
/// entre les surfaces qu'il habille — c'est même ce que fait la scène de référence,
/// qui laisse les trois origines à zéro. Pour la texture c'est sans effet, la
/// projection ne gardant que les composantes tangentielles ; pour la cuisson, prendre
/// le point tel quel place les luxels d'un plafond à hauteur de sol, la lumière les
/// atteint par-derrière et la face reste noire. La composante normale est donc
/// annulée contre un sommet de la surface, ce qui ne laisse à l'origine que son
/// décalage dans le plan.
#[allow(clippy::too_many_arguments)]
fn luxel_point(
    mapping: Mapping,
    u: f32,
    v: f32,
    inverse_u: f32,
    inverse_v: f32,
    unit: Vec3,
    anchor: Vec3,
) -> Vec3 {
    let su = u * inverse_u;
    let sv = v * inverse_v;
    let raw = Vec3::new(
        mapping.origin.x + mapping.u.x * su + mapping.v.x * sv,
        mapping.origin.y + mapping.u.y * su + mapping.v.y * sv,
        mapping.origin.z + mapping.u.z * su + mapping.v.z * sv,
    );
    let drift = dot(unit, sub(raw, anchor));
    Vec3::new(
        raw.x - unit.x * drift,
        raw.y - unit.y * drift,
        raw.z - unit.z * drift,
    )
}

/// La lumière reçue en un point, tous les luminaires confondus.
///
/// L'atténuation est celle de l'étape 3 — `(1 − d²/r²)²`, qui s'annule en `r` avec
/// une dérivée nulle, donc sans l'anneau visible que `1 − d²/r²` dessine — et s'y
/// ajoute le terme de Lambert. Sans lui, les deux faces d'un angle reçoivent la
/// même valeur et l'angle disparaît.
fn gather(
    point: Vec3,
    normal: Vec3,
    lights: &[Light],
    occluders: &[Occluder],
    cell: &Cell,
    surface: &Surface,
) -> [f32; 3] {
    let mut total = [0.0f32; 3];
    for light in lights {
        let to_light = Vec3::new(
            light.position.x - point.x,
            light.position.y - point.y,
            light.position.z - point.z,
        );
        let square = dot(to_light, to_light);
        if square >= light.radius * light.radius {
            continue;
        }
        let direction = normalized(to_light);
        let lambert = dot(normal, direction);
        // Le rejet le plus payant du calcul : une face détournée est noire sans
        // qu'aucun rayon ne parte.
        if lambert <= 0.0 {
            continue;
        }
        if shadowed(point, light.position, occluders, cell, surface) {
            continue;
        }
        let falloff = 1.0 - square / (light.radius * light.radius);
        let weight = falloff * falloff * lambert;
        let channels = [light.color.r, light.color.g, light.color.b];
        for (slot, channel) in total.iter_mut().zip(channels) {
            *slot += weight * channel as f32;
        }
    }
    total
}

/// Vrai si un occulteur coupe le segment entre le point et la lumière.
///
/// La surface éclairée est exclue de ses propres occulteurs : un luxel posé sur
/// elle s'ombrerait lui-même, et le décalage le long de la normale ne suffirait pas
/// à l'en sauver sur une surface rasante.
fn shadowed(
    point: Vec3,
    light: Vec3,
    occluders: &[Occluder],
    cell: &Cell,
    surface: &Surface,
) -> bool {
    let own: Vec<Vec3> = (0..surface.corners.len())
        .map(|i| cell.vertices[point_of(cell, surface, i)].position)
        .collect();
    occluders
        .iter()
        .filter(|o| o.points != own)
        .any(|o| crosses(&o.points, point, light))
}

/// Vrai si le segment traverse ce polygone plan.
///
/// **Le polygone entier, pas ses triangles** : c'est la clause du brief, et le
/// défaut qu'elle évite est un trou d'épingle par lequel la lumière passe à
/// travers un mur.
fn crosses(points: &[Vec3], from: Vec3, to: Vec3) -> bool {
    if points.len() < 3 {
        return false;
    }
    let normal = newell(points);
    let side_from = dot(normal, sub(from, points[0]));
    let side_to = dot(normal, sub(to, points[0]));
    if !((side_from > 0.0 && side_to < 0.0) || (side_from < 0.0 && side_to > 0.0)) {
        return false;
    }
    let t = side_from / (side_from - side_to);
    let hit = Vec3::new(
        from.x + (to.x - from.x) * t,
        from.y + (to.y - from.y) * t,
        from.z + (to.z - from.z) * t,
    );

    contains(points, normal, hit)
}

/// Le point du plan appartient-il au polygone ?
///
/// Comptage de traversées d'un rayon, dans le plan dominant de la normale : un
/// polygone de surface n'a pas à être convexe, et c'est le seul test qui vaille
/// pour un polygone simple quelconque. La règle de bord est celle de la traversée
/// et du rasteriseur — une arête appartient à celle de ses extrémités qui est
/// strictement au-dessus du rayon —, ce qui donne à une arête partagée un seul
/// propriétaire et à un point posé dessus une réponse et une seule.
///
/// L'abscisse de l'intersection se compare par produit croisé plutôt que par
/// division : la division n'est pas interdite, mais le produit évite d'avoir à
/// écrire ce que vaut le quotient quand l'arête est horizontale — cas que le test
/// de traversée a déjà écarté, et qu'on relirait quand même.
fn contains(points: &[Vec3], normal: Vec3, hit: Vec3) -> bool {
    let (i0, i1) = plane_axes(normal);
    let n = points.len();
    let (hu, hv) = (axis(hit, i0), axis(hit, i1));

    let mut inside = false;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        let (au, av) = (axis(a, i0), axis(a, i1));
        let (bu, bv) = (axis(b, i0), axis(b, i1));
        if (av > hv) == (bv > hv) {
            continue;
        }
        let dv = bv - av;
        let left = (hu - au) * dv;
        let right = (hv - av) * (bu - au);
        if (dv > 0.0 && left < right) || (dv < 0.0 && left > right) {
            inside = !inside;
        }
    }
    inside
}

/// Les deux axes sur lesquels projeter pour que la surface ne dégénère pas.
///
/// Celui dont la normale porte la plus grande composante est écarté : c'est celui
/// le long duquel le polygone est le plus plat, et le projeter dessus pourrait
/// l'aplatir en un segment.
fn plane_axes(normal: Vec3) -> (usize, usize) {
    let (x, y, z) = (
        magnitude(normal.x),
        magnitude(normal.y),
        magnitude(normal.z),
    );
    if x >= y && x >= z {
        (1, 2)
    } else if y >= z {
        (0, 2)
    } else {
        (0, 1)
    }
}

/// La composante d'un vecteur, par rang.
fn axis(v: Vec3, i: usize) -> f32 {
    match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// La valeur absolue, écrite plutôt qu'empruntée à la libm.
fn magnitude(value: f32) -> f32 {
    if value < 0.0 { -value } else { value }
}

/// La normale de Newell d'un polygone.
fn newell(points: &[Vec3]) -> Vec3 {
    let mut normal = Vec3::ZERO;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        normal.x += (a.y - b.y) * (a.z + b.z);
        normal.y += (a.z - b.z) * (a.x + b.x);
        normal.z += (a.x - b.x) * (a.y + b.y);
    }
    normal
}

/// La valeur ramenée dans un octet.
///
/// Le bornage est écrit, jamais laissé à la saturation de `as` : celle-ci diffère
/// entre le scalaire et les chemins vectoriels, et elle ne sert jamais de garde.
fn quantize(value: f32) -> u8 {
    let scaled = value + 0.5;
    if scaled <= 0.0 {
        0
    } else if scaled >= 255.0 {
        255
    } else {
        scaled as u8
    }
}

/// Recopie le bord du rectangle dans sa gouttière.
///
/// Sans elle, le bilinéaire du rasteriseur irait chercher la surface voisine dans
/// l'atlas au bord de celle-ci.
fn fill_gutter(slot: Slot, side: u32, texels: &mut [u8]) {
    let at = |x: u32, y: u32| ((y * side + x) * 4) as usize;
    for y in slot.y..slot.y + slot.height {
        for x in slot.x..slot.x + slot.width {
            let inner_x = clamp_u32(x, slot.x + GUTTER, slot.x + slot.width - GUTTER - 1);
            let inner_y = clamp_u32(y, slot.y + GUTTER, slot.y + slot.height - GUTTER - 1);
            if inner_x == x && inner_y == y {
                continue;
            }
            let (source, target) = (at(inner_x, inner_y), at(x, y));
            for channel in 0..4 {
                texels[target + channel] = texels[source + channel];
            }
        }
    }
}

/// La valeur ramenée entre deux bornes entières.
fn clamp_u32(value: u32, low: u32, high: u32) -> u32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// La différence de deux points.
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Le produit scalaire.
fn dot(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// Le vecteur unitaire, par la racine inverse du noyau.
///
/// Jamais `sqrt` : son résultat dépend de l'implémentation, et deux cibles
/// donneraient deux éclairages pour la même carte.
fn normalized(v: Vec3) -> Vec3 {
    v.normalize()
}

#[cfg(test)]
mod tests;
