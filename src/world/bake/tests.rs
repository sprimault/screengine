// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le calcul se teste sur des luxels, pas sur une image.
//!
//! **Le cas qui compte est le coin de mur** : une lampe posée dans un angle doit
//! éclairer ses deux faces différemment, faute de quoi l'angle disparaît. C'est ce
//! que le terme de Lambert apporte, et c'est le seul de ces tests qui juge une
//! intention plutôt qu'une mécanique.

use alloc::vec::Vec;

use super::*;
use crate::format::world::tests::{cell_bytes, file, frame, material, words};
use crate::world::atlas::pack;

/// Une boîte de huit unités de côté, éclairée par une lampe.
///
/// Les six faces portent un repère dans leur plan, d'un luxel par unité. La lampe
/// est décrite en octets comme le format l'attend.
fn lit_box(light: [f32; 3], radius: f32) -> World {
    let points = [
        [0.0f32, 0.0, 0.0],
        [8.0, 0.0, 0.0],
        [8.0, 8.0, 0.0],
        [0.0, 8.0, 0.0],
        [0.0, 0.0, 8.0],
        [8.0, 0.0, 8.0],
        [8.0, 8.0, 8.0],
        [0.0, 8.0, 8.0],
    ];
    // Sol, plafond, et les quatre murs ; chaque repère a ses axes dans le plan de
    // sa face, comme le chargement l'exige.
    let faces: [([u32; 4], [f32; 3], [f32; 3]); 6] = [
        ([0, 1, 2, 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([7, 6, 5, 4], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0, 4, 5, 1], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([1, 5, 6, 2], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([2, 6, 7, 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([3, 7, 4, 0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let surfaces: Vec<Vec<u8>> = faces
        .iter()
        .enumerate()
        .map(|(i, (indices, u, v))| {
            let mut bytes = words(&[i as u32 + 1, 0, 1, 4]);
            bytes.extend_from_slice(&words(indices));
            bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], *u, *v));
            bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], *u, *v));
            bytes
        })
        .collect();

    let mut lights = words(&[1]);
    for value in [light[0], light[1], light[2], radius] {
        lights.extend_from_slice(&value.to_le_bytes());
    }
    lights.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]);

    let cell = cell_bytes(7, 0, &points, &surfaces, &[]);
    World::load(&file(&cell, &[], &lights, &material(1, "mur"))).expect("carte valide")
}

/// Une cellule en L, huit unités sur huit, avec une lampe posée à l'intérieur.
///
/// L'empreinte concave est celle du décor de validation, et elle est ici parce
/// qu'une boîte ne met à l'épreuve ni la découpe d'oreilles du chargement, ni un
/// mur dont l'arête recule.
fn lit_ell(light: [f32; 3], radius: f32) -> World {
    let footprint = [
        [0.0f32, 0.0],
        [8.0, 0.0],
        [8.0, 4.0],
        [4.0, 4.0],
        [4.0, 8.0],
        [0.0, 8.0],
    ];
    let n = footprint.len();

    let mut points = Vec::new();
    for z in [0.0f32, 4.0] {
        for corner in &footprint {
            points.push([corner[0], corner[1], z]);
        }
    }

    let mut surfaces: Vec<Vec<u8>> = Vec::new();
    let mut push = |id: u32, material: u32, indices: &[u32], u: [f32; 3], v: [f32; 3]| {
        let mut bytes = words(&[id, 0, material, indices.len() as u32]);
        bytes.extend_from_slice(&words(indices));
        bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], u, v));
        bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], u, v));
        surfaces.push(bytes);
    };

    let floor: Vec<u32> = (0..n as u32).collect();
    push(1, 1, &floor, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let ceiling: Vec<u32> = (n as u32..(n * 2) as u32).rev().collect();
    push(2, 1, &ceiling, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b) = (footprint[i], footprint[j]);
        push(
            i as u32 + 3,
            1,
            &[i as u32, (i + n) as u32, (j + n) as u32, j as u32],
            [b[0] - a[0], b[1] - a[1], 0.0],
            [0.0, 0.0, 1.0],
        );
    }

    let mut lights = words(&[1]);
    for value in [light[0], light[1], light[2], radius] {
        lights.extend_from_slice(&value.to_le_bytes());
    }
    lights.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]);

    let cell = cell_bytes(9, 0, &points, &surfaces, &[]);
    World::load(&file(&cell, &[], &lights, &material(1, "mur"))).expect("carte valide")
}

