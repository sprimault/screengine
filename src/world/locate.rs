// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Où se trouve la caméra : trouvée une fois, suivie ensuite.
//!
//! **La cellule de la caméra n'est pas un état du moteur.** L'hôte la garde et la
//! passe à chaque soumission ; ces deux fonctions ne sont que des interrogations
//! de la carte, sans contexte, sans image et sans mémoire. C'est ce qui empêche le
//! suivi d'une caméra — une notion de jeu — de venir se loger dans le moteur.
//!
//! **Chercher coûte, suivre ne coûte rien.** [`locate`] parcourt toutes les
//! cellules et toutes leurs faces : c'est un appel nommé, que l'hôte fait au
//! chargement ou quand il a perdu le fil. [`track`] ne regarde que la cellule
//! courante et ses portails.

use crate::format::World;
use crate::math::Vec3;

use super::traversal::MAX_DEPTH;

/// La cellule qui contient ce point, ou `None` s'il n'en est dans aucune.
///
/// **Deux cellules superposées peuvent contenir le même point**, et la première
/// dans l'ordre du fichier gagne. Ce n'est pas une erreur, c'est un arbitrage, et
/// il est écrit pour que deux constructions ne le tranchent pas différemment.
pub(crate) fn locate(world: &World, point: Vec3) -> Option<u32> {
    (0..world.cells().len() as u32).find(|&index| contains(world, index, point))
}

/// La cellule où aboutit un déplacement, partant de celle où il commence.
///
/// Rend `None` quand le segment sort par un mur ou par un portail non apparié :
/// la caméra est alors dehors, et c'est une clause, pas un défaut — un hôte peut
/// légitimement la pousser dans un interstice d'une carte en cours d'édition. Le
/// moteur ne se relocalise jamais de lui-même ; c'est à l'hôte de rappeler
/// [`locate`].
///
/// **Plusieurs cellules peuvent être franchies d'un seul pas**, et la boucle les
/// suit jusqu'à la borne de la traversée. Sans elle, un pas rapide rendrait la
/// première voisine alors que le point d'arrivée est deux cellules plus loin, et
/// la cellule rendue ne contiendrait pas la caméra.
pub(crate) fn track(world: &World, from_cell: u32, from: Vec3, to: Vec3) -> Option<u32> {
    let cells = world.cells();
    if from_cell as usize >= cells.len() {
        return None;
    }

    let mut current = from_cell;
    for _ in 0..MAX_DEPTH {
        if contains(world, current, to) {
            return Some(current);
        }
        let cell = &cells[current as usize];
        let mut crossed = None;
        for portal in &cell.portals {
            let Some((next, _)) = portal.link else {
                continue;
            };
            if crosses(&portal.points, from, to) {
                crossed = Some(next);
                break;
            }
        }
        // Sans portail franchi et sans arrivée dans la cellule, le segment est
        // sorti par une surface : la caméra est dehors.
        current = crossed?;
    }
    None
}

/// Vrai si le point est dans la cellule.
///
/// **Par comptage de traversées, parce qu'une cellule n'a pas à être convexe.**
/// Un rayon part du point vers le `+X` et l'on compte les faces qu'il franchit :
/// un compte impair met le point dedans. Les portails comptent comme les
/// surfaces — c'est eux qui ferment le volume, et les ignorer rendrait toute
/// cellule ouverte.
///
/// **Le comptage porte sur les triangles, pas sur les polygones d'origine**, que
/// le chargement ne conserve pas. Une arête interne de la découpe d'oreilles est
/// donc traversée par le rayon aussi bien qu'une arête vraie, et il faut qu'elle
/// appartienne à **exactement un** des deux triangles qui la partagent : sinon
/// elle se compte deux fois ou zéro, et la parité s'inverse. C'est la règle
/// top-left du rasteriseur, transposée d'un balayage à un rayon — voir
/// [`owns_edge`].
fn contains(world: &World, cell: u32, point: Vec3) -> bool {
    let cells = world.cells();
    if cell as usize >= cells.len() {
        return false;
    }
    let cell = &cells[cell as usize];
    let mut crossings = 0u32;

    for triangle in &cell.triangles {
        let corners = triangle.map(|index| cell.vertices[index as usize].position);
        if hits(corners, point) {
            crossings += 1;
        }
    }
    // Un portail n'est pas triangulé au chargement : son éventail suffit, sa
    // convexité étant vérifiée.
    for portal in &cell.portals {
        for i in 1..portal.points.len() - 1 {
            let corners = [portal.points[0], portal.points[i], portal.points[i + 1]];
            if hits(corners, point) {
                crossings += 1;
            }
        }
    }

    crossings % 2 == 1
}

