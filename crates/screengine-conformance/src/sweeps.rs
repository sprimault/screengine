// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les balayages de la scène de collision, et leur empreinte.
//!
//! **Une scène d'interrogation, pas une scène d'image.** Elle ne rend aucun
//! pixel, donc elle n'a ni passe ni résolution : les six configurations de
//! découpage n'ont pas d'objet ici, et l'empreinte se calcule une fois.
//!
//! **La liste des balayages se dérive d'une règle, elle ne s'écrit pas en
//! table.** Une table serait une cinquième copie d'une liste, dans quatre
//! langages, le jour où les hôtes rendront cette empreinte ; une règle tient en
//! dix lignes partout. Le treillis et les directions sont donc figés ici, et ce
//! qu'ils produisent est entièrement déterminé par eux.
//!
//! **Les résultats se hachent par leurs bits**, `to_bits` et jamais une valeur
//! quantifiée : une empreinte quantifiée serait aveugle à l'écart d'un ULP entre
//! deux cibles ou deux chemins, qui est exactement ce qu'elle existe pour
//! attraper. Deux clauses en découlent, que le noyau tient : **jamais de `NaN`,
//! jamais de `-0,0`** dans un résultat, sans quoi deux résultats égaux
//! donneraient deux empreintes.

use screengine::{Hit, Vec3, World};
use screengine_conformance::collision_file;

use crate::hash;

/// Le pas du treillis de départs, en unités de monde.
const STEP: f32 = 2.0;

/// De combien le treillis est décalé du bord d'une cellule.
///
/// **Un demi-pas, et c'est ce qui décide de l'utilité de la scène.** Un treillis
/// aligné sur les coins tombe sur les murs : le premier essai partait de `(0, 0)`
/// et rendait 440 départs dans le solide et 1 760 points hors de toute cellule
/// sur 2 400 — une empreinte qui n'éprouvait presque rien, et qui se serait figée
/// aussi bien qu'une autre. C'est le rapport texte qui l'a montré, pas
/// l'empreinte.
const INSET: f32 = 1.0;

/// La longueur d'un balayage, en unités de monde.
///
/// Assez pour traverser le couloir de part en part depuis la salle, ce qui est le
/// seul cas où la traversée franchit un portail.
const REACH: f32 = 10.0;

/// Les demi-étendues des deux boîtes d'épreuve.
///
/// La première tient dans le couloir, la seconde non : c'est le couple qui rend
/// la constante de dilatation **mesurable** — l'une doit passer, l'autre non, et
/// une marge mal réglée casse l'un des deux.
///
/// **Ni l'une ni l'autre ne vaut exactement la demi-largeur du couloir**, et
/// c'est le résultat de la mesure, pas un contournement. Une boîte d'exactement
/// une unité dans un couloir d'exactement une unité touche ses deux murs dès
/// qu'elle est dilatée : elle part donc dans le solide et n'entre jamais. La
/// dilatation coûte cela, et le décor le dit — la petite passe, la grande non.
const HALVES: [f32; 2] = [0.45, 0.55];

/// Les dix directions balayées, normalisées par construction du treillis.
///
/// Les six axes, puis les quatre diagonales horizontales : ce sont elles qui font
/// travailler les prismes d'arêtes, qu'un balayage aligné sur un axe ne rencontre
/// jamais de biais.
const DIRECTIONS: [[f32; 3]; 10] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [-1.0, -1.0, 0.0],
];

/// Un balayage à jouer : sa boîte, son départ, son arrivée.
pub struct Sweep {
    /// Les demi-étendues de la boîte.
    pub half: Vec3,
    /// Le point de départ.
    pub from: Vec3,
    /// Le point d'arrivée.
    pub to: Vec3,
}

/// Tous les balayages de la scène, dans l'ordre où l'empreinte les prend.
///
/// L'ordre est contractuel : c'est lui qui fait qu'une empreinte se compare, et
/// le changer déplacerait la référence sans que rien n'ait bougé du moteur.
pub fn all() -> Vec<Sweep> {
    let mut starts = Vec::new();
    // **Un treillis par cellule, dans sa propre boîte englobante**, plutôt qu'un
    // treillis unique sur le décor entier : le couloir ne fait qu'une unité de
    // large, et un pas de deux unités sur une boîte commune ne serait jamais
    // tombé dedans. La règle suit la carte, et les deux ne peuvent pas dériver.
    for footprint in [&collision_file::ROOM[..], &collision_file::CORRIDOR[..]] {
        let (low, high) = bounds(footprint);
        for z in axis(collision_file::FLOOR_Z, collision_file::CEILING_Z) {
            for y in axis(low[1], high[1]) {
                for x in axis(low[0], high[0]) {
                    starts.push(Vec3::new(x, y, z));
                }
            }
        }
    }

    let mut sweeps = Vec::new();
    for half in HALVES {
        let half = Vec3::new(half, half, half);
        for from in &starts {
            for direction in DIRECTIONS {
                sweeps.push(Sweep {
                    half,
                    from: *from,
                    to: Vec3::new(
                        from.x + direction[0] * REACH,
                        from.y + direction[1] * REACH,
                        from.z + direction[2] * REACH,
                    ),
                });
            }
        }
    }
    sweeps
}

/// Les positions de départ sur un axe, retirées des deux bords.
///
/// **Un seul point, au centre, quand l'étendue est trop étroite pour le
/// retrait.** Le couloir fait une unité de large : un treillis qui exigerait une
/// unité de marge de chaque côté n'y aurait placé aucun départ, et la scène
/// n'aurait jamais balayé la seule cellule qu'elle a construite pour cela.
fn axis(low: f32, high: f32) -> Vec<f32> {
    if high - low <= 2.0 * INSET {
        return vec![(low + high) * 0.5];
    }
    let mut values = Vec::new();
    let mut value = low + INSET;
    while value < high - INSET * 0.5 {
        values.push(value);
        value += STEP;
    }
    values
}

