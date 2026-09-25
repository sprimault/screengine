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

/// L'empreinte d'une scène par la passe des hôtes, toutes vues enchaînées.
fn host_hash(scene: Scene) -> u64 {
    let hashes: Vec<u64> = scene
        .views()
        .into_iter()
        .map(|view| {
            scene
                .render_view(Scene::HOST_PASS, view)
                .expect("scène valide")
        })
        .collect();
    hash::chain(&hashes)
}

/// La scène se rend, et deux rendus donnent la même empreinte : sans quoi il
/// n'y aurait rien à comparer entre le chemin Rust et les hôtes.
#[test]
fn la_scene_des_hotes_rend_une_empreinte_stable() {
    assert_eq!(host_hash(Scene::Edge), host_hash(Scene::Edge));
}

/// Toutes les passes — tailles de tuile, image entière, ordre mélangé,
/// threads — rendent l'empreinte de `--print`, celle que les hôtes
/// comparent : une référence écrite depuis une autre passe ne vaudrait que
/// pour elle.
#[test]
fn toutes_les_passes_rendent_l_empreinte_des_hotes() {
    for scene in Scene::ALL {
        assert_eq!(scene.render_all(), Ok(host_hash(scene)), "{}", scene.name());
    }
}

/// Une scène à vue unique a pour empreinte celle de son image, sans enveloppe.
///
/// C'est ce qui permet à un hôte de comparer l'empreinte qu'il calcule sur ses
/// propres pixels au fichier de référence versionné. L'enveloppe n'apparaît
/// qu'à partir de deux vues.
#[test]
fn une_scene_a_vue_unique_garde_l_empreinte_de_son_image() {
    let view = Scene::Edge.views()[0];
    let image = Scene::Edge
        .render_view(Scene::HOST_PASS, view)
        .expect("scène valide");
    assert_eq!(Scene::Edge.render_all(), Ok(image));
    assert_eq!(Scene::Edge.views().len(), 1);
}

/// Chaque scène couvre une part franche de l'image.
///
/// Le contrôle qui manquerait le plus : une scène soumise dans le mauvais sens
/// est éliminée comme dos de face, rend le fond seul, et son empreinte reste
/// parfaitement stable d'une plateforme à l'autre. La suite entière resterait
/// verte en ne comparant rien.
///
/// **Un pixel peint est un pixel qui diffère du fond de sa propre scène**, et
/// non un pixel non noir. Ce dernier critère, celui d'avant, ne mordait que sur
/// les scènes dont le fond est effectivement noir : `brouillard` peint le sien
/// de la couleur du brouillard, `gamma` fait traverser le noir par la courbe de
/// sortie, qui le porte aux environs de (59, 0, 81). Les deux rendaient donc
/// « toute l'image couverte », quoi qu'elles soumettent.
///
/// Le fond se rend par la scène elle-même, sans soumission — voir
/// `Scene::render_background`.
///
/// Un dixième de l'image : assez haut pour qu'un fond seul ou quelques pixels
/// égarés échouent, assez bas pour n'imposer aucun cadrage aux scènes à venir.
#[test]
fn chaque_scene_couvre_une_part_de_l_image() {
    for scene in Scene::ALL {
        for view in scene.views() {
            let pixels = scene
                .render_pixels(Scene::HOST_PASS, view)
                .expect("scène valide");
            let background = scene
                .render_background(Scene::HOST_PASS, view)
                .expect("scène valide");
            let total = view.width as usize * view.height as usize;
            let drawn = pixels
                .chunks_exact(BYTES_PER_PIXEL)
                .zip(background.chunks_exact(BYTES_PER_PIXEL))
                .filter(|(pixel, empty)| pixel != empty)
                .count();
            assert!(
                drawn * 10 > total,
                "{} ({}) : {drawn} pixels dessinés sur {total}",
                scene.name(),
                view.label()
            );
        }
    }
}

