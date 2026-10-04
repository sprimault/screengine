// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les interrogations par rayon de la scène de sélection, et leur empreinte.
//!
//! **Le pendant de [`crate::sweeps`] pour l'étape 8**, et il en reprend tout ce
//! qui ne dépend pas de la boîte : le décor, le treillis de départs, les dix
//! directions, le format du fichier, la règle qui veut qu'on ne hache que ce que
//! la frontière publie.
//!
//! **Ce qu'il ajoute est le filtre**, et c'est la seule chose qui sépare un
//! rayon d'un balayage d'étendue nulle. Chaque départ est donc joué **deux
//! fois** — les surfaces solides seules, puis toutes —, et l'empreinte porte le
//! filtre avec le résultat : sur un décor dont une surface est non solide, les
//! deux réponses diffèrent, et c'est précisément ce que la scène existe pour
//! figer.
//!
//! **Le rayon n'est pas dilaté**, la marge de sécurité étant relative à la plus
//! grande demi-étendue : il touche ce qu'il croise et non ce qu'il frôle. Cette
//! propriété-là ne se lit pas dans l'empreinte, qui ne dit que « ces bits » ; ce
//! sont les tests du noyau qui la gardent, et le rapport texte qui la montre.

use screengine::{Hit, Surfaces, Vec3, World};
use screengine_conformance::collision_file;

use crate::hash;
use crate::sweeps::{
    DIRECTIONS, REACH, STATUS_INCOMPLETE, STATUS_NO_CELL, STATUS_OK, STATUS_START_SOLID, starts,
};

/// Les deux filtres que `scg_world_pick` accepte, tels que `docs/abi.md` les
/// publie.
///
/// **Recopiés plutôt que partagés**, pour la raison des codes de statut : un
/// écart entre ces valeurs et celles du header ferait diverger cette empreinte
/// de celles des hôtes, qui passent l'entier à la frontière sans le traduire.
const PICK_SOLID: u32 = 1;
/// Toutes les surfaces, non solides comprises.
const PICK_ALL: u32 = 2;

/// Un rayon à jouer : son départ, son arrivée, et ce qu'il retient.
pub struct Pick {
    /// Le point de départ.
    pub from: Vec3,
    /// Le point d'arrivée.
    pub to: Vec3,
    /// Le filtre, [`PICK_SOLID`] ou [`PICK_ALL`].
    pub filter: u32,
}

/// Tous les rayons de la scène, dans l'ordre où l'empreinte les prend.
///
/// L'ordre est contractuel, comme celui des balayages : le changer déplacerait
/// la référence sans que rien n'ait bougé du moteur.
///
/// **Le filtre varie le plus lentement**, si bien que la liste se lit en deux
/// moitiés — les mêmes rayons, d'abord solides puis tous. Une divergence qui ne
/// toucherait qu'une moitié se voit alors dans le rapport sans avoir à le
/// trier.
pub fn all() -> Vec<Pick> {
    let starts = starts();
    let mut picks = Vec::new();
    for filter in [PICK_SOLID, PICK_ALL] {
        for from in &starts {
            for direction in DIRECTIONS {
                picks.push(Pick {
                    from: *from,
                    to: Vec3::new(
                        from.x + direction[0] * REACH,
                        from.y + direction[1] * REACH,
                        from.z + direction[2] * REACH,
                    ),
                    filter,
                });
            }
        }
        picks.push(saturating(filter));
    }
    picks
}

/// Le rayon qui épuise le budget de cellules, partagé avec le balayage.
///
/// **La sélection a le même budget et le même statut**, et c'est tout ce que ce
/// cas ajoute : un rayon n'a aucun jeu à perdre, donc il ne peut pas rendre
/// `SCG_STATUS_NO_GAP`, et son troisième statut possible est celui-ci.
///
/// **En queue de chaque moitié et non de la liste**, donc une fois par filtre :
/// la liste se coupe en deux moitiés qui posent les mêmes rayons, et c'est elle
/// qui rend son rapport lisible sans tri. Un cas ajouté à la seule queue l'aurait
/// rendue impaire et aurait désapparié les deux moitiés — ce que [`tests`]
/// refuse, et qui vaut mieux qu'un rapport qu'on ne peut plus lire en regard.
/// L'enfilade n'ayant aucune surface non solide, les deux filtres y voient la
/// même chose : la seconde moitié ne coûte donc qu'un enregistrement, pour que la
/// structure reste vraie.
fn saturating(filter: u32) -> Pick {
    let entry = collision_file::chain_entry();
    let exit = collision_file::chain_exit();
    Pick {
        from: Vec3::new(entry[0], entry[1], entry[2]),
        to: Vec3::new(exit[0], exit[1], exit[2]),
        filter,
    }
}