/// Les deux coins de la boîte d'une empreinte, par comparaisons écrites.
fn bounds(footprint: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    let mut low = [f32::MAX; 2];
    let mut high = [f32::MIN; 2];
    for point in footprint {
        for axis in 0..2 {
            if point[axis] < low[axis] {
                low[axis] = point[axis];
            }
            if point[axis] > high[axis] {
                high[axis] = point[axis];
            }
        }
    }
    (low, high)
}

/// Joue tous les balayages et rend leur empreinte.
///
/// **Chaque balayage est joué deux fois**, par la traversée et par le chemin de
/// force brute, et leurs résultats doivent être identiques au bit près. C'est le
/// théorème de l'étape, l'analogue de l'égalité entre la traversée de rendu et
/// son chemin brut : il n'attrape pas une fenêtre trop large, et c'est normal —
/// une traversée trop large ne change pas le résultat.
pub fn digest() -> Result<u64, String> {
    let world = World::load(&collision_file::bytes())
        .map_err(|error| format!("collision : le décor est refusé : {error:?}"))?;

    let mut bytes = Vec::new();
    for (index, sweep) in all().into_iter().enumerate() {
        let cell = world.locate(sweep.from);
        let fast = if cell == 0 {
            None
        } else {
            world.sweep(cell, sweep.half, sweep.from, sweep.to)
        };
        let slow = world.sweep_brute(sweep.half, sweep.from, sweep.to);

        if let Some(fast) = &fast
            && !same(fast, &slow)
        {
            return Err(format!(
                "collision : balayage {index}, la traversée et la force brute divergent\n  \
                 traversée {fast:?}\n  brute     {slow:?}"
            ));
        }

        // Le départ sans cellule entre dans l'empreinte comme les autres : c'est
        // un cas que la carte produit, pas un trou dans la liste.
        match &fast {
            Some(hit) => absorb(hit, &mut bytes),
            None => bytes.push(0xFF),
        }
    }
    Ok(hash::of(&bytes))
}

/// Écrit un résultat dans le tampon à hacher, octet de poids faible en tête.
///
/// Tout par `to_bits`, sans une seule opération flottante : ce qui est haché est
/// ce que le moteur a écrit, et non ce qu'un formatage en aurait fait.
fn absorb(hit: &Hit, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&hit.fraction.to_bits().to_le_bytes());
    for value in [hit.normal.x, hit.normal.y, hit.normal.z] {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    for value in [hit.point.x, hit.point.y, hit.point.z] {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    bytes.extend_from_slice(&hit.surface.to_le_bytes());
    bytes.extend_from_slice(&hit.cell.to_le_bytes());
    bytes.push(u8::from(hit.start_solid));
    bytes.push(u8::from(hit.incomplete));
}

/// Deux résultats portent-ils exactement les mêmes bits ?
///
/// Par les bits et non par `==` : deux zéros de signes opposés se comparent
/// égaux en flottant et donnent deux empreintes différentes. C'est précisément
/// l'écart qu'une comparaison naïve laisserait passer ici.
fn same(a: &Hit, b: &Hit) -> bool {
    a.fraction.to_bits() == b.fraction.to_bits()
        && a.normal.x.to_bits() == b.normal.x.to_bits()
        && a.normal.y.to_bits() == b.normal.y.to_bits()
        && a.normal.z.to_bits() == b.normal.z.to_bits()
        && a.point.x.to_bits() == b.point.x.to_bits()
        && a.point.y.to_bits() == b.point.y.to_bits()
        && a.point.z.to_bits() == b.point.z.to_bits()
        && a.surface == b.surface
        && a.cell == b.cell
        && a.start_solid == b.start_solid
        && a.incomplete == b.incomplete
}

#[cfg(test)]
mod tests;

/// Le texte que `make conform-images` écrit, un balayage par ligne.
///
/// **C'est le pendant de « regarder les images » pour une étape qui n'en produit
/// pas.** Une empreinte dit qu'un résultat a changé, jamais qu'il est juste : une
/// scène où tout traverse tout se fige aussi bien qu'une autre.
pub fn report() -> Result<String, String> {
    let world = World::load(&collision_file::bytes())
        .map_err(|error| format!("collision : le décor est refusé : {error:?}"))?;

    let mut text = String::from(
        "# balayage : depart -> arrivee | boite | fraction | normale | surface | cellule | etat\n",
    );
    for (index, sweep) in all().into_iter().enumerate() {
        let cell = world.locate(sweep.from);
        let Some(hit) = world.sweep(cell, sweep.half, sweep.from, sweep.to) else {
            text.push_str(&format!("{index:4} : hors de toute cellule\n"));
            continue;
        };
        let state = match (hit.start_solid, hit.incomplete) {
            (true, _) => "depart-solide",
            (false, true) => "tronque",
            (false, false) => "-",
        };
        text.push_str(&format!(
            "{index:4} : ({:.2}, {:.2}, {:.2}) -> ({:.2}, {:.2}, {:.2}) | {:.2} | {:.4} | \
             ({:.3}, {:.3}, {:.3}) | {} | {} | {state}\n",
            sweep.from.x,
            sweep.from.y,
            sweep.from.z,
            sweep.to.x,
            sweep.to.y,
            sweep.to.z,
            sweep.half.x,
            hit.fraction,
            hit.normal.x,
            hit.normal.y,
            hit.normal.z,
            hit.surface,
            hit.cell
        ));
    }
    Ok(text)
}