/// Deux scènes ont un fond qui n'est pas noir, et c'est ce qui rendait le
/// critère d'avant inerte.
///
/// La preuve que le changement de critère servait à quelque chose. La couverture
/// comptait les pixels non noirs : sur ces deux scènes, le fond lui-même en est
/// un, donc `drawn` valait le nombre total de pixels quoi qu'elles soumettent,
/// et une scène rendue à l'envers y aurait été déclarée couverte.
///
/// Les deux sont nommées, plutôt que comptées : ce sont celles de l'audit, et un
/// réglage retiré de l'une des deux doit faire tomber ce test — c'est alors le
/// cas de figure qui disparaît, pas le contrôle qui se périme.
#[test]
fn le_fond_de_gamma_et_de_brouillard_n_est_pas_noir() {
    for name in ["gamma", "brouillard"] {
        let scene = Scene::ALL
            .into_iter()
            .find(|s| s.name() == name)
            .expect("scène de l'audit");
        let view = scene.views()[0];
        let background = scene
            .render_background(Scene::HOST_PASS, view)
            .expect("scène valide");
        assert_ne!(
            background[..3],
            [0, 0, 0],
            "{name} : le fond est noir, le critère d'avant n'était donc pas inerte sur elle"
        );
    }
}

/// Le fond d'une scène est une image d'une seule couleur.
///
/// La contrepartie du test précédent, et ce qui le garde honnête. Sans
/// géométrie, tous les pixels suivent le même chemin : couleur de fond, puis
/// brouillard à profondeur infinie, puis la courbe de sortie — donc une image
/// uniforme. Si `render_background` soumettait quoi que ce soit, la comparaison
/// du test précédent porterait sur deux images voisines et la couverture
/// tomberait sous le seuil sans qu'on sache pourquoi ; ici, le défaut se nomme.
#[test]
fn le_fond_d_une_scene_est_uniforme() {
    for scene in Scene::ALL {
        for view in scene.views() {
            let background = scene
                .render_background(Scene::HOST_PASS, view)
                .expect("scène valide");
            let first = &background[..BYTES_PER_PIXEL];
            let strays = background
                .chunks_exact(BYTES_PER_PIXEL)
                .filter(|pixel| *pixel != first)
                .count();
            assert_eq!(
                strays,
                0,
                "{} ({}) : {strays} pixels s'écartent du fond, qui devrait être uni — \
                 render_background a soumis quelque chose",
                scene.name(),
                view.label()
            );
        }
    }
}

