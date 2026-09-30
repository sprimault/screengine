// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les tests des trois familles du volume dilaté.
//!
//! Ils portent sur des **propriétés vraies pour tout un intervalle** — la boîte
//! s'arrête avant la surface, la normale s'oppose au mouvement, un mouvement
//! rasant ne touche rien — et jamais sur une fraction précise : la constante de
//! dilatation n'est pas encore figée, et des valeurs attendues les feraient tous
//! rougir le jour où elle le sera.

use super::*;

/// Un carré de quatre unités dans le plan `z = 0`, parcouru dans le sens direct.
fn square() -> [Vec3d; 4] {
    [
        Vec3d::new(0.0, 0.0, 0.0),
        Vec3d::new(4.0, 0.0, 0.0),
        Vec3d::new(4.0, 4.0, 0.0),
        Vec3d::new(0.0, 4.0, 0.0),
    ]
}

/// La normale intérieure du carré, vers le haut. Non unitaire, comme celle que le
/// chargement dérive.
fn up() -> Vec3d {
    Vec3d::new(0.0, 0.0, 32.0)
}

/// Une boîte qui tombe sur le carré s'arrête avant de le traverser.
///
/// La propriété, et non la valeur : la fraction rendue doit placer le bas de la
/// boîte au-dessus du plan, quelle que soit la dilatation.
#[test]
fn une_boite_qui_tombe_s_arrete_au_dessus_du_plan() {
    let half = Vec3d::new(0.5, 0.5, 1.0);
    let from = Vec3d::new(2.0, 2.0, 5.0);
    let to = Vec3d::new(2.0, 2.0, -5.0);

    let touch = face(&square(), up(), square()[0], half, from, to).expect("la boîte traverse");
    assert!(touch.fraction > 0.0 && touch.fraction < 1.0);
    let centre = from + (to - from) * touch.fraction;
    assert!(
        centre.z >= half.z,
        "le bas de la boîte reste au-dessus du sol"
    );
    assert_eq!(touch.rank, RANK_FACE);
}

/// Une boîte qui passe à côté du carré ne le touche pas par sa face.
///
/// C'est l'appartenance au polygone **non dilaté** qui l'écarte : sans elle, le
/// plan infini de la surface arrêterait la boîte à huit unités de tout mur.
#[test]
fn une_boite_a_cote_du_carre_ne_touche_pas_sa_face() {
    let half = Vec3d::new(0.5, 0.5, 1.0);
    let from = Vec3d::new(20.0, 20.0, 5.0);
    let to = Vec3d::new(20.0, 20.0, -5.0);

    assert_eq!(face(&square(), up(), square()[0], half, from, to), None);
}

/// Une surface franchie **en sortant** ne retient rien.
///
/// C'est la clause (b) de la règle de l'arête partagée : deux murs dos à dos, de
/// part et d'autre d'une frontière entre cellules, se départagent par elle seule.
#[test]
fn une_surface_franchie_en_sortant_ne_retient_rien() {
    let half = Vec3d::new(0.5, 0.5, 1.0);
    let from = Vec3d::new(2.0, 2.0, -5.0);
    let to = Vec3d::new(2.0, 2.0, 5.0);

    assert_eq!(face(&square(), up(), square()[0], half, from, to), None);
}

/// Un mouvement parallèle au plan est écarté avant toute division.
///
/// Le cas est écrit plutôt que rattrapé par un epsilon : il n'existe aucun
/// quotient à prendre, et un seuil aurait décidé à sa place sur les mouvements
/// presque rasants.
#[test]
fn un_mouvement_parallele_au_plan_ne_divise_pas() {
    let half = Vec3d::new(0.5, 0.5, 1.0);
    let from = Vec3d::new(0.0, 2.0, 3.0);
    let to = Vec3d::new(8.0, 2.0, 3.0);

    assert_eq!(face(&square(), up(), square()[0], half, from, to), None);
}

/// Le support d'une boîte est le même pour une normale et son opposée.
///
/// La valeur absolue le garantit, et la propriété compte : une surface et sa
/// voisine dos à dos portent des normales opposées, et une épaisseur qui
/// changerait de signe laisserait passer la boîte d'un côté.
#[test]
fn le_support_ne_depend_pas_du_signe_de_la_normale() {
    let half = Vec3d::new(0.5, 1.0, 2.0);
    let normal = Vec3d::new(3.0, -4.0, 5.0);
    assert_eq!(support(normal, half), support(-normal, half));
}

/// Le prisme d'une arête arrête une boîte qui vise le bord du carré.
///
/// Elle passe à côté de la face — son centre tombe hors du polygone — et c'est
/// l'arête qui la retient. Sans ce volume, elle traverserait au ras du bord.
#[test]
fn le_prisme_d_une_arete_arrete_une_boite_qui_vise_le_bord() {
    let half = Vec3d::new(1.0, 1.0, 1.0);
    let a = Vec3d::new(0.0, 0.0, 0.0);
    let b = Vec3d::new(4.0, 0.0, 0.0);
    let from = Vec3d::new(2.0, -5.0, 0.0);
    let to = Vec3d::new(2.0, 5.0, 0.0);

    let touch = edge(a, b, half, from, to).expect("la boîte rencontre l'arête");
    assert!(touch.fraction > 0.0 && touch.fraction < 1.0);
    assert_eq!(touch.rank, RANK_EDGE);
    let centre = from + (to - from) * touch.fraction;
    assert!(centre.y <= -half.y, "la boîte s'arrête avant l'arête");
}