/// La cellule en L, plus le couloir qu'un portail lui raccorde.
///
/// Les occulteurs d'une cellule sont les siens **et ceux de ses voisines à un
/// portail** : une cellule seule ne met donc à l'épreuve que la moitié du calcul
/// d'ombre, et c'est l'autre moitié qui décide de ce qu'un décor rend.
fn lit_pair(light: [f32; 3], radius: f32) -> World {
    let ell = [
        [0.0f32, 0.0],
        [8.0, 0.0],
        [8.0, 4.0],
        [4.0, 4.0],
        [4.0, 8.0],
        [0.0, 8.0],
    ];
    let corridor = [[8.0f32, 0.0], [16.0, 0.0], [12.0, 4.0], [8.0, 4.0]];

    let mut cells = prism_at(1, 1, &ell, &[1], 0.0);
    cells.extend_from_slice(&prism_at(2, 100, &corridor, &[3], 0.0));

    let mut lights = words(&[1]);
    for value in [light[0], light[1], light[2], radius] {
        lights.extend_from_slice(&value.to_le_bytes());
    }
    lights.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]);

    World::load(&file(&cells, &[], &lights, &material(1, "mur"))).expect("carte valide")
}

/// Les octets d'une cellule prismatique, sur le modèle du décor de validation.
///
/// `low` est l'altitude de son sol ; son plafond est quatre unités plus haut.
fn prism_at(
    id: u32,
    first_id: u32,
    footprint: &[[f32; 2]],
    portals: &[usize],
    low: f32,
) -> Vec<u8> {
    let n = footprint.len();
    let mut points = Vec::new();
    for z in [low, low + 4.0] {
        for corner in footprint {
            points.push([corner[0], corner[1], z]);
        }
    }

    let mut surfaces: Vec<Vec<u8>> = Vec::new();
    let mut push = |id: u32, indices: &[u32], u: [f32; 3], v: [f32; 3]| {
        let mut bytes = words(&[id, 0, 1, indices.len() as u32]);
        bytes.extend_from_slice(&words(indices));
        bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], u, v));
        bytes.extend_from_slice(&frame([0.0, 0.0, 0.0], u, v));
        surfaces.push(bytes);
    };

    let floor: Vec<u32> = (0..n as u32).collect();
    push(first_id, &floor, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let ceiling: Vec<u32> = (n as u32..(n * 2) as u32).rev().collect();
    push(first_id + 1, &ceiling, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);

    let mut next = first_id + 2;
    for i in 0..n {
        if portals.contains(&i) {
            continue;
        }
        let j = (i + 1) % n;
        let (a, b) = (footprint[i], footprint[j]);
        push(
            next,
            &[i as u32, (i + n) as u32, (j + n) as u32, j as u32],
            [b[0] - a[0], b[1] - a[1], 0.0],
            [0.0, 0.0, 1.0],
        );
        next += 1;
    }

    let mut openings = Vec::new();
    for i in portals {
        let j = (i + 1) % n;
        openings.push(words(&[next, 4]));
        let last = openings.len() - 1;
        openings[last].extend_from_slice(&words(&[
            *i as u32,
            j as u32,
            (j + n) as u32,
            (i + n) as u32,
        ]));
        next += 1;
    }

    cell_bytes(id, 0, &points, &surfaces, &openings)
}

/// La plus claire des valeurs écrites pour une surface de la première cellule.
fn brightest(baked: &Baked, world: &World, surface: usize) -> u8 {
    brightest_of(baked, world, 0, surface)
}

/// La plus claire des valeurs écrites pour une surface d'une cellule quelconque.
fn brightest_of(baked: &Baked, world: &World, cell: usize, surface: usize) -> u8 {
    let extent = world.cells()[cell].surfaces[surface].luxels;
    let slot = baked.atlas.slots[surface];
    let mut best = 0;
    for j in 0..extent.height {
        for i in 0..extent.width {
            let x = slot.x + GUTTER + i;
            let y = slot.y + GUTTER + j;
            let value = baked.texels[((y * baked.side + x) * 4) as usize];
            if value > best {
                best = value;
            }
        }
    }
    best
}