/// Le filtre du noyau qu'une constante d'ABI désigne.
///
/// La liste n'en porte que deux, et elle est engendrée ici : un troisième cas
/// serait un défaut de ce module, pas une donnée.
fn surfaces(filter: u32) -> Surfaces {
    if filter == PICK_ALL {
        Surfaces::All
    } else {
        Surfaces::Solid
    }
}

/// L'empreinte de la scène d'interrogation.
///
/// Chaque rayon est joué **deux fois**, par la traversée et par la force brute,
/// et les deux doivent rendre les mêmes bits : c'est le théorème de l'étape 7
/// transposé au rayon, et le seul contrôle qui attrape une traversée trop
/// étroite.
pub fn digest() -> Result<u64, String> {
    let world = World::load(&collision_file::bytes())
        .map_err(|error| format!("selection : le décor est refusé : {error:?}"))?;

    let mut bytes = Vec::new();
    for (index, pick) in all().into_iter().enumerate() {
        let filter = surfaces(pick.filter);
        let cell = world.locate(pick.from);
        let fast = if cell == 0 {
            None
        } else {
            world.pick(cell, pick.from, pick.to, filter)
        };
        let slow = world.pick_brute(pick.from, pick.to, filter);

        if let Some(fast) = &fast
            && let Err(cause) = agree(fast, &slow)
        {
            return Err(format!(
                "selection : rayon {index}, {cause}\n  \
                 traversée {fast:?}\n  brute     {slow:?}"
            ));
        }

        let (published, status) = publish(fast, pick.to);
        absorb(&published, status, pick.filter, &mut bytes);
    }
    Ok(hash::of(&bytes))
}

/// Ce que la frontière publie d'une interrogation : un résultat et un statut.
///
/// La même règle que pour le balayage, et le même ordre de priorité — il est
/// contractuel, et il entre dans la référence avec le reste.
///
/// **Le statut « sans jeu » n'y figure pas, et c'est une propriété du rayon**, pas
/// un oubli : sa dilatation est nulle par construction, donc il n'a aucun jeu à
/// perdre et le noyau ne lève jamais le drapeau ici. L'écrire quand même serait
/// une branche inatteignable.
fn publish(hit: Option<Hit>, to: Vec3) -> (Hit, u8) {
    match hit {
        None => (free(to), STATUS_NO_CELL),
        Some(hit) if hit.start_solid => (hit, STATUS_START_SOLID),
        Some(hit) if hit.incomplete => (hit, STATUS_INCOMPLETE),
        Some(hit) => (hit, STATUS_OK),
    }
}

/// Le rayon libre que rend un départ hors de toute cellule.
fn free(to: Vec3) -> Hit {
    Hit {
        fraction: 1.0,
        normal: Vec3::ZERO,
        point: to,
        surface: 0,
        cell: 0,
        start_solid: false,
        incomplete: false,
        no_gap: false,
    }
}

