// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le chargement d'une carte, appelé comme le ferait un hôte.
//!
//! Les octets s'écrivent à la main, sans rien partager avec les tests du
//! décodeur : ce qu'on veut savoir ici est qu'un bloc quelconque franchit la
//! frontière et revient en handle, pas que le décodeur est juste — le noyau en
//! répond.

use std::ffi::CStr;
use std::ptr;

use screengine_ffi::*;

/// Lit le message du thread courant, celui des appels sans contexte.
fn last_error() -> String {
    // SAFETY: le pointeur nul demande l'emplacement du thread, et le pointeur
    // rendu reste valide jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ptr::null())) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

/// Les octets d'une suite d'entiers.
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Les octets d'une suite de flottants.
fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Un repère unitaire : origine nulle, axes sur X et Y.
fn unit_frame() -> Vec<u8> {
    floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])
}

/// Une carte d'une cellule, un carré, deux matériaux.
fn one_cell_world() -> Vec<u8> {
    let mut surface = words(&[11, 0, 1, 4]);
    surface.extend_from_slice(&words(&[0, 1, 2, 3]));
    surface.extend_from_slice(&unit_frame());
    surface.extend_from_slice(&unit_frame());

    let mut body = words(&[7, 0, 4, 1, 0]);
    for point in [
        [0.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 4.0, 0.0],
        [0.0, 4.0, 0.0],
    ] {
        body.extend_from_slice(&floats(&point));
    }
    body.extend_from_slice(&surface);

    let mut cells = words(&[body.len() as u32]);
    cells.extend_from_slice(&body);

    let mut mats = Vec::new();
    for (id, name) in [(1u32, "mur"), (2, "plafond")] {
        mats.extend_from_slice(&words(&[id]));
        mats.extend_from_slice(&(name.len() as u16).to_le_bytes());
        mats.extend_from_slice(name.as_bytes());
    }

    file(&cells, &mats, b"WRLD", 1)
}

/// Une carte d'une cellule, une lumière et une entité.
fn peopled_world() -> Vec<u8> {
    let mut light = words(&[41]);
    light.extend_from_slice(&floats(&[4.0, 5.0, 6.0, 8.0]));
    light.extend_from_slice(&[0xF0, 0x80, 0x40, 0]);

    let mut body = words(&[31, 7]);
    body.extend_from_slice(&6u16.to_le_bytes());
    body.extend_from_slice(b"depart");
    body.extend_from_slice(&floats(&[1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 2.0]));
    body.extend_from_slice(&words(&[2]));
    body.extend_from_slice(&[0xDE, 0xAD]);
    let mut entity = words(&[body.len() as u32]);
    entity.extend_from_slice(&body);

    let cells = one_cell_world_cells();
    let mut mats = words(&[1]);
    mats.extend_from_slice(&3u16.to_le_bytes());
    mats.extend_from_slice(b"mur");

    sections(&cells, &entity, &light, &mats, b"WRLD", 1)
}

/// La section des cellules de la carte d'épreuve.
fn one_cell_world_cells() -> Vec<u8> {
    let mut surface = words(&[11, 0, 1, 4]);
    surface.extend_from_slice(&words(&[0, 1, 2, 3]));
    surface.extend_from_slice(&unit_frame());
    surface.extend_from_slice(&unit_frame());

    let mut body = words(&[7, 0, 4, 1, 0]);
    for point in [
        [0.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 4.0, 0.0],
        [0.0, 4.0, 0.0],
    ] {
        body.extend_from_slice(&floats(&point));
    }
    body.extend_from_slice(&surface);

    let mut cells = words(&[body.len() as u32]);
    cells.extend_from_slice(&body);
    cells
}

/// Un fichier de carte, ses quatre sections.
fn sections(
    cells: &[u8],
    ents: &[u8],
    lgts: &[u8],
    mats: &[u8],
    kind: &[u8; 4],
    version: u32,
) -> Vec<u8> {
    let sections: Vec<([u8; 4], &[u8])> = [
        (*b"CELL", cells),
        (*b"ENTS", ents),
        (*b"LGTS", lgts),
        (*b"MATS", mats),
    ]
    .into_iter()
    .filter(|(_, body)| !body.is_empty())
    .collect();

    build(&sections, kind, version)
}

/// Un fichier de carte : en-tête, table de sections, sections.
fn file(cells: &[u8], mats: &[u8], kind: &[u8; 4], version: u32) -> Vec<u8> {
    let sections: Vec<([u8; 4], &[u8])> = [(*b"CELL", cells), (*b"MATS", mats)]
        .into_iter()
        .filter(|(_, body)| !body.is_empty())
        .collect();

    build(&sections, kind, version)
}