/// Le luxel d'une surface, lu dans l'atlas.
fn luxel(baked: &Baked, surface: usize, i: u32, j: u32) -> [u8; 3] {
    let slot = baked.atlas.slots[surface];
    let x = slot.x + GUTTER + i;
    let y = slot.y + GUTTER + j;
    let base = ((y * baked.side + x) * 4) as usize;
    [
        baked.texels[base],
        baked.texels[base + 1],
        baked.texels[base + 2],
    ]
}

/// Cuit la cellule unique d'une carte.
fn bake_one(world: &World) -> Baked {
    let atlas = pack(&world.cells()[0]).expect("rangement possible");
    bake(world, 0, atlas).expect("cuisson possible")
}

/// Une lampe dans un coin éclaire ses deux faces, et différemment.
///
/// **C'est le critère qui a été demandé**, et il tient au terme de Lambert : sans
/// lui, les deux murs d'un angle à égale distance de la lampe reçoivent la même
/// valeur, l'angle s'aplatit et la cuisson perd son objet. Ici la lampe est plus
/// près d'un mur que de l'autre, et les deux doivent le montrer.
#[test]
fn une_lampe_dans_un_angle_eclaire_ses_deux_faces() {
    let world = lit_box([1.0, 3.0, 4.0], 12.0);
    let baked = bake_one(&world);

    // Le mur `y = 0` (surface 2) et le mur `x = 0` (surface 5) forment l'angle où
    // la lampe se tient.
    let near = luxel(&baked, 2, 1, 4);
    let side = luxel(&baked, 5, 3, 4);

    assert!(near[0] > 0, "le mur proche n'est pas éclairé");
    assert!(side[0] > 0, "le mur de côté n'est pas éclairé");
    assert_ne!(
        near, side,
        "les deux faces de l'angle reçoivent la même valeur : l'angle a disparu"
    );
}

/// Un luxel que la lumière frôle est bien plus sombre que celui qu'elle frappe de
/// face.
///
/// **C'est ici que le terme de Lambert se prouve, et nulle part ailleurs.**
/// Comparer deux faces à des distances différentes ne prouve rien : l'atténuation
/// seule les distingue déjà, et le test reste vert quand on retire Lambert — vu en
/// l'écrivant. Une lampe posée près du sol, en revanche, frappe le luxel qui est
/// sous elle de plein fouet et frôle celui du coin opposé : l'atténuation seule
/// mettrait les deux dans un rapport d'environ 1,3, Lambert le porte au-delà de 3.
#[test]
fn un_luxel_rasant_est_bien_plus_sombre_qu_un_luxel_de_face() {
    let world = lit_box([4.0, 4.0, 1.0], 16.0);
    let baked = bake_one(&world);

    let under = luxel(&baked, 0, 4, 4)[0] as u32;
    let grazing = luxel(&baked, 0, 0, 0)[0] as u32;
    assert!(grazing > 0, "le coin doit recevoir quelque chose");
    assert!(
        under > grazing * 3,
        "sous la lampe {under}, au coin {grazing} : l'atténuation seule suffirait \
         à expliquer cet écart, donc l'orientation n'entre pas dans le calcul"
    );
}

/// Le plafond s'éclaire comme le sol quand la lampe est entre les deux.
///
/// Les deux faces horizontales d'une cellule ont des enroulements inverses, donc
/// des normales de Newell opposées : si l'une est prise à l'endroit, l'autre l'est
/// à l'envers, son terme de Lambert devient négatif et elle reste noire. Le test
/// les prend ensemble parce que c'est l'écart entre elles qui dit le défaut, et
/// non leur valeur.
#[test]
fn le_plafond_s_eclaire_comme_le_sol() {
    let world = lit_box([4.0, 4.0, 4.0], 16.0);
    let baked = bake_one(&world);

    let floor = luxel(&baked, 0, 4, 4)[0];
    let ceiling = luxel(&baked, 1, 4, 4)[0];
    assert!(floor > 0, "le sol est éteint");
    assert!(
        ceiling > 0,
        "le plafond est éteint alors que le sol reçoit {floor} : sa normale est à l'envers"
    );
}