/// Absorbe un résultat dans l'empreinte, son statut et son filtre compris.
///
/// **Le filtre en fait partie**, et c'est ce qui distingue cette empreinte de
/// celle du balayage : sans lui, les deux moitiés de la liste se hacheraient
/// comme si elles posaient la même question.
fn absorb(hit: &Hit, status: u8, filter: u32, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&hit.fraction.to_bits().to_le_bytes());
    for value in [hit.normal.x, hit.normal.y, hit.normal.z] {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    for value in [hit.point.x, hit.point.y, hit.point.z] {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    bytes.extend_from_slice(&hit.surface.to_le_bytes());
    bytes.extend_from_slice(&hit.cell.to_le_bytes());
    bytes.push(status);
    bytes.extend_from_slice(&filter.to_le_bytes());
}

/// Ce que les deux chemins se doivent, selon que la traversée a tout examiné.
///
/// Même relation que pour le balayage, et pour la même raison : la sélection
/// partage son budget de cellules, donc elle tronque aux mêmes endroits, alors
/// que la force brute n'a pas de borne. Le détail est dans
/// [`crate::sweeps::agree`] — ici comme là, un résultat tronqué promet le
/// conservatisme de sa fraction et rien de plus.
fn agree(fast: &Hit, slow: &Hit) -> Result<(), &'static str> {
    if !fast.incomplete {
        return if same(fast, slow) {
            Ok(())
        } else {
            Err("la traversée et la force brute divergent")
        };
    }
    if fast.fraction < 1.0 {
        Ok(())
    } else {
        Err("la traversée annonce une troncature et rend le rayon entier")
    }
}

/// Deux résultats portent-ils exactement les mêmes bits ?
///
/// Par les bits et non par `==`, pour la raison écrite dans [`crate::sweeps`] :
/// deux zéros de signes opposés se comparent égaux et donnent deux empreintes.
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

/// La magie du fichier de rayons, en tête de celui-ci.
///
/// Sans numéro de version, pour la raison écrite sur celle des balayages : ce
/// n'est pas un format du moteur, mais un fichier d'épreuve versionné à côté des
/// hôtes qui le lisent et comparé octet pour octet par un test.
const MAGIC: &[u8; 8] = b"SCGPICKS";

/// La liste des rayons, telle que les hôtes la lisent.
///
/// Six flottants puis le filtre en `u32`, octet de poids faible en tête, dans
/// l'ordre où [`all`] les rend. Vingt-huit octets par rayon.
///
/// **Ils lisent une liste, ils ne reportent pas la règle** : un treillis écrit
/// dans cinq langages prouverait que cinq programmeurs ont su reporter la même
/// géométrie, ce qui n'est pas ce qu'une empreinte d'hôte établit.
pub fn file_bytes() -> Vec<u8> {
    let picks = all();
    let mut bytes = Vec::with_capacity(MAGIC.len() + 4 + picks.len() * 28);

    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(picks.len() as u32).to_le_bytes());
    for pick in &picks {
        for vector in [pick.from, pick.to] {
            for value in [vector.x, vector.y, vector.z] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&pick.filter.to_le_bytes());
    }
    bytes
}

/// Le texte que `make conform-images` écrit, un rayon par ligne.
///
/// **Le pendant de « regarder les images » pour une scène qui n'en produit
/// pas.** Une empreinte dit qu'un résultat a changé, jamais qu'il est juste : un
/// décor où aucun rayon ne toucherait rien se figerait aussi bien qu'un autre.
pub fn report() -> Result<String, String> {
    let world = World::load(&collision_file::bytes())
        .map_err(|error| format!("selection : le décor est refusé : {error:?}"))?;

    let mut text = String::from(
        "# rayon : depart -> arrivee | filtre | fraction | normale | surface | cellule | etat\n",
    );
    for (index, pick) in all().into_iter().enumerate() {
        let cell = world.locate(pick.from);
        let found = if cell == 0 {
            None
        } else {
            world.pick(cell, pick.from, pick.to, surfaces(pick.filter))
        };
        let (hit, status) = publish(found, pick.to);
        let state = match status {
            STATUS_START_SOLID => "depart-solide",
            STATUS_INCOMPLETE => "tronque",
            STATUS_NO_CELL => "hors-cellule",
            _ => "-",
        };
        let filter = if pick.filter == PICK_ALL {
            "toutes"
        } else {
            "solides"
        };
        text.push_str(&format!(
            "{index:4} : ({:.2}, {:.2}, {:.2}) -> ({:.2}, {:.2}, {:.2}) | {filter:7} | {:.4} | \
             ({:.3}, {:.3}, {:.3}) | {} | {} | {state}\n",
            pick.from.x,
            pick.from.y,
            pick.from.z,
            pick.to.x,
            pick.to.y,
            pick.to.z,
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

#[cfg(test)]
mod tests;