/// Assemble l'en-tête, la table et les sections.
fn build(sections: &[([u8; 4], &[u8])], kind: &[u8; 4], version: u32) -> Vec<u8> {
    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(sections.len() as u32).to_le_bytes());

    let mut offset = first;
    for (tag, body) in sections {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        offset += body.len();
    }
    for (_, body) in sections {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// Une carte peuplée rend sa lumière dans la structure que l'hôte lui donne.
///
/// **La structure est celle que l'hôte repasse à `scg_set_lights`** : c'est tout
/// ce qu'il en fait à cette étape, et la lui faire reconstruire champ par champ
/// n'aurait servi qu'à respecter la lettre d'un principe qui vise la mémoire du
/// moteur, pas un tampon de l'appelant.
#[test]
fn rend_une_lumiere_dans_la_structure_de_l_hote() {
    let world = load(&peopled_world());
    let mut count = 0;
    let mut light = ScgLight {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        radius: 0.0,
        r: 0,
        g: 0,
        b: 0,
        _reserved: 0xFF,
    };

    // SAFETY: handle vivant, paramètres de sortie locaux.
    unsafe {
        assert_eq!(scg_world_light_count(world, &mut count), SCG_OK);
        assert_eq!(scg_world_light(world, 0, &mut light), SCG_OK);
    }
    assert_eq!(count, 1);
    assert_eq!((light.x, light.y, light.z), (4.0, 5.0, 6.0));
    assert_eq!(light.radius, 8.0);
    assert_eq!((light.r, light.g, light.b), (0xF0, 0x80, 0x40));
    assert_eq!(
        light._reserved, 0,
        "l'octet réservé est écrit nul, ce que le contrat exige de l'hôte"
    );

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Une carte peuplée rend son entité : identifiants, pose, classe, octets.
#[test]
fn rend_une_entite_par_ses_accesseurs() {
    let world = load(&peopled_world());
    let mut count = 0;
    let mut id = 0;
    let mut cell = 0;
    let mut pose = [0.0f32; 7];
    let mut len = 0;

    // SAFETY: handle vivant, paramètres de sortie locaux, `pose` couvrant sept
    // flottants.
    unsafe {
        assert_eq!(scg_world_entity_count(world, &mut count), SCG_OK);
        assert_eq!(scg_world_entity_ids(world, 0, &mut id, &mut cell), SCG_OK);
        assert_eq!(scg_world_entity_pose(world, 0, pose.as_mut_ptr()), SCG_OK);
    }
    assert_eq!(count, 1);
    assert_eq!((id, cell), (31, 7));
    assert_eq!(&pose[..3], &[1.0, 2.0, 3.0]);
    assert_eq!(
        &pose[3..],
        &[0.0, 0.0, 0.0, 1.0],
        "le quaternion est normalisé au chargement"
    );

    // La classe, en deux temps.
    // SAFETY: handle vivant ; le tampon nul avec une capacité nulle mesure.
    let code = unsafe { scg_world_entity_class(world, 0, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(len, 6);
    let mut class = vec![0u8; len + 1];
    // SAFETY: handle vivant, tampon couvrant ce que la mesure demande.
    let code = unsafe {
        scg_world_entity_class(world, 0, class.as_mut_ptr().cast(), class.len(), &mut len)
    };
    assert_eq!(code, SCG_OK);
    assert_eq!(&class[..len], b"depart");

    // Les octets opaques, en deux temps aussi, mais sans terminateur.
    // SAFETY: mêmes préconditions.
    let code = unsafe { scg_world_entity_data(world, 0, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(len, 2);
    let mut data = vec![0u8; len];
    // SAFETY: handle vivant, tampon de la longueur mesurée.
    let code = unsafe { scg_world_entity_data(world, 0, data.as_mut_ptr(), data.len(), &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(data, vec![0xDE, 0xAD]);

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un index au-delà de ce que la carte porte est une faute d'appel.
#[test]
fn refuse_un_index_au_dela_de_la_carte() {
    let world = load(&peopled_world());
    let mut light = ScgLight {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        radius: 0.0,
        r: 0,
        g: 0,
        b: 0,
        _reserved: 0,
    };
    let mut id = 0;
    let mut cell = 0;
    let mut pose = [0.0f32; 7];
    let mut len = usize::MAX;

    // SAFETY: handle vivant, paramètres de sortie locaux.
    unsafe {
        assert_eq!(
            scg_world_light(world, 1, &mut light),
            SCG_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            scg_world_entity_ids(world, 1, &mut id, &mut cell),
            SCG_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            scg_world_entity_pose(world, 1, pose.as_mut_ptr()),
            SCG_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            scg_world_entity_class(world, 1, ptr::null_mut(), 0, &mut len),
            SCG_ERR_INVALID_ARGUMENT
        );
        assert_eq!(
            scg_world_entity_data(world, 1, ptr::null_mut(), 0, &mut len),
            SCG_ERR_INVALID_ARGUMENT
        );
        scg_world_destroy(world);
    }
    assert_eq!(len, usize::MAX, "rien n'est écrit sur un index refusé");
}

/// Un tampon d'octets plus court que les données est refusé sans rien écrire.
#[test]
fn refuse_un_tampon_de_donnees_trop_court() {
    let world = load(&peopled_world());
    let mut buf = [0xaau8; 4];
    let mut len = usize::MAX;
    // SAFETY: handle vivant, tampon local couvrant la capacité annoncée.
    let code = unsafe { scg_world_entity_data(world, 0, buf.as_mut_ptr(), 1, &mut len) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(len, usize::MAX);
    assert_eq!(buf, [0xaa; 4]);
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Les pointeurs nuls des accesseurs de lumières et d'entités sont refusés.
#[test]
fn refuse_les_pointeurs_nuls_des_accesseurs_peuples() {
    let world = load(&peopled_world());
    let mut id = 0;
    let mut len = 0;
    // SAFETY: handle vivant ; chaque pointeur nul est refusé avant lecture.
    unsafe {
        assert_eq!(scg_world_light(world, 0, ptr::null_mut()), SCG_ERR_NULL);
        assert_eq!(
            scg_world_entity_ids(world, 0, ptr::null_mut(), &mut id),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_entity_ids(world, 0, &mut id, ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_entity_pose(world, 0, ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_entity_class(world, 0, ptr::null_mut(), 0, ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_entity_data(world, 0, ptr::null_mut(), 8, &mut len),
            SCG_ERR_NULL
        );
        assert_eq!(scg_world_entity_count(ptr::null(), &mut id), SCG_ERR_NULL);
        assert_eq!(scg_world_light_count(ptr::null(), &mut id), SCG_ERR_NULL);
        scg_world_destroy(world);
    }
}

/// Charge une carte, ou échoue en disant pourquoi.
fn load(bytes: &[u8]) -> *mut ScgWorld {
    let mut out = ptr::null_mut();
    // SAFETY: le bloc et le paramètre de sortie sont des valeurs locales
    // vivantes, et la longueur est celle de la tranche.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_OK, "chargement refusé : {}", last_error());
    assert!(!out.is_null());
    out
}

/// Une carte se charge et se détruit sans contexte, comme un maillage.
///
/// C'est ce qui permettra à la collision de charger une carte sur un serveur
/// sans jamais allouer de tampon d'image.
#[test]
fn charge_et_detruit_une_carte_sans_contexte() {
    let world = load(&one_cell_world());
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Détruire un pointeur nul ne fait rien, comme `free`.
#[test]
fn detruire_une_carte_nulle_ne_fait_rien() {
    // SAFETY: le pointeur nul est admis, et la fonction ne fait rien.
    unsafe { scg_world_destroy(ptr::null_mut()) };
}

/// Les deux comptes viennent du fichier, et l'hôte s'en sert avant de créer son
/// contexte.
#[test]
fn rend_les_comptes_de_la_carte() {
    let world = load(&one_cell_world());
    let mut triangles = 0;
    let mut materials = 0;
    // SAFETY: handle vivant, paramètres de sortie locaux.
    unsafe {
        assert_eq!(scg_world_triangle_count(world, &mut triangles), SCG_OK);
        assert_eq!(scg_world_material_count(world, &mut materials), SCG_OK);
        scg_world_destroy(world);
    }
    assert_eq!(triangles, 2, "le carré donne deux triangles");
    assert_eq!(materials, 2);
}

/// Un nom de matériau se lit en deux temps, comme un nom d'emplacement.
#[test]
fn lit_un_nom_de_materiau_en_deux_temps() {
    let world = load(&one_cell_world());
    let mut len = usize::MAX;
    // SAFETY: handle vivant ; le tampon nul avec une capacité nulle est le
    // premier temps documenté.
    let code = unsafe { scg_world_material_name(world, 1, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(len, 7, "« plafond »");

    let mut buf = vec![0u8; len + 1];
    // SAFETY: handle vivant, tampon couvrant la capacité que la mesure demande.
    let code =
        unsafe { scg_world_material_name(world, 1, buf.as_mut_ptr().cast(), buf.len(), &mut len) };
    assert_eq!(code, SCG_OK);
    assert_eq!(&buf[..len], b"plafond");
    assert_eq!(buf[len], 0, "le tampon rendu est terminé par un octet nul");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un tampon trop court est refusé sans rien écrire, `out_len` compris.
///
/// La même clause que pour un nom d'emplacement, et c'est le même code des deux
/// côtés : un protocole recopié qui divergerait d'un mot ferait mentir le header
/// pour l'une des deux ressources.
#[test]
fn refuse_un_tampon_de_nom_trop_court() {
    let world = load(&one_cell_world());
    let mut buf = [0xaau8; 16];
    let mut len = usize::MAX;
    // SAFETY: handle vivant, tampon local couvrant la capacité annoncée.
    let code = unsafe { scg_world_material_name(world, 1, buf.as_mut_ptr().cast(), 4, &mut len) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(len, usize::MAX, "rien n'est écrit dans out_len");
    assert_eq!(buf, [0xaa; 16], "rien n'est écrit dans le tampon");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un indice de matériau au-delà du dernier est une faute d'appel.
#[test]
fn refuse_un_materiau_au_dela_du_dernier() {
    let world = load(&one_cell_world());
    let mut len = usize::MAX;
    // SAFETY: handle vivant, paramètre de sortie local.
    let code = unsafe { scg_world_material_name(world, 2, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(len, usize::MAX);
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Un maillage là où une carte est attendue est refusé par le genre.
#[test]
fn refuse_un_maillage_au_chargement_d_une_carte() {
    let bytes = file(&[], &[], b"MESH", 1);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_INVALID_FORMAT);
    assert!(out.is_null());
    assert!(last_error().contains("kind"));
}

/// Une version de format inconnue a son propre code.
#[test]
fn refuse_une_version_de_carte_inconnue() {
    let bytes = file(&[], &[], b"WRLD", 3);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_UNSUPPORTED_FORMAT_VERSION);
    assert!(out.is_null());
}

/// Les pointeurs nuls du chargement et des accesseurs sont refusés.
#[test]
fn refuse_les_pointeurs_nuls() {
    let bytes = one_cell_world();
    let world = load(&bytes);
    let mut count = 0;
    let mut len = 0;
    // SAFETY: handle vivant ; chaque pointeur nul est refusé avant lecture.
    unsafe {
        assert_eq!(
            scg_world_load(bytes.as_ptr(), bytes.len(), ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_load(ptr::null(), 20, &mut ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_triangle_count(ptr::null(), &mut count),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_material_count(world, ptr::null_mut()),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_material_name(ptr::null(), 0, ptr::null_mut(), 0, &mut len),
            SCG_ERR_NULL
        );
        assert_eq!(
            scg_world_material_name(world, 0, ptr::null_mut(), 0, ptr::null_mut()),
            SCG_ERR_NULL
        );
        scg_world_destroy(world);
    }
}

/// Une carte malformée est refusée par le code des données, et le message dit
/// quoi.
#[test]
fn refuse_une_carte_malformee() {
    // Une cellule dont l'identifiant est nul : l'éditeur réserve zéro à
    // « aucun ».
    let mut body = words(&[0, 0, 0, 0, 0]);
    body.truncate(20);
    let mut cells = words(&[body.len() as u32]);
    cells.extend_from_slice(&body);

    let bytes = file(&cells, &[], b"WRLD", 1);
    let mut out = ptr::null_mut();
    // SAFETY: bloc local vivant, longueur celle de la tranche.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_ERR_INVALID_FORMAT);
    assert!(out.is_null());
    assert!(last_error().contains("identifier"));
}

/// La même carte se lit depuis plusieurs threads, étant immuable et sans
/// contexte.
#[test]
fn une_carte_se_lit_depuis_plusieurs_threads() {
    let world = load(&one_cell_world());
    let address = world as usize;

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(move || {
                let handle = address as *const ScgWorld;
                let mut count = 0;
                // SAFETY: le handle reste vivant pendant toute la portée, et la
                // ressource est immuable : les accesseurs ne font que lire.
                let code = unsafe { scg_world_triangle_count(handle, &mut count) };
                assert_eq!(code, SCG_OK);
                assert_eq!(count, 2);
            });
        }
    });

    // SAFETY: handle vivant, détruit une seule fois, après la portée.
    unsafe { scg_world_destroy(world) };
}