/// Une face détournée de la lampe reste noire.
///
/// Le rejet par le terme de Lambert, qui est aussi le plus payant du calcul :
/// aucun rayon ne part vers une face que la lumière ne peut pas atteindre.
///
/// **La lampe est hors de la cellule**, et il n'y a pas d'autre moyen : les six
/// faces d'une boîte regardent son intérieur, donc une lampe posée dedans les
/// éclaire toutes. Une lampe posée derrière le mur `y = 8` est la seule façon de
/// lui présenter la lumière à contre-sens, et elle reste à portée pour que ce soit
/// bien l'orientation qui la rejette, et non la distance.
#[test]
fn une_face_detournee_reste_noire() {
    let world = lit_box([4.0, 12.0, 4.0], 16.0);
    let baked = bake_one(&world);
    assert_eq!(luxel(&baked, 4, 4, 4), [0, 0, 0]);
}

/// Un mur entre la lampe et une face laisse celle-ci noire.
///
/// **Le test d'appartenance au polygone se prouve ici, et nulle part ailleurs.**
/// Une lampe posée derrière le mur `y = 8` fait bien face au mur `y = 0`, qui est
/// à portée et correctement orienté : seule l'occlusion peut l'éteindre, et le
/// rayon frappe le mur occultant en son centre. Un test d'appartenance qui compte
/// une parité d'arêtes plutôt que des traversées rend « dehors » pour ce centre —
/// quatre arêtes du même côté, parité paire —, la lumière traverse le mur et la
/// face s'éclaire. Vu en l'écrivant.
#[test]
fn un_mur_interpose_eteint_la_face_qu_il_couvre() {
    let world = lit_box([4.0, 12.0, 4.0], 32.0);
    let baked = bake_one(&world);
    assert_eq!(luxel(&baked, 2, 4, 4), [0, 0, 0]);
}

/// Toutes les faces d'une cellule en L reçoivent quelque chose.
///
/// Une lampe posée dans la branche basse du L voit ses huit faces : aucune ne lui
/// tourne le dos, et aucune n'est derrière une autre. Une face entièrement noire ne
/// peut donc venir que du calcul, et c'est ce qu'une empreinte concave attrape et
/// qu'une boîte laisse passer.
#[test]
fn une_cellule_en_l_eclaire_toutes_ses_faces() {
    let world = lit_ell([3.0, 3.0, 2.0], 20.0);
    let baked = bake_one(&world);

    let dark: Vec<usize> = (0..world.cells()[0].surfaces.len())
        .filter(|&i| brightest(&baked, &world, i) == 0)
        .collect();
    assert!(dark.is_empty(), "faces entièrement noires : {dark:?}");
}

/// Le voisin par portail n'éteint aucune face de la cellule.
///
/// Même lampe, même cellule que le cas précédent : seul le couloir s'ajoute, et
/// avec lui ses surfaces dans les occulteurs. Une face qui s'éclairait et qui
/// s'éteint ne peut alors venir que de là.
#[test]
fn le_voisin_par_portail_n_eteint_aucune_face() {
    let world = lit_pair([3.0, 3.0, 2.0], 20.0);
    let baked = bake_one(&world);

    let dark: Vec<usize> = (0..world.cells()[0].surfaces.len())
        .filter(|&i| brightest(&baked, &world, i) == 0)
        .collect();
    assert!(dark.is_empty(), "faces entièrement noires : {dark:?}");
}

/// Le mur dont le plan passe par le barycentre de sa cellule s'éclaire.
///
/// **Le cas qui a fait tomber l'orientation par barycentre.** Le mur `y = 4` de la
/// salle en L contient `(4, 4, 2)`, qui est le barycentre de ses douze sommets :
/// le produit scalaire qui devait décider du sens s'annule, le signe brut de Newell
/// passe, et la normale sort à l'envers. Le test vise ce mur nommément, parce qu'un
/// compte de faces noires ne dirait pas laquelle.
#[test]
fn un_mur_qui_contient_le_barycentre_s_eclaire() {
    let world = lit_pair([3.0, 3.0, 2.0], 20.0);
    let baked = bake_one(&world);
    assert!(
        brightest(&baked, &world, 3) > 0,
        "le mur y = 4 est noir : sa normale est prise à l'envers"
    );
}