/// Une boîte qui passe loin d'une arête ne la rencontre pas.
#[test]
fn une_boite_loin_d_une_arete_ne_la_rencontre_pas() {
    let half = Vec3d::new(0.25, 0.25, 0.25);
    let a = Vec3d::new(0.0, 0.0, 0.0);
    let b = Vec3d::new(4.0, 0.0, 0.0);
    let from = Vec3d::new(2.0, -5.0, 10.0);
    let to = Vec3d::new(2.0, 5.0, 10.0);

    assert_eq!(edge(a, b, half, from, to), None);
}

/// La boîte d'un sommet arrête une boîte qui vise le coin.
#[test]
fn la_boite_d_un_sommet_arrete_une_boite_qui_vise_le_coin() {
    let half = Vec3d::new(1.0, 1.0, 1.0);
    let point = Vec3d::new(0.0, 0.0, 0.0);
    let from = Vec3d::new(-5.0, -5.0, -5.0);
    let to = Vec3d::new(5.0, 5.0, 5.0);

    let touch = vertex(point, half, from, to).expect("la boîte rencontre le sommet");
    assert!(touch.fraction > 0.0 && touch.fraction < 1.0);
    assert_eq!(touch.rank, RANK_VERTEX);
}

/// Un déplacement nul ne rend aucun contact d'arête ni de sommet.
///
/// Le segment est un point : il n'entre par aucun plan, donc il n'a pas d'instant
/// d'entrée. C'est au balayage de traiter le départ dans le solide, par le
/// prédicat de recouvrement, et non à la découpe d'intervalle de l'inventer.
#[test]
fn un_deplacement_nul_ne_rend_aucun_contact() {
    let half = Vec3d::new(1.0, 1.0, 1.0);
    let point = Vec3d::new(0.0, 0.0, 0.0);
    assert_eq!(vertex(point, half, point, point), None);
}

/// À instant égal, la face l'emporte sur l'arête qui la borde.
///
/// C'est le premier critère de départage, et il est contractuel : l'ordre
/// inverse rendrait la normale d'une arête là où celle de la face est la bonne,
/// donc une glissade qui part de travers dans un coin de couloir. Le test le
/// vérifie sur les rangs **que les trois familles rendent**, et non en comparant
/// les constantes entre elles — ce que le compilateur sait déjà.
#[test]
fn a_instant_egal_la_face_l_emporte_sur_son_arete() {
    let half = Vec3d::new(1.0, 1.0, 1.0);
    let from = Vec3d::new(2.0, 2.0, 5.0);
    let to = Vec3d::new(2.0, 2.0, -5.0);

    let on_face = face(&square(), up(), square()[0], half, from, to).expect("la face répond");
    let corner = vertex(square()[0], half, from, to);

    assert_eq!(on_face.rank, RANK_FACE);
    assert!(
        corner.is_none() || on_face.rank < corner.expect("déjà testé").rank,
        "la face passe avant ce que le coin dirait"
    );
}

/// **Un segment qui part dans le prisme et s'y enfonce touche tout de suite.**
///
/// Sans instant d'entrée, la découpe d'intervalle ne rendait rien : le prisme
/// était franchi par tous ses plans avant le départ, donc aucun ne posait
/// d'entrée. Une position qu'un balayage précédent a posée au contact tombe
/// exactement là — à une dilatation près —, et le balayage suivant la laissait
/// entrer librement.
///
/// La normale rendue est celle du plan dont on est le plus proche de sortir,
/// donc celle qui demande le moins de recul : ici le côté par lequel on est
/// entré.
#[test]
fn un_segment_parti_dans_le_prisme_touche_immediatement() {
    let half = Vec3d::new(1.0, 1.0, 1.0);
    let (a, b) = (Vec3d::new(0.0, 0.0, 0.0), Vec3d::new(0.0, 0.0, 4.0));

    // Juste à l'intérieur du prisme, du côté des `x` positifs, et qui s'enfonce.
    let from = Vec3d::new(0.99, 0.0, 2.0);
    let to = Vec3d::new(0.5, 0.0, 2.0);

    let touch = edge(a, b, half, from, to).expect("le contact est immédiat");
    assert_eq!(touch.fraction, 0.0);
    assert_eq!(touch.rank, RANK_EDGE);
    assert!(
        touch.normal.dot(to - from) < 0.0,
        "la normale s'oppose au mouvement"
    );
}

/// **Un segment qui part dans le prisme et en ressort n'est pas arrêté.**
///
/// L'autre moitié de la clause, et celle qui décide qu'un mobile ne reste pas
/// collé : bloquer ce cas rendrait une fraction nulle à chaque image, sans rien
/// qui permette à l'hôte d'en sortir — indiscernable d'un mur, du dehors.
#[test]
fn un_segment_qui_ressort_du_prisme_n_est_pas_arrete() {
    let half = Vec3d::new(1.0, 1.0, 1.0);
    let (a, b) = (Vec3d::new(0.0, 0.0, 0.0), Vec3d::new(0.0, 0.0, 4.0));

    let from = Vec3d::new(0.99, 0.0, 2.0);
    for to in [
        // Il s'éloigne.
        Vec3d::new(1.5, 0.0, 2.0),
        // Il longe, sans entrer ni sortir : glisser ne bloque pas non plus.
        Vec3d::new(0.99, 0.0, 2.5),
    ] {
        assert_eq!(edge(a, b, half, from, to), None, "vers {to:?}");
    }
}
