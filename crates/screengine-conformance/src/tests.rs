// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! La ligne de commande de la conformance, et la comparaison aux références.

use super::*;

/// Un répertoire de références propre à un test, vidé avant usage.
///
/// Sous `.tmp/` du dépôt et non dans le répertoire temporaire du système : sur
/// le poste de développement, l'antivirus surveille ce dernier.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/conformance-tests")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("répertoire de test");
    dir
}

/// Construit une liste d'arguments à partir de littéraux.
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

/// Les deux modes se reconnaissent par leur option.
#[test]
fn recognises_both_modes() {
    assert_eq!(parse_mode(&args(&["--check"])), Ok(Mode::Check));
    assert_eq!(parse_mode(&args(&["--update"])), Ok(Mode::Update));
}

/// Sans argument, aucun mode n'est choisi par défaut.
#[test]
fn rejects_missing_mode() {
    assert!(parse_mode(&[]).is_err());
}

/// Deux modes à la fois, ou une option inconnue, sont refusés.
#[test]
fn rejects_ambiguous_usage() {
    assert!(parse_mode(&args(&["--check", "--update"])).is_err());
    assert!(parse_mode(&args(&["--verify"])).is_err());
}

/// `--print` exige une scène connue : un nom mal orthographié rendrait sinon
/// une empreinte vide, et la comparaison avec l'hôte échouerait sans dire
/// pourquoi.
#[test]
fn print_exige_une_scene_connue() {
    assert_eq!(
        parse_mode(&args(&["--print", "arete"])),
        Ok(Mode::Print(Scene::Edge))
    );
    assert!(parse_mode(&args(&["--print", "cube"])).is_err());
    assert!(parse_mode(&args(&["--print"])).is_err());
}

/// La scène se rend, et deux rendus donnent la même empreinte : sans quoi il
/// n'y aurait rien à comparer entre le chemin Rust et les hôtes.
#[test]
fn la_scene_des_hotes_rend_une_empreinte_stable() {
    let first = Scene::Edge.render(Scene::HOST_PASS).expect("scène valide");
    assert_eq!(Scene::Edge.render(Scene::HOST_PASS), Ok(first));
}

/// Toutes les passes — tailles de tuile, image entière, ordre mélangé,
/// threads — rendent l'empreinte de `--print`, celle que les hôtes
/// comparent : une référence écrite depuis une autre passe ne vaudrait que
/// pour elle.
#[test]
fn toutes_les_passes_rendent_l_empreinte_des_hotes() {
    let host = Scene::Edge.render(Scene::HOST_PASS).expect("scène valide");
    assert_eq!(Scene::Edge.render_all(), Ok(host));
}

/// Chaque scène couvre une part franche de l'image.
///
/// Le contrôle qui manquerait le plus : une scène soumise dans le mauvais sens
/// est éliminée comme dos de face, rend un fond noir, et son empreinte reste
/// parfaitement stable d'une plateforme à l'autre. La suite entière resterait
/// verte en ne comparant rien.
///
/// Un dixième de l'image : assez haut pour qu'un fond noir ou quelques pixels
/// égarés échouent, assez bas pour n'imposer aucun cadrage aux scènes à venir.
#[test]
fn chaque_scene_couvre_une_part_de_l_image() {
    let (width, height) = Scene::RESOLUTION;
    let total = width as usize * height as usize;
    for scene in Scene::ALL {
        let pixels = scene.render_pixels(Scene::HOST_PASS).expect("scène valide");
        let drawn = pixels
            .chunks_exact(BYTES_PER_PIXEL)
            .filter(|pixel| pixel[..3] != [0, 0, 0])
            .count();
        assert!(
            drawn * 10 > total,
            "{} : {drawn} pixels dessinés sur {total}",
            scene.name()
        );
    }
}

/// Deux scènes ne rendent pas la même image.
///
/// Deux références identiques se relisent sans qu'on les remarque, et la scène
/// recopiée par erreur n'éprouve alors rien de ce que son nom annonce.
#[test]
fn deux_scenes_ne_rendent_pas_la_meme_image() {
    let hashes: Vec<u64> = Scene::ALL
        .iter()
        .map(|scene| scene.render_all().expect("scène valide"))
        .collect();
    for (i, a) in hashes.iter().enumerate() {
        for (j, b) in hashes.iter().enumerate().skip(i + 1) {
            assert_ne!(a, b, "{} et {}", Scene::ALL[i].name(), Scene::ALL[j].name());
        }
    }
}

/// Une référence absente est un échec qui dit quoi faire, jamais un « rien à
/// comparer » qui laisserait croire la suite verte.
#[test]
fn une_reference_absente_echoue() {
    let dir = scratch("absente");
    let error = check(Scene::Edge, &dir).expect_err("sans référence");
    assert!(error.contains("référence absente"), "{error}");
}

/// Ce que `--update` écrit, `--check` le reconnaît.
#[test]
fn check_accepte_ce_qu_update_ecrit() {
    let dir = scratch("aller-retour");
    update(Scene::Edge, &dir).expect("écriture");
    assert!(check(Scene::Edge, &dir).is_ok());
}

/// Une référence qui diffère d'un seul chiffre est une divergence, et le
/// message donne les deux empreintes.
#[test]
fn une_reference_differente_diverge() {
    let dir = scratch("divergente");
    let rendered = Scene::Edge.render_all().expect("scène valide");
    fs::write(dir.join("arete"), reference_text(rendered ^ 1)).expect("écriture");
    let error = check(Scene::Edge, &dir).expect_err("divergence");
    assert!(error.contains(&hash::format(rendered)), "{error}");
}