/// Une cellule close au-dessus d'une lampe reste entièrement noire.
///
/// Deux cellules superposées, sans portail entre elles : le plancher de celle du
/// haut ferme la région, et rien de ce qui brûle en bas ne doit y entrer. C'est le
/// pendant de la clause d'étanchéité — « rien d'extérieur ne peut contribuer » —,
/// et c'est aussi ce qui rend suffisante l'empreinte du cache : une lightmap qui
/// dépendrait d'une lampe d'une autre région ne saurait plus quand se refaire.
#[test]
fn une_cellule_close_au_dessus_d_une_lampe_reste_noire() {
    let ell = [
        [0.0f32, 0.0],
        [8.0, 0.0],
        [8.0, 4.0],
        [4.0, 4.0],
        [4.0, 8.0],
        [0.0, 8.0],
    ];
    let mut cells = prism_at(1, 1, &ell, &[], 0.0);
    cells.extend_from_slice(&prism_at(2, 100, &ell, &[], 8.0));

    let mut lights = words(&[1]);
    for value in [3.0f32, 3.0, 2.0, 32.0] {
        lights.extend_from_slice(&value.to_le_bytes());
    }
    lights.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]);

    let world =
        World::load(&file(&cells, &[], &lights, &material(1, "mur"))).expect("carte valide");
    let atlas = pack(&world.cells()[1]).expect("rangement possible");
    let baked = bake(&world, 1, atlas).expect("cuisson possible");

    let lit: Vec<usize> = (0..world.cells()[1].surfaces.len())
        .filter(|&i| brightest_of(&baked, &world, 1, i) > 0)
        .collect();
    assert!(
        lit.is_empty(),
        "la lumière a traversé le plancher : {lit:?}"
    );
}

/// Un luxel plus proche de la lampe est plus clair.
#[test]
fn l_eclairement_decroit_avec_la_distance() {
    let world = lit_box([4.0, 4.0, 1.0], 16.0);
    let baked = bake_one(&world);

    // Sur le sol, sous la lampe puis au coin : la distance croît, l'éclairement
    // décroît.
    let under = luxel(&baked, 0, 4, 4);
    let corner = luxel(&baked, 0, 0, 0);
    assert!(
        under[0] > corner[0],
        "sous la lampe {under:?}, au coin {corner:?}"
    );
}

/// Une lampe hors de portée n'éclaire rien.
///
/// L'atténuation s'annule en `r` avec une dérivée nulle : au-delà, rien, et pas
/// même l'anneau que `1 − d²/r²` seule dessinerait à son bord.
#[test]
fn une_lampe_hors_de_portee_n_eclaire_rien() {
    let world = lit_box([4.0, 4.0, 4.0], 0.5);
    let baked = bake_one(&world);
    assert_eq!(luxel(&baked, 0, 4, 4), [0, 0, 0]);
}

/// La gouttière porte la valeur du bord qu'elle prolonge.
///
/// Sans elle, le bilinéaire du rasteriseur irait chercher la surface voisine dans
/// l'atlas au bord de celle-ci.
#[test]
fn la_gouttiere_prolonge_le_bord() {
    let world = lit_box([4.0, 4.0, 1.0], 16.0);
    let baked = bake_one(&world);
    let slot = baked.atlas.slots[0];

    let at = |x: u32, y: u32| {
        let base = ((y * baked.side + x) * 4) as usize;
        [
            baked.texels[base],
            baked.texels[base + 1],
            baked.texels[base + 2],
        ]
    };
    // Le luxel de gouttière du coin haut gauche reprend le premier luxel utile.
    assert_eq!(at(slot.x, slot.y), at(slot.x + GUTTER, slot.y + GUTTER));
}

/// Deux cuissons de la même cellule rendent les mêmes octets.
///
/// Le déterminisme n'est pas une élégance ici : une lightmap entre dans l'image,
/// et deux constructions qui en donneraient deux feraient diverger les empreintes
/// entre plateformes.
#[test]
fn deux_cuissons_rendent_les_memes_octets() {
    let world = lit_box([2.0, 2.0, 6.0], 14.0);
    assert_eq!(bake_one(&world).texels, bake_one(&world).texels);
}

/// L'alpha d'un luxel est toujours écrit à pleine valeur.
#[test]
fn l_alpha_est_toujours_opaque() {
    let world = lit_box([4.0, 4.0, 4.0], 12.0);
    let baked = bake_one(&world);
    let slot = baked.atlas.slots[0];
    let base = (((slot.y + GUTTER) * baked.side + slot.x + GUTTER) * 4) as usize;
    assert_eq!(baked.texels[base + 3], 0xFF);
}