/// Aucune ligne d'image ne montre de trou à l'intérieur de la scène.
///
/// Ce qu'une empreinte ne peut pas dire : elle change aussi bien pour un trou
/// que pour une teinte, et une référence fausse figerait le trou. Ici, la
/// propriété se vérifie sans référence : le quadrilatère projeté reste convexe,
/// quelle que soit la découpe, donc son intersection avec une ligne de pixels
/// est un segment. Un pixel de fond entre le premier et le dernier pixel peint
/// d'une ligne est un trou, et rien d'autre ne peut le produire.
///
/// **Ce qu'il attrape, et ce qu'il n'attrape pas.** Il voit une couture de
/// tuile, un triangle rétréci d'un pixel, une découpe qui mord — vu rougir sur
/// un parcours de tuile raccourci d'une colonne. Il ne voit **pas** la règle
/// top-left : le biais ne départage que les pixels dont le centre tombe
/// exactement sur l'arête, ce qui n'arrive pour ainsi dire jamais sur des
/// coordonnées issues de la projection. Cette règle-là se prouve dans les tests
/// du noyau, qui placent les arêtes sur la ligne des centres exprès.
///
/// Sur toutes les vues et toutes les passes : seize orientations d'arête, trois
/// résolutions internes, cinq découpages.
#[test]
fn aucune_couture_dans_la_scene_en_rotation() {
    for view in Scene::Rotation.views() {
        for pass in Pass::ALL {
            let pixels = Scene::Rotation
                .render_pixels(pass, view)
                .expect("scène valide");
            for y in 0..view.height as usize {
                let row = &pixels[y * view.width as usize * BYTES_PER_PIXEL..]
                    [..view.width as usize * BYTES_PER_PIXEL];
                let painted: Vec<usize> = row
                    .chunks_exact(BYTES_PER_PIXEL)
                    .enumerate()
                    .filter(|(_, pixel)| pixel[..3] != [0, 0, 0])
                    .map(|(x, _)| x)
                    .collect();
                let (Some(&first), Some(&last)) = (painted.first(), painted.last()) else {
                    continue;
                };
                assert_eq!(
                    painted.len(),
                    last - first + 1,
                    "{} ({}, {}) : trou dans la ligne {y}",
                    Scene::Rotation.name(),
                    view.label(),
                    pass.name()
                );
            }
        }
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

/// Le sol texturé est plus lisse au loin qu'au près.
///
/// **Le critère de l'étape, sous la seule forme qu'une suite sans mouvement
/// puisse vérifier.** Un sol correctement filtré se fond avec la distance : ses
/// texels s'y moyennent, et la variation d'un pixel au suivant s'effondre. Un
/// sol échantillonné au niveau zéro fait l'inverse — plus il s'éloigne, plus
/// chaque pixel saute d'un texel à l'autre, et c'est exactement le
/// scintillement qu'on voit en mouvement.
///
/// **Sans référence, et monotone** : il n'y a pas de seuil à calibrer, donc
/// rien qui vieillisse. Une empreinte, elle, ne pourrait pas le dire — elle
/// change aussi bien pour un filtrage correct que pour un rendu bruité.
#[test]
fn le_sol_texture_est_plus_lisse_au_loin_qu_au_pres() {
    for pass in Pass::ALL {
        for view in Scene::Textured.views() {
            let pixels = Scene::Textured
                .render_pixels(pass, view)
                .expect("scène valide");
            let (width, height) = (view.width as usize, view.height as usize);

            // Le **contraste maximal** entre deux pixels voisins d'une bande,
            // et non la somme des écarts : au premier plan les transitions sont
            // rares et franches, au loin nombreuses et atténuées, si bien que
            // les sommes se compensent et ne disent rien. Le maximum, lui, dit
            // ce qui compte — un sol minifié correctement n'a plus nulle part
            // deux voisins que tout oppose.
            let variation = |from: usize, to: usize| {
                let (mut worst, mut count) = (0u64, 0u64);
                for y in from..to {
                    let row = &pixels[y * width * BYTES_PER_PIXEL..][..width * BYTES_PER_PIXEL];
                    let line: Vec<&[u8]> = row.chunks_exact(BYTES_PER_PIXEL).collect();
                    for pair in line.windows(2) {
                        // Les pixels de fond ne disent rien du filtrage.
                        if pair[0][..3] == [0, 0, 0] || pair[1][..3] == [0, 0, 0] {
                            continue;
                        }
                        let gap = (0..3)
                            .map(|c| u64::from(pair[0][c].abs_diff(pair[1][c])))
                            .max()
                            .unwrap_or(0);
                        worst = worst.max(gap);
                        count += 1;
                    }
                }
                (worst, count)
            };

            // Le sol de cette scène occupe la moitié basse, son horizon tombant
            // à mi-hauteur. La bande lointaine se prend **juste sous
            // l'horizon**, là où la minification est forte ; plus bas, le
            // niveau redescend et le damier redevient franc, ce qui est le
            // comportement voulu et non un défaut.
            let (loin, loin_n) = variation(height / 2, height * 9 / 16);
            let (pres, pres_n) = variation(height * 7 / 8, height);
            assert!(
                loin_n > 1000 && pres_n > 1000,
                "{} ({}) : bandes trop maigres, {loin_n} et {pres_n} couples",
                Scene::Textured.name(),
                pass.name()
            );
            assert!(
                loin < pres,
                "{} ({}) : contraste de {loin} au loin contre {pres} au près — \
                 le filtrage ne minifie pas",
                Scene::Textured.name(),
                pass.name()
            );
        }
    }
}

/// Le bilinéaire fabrique des couleurs que le tramage ne peut pas rendre.
///
/// **C'est la différence de nature entre les deux modes**, et le seul critère
/// qui ne se calibre pas : le tramage déplace une coordonnée et lit un texel,
/// il ne peut donc rendre que des couleurs présentes dans la texture ou dans
/// ses mipmaps ; le bilinéaire en interpole entre elles. Sur un damier de deux
/// teintes, le compte des couleurs distinctes sépare les deux sans ambiguïté.
///
/// Il vaut aussi contrôle de la référence nouvelle : une empreinte dit qu'une
/// image n'a pas changé, jamais qu'elle est filtrée comme on croit.
#[test]
fn le_bilineaire_fabrique_des_couleurs_que_le_tramage_ne_peut_pas() {
    let couleurs = |scene: Scene| {
        let view = scene.views()[0];
        let pixels = scene
            .render_pixels(Scene::HOST_PASS, view)
            .expect("scène valide");
        let mut vues: Vec<[u8; 3]> = Vec::new();
        for pixel in pixels.chunks_exact(BYTES_PER_PIXEL) {
            let rgb = [pixel[0], pixel[1], pixel[2]];
            // Le fond ne dit rien du filtrage, et il est le même des deux côtés.
            if rgb != [0, 0, 0] && !vues.contains(&rgb) {
                vues.push(rgb);
            }
        }
        vues.len()
    };

    let tramage = couleurs(Scene::Textured);
    let bilineaire = couleurs(Scene::TexturedBilinear);

    assert!(
        bilineaire > tramage * 4,
        "{bilineaire} couleurs en bilinéaire contre {tramage} au tramage : \
         le filtrage n'interpole pas"
    );
}

/// Le fichier de maillage versionné est exactement celui que la conformance
/// engendre.
///
/// Sur le modèle de `make header-verif` : le binaire que les quatre hôtes
/// chargent n'est jamais la source de vérité. Sans ce contrôle, un changement de
/// format laisserait les hôtes charger un fichier périmé — et leurs empreintes
/// diverger sans qu'on sache si la faute est au décodeur, au rendu ou au
/// fichier.
///
/// `include_bytes!` plutôt qu'une lecture : le fichier absent échoue à la
/// compilation, et le test ne dépend pas du répertoire courant.
#[test]
fn le_fichier_de_maillage_versionne_est_a_jour() {
    const VERSIONED: &[u8] = include_bytes!("../../../hosts/caisse.mesh");
    let engendre = mesh_file::bytes();

    assert_eq!(
        VERSIONED.len(),
        engendre.len(),
        "hosts/caisse.mesh fait {} octets, la conformance en écrit {} — \
         relancer `make mesh`",
        VERSIONED.len(),
        engendre.len()
    );
    let ecart = VERSIONED
        .iter()
        .zip(&engendre)
        .position(|(versionne, engendre)| versionne != engendre);
    assert_eq!(
        ecart, None,
        "hosts/caisse.mesh diverge à l'octet {ecart:?} — relancer `make mesh`"
    );
}

/// Le fichier de carte versionné est exactement celui que la conformance
/// engendre.
///
/// Même règle que pour le maillage : le décor que les hôtes de démonstration
/// parcourent n'est pas la source de vérité, et un fichier périmé échoue ici
/// plutôt que de leur faire afficher autre chose que la scène de référence.
#[test]
fn le_fichier_de_carte_versionne_est_a_jour() {
    const VERSIONED: &[u8] = include_bytes!("../../../hosts/couloir.world");
    let engendre = world_file::bytes();

    assert_eq!(
        VERSIONED.len(),
        engendre.len(),
        "hosts/couloir.world fait {} octets, la conformance en écrit {} — \
         relancer `make mesh`",
        VERSIONED.len(),
        engendre.len()
    );
    let ecart = VERSIONED
        .iter()
        .zip(&engendre)
        .position(|(versionne, engendre)| versionne != engendre);
    assert_eq!(
        ecart, None,
        "hosts/couloir.world diverge à l'octet {ecart:?} — relancer `make mesh`"
    );
}

/// Le fichier de carte versionné se décode, et porte le couloir.
#[test]
fn le_fichier_de_carte_versionne_porte_le_couloir() {
    const VERSIONED: &[u8] = include_bytes!("../../../hosts/couloir.world");
    let world = World::load(VERSIONED).expect("le fichier versionné se décode");

    assert_eq!(world.material_count(), 2);
    assert_eq!(world.material_name(0), Some("mur"));
    assert_eq!(world.material_name(1), Some("sol"));
    assert_eq!(
        world.triangle_count(),
        16,
        "deux cellules de quatre surfaces, deux triangles chacune"
    );
}

/// Le fichier versionné se décode, et rend ce que la scène rend.
///
/// C'est ce qui relie le fichier à l'image : le contrôle précédent dit qu'il est
/// à jour, celui-ci dit qu'il porte bien la caisse — sans quoi un générateur
/// changé des deux côtés passerait.
#[test]
fn le_fichier_de_maillage_versionne_porte_la_caisse() {
    const VERSIONED: &[u8] = include_bytes!("../../../hosts/caisse.mesh");
    let mesh = Mesh::load(VERSIONED).expect("le fichier versionné se décode");

    assert_eq!(mesh.triangle_count(), 12, "six faces de deux triangles");
    assert_eq!(mesh.texture_count(), 2);
    assert_eq!(mesh.texture_name(0), Some("cote"));
    assert_eq!(mesh.texture_name(1), Some("chapeau"));
}