/// Vrai si le rayon parti du point vers le `+X` franchit ce triangle.
///
/// Le triangle se projette dans le plan `YZ`, perpendiculaire au rayon : le test
/// devient « le point est-il dans le triangle projeté », puis « l'abscisse du plan
/// est-elle devant le point ». C'est le rasteriseur en deux dimensions, et la
/// règle de bord y est la même — sans elle, un point posé exactement en face
/// d'une arête compterait deux fois.
fn hits(corners: [Vec3; 3], point: Vec3) -> bool {
    let projected = corners.map(|c| (c.y, c.z));
    let target = (point.y, point.z);

    // L'aire signée du triangle projeté décide du sens dans lequel on lit les
    // fonctions de bord. **Les deux sens comptent** : la parité n'a que faire de
    // savoir si une face est vue de face ou de dos, et n'en compter qu'une sur
    // deux mettrait tout point hors de toute cellule.
    let (a, b, c) = (projected[0], projected[1], projected[2]);
    let twice_area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    // Un triangle vu par la tranche n'est pas traversé : son aire projetée est
    // nulle, et il n'y a pas de dedans où tomber.
    if twice_area == 0.0 {
        return false;
    }
    let sign = if twice_area < 0.0 { -1.0f32 } else { 1.0f32 };

    let mut inside = true;
    for i in 0..3 {
        let from = projected[i];
        let to = projected[(i + 1) % 3];
        let (dy, dz) = (to.0 - from.0, to.1 - from.1);
        let edge = (dy * (target.1 - from.1) - dz * (target.0 - from.0)) * sign;
        // Sur l'arête, c'est la règle de bord qui tranche, et elle se lit dans le
        // sens de parcours du triangle — d'où les écarts multipliés par le signe
        // eux aussi, faute de quoi un triangle d'orientation inverse
        // revendiquerait exactement les arêtes que son voisin revendique déjà.
        if edge < 0.0 || (edge == 0.0 && !owns_edge(dy * sign, dz * sign)) {
            inside = false;
        }
    }
    if !inside {
        return false;
    }

    // L'abscisse du point d'intersection, par le plan du triangle. La normale ne
    // peut pas être perpendiculaire au rayon, l'aire projetée étant non nulle.
    let edge1 = Vec3::new(
        corners[1].x - corners[0].x,
        corners[1].y - corners[0].y,
        corners[1].z - corners[0].z,
    );
    let edge2 = Vec3::new(
        corners[2].x - corners[0].x,
        corners[2].y - corners[0].y,
        corners[2].z - corners[0].z,
    );
    let normal = Vec3::new(
        edge1.y * edge2.z - edge1.z * edge2.y,
        edge1.z * edge2.x - edge1.x * edge2.z,
        edge1.x * edge2.y - edge1.y * edge2.x,
    );
    if normal.x == 0.0 {
        return false;
    }
    let to_plane = (corners[0].x - point.x) * normal.x
        + (corners[0].y - point.y) * normal.y
        + (corners[0].z - point.z) * normal.z;
    // Le rayon va vers le `+X` : le franchissement est devant quand le quotient
    // est positif. La comparaison passe par le produit pour éviter la division,
    // dont l'arrondi n'apporterait rien ici.
    to_plane * normal.x > 0.0
}

/// Vrai si le segment franchit ce polygone plan convexe.
///
/// Le portail est plan et convexe, vérifié au chargement : son plan se prend sur
/// ses trois premiers sommets, et l'appartenance du point d'intersection se lit
/// sur le signe des produits mixtes, tous du même côté.
///
/// **Le franchissement est strict aux deux bouts.** Un segment qui s'arrête
/// exactement dans le plan du portail ne le franchit pas : sans cela, une caméra
/// posée pile sur un seuil oscillerait entre les deux cellules d'un pas à l'autre,
/// et l'image sauterait sans que rien ne bouge.
fn crosses(points: &[Vec3], from: Vec3, to: Vec3) -> bool {
    if points.len() < 3 {
        return false;
    }
    let edge1 = sub(points[1], points[0]);
    let edge2 = sub(points[2], points[0]);
    let normal = cross(edge1, edge2);

    let side_from = dot(normal, sub(from, points[0]));
    let side_to = dot(normal, sub(to, points[0]));
    // Les deux extrémités doivent être strictement de part et d'autre.
    if !((side_from > 0.0 && side_to < 0.0) || (side_from < 0.0 && side_to > 0.0)) {
        return false;
    }

    // Le point d'intersection, par interpolation linéaire du paramètre. Le
    // dénominateur est non nul, les deux côtés étant de signes opposés.
    let t = side_from / (side_from - side_to);
    let hit = Vec3::new(
        from.x + (to.x - from.x) * t,
        from.y + (to.y - from.y) * t,
        from.z + (to.z - from.z) * t,
    );

    // Dans un polygone convexe, le point est du même côté de chaque arête. Le
    // signe se compare à celui de la normale, ce qui rend le test indépendant de
    // l'enroulement — et c'est nécessaire, le format ne fixant pas quel côté d'un
    // portail est l'avant.
    let mut positive = 0;
    let mut negative = 0;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let side = dot(normal, cross(sub(b, a), sub(hit, a)));
        if side > 0.0 {
            positive += 1;
        } else if side < 0.0 {
            negative += 1;
        }
    }
    positive == 0 || negative == 0
}

/// La différence de deux points.
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Le produit vectoriel de deux vecteurs.
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// Le produit scalaire de deux vecteurs.
///
/// Écrit ici plutôt que pris sur [`Vec3`] pour que l'ordre des opérations de ce
/// module soit lisible d'un seul endroit : ce qu'il rend décide de la cellule où
/// l'hôte croit être, donc de l'image.
fn dot(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// Vrai si l'arête appartient au triangle qui la porte dans ce sens.
///
/// Même règle que le rasteriseur — haute, ou horizontale vers la droite —, et pour
/// la même raison : deux triangles qui partagent une arête doivent se la
/// partager sans la compter deux fois ni l'oublier. Ici ce n'est pas une couture
/// qui serait en jeu mais la **parité**, donc le résultat : une caméra déclarée
/// nulle part à une position parfaitement légitime.
fn owns_edge(dy: f32, dz: f32) -> bool {
    dz < 0.0 || (dz == 0.0 && dy > 0.0)
}

#[cfg(test)]
mod tests;
