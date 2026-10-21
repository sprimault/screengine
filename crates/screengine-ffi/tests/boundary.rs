// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les points d'entrée appelés comme le ferait un hôte.
//!
//! Pointeurs nuls, séquences invalides, panique rattrapée : ce que les tests
//! unitaires ne voient pas, parce qu'ils n'ont pas de pointeur à passer. Un
//! test qui provoque une panique laisse le message par défaut sur la sortie
//! d'erreur — c'est attendu, et c'est justement ce qui est rattrapé.

use std::ffi::CStr;
use std::ptr;

use screengine_ffi::*;

/// Une configuration qui passe, dont les tests dérivent leurs variantes.
fn sane() -> ScgContextConfig {
    ScgContextConfig {
        max_width: 64,
        max_height: 32,
        width: 64,
        height: 32,
        tile_size: 32,
        max_triangles: 0,
        max_lines: 0,
        reserved2: 0,
    }
}

/// Crée un contexte dont la configuration est saine.
fn create(config: &ScgContextConfig) -> *mut ScgContext {
    let mut ctx = ptr::null_mut();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    let code = unsafe { scg_create(config, &mut ctx) };
    assert_eq!(code, SCG_OK);
    assert!(!ctx.is_null());
    ctx
}

/// Lit le message d'un contexte, ou celui du thread si `ctx` est nul.
fn last_error(ctx: *const ScgContext) -> String {
    // SAFETY: `ctx` est nul ou vivant, et le pointeur rendu reste valide
    // jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ctx)) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

/// La constante et la fonction doivent dire la même chose : une liaison compare
/// celle du header à celle de la bibliothèque chargée, et un écart entre les
/// deux ici ferait refuser toute bibliothèque, y compris la bonne.
#[test]
fn la_version_d_abi_est_celle_du_crate() {
    assert_eq!(scg_abi_version(), SCG_ABI_VERSION);
}

/// Le cycle de vie complet, tel qu'un hôte l'écrit. Sous un détecteur de
/// fuites, c'est aussi ce qui montre que le handle rend bien son allocation.
#[test]
fn cree_et_detruit_un_contexte() {
    let ctx = create(&sane());
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Comme `free`, et le header le promet : une liaison qui libère en cascade
/// après un échec de création compte dessus.
#[test]
fn detruire_un_pointeur_nul_ne_fait_rien() {
    // SAFETY: `scg_destroy` accepte explicitement le pointeur nul.
    unsafe { scg_destroy(ptr::null_mut()) };
}

/// Le pointeur nul se refuse au lieu d'être déréférencé, et le paramètre de
/// sortie reste intact — une liaison qui teste `out` après l'échec ne doit pas
/// tomber sur un handle à moitié écrit.
#[test]
fn refuse_une_configuration_nulle() {
    let mut ctx = ptr::null_mut();
    // SAFETY: le premier pointeur est nul, ce que la fonction doit refuser
    // plutôt que déréférencer.
    let code = unsafe { scg_create(ptr::null(), &mut ctx) };
    assert_eq!(code, SCG_ERR_NULL);
    assert!(ctx.is_null());
    assert_eq!(last_error(ptr::null()), "null pointer argument");
}

/// L'autre pointeur, testé à part : une vérification qui n'en couvrirait qu'un
/// des deux laisserait l'autre écrire à l'adresse nulle.
#[test]
fn refuse_un_parametre_de_sortie_nul() {
    let config = sane();
    // SAFETY: le second pointeur est nul, ce que la fonction doit refuser.
    let code = unsafe { scg_create(&config, ptr::null_mut()) };
    assert_eq!(code, SCG_ERR_NULL);
}

/// C'est l'appel où un intégrateur se trompe de configuration, et il n'a pas de
/// contexte auquel rattacher la cause : sans l'emplacement par thread, elle
/// serait perdue.
#[test]
fn refuse_une_configuration_invalide_et_dit_pourquoi() {
    let mut config = sane();
    config.tile_size = 48;

    let mut ctx = ptr::null_mut();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    let code = unsafe { scg_create(&config, &mut ctx) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert!(ctx.is_null());
    assert_eq!(
        last_error(ptr::null()),
        "invalid tile size: must be 32 or 64"
    );
}

/// Le même refus que côté unitaire, mais vu depuis l'ABI : c'est là qu'il a son
/// sens, puisqu'il ne sert qu'aux liaisons qui écrivent la structure elles-mêmes.
#[test]
fn refuse_un_champ_reserve_non_nul() {
    // Il n'en reste qu'un : `reserved1` est devenu `max_lines` à l'étape 8,
    // comme le premier était devenu `max_triangles` à l'étape 1. Le refus reste
    // écrit pour le dernier, et c'est lui qui permettra de l'employer à son tour
    // sans casser une liaison déjà écrite. La boucle qui parcourait les champs a
    // disparu avec l'avant-dernier : elle n'aurait plus qu'un tour.
    let mut config = sane();
    config.reserved2 = 1;

    let mut ctx = ptr::null_mut();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    let code = unsafe { scg_create(&config, &mut ctx) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(last_error(ptr::null()), "reserved fields must be zero");
}

/// Un handle nul sur une fonction qui prend aussi un tampon : le contexte se
/// vérifie avant tout le reste, sinon l'enveloppe déréférencerait pour rien.
#[test]
fn refuse_une_fin_d_image_sans_contexte() {
    let mut pixels = [0u8; 64 * 32 * 4];
    // SAFETY: le contexte est nul, ce que la fonction doit refuser.
    let code = unsafe { scg_frame_end(ptr::null_mut(), pixels.as_mut_ptr(), 64) };
    assert_eq!(code, SCG_ERR_NULL);
}

/// Le tampon nul se refuse avant la construction de la tranche : après, c'est
/// déjà un comportement indéfini. Le message va dans le contexte, pas dans
/// l'emplacement par thread, parce qu'il y a un contexte auquel le rattacher.
#[test]
fn refuse_un_tampon_nul() {
    let ctx = create(&sane());
    // SAFETY: handle vivant ; le tampon est nul, ce que la fonction refuse.
    let code = unsafe { scg_frame_end(ctx, ptr::null_mut(), 64) };
    assert_eq!(code, SCG_ERR_NULL);
    assert_eq!(last_error(ctx), "null pointer argument");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Le contrôle traverse bien la frontière : le noyau le fait, et l'hôte reçoit
/// le code plutôt qu'une image tronquée sans avertissement.
#[test]
fn refuse_un_stride_plus_court_que_la_largeur() {
    let ctx = create(&sane());
    let mut pixels = [0u8; 64 * 32 * 4];
    // SAFETY: handle vivant, et le tampon couvre `stride × hauteur` pixels
    // puisque le stride est plus petit que la largeur.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 63) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(
        last_error(ctx),
        "invalid stride: must be at least the internal width"
    );
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Soumet le quadrilatère de la scène de référence, par les structures de
/// l'ABI, et rend le code de retour.
fn submit_scene(ctx: *mut ScgContext) -> i32 {
    let model = ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };
    let vertices = [
        ScgVertex {
            x: 2.0,
            y: 2.5,
            z: 1.6,
        },
        ScgVertex {
            x: 3.5,
            y: -2.5,
            z: 1.6,
        },
        ScgVertex {
            x: 3.5,
            y: -2.5,
            z: -1.6,
        },
        ScgVertex {
            x: 2.0,
            y: 2.5,
            z: -1.6,
        },
    ];
    let triangles = [
        ScgTriangle {
            i0: 0,
            i1: 2,
            i2: 1,
            r: 0xE0,
            g: 0xA0,
            b: 0x30,
            a: 0xFF,
        },
        ScgTriangle {
            i0: 0,
            i1: 3,
            i2: 2,
            r: 0xA0,
            g: 0xE0,
            b: 0x30,
            a: 0xFF,
        },
    ];

    // SAFETY: handle vivant, et chaque pointeur couvre le nombre d'éléments
    // annoncé — ce sont des tableaux locaux qui vivent jusqu'au retour.
    unsafe {
        scg_submit(
            ctx,
            &model,
            vertices.as_ptr(),
            vertices.len() as u32,
            triangles.as_ptr(),
            triangles.len() as u32,
        )
    }
}

/// La séquence complète telle qu'un hôte l'écrira, et le seul test qui regarde
/// ce qui sort du tampon. Deux clauses de l'ABI s'y vérifient : quelque chose
/// est effectivement peint, et l'alpha est **écrit** partout — un octet laissé
/// indéfini donnerait un rendu troué dans un navigateur, seule cible où ce canal
/// est réellement composité.
#[test]
fn rend_la_scene_dans_le_tampon_de_l_hote() {
    let ctx = create(&sane());
    let mut pixels = [0u8; 64 * 32 * 4];

    assert_eq!(submit_scene(ctx), SCG_OK);
    // SAFETY: handle vivant, tampon d'au moins `stride × hauteur` pixels.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(code, SCG_OK);
    assert_eq!(last_error(ctx), "");

    let opaque_black = [0, 0, 0, 255];
    assert!(
        pixels.chunks_exact(4).any(|p| p != opaque_black),
        "le triangle doit peindre autre chose que le fond"
    );
    assert!(
        pixels.chunks_exact(4).all(|p| p[3] == 255),
        "l'alpha est écrit sur chaque pixel, fond compris"
    );

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Lit MXCSR sur le thread courant.
#[cfg(target_arch = "x86_64")]
fn mxcsr() -> u32 {
    let mut control = 0u32;
    // SAFETY: `stmxcsr` écrit quatre octets dans un `u32` vivant et aligné.
    unsafe { std::arch::asm!("stmxcsr [{}]", in(reg) &mut control, options(nostack)) };
    control
}

/// Écrit MXCSR sur le thread courant.
#[cfg(target_arch = "x86_64")]
fn set_mxcsr(control: u32) {
    // SAFETY: `ldmxcsr` relit un `u32` vivant et aligné. Les valeurs écrites par
    // ce fichier ne posent que des bits définis du registre.
    unsafe { std::arch::asm!("ldmxcsr [{}]", in(reg) &control, options(nostack)) };
}

/// Un hôte qui a démasqué toutes les exceptions flottantes — inexactitude
/// comprise — et changé l'arrondi et DAZ/FZ : la fin d'image aboutit quand
/// même, et l'hôte retrouve exactement son registre. Sans masquage à
/// l'entrée, ce test ne rougit pas : il fait tomber le processus de test sur
/// la première multiplication inexacte du moteur.
#[test]
#[cfg(target_arch = "x86_64")]
fn un_hote_aux_exceptions_demasquees_ne_tombe_pas() {
    let ctx = create(&sane());
    let mut pixels = [0u8; 64 * 32 * 4];

    // Une scène, et pas une image vide : sans triangle, le pipeline ne fait
    // aucune multiplication flottante, et le registre hostile n'a rien à
    // piéger. Le test passerait alors quelle que soit l'enveloppe.
    assert_eq!(submit_scene(ctx), SCG_OK);

    let default = mxcsr();
    // Masques effacés (12:7), arrondi vers le haut (14:13 = 10), DAZ et FZ.
    let hostile = (default & !0x7F80) | 0x4000 | 0x8040;
    set_mxcsr(hostile);
    // SAFETY: handle vivant, tampon d'au moins `stride × hauteur` pixels.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    let after = mxcsr();
    set_mxcsr(default);

    assert_eq!(code, SCG_OK);
    assert_eq!(
        after, hostile,
        "le registre de l'hôte est rendu à l'identique"
    );

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Jamais un pointeur nul : le header le promet, et une liaison qui
/// déréférencerait sans vérifier tomberait dessus au premier appel réussi.
#[test]
fn sans_erreur_le_message_est_la_chaine_vide() {
    let ctx = create(&sane());
    assert_eq!(last_error(ctx), "");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// L'alignement est une constante de l'ABI, pas une propriété de l'allocateur
/// du jour : une liaison wasm construit une vue typée dessus, et un chemin SSE
/// le supposera à l'étape 9. L'écriture complète du tampon vérifie au passage
/// que la longueur demandée est bien celle qu'on obtient.
#[test]
fn alloue_et_libere_un_tampon_aligne() {
    let len = 64 * 32 * 4;
    let ptr = scg_buffer_alloc(len);
    assert!(!ptr.is_null());
    assert_eq!(
        ptr as usize % SCG_BUFFER_ALIGNMENT,
        0,
        "l'alignement est une constante de l'ABI"
    );

    // SAFETY: le tampon vient d'être alloué, sa longueur est celle-là même.
    unsafe { ptr::write_bytes(ptr, 0, len) };
    // SAFETY: même pointeur, même longueur, libéré une seule fois.
    unsafe { scg_buffer_free(ptr, len) };
}

/// Une allocation de taille nulle est indéfinie côté Rust : elle se refuse ici
/// plutôt que de rendre un pointeur que personne ne saurait libérer.
#[test]
fn allouer_zero_octet_rend_un_pointeur_nul() {
    assert!(scg_buffer_alloc(0).is_null());
}

/// Le pendant du cas précédent : une liaison qui libère sans condition après un
/// échec d'allocation ne doit pas avoir à tester elle-même.
#[test]
fn liberer_un_pointeur_nul_ne_fait_rien() {
    // SAFETY: `scg_buffer_free` accepte explicitement le pointeur nul.
    unsafe { scg_buffer_free(ptr::null_mut(), 0) };
}

/// L'emplacement sans contexte est vidé à chaque entrée : un appel qui aboutit
/// ne laisse pas derrière lui le message du précédent.
#[test]
fn un_appel_reussi_efface_le_message_sans_contexte() {
    let mut ctx = ptr::null_mut();
    // SAFETY: le premier pointeur est nul, ce que la fonction refuse.
    unsafe { scg_create(ptr::null(), &mut ctx) };
    assert_ne!(last_error(ptr::null()), "");

    let ctx = create(&sane());
    assert_eq!(last_error(ptr::null()), "");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Largeur de l'image des tests de tuiles : 7 colonnes de 32, la dernière
/// partielle.
const TILES_W: u32 = 203;

/// Hauteur de l'image des tests de tuiles : 5 lignes de 32, la dernière
/// partielle.
const TILES_H: u32 = 150;

/// Un contexte pour les tests de tuiles.
fn tiled() -> *mut ScgContext {
    create(&ScgContextConfig {
        max_width: TILES_W,
        max_height: TILES_H,
        width: TILES_W,
        height: TILES_H,
        ..sane()
    })
}

/// Un tampon d'hôte à la taille de l'image des tests de tuiles.
fn tiles_buffer() -> Vec<u8> {
    vec![0; TILES_W as usize * TILES_H as usize * 4]
}

/// La région rend ce que les tuiles rendent, et c'est l'oracle que l'ABI
/// n'avait pas.
///
/// **L'égalité est tout l'objet de la fonction** : la région dessine toutes les
/// primitives dans l'ordre, sans répartition, donc une tuile qui en diffère a
/// perdu quelque chose à la répartition ou dépend du découpage. Deux tampons
/// distincts, parce que la fin réécrirait le premier.
///
/// Le triangle est pris en travers de plusieurs tuiles — c'est la seule
/// géométrie qui fasse travailler la répartition, et un triangle tenant dans
/// une seule tuile rendrait l'égalité vraie sans rien éprouver.
#[test]
fn la_region_rend_ce_que_les_tuiles_rendent() {
    let ctx = tiled();
    let mut par_region = tiles_buffer();
    let mut par_tuiles = tiles_buffer();
    let cells = TILES_W as usize * TILES_H as usize;
    let mut color = vec![0u32; cells];
    let mut depth = vec![0u32; cells];
    let mut count = 0;

    let vertices = [
        ScgVertex {
            x: 4.0,
            y: -1.5,
            z: -1.0,
        },
        ScgVertex {
            x: 4.0,
            y: 1.5,
            z: -1.0,
        },
        ScgVertex {
            x: 4.0,
            y: 0.0,
            z: 1.2,
        },
    ];
    let triangles = [ScgTriangle {
        i0: 0,
        i1: 1,
        i2: 2,
        r: 0x20,
        g: 0x90,
        b: 0xd0,
        a: 255,
    }];
    let model = ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    // SAFETY: handle vivant pour tout ce test, tableaux et tampons locaux, et
    // les deux tampons de travail couvrent l'image entière.
    unsafe {
        assert_eq!(
            scg_submit(ctx, &model, vertices.as_ptr(), 3, triangles.as_ptr(), 1),
            SCG_OK
        );
        assert_eq!(scg_frame_begin(ctx, &mut count), SCG_OK);
        assert_eq!(
            scg_frame_region(
                ctx,
                0,
                0,
                TILES_W,
                TILES_H,
                color.as_mut_ptr(),
                depth.as_mut_ptr(),
                cells,
                par_region.as_mut_ptr(),
                TILES_W,
            ),
            SCG_OK
        );
        assert_eq!(scg_frame_end(ctx, par_tuiles.as_mut_ptr(), TILES_W), SCG_OK);
        scg_destroy(ctx);
    }

    assert!(
        par_region.iter().any(|&b| b != 0),
        "la région n'a rien peint"
    );
    assert_eq!(par_region, par_tuiles, "la région et les tuiles divergent");
}

/// Quatre régions partielles recomposent l'image que les tuiles rendent.
///
/// **Le seul appel réussi de cette fonction passait l'image entière**, ici comme
/// partout : la clause qui l'intéresse — un rectangle quelconque, et la
/// composition de plusieurs — n'était éprouvée par rien à travers l'ABI. Un
/// défaut qui aurait décalé la copie d'une région de son origine serait passé,
/// l'origine valant zéro dans le seul cas joué.
///
/// **Les quatre découpes sont désalignées sur les deux axes** : 100 et 70 ne
/// sont multiples ni de 32 ni de 64, donc aucune n'épouse un bord de tuile. La
/// leçon est celle du cas jumeau du noyau, qui ne désalignait que l'abscisse et
/// laissait la moitié de sa propriété sans épreuve.
///
/// Les tampons de travail couvrent l'image entière, donc chaque région s'en
/// sert partiellement : la clause est « au moins la région », et la passer plus
/// grande est ce qu'un hôte fait pour n'allouer qu'une fois.
#[test]
fn des_regions_partielles_recomposent_l_image() {
    let ctx = tiled();
    let mut par_regions = tiles_buffer();
    let mut par_tuiles = tiles_buffer();
    let cells = TILES_W as usize * TILES_H as usize;
    let mut color = vec![0u32; cells];
    let mut depth = vec![0u32; cells];
    let mut count = 0;

    // Le même triangle en travers de plusieurs tuiles que le cas ci-dessus, et
    // il traverse aussi les quatre découpes : une géométrie tenant dans une
    // seule rendrait la recomposition vraie sans rien éprouver.
    let vertices = [
        ScgVertex {
            x: 4.0,
            y: -1.5,
            z: -1.0,
        },
        ScgVertex {
            x: 4.0,
            y: 1.5,
            z: -1.0,
        },
        ScgVertex {
            x: 4.0,
            y: 0.0,
            z: 1.2,
        },
    ];
    let triangles = [ScgTriangle {
        i0: 0,
        i1: 1,
        i2: 2,
        r: 0x20,
        g: 0x90,
        b: 0xd0,
        a: 255,
    }];
    let model = ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    // Un pavage de l'image par quatre rectangles disjoints, aucun aligné sur une
    // tuile : leur union est l'image, donc leur somme doit l'être aussi.
    const DECOUPES: [(u32, u32, u32, u32); 4] = [
        (0, 0, 100, 70),
        (100, 0, 103, 70),
        (0, 70, 100, 80),
        (100, 70, 103, 80),
    ];

    // SAFETY: handle vivant pour tout ce test, tableaux et tampons locaux, et
    // les tampons de travail couvrent l'image entière donc chaque région.
    unsafe {
        assert_eq!(
            scg_submit(ctx, &model, vertices.as_ptr(), 3, triangles.as_ptr(), 1),
            SCG_OK
        );
        assert_eq!(scg_frame_begin(ctx, &mut count), SCG_OK);
        for (x, y, w, h) in DECOUPES {
            assert_eq!(
                scg_frame_region(
                    ctx,
                    x,
                    y,
                    w,
                    h,
                    color.as_mut_ptr(),
                    depth.as_mut_ptr(),
                    cells,
                    par_regions.as_mut_ptr(),
                    TILES_W,
                ),
                SCG_OK,
                "la région ({x}, {y}, {w}, {h})"
            );
        }
        assert_eq!(scg_frame_end(ctx, par_tuiles.as_mut_ptr(), TILES_W), SCG_OK);
        scg_destroy(ctx);
    }

    assert!(
        par_regions.iter().any(|&b| b != 0),
        "les régions n'ont rien peint"
    );
    assert_eq!(
        par_regions, par_tuiles,
        "les quatre régions et les tuiles divergent"
    );
}

/// Ce que la région refuse, et qu'aucune autre fonction ne refuse pour elle.
///
/// Les deux clauses qui lui sont propres sont le rectangle et la longueur des
/// tampons de travail : hors d'une image, elle se refuse comme une tuile.
#[test]
fn la_region_refuse_un_rectangle_ou_un_tampon_hors_borne() {
    let ctx = tiled();
    let mut pixels = tiles_buffer();
    let cells = TILES_W as usize * TILES_H as usize;
    let mut color = vec![0u32; cells];
    let mut depth = vec![0u32; cells];
    let mut count = 0;

    // SAFETY: handle vivant, tampons locaux couvrant l'image.
    let mut region = |x, y, w, h, scratch| unsafe {
        scg_frame_region(
            ctx,
            x,
            y,
            w,
            h,
            color.as_mut_ptr(),
            depth.as_mut_ptr(),
            scratch,
            pixels.as_mut_ptr(),
            TILES_W,
        )
    };

    assert_eq!(
        region(0, 0, TILES_W, TILES_H, cells),
        SCG_ERR_INVALID_STATE,
        "hors d'une image"
    );

    // SAFETY: comme ci-dessus.
    unsafe { assert_eq!(scg_frame_begin(ctx, &mut count), SCG_OK) };

    assert_eq!(
        region(1, 0, TILES_W, TILES_H, cells),
        SCG_ERR_INVALID_ARGUMENT,
        "le rectangle dépasse à droite"
    );
    assert_eq!(
        region(0, 0, TILES_W, TILES_H, cells - 1),
        SCG_ERR_INVALID_ARGUMENT,
        "les tampons de travail sont trop courts"
    );
    assert_eq!(
        region(0, 0, TILES_W, TILES_H, cells),
        SCG_OK,
        "et le même appel borné passe"
    );

    // SAFETY: comme ci-dessus.
    unsafe {
        let null = ptr::null_mut();
        assert_eq!(
            scg_frame_region(
                ctx,
                0,
                0,
                1,
                1,
                null,
                depth.as_mut_ptr(),
                cells,
                pixels.as_mut_ptr(),
                TILES_W,
            ),
            SCG_ERR_NULL
        );
        scg_destroy(ctx);
    }
}

/// Un pointeur que les threads d'un test se partagent.
///
/// Les pointeurs bruts ne sont pas `Send` ; ce qui rend le partage sain est le
/// contrat de l'ABI — des tuiles d'index distincts sur des rectangles
/// disjoints —, que le test respecte.
#[derive(Clone, Copy)]
struct Shared(*mut ScgContext, *mut u8);

// SAFETY: voir la documentation de `Shared`.
unsafe impl Send for Shared {}

/// Des tuiles rendues depuis plusieurs threads par la frontière, puis la
/// fin : l'image est celle de la fin seule, octet pour octet.
#[test]
fn des_tuiles_sur_plusieurs_threads_rendent_l_image_de_la_fin_seule() {
    let reference_ctx = tiled();
    let mut expected = tiles_buffer();
    // SAFETY: handle vivant, tampon à la taille de l'image.
    let code = unsafe { scg_frame_end(reference_ctx, expected.as_mut_ptr(), TILES_W) };
    assert_eq!(code, SCG_OK);

    let ctx = tiled();
    let mut pixels = tiles_buffer();
    let mut count = 0;
    // SAFETY: handle vivant, compteur local.
    assert_eq!(unsafe { scg_frame_begin(ctx, &mut count) }, SCG_OK);
    assert_eq!(count, 35);

    let shared = Shared(ctx, pixels.as_mut_ptr());
    std::thread::scope(|scope| {
        for worker in 0..4 {
            scope.spawn(move || {
                let shared = shared;
                // Un tiers des tuiles seulement : la fin rend le reste.
                for index in (0..count).filter(|i| i % 4 == worker && i % 3 != 0) {
                    // SAFETY: handle vivant, index propre à ce thread, tampon
                    // à la taille de l'image.
                    let code = unsafe { scg_frame_tile(shared.0, index, shared.1, TILES_W) };
                    assert_eq!(code, SCG_OK, "tuile {index}");
                }
            });
        }
    });

    // SAFETY: handle vivant, plus aucune tuile en cours.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), TILES_W) };
    assert_eq!(code, SCG_OK);
    assert!(pixels == expected);

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_destroy(ctx);
        scg_destroy(reference_ctx);
    }
}

/// La séquence de l'image vue par un hôte : chaque appel hors séquence a son
/// code, et le message d'une tuile se lit sur l'emplacement du thread, pas
/// sur celui du contexte.
#[test]
fn la_sequence_de_l_image_est_verifiee_a_la_frontiere() {
    let ctx = tiled();
    let mut pixels = tiles_buffer();
    let mut count = 0;

    let base = pixels.as_mut_ptr();
    // SAFETY: pour tout ce test, handle vivant, tampon à la taille de l'image
    // et compteur local.
    let tile = |index, stride| unsafe { scg_frame_tile(ctx, index, base, stride) };

    assert_eq!(tile(0, TILES_W), SCG_ERR_INVALID_STATE);
    assert_ne!(last_error(ptr::null()), "", "message de la tuile");
    assert_eq!(last_error(ctx), "", "message du contexte intact");

    // SAFETY: comme ci-dessus.
    unsafe {
        assert_eq!(scg_frame_begin(ctx, ptr::null_mut()), SCG_ERR_NULL);
        assert_eq!(scg_frame_begin(ctx, &mut count), SCG_OK);
        assert_eq!(scg_frame_begin(ctx, &mut count), SCG_ERR_INVALID_STATE);
        let null = ptr::null_mut();
        assert_eq!(scg_frame_tile(ctx, 0, null, TILES_W), SCG_ERR_NULL);
    }

    assert_eq!(tile(count, TILES_W), SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(tile(0, TILES_W - 1), SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(tile(0, TILES_W), SCG_OK);
    assert_eq!(tile(0, TILES_W), SCG_ERR_INVALID_STATE);

    // SAFETY: comme ci-dessus.
    assert_eq!(unsafe { scg_frame_end(ctx, base, TILES_W) }, SCG_OK);
    assert_eq!(tile(1, TILES_W), SCG_ERR_INVALID_STATE, "après la fin");
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Une description de texture valide, dont les tests dérivent leurs variantes.
fn desc(width: u32, height: u32) -> ScgTextureDesc {
    ScgTextureDesc {
        width,
        height,
        format: SCG_TEXTURE_FORMAT_RGBA8,
        reserved0: 0,
        reserved1: 0,
        reserved2: 0,
    }
}

/// Charge une texture, ou échoue en disant pourquoi.
fn load(desc: &ScgTextureDesc, pixels: &[u8]) -> *mut ScgTexture {
    let mut out = ptr::null_mut();
    // SAFETY: la description, le bloc et le paramètre de sortie sont des
    // valeurs locales vivantes, et la longueur est celle de la tranche.
    let code = unsafe { scg_texture_load(desc, pixels.as_ptr(), pixels.len(), &mut out) };
    assert_eq!(
        code,
        SCG_OK,
        "chargement refusé : {}",
        last_error(ptr::null())
    );
    assert!(!out.is_null());
    out
}

/// Une texture se charge et se détruit sans contexte : c'est ce qui permet à
/// un hôte de préparer ses ressources avant d'ouvrir sa fenêtre, et à un
/// serveur de n'en ouvrir jamais.
#[test]
fn charge_et_detruit_une_texture_sans_contexte() {
    let texture = load(&desc(4, 2), &[0x80; 4 * 2 * 4]);
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_texture_destroy(texture) };
}

/// Détruire un pointeur nul ne fait rien, comme `free`.
#[test]
fn detruire_une_texture_nulle_ne_fait_rien() {
    // SAFETY: le pointeur nul est admis, et la fonction ne fait rien.
    unsafe { scg_texture_destroy(ptr::null_mut()) };
}

/// Une description laissée à zéro est refusée plutôt que lue comme valide :
/// c'est toute la raison pour laquelle le format RGBA8 vaut un et non zéro.
#[test]
fn refuse_une_description_a_zero() {
    let zeroed = desc(4, 4);
    let mut out = ptr::null_mut();
    // SAFETY: description locale vivante, bloc de la longueur annoncée.
    let code = unsafe {
        scg_texture_load(
            &ScgTextureDesc {
                format: 0,
                ..zeroed
            },
            [0u8; 64].as_ptr(),
            64,
            &mut out,
        )
    };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert!(out.is_null(), "rien ne doit être écrit en cas d'échec");
    assert!(last_error(ptr::null()).contains("format"));
}

/// Un mode de mélange inconnu est refusé, **et zéro en fait partie**.
///
/// C'est la règle générale que l'étape écrit : zéro ne vaut défaut que pour un
/// réglage de contexte, comme le filtrage. Un mode passé à une soumission est
/// une description, et une description laissée à zéro se refuse plutôt que de
/// s'interpréter — sans quoi un hôte qui oublie ce champ obtient une image
/// rendue autrement qu'il ne croit, sans rien pour l'en avertir.
#[test]
fn refuse_un_mode_de_melange_inconnu() {
    let ctx = create(&sane());
    let model = ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };
    let sommets = [
        ScgVertexUv {
            x: 2.0,
            y: 2.5,
            z: 1.6,
            u: 0.0,
            v: 0.0,
        },
        ScgVertexUv {
            x: 3.5,
            y: -2.5,
            z: 1.6,
            u: 1.0,
            v: 0.0,
        },
        ScgVertexUv {
            x: 3.5,
            y: -2.5,
            z: -1.6,
            u: 1.0,
            v: 1.0,
        },
    ];
    let faces = [ScgTriangle {
        i0: 0,
        i1: 1,
        i2: 2,
        r: 0x80,
        g: 0x80,
        b: 0x80,
        a: 0xFF,
    }];

    for mode in [0, SCG_BLEND_MODULATE + 1, u32::MAX] {
        // SAFETY: handle vivant, pointeurs locaux, comptes exacts.
        let code = unsafe {
            scg_submit_blended(
                ctx,
                &model,
                sommets.as_ptr(),
                sommets.len() as u32,
                faces.as_ptr(),
                faces.len() as u32,
                ptr::null(),
                mode,
            )
        };
        assert_eq!(code, SCG_ERR_INVALID_ARGUMENT, "mode {mode}");
        assert!(last_error(ctx).contains("blend"));
    }

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Une orientation de sprite inconnue est refusée, **et zéro en fait partie**.
///
/// Même règle que pour le mode de mélange, et c'est bien une règle générale et
/// non deux coïncidences : `orientation` décrit le lot, il ne règle pas le
/// contexte.
#[test]
fn refuse_une_orientation_de_sprite_inconnue() {
    let ctx = create(&sane());
    let model = ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };
    let sprites = [ScgSprite {
        x: 10.0,
        y: 0.0,
        z: 0.0,
        half_width: 1.0,
        half_height: 1.0,
        u0: 0.0,
        v0: 0.0,
        u1: 1.0,
        v1: 1.0,
        roll: 0,
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
        a: 0xFF,
    }];

    for mode in [0, SCG_SPRITE_FACING + 1, u32::MAX] {
        // SAFETY: handle vivant, pointeurs locaux, comptes exacts.
        let code = unsafe {
            scg_submit_sprites(
                ctx,
                &model,
                sprites.as_ptr(),
                sprites.len() as u32,
                ptr::null(),
                mode,
            )
        };
        assert_eq!(code, SCG_ERR_INVALID_ARGUMENT, "orientation {mode}");
        assert!(last_error(ctx).contains("orientation"));
    }

    // Les deux valeurs publiées, elles, passent.
    for mode in [SCG_SPRITE_AXIAL, SCG_SPRITE_FACING] {
        // SAFETY: handle vivant, pointeurs locaux, comptes exacts.
        let code = unsafe {
            scg_submit_sprites(
                ctx,
                &model,
                sprites.as_ptr(),
                sprites.len() as u32,
                ptr::null(),
                mode,
            )
        };
        assert_eq!(code, SCG_OK, "orientation {mode}");
    }

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Le format masqué se charge par la même fonction, sans entrée de plus.
///
/// C'est tout ce qu'un hôte a à faire pour obtenir la transparence : la
/// propriété est portée par la texture, et chaque chemin de soumission qui
/// prend une texture en hérite.
#[test]
fn charge_une_texture_masquee() {
    let masked = ScgTextureDesc {
        format: SCG_TEXTURE_FORMAT_RGBA8_MASKED,
        ..desc(4, 4)
    };
    let texture = load(&masked, &[0x80; 4 * 4 * 4]);
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_texture_destroy(texture) };
}

/// Un format que cette version ne connaît pas est refusé, jamais rabattu sur
/// le défaut.
///
/// C'est ce qui rend l'ajout d'un format compatible : une liaison écrite
/// contre une version ultérieure reçoit une erreur, au lieu d'une image lue
/// autrement qu'elle ne le croit.
#[test]
fn refuse_un_format_inconnu() {
    let inconnu = ScgTextureDesc {
        format: SCG_TEXTURE_FORMAT_RGBA8_MASKED + 1,
        ..desc(4, 4)
    };
    let mut out = ptr::null_mut();
    // SAFETY: description locale vivante, bloc de la longueur annoncée.
    let code = unsafe { scg_texture_load(&inconnu, [0u8; 64].as_ptr(), 64, &mut out) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert!(out.is_null(), "rien ne doit être écrit en cas d'échec");
    assert!(last_error(ptr::null()).contains("format"));
}

/// Un champ réservé non nul est refusé, et le message le dit : c'est la clause
/// qui permettra d'en employer un sans casser les liaisons déjà écrites.
#[test]
fn refuse_un_champ_reserve_de_texture_non_nul() {
    // Les trois, séparément : la description en porte trois, et un contrôle
    // écrit sur un seul ne dit rien des deux autres.
    for quoi in 0..3 {
        let mut d = desc(4, 4);
        match quoi {
            0 => d.reserved0 = 1,
            1 => d.reserved1 = 1,
            _ => d.reserved2 = 1,
        }

        let mut out = ptr::null_mut();
        // SAFETY: description locale vivante, bloc de la longueur annoncée.
        let code = unsafe { scg_texture_load(&d, [0u8; 64].as_ptr(), 64, &mut out) };
        assert_eq!(code, SCG_ERR_INVALID_ARGUMENT, "reserved{quoi}");
        assert!(last_error(ptr::null()).contains("reserved"));
    }
}

/// Les refus du noyau traversent la frontière avec leur message : un côté qui
/// n'est pas une puissance de deux, un bloc de la mauvaise longueur.
#[test]
fn les_refus_du_noyau_traversent_la_frontiere() {
    for (d, pixels, mot) in [
        (desc(3, 4), vec![0u8; 3 * 4 * 4], "power of two"),
        (desc(4, 4), vec![0u8; 60], "width x height x 4"),
    ] {
        let mut out = ptr::null_mut();
        // SAFETY: description et bloc locaux, longueur celle de la tranche.
        let code = unsafe { scg_texture_load(&d, pixels.as_ptr(), pixels.len(), &mut out) };
        assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
        let message = last_error(ptr::null());
        assert!(message.contains(mot), "message inattendu : {message}");
    }
}

/// Une soumission texturée traverse la frontière, et une texture détruite
/// aussitôt après ne trouble pas l'image : le moteur en garde une référence
/// jusqu'à la fin.
#[test]
fn soumet_un_lot_texture_puis_detruit_la_texture() {
    let ctx = create(&sane());
    let texture = load(&desc(2, 2), &[0x40; 16]);

    let vertices = [
        ScgVertexUv {
            x: 10.0,
            y: -2.0,
            z: -2.0,
            u: 0.0,
            v: 0.0,
        },
        ScgVertexUv {
            x: 10.0,
            y: 0.0,
            z: 2.0,
            u: 4.0,
            v: 0.0,
        },
        ScgVertexUv {
            x: 10.0,
            y: 2.0,
            z: -2.0,
            u: 4.0,
            v: 4.0,
        },
    ];
    let triangles = [ScgTriangle {
        i0: 0,
        i1: 1,
        i2: 2,
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
        a: 0xFF,
    }];
    let identity = ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    // SAFETY: contexte et texture vivants, les deux tableaux couvrent les
    // comptes annoncés.
    let code = unsafe {
        scg_submit_textured(
            ctx,
            &identity,
            vertices.as_ptr(),
            3,
            triangles.as_ptr(),
            1,
            texture,
        )
    };
    assert_eq!(code, SCG_OK, "soumission refusée : {}", last_error(ctx));

    // SAFETY: handle vivant ; le moteur en garde sa propre référence.
    unsafe { scg_texture_destroy(texture) };

    let mut pixels = vec![0u8; 64 * 32 * 4];
    // SAFETY: contexte vivant, tampon de la taille annoncée par le stride.
    let end = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(end, SCG_OK);
    assert!(
        pixels.chunks_exact(4).any(|p| p[..3] == [0x40, 0x40, 0x40]),
        "le triangle texturé n'a rien peint"
    );
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Les deux filtres se posent, et une valeur inconnue est refusée sans changer
/// celui du contexte.
///
/// **Le refus est ce qui rend l'ajout d'un filtrage compatible** : une liaison
/// écrite contre une version ultérieure reçoit une erreur franche au lieu d'une
/// image filtrée autrement qu'elle ne le croit. Le rabattre sur le défaut
/// serait silencieux, donc pire.
#[test]
fn pose_les_deux_filtres_et_refuse_le_reste() {
    let ctx = create(&sane());

    for filter in [SCG_FILTER_DITHER, SCG_FILTER_BILINEAR] {
        // SAFETY: `ctx` est un handle vivant, seul ce thread l'emploie.
        assert_eq!(unsafe { scg_set_filter(ctx, filter) }, SCG_OK);
    }

    // SAFETY: mêmes préconditions ; la valeur, elle, n'est pas un pointeur.
    let refus = unsafe { scg_set_filter(ctx, 2) };
    assert_eq!(refus, SCG_ERR_INVALID_ARGUMENT);
    assert!(last_error(ctx).contains("SCG_FILTER_BILINEAR"));

    // SAFETY: idem, le handle nul est refusé et non déréférencé.
    assert_eq!(unsafe { scg_set_filter(ptr::null_mut(), 0) }, SCG_ERR_NULL);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Le filtre ne se change pas pendant une image : les tuiles s'en lisent depuis
/// des threads que le moteur ne connaît pas, et deux filtrages dans la même
/// image ne décriraient plus rien.
#[test]
fn refuse_de_changer_le_filtre_pendant_une_image() {
    let ctx = create(&sane());
    let mut tiles = 0;
    // SAFETY: `ctx` est vivant, le compteur est local, et l'image se referme
    // plus bas.
    assert_eq!(unsafe { scg_frame_begin(ctx, &mut tiles) }, SCG_OK);

    // SAFETY: mêmes préconditions.
    let pendant = unsafe { scg_set_filter(ctx, SCG_FILTER_BILINEAR) };
    assert_eq!(pendant, SCG_ERR_INVALID_STATE);

    let mut pixels = vec![0u8; 64 * 32 * 4];
    // SAFETY: le tampon porte bien `stride × hauteur × 4` octets.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(code, SCG_OK);

    // SAFETY: l'image est close, le réglage redevient permis.
    assert_eq!(unsafe { scg_set_filter(ctx, SCG_FILTER_BILINEAR) }, SCG_OK);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Une fin d'image refusée sur sa sortie laisse le contexte utilisable.
///
/// `scg_frame_end` ouvre l'image elle-même quand personne ne l'a fait. Si elle
/// l'ouvrait avant de regarder la sortie, un `stride` fautif laisserait
/// derrière lui un contexte en rendu, et tout appel exclusif rendrait ensuite
/// `SCG_ERR_INVALID_STATE` — un hôte qui s'est trompé de tampon perdrait sa
/// session sans rien pour comprendre pourquoi.
#[test]
fn une_sortie_refusee_laisse_le_contexte_utilisable() {
    let ctx = create(&sane());
    let mut pixels = vec![0u8; 64 * 32 * 4];

    // SAFETY: `ctx` est vivant, et le tampon couvre bien 64×32 pixels — c'est
    // le `stride` qui est fautif, pas la longueur.
    let court = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 63) };
    assert_eq!(court, SCG_ERR_INVALID_ARGUMENT);

    // SAFETY: mêmes préconditions. C'est la preuve utile : le contexte répond
    // encore, donc l'image refusée ne s'est jamais ouverte.
    assert_eq!(unsafe { scg_set_resolution(ctx, 32, 16) }, SCG_OK);

    // SAFETY: le tampon reste assez long pour la résolution réduite.
    let rendu = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 32) };
    assert_eq!(rendu, SCG_OK);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Une courbe de sortie neutre, celle qu'un hôte écrit après avoir mis la
/// structure à zéro.
fn neutral_grade() -> ScgGrade {
    ScgGrade {
        gamma: 1.0,
        gain_r: 1.0,
        gain_g: 1.0,
        gain_b: 1.0,
        offset_r: 0.0,
        offset_g: 0.0,
        offset_b: 0.0,
        reserved0: 0,
        reserved1: 0,
    }
}

/// La courbe se pose dans ses bornes, et rien d'autre.
///
/// Le champ réservé non nul compte autant que les bornes : c'est lui qui rend
/// la promesse d'extension vraie. Sans ce refus, un hôte pourrait y écrire
/// n'importe quoi aujourd'hui, et le jour où le champ prendrait un sens il
/// recevrait ce n'importe quoi comme réglage.
#[test]
fn pose_une_courbe_dans_ses_bornes_et_refuse_le_reste() {
    let ctx = create(&sane());

    // SAFETY: `ctx` est vivant, et la structure est un local qui vit jusqu'au
    // retour de l'appel — le moteur n'en garde aucune référence.
    assert_eq!(unsafe { scg_set_grade(ctx, &neutral_grade()) }, SCG_OK);

    let chaude = ScgGrade {
        gamma: 2.2,
        gain_r: 1.15,
        gain_b: 0.85,
        offset_g: 0.05,
        ..neutral_grade()
    };
    // SAFETY: mêmes préconditions.
    assert_eq!(unsafe { scg_set_grade(ctx, &chaude) }, SCG_OK);

    // Chaque variante part du neutre et n'en change qu'un champ : le refus
    // porte alors sur ce champ, et sur rien d'autre.
    let refuse = |quoi: &str, modifier: fn(&mut ScgGrade)| {
        let mut grade = neutral_grade();
        modifier(&mut grade);
        // SAFETY: mêmes préconditions.
        let code = unsafe { scg_set_grade(ctx, &grade) };
        assert_eq!(code, SCG_ERR_INVALID_ARGUMENT, "{quoi}");
    };

    refuse("gamma nul", |g| g.gamma = 0.0);
    refuse("gamma haut", |g| g.gamma = 9.0);
    refuse("gain négatif", |g| g.gain_g = -1.0);
    refuse("gain haut", |g| g.gain_b = 5.0);
    refuse("décalage bas", |g| g.offset_r = -1.5);
    refuse("décalage haut", |g| g.offset_b = 1.5);
    refuse("réservé 0 non nul", |g| g.reserved0 = 1);
    refuse("réservé 1 non nul", |g| g.reserved1 = 1);

    // SAFETY: le pointeur nul est le cas que la fonction doit refuser.
    assert_eq!(unsafe { scg_set_grade(ctx, ptr::null()) }, SCG_ERR_NULL);
    // SAFETY: mêmes préconditions.
    assert_eq!(unsafe { scg_clear_grade(ptr::null_mut()) }, SCG_ERR_NULL);

    // Éteindre une courbe qu'on n'a pas réglée n'est pas une erreur, et le
    // faire deux fois non plus.
    // SAFETY: `ctx` est un handle vivant, employé par ce seul thread.
    assert_eq!(unsafe { scg_clear_grade(ctx) }, SCG_OK);
    // SAFETY: mêmes préconditions.
    assert_eq!(unsafe { scg_clear_grade(ctx) }, SCG_OK);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// La courbe ne se change pas pendant une image, pour la raison qui vaut pour
/// le filtre : les tuiles se rendent depuis des threads que le moteur ne
/// connaît pas, et deux courbes dans la même image ne décriraient rien.
#[test]
fn refuse_de_changer_la_courbe_pendant_une_image() {
    let ctx = create(&sane());
    let mut tiles = 0;
    // SAFETY: `ctx` est vivant, le compteur est local, l'image se referme plus
    // bas.
    assert_eq!(unsafe { scg_frame_begin(ctx, &mut tiles) }, SCG_OK);

    // SAFETY: mêmes préconditions.
    let pendant = unsafe { scg_set_grade(ctx, &neutral_grade()) };
    assert_eq!(pendant, SCG_ERR_INVALID_STATE);
    // SAFETY: mêmes préconditions.
    assert_eq!(unsafe { scg_clear_grade(ctx) }, SCG_ERR_INVALID_STATE);

    let mut pixels = vec![0u8; 64 * 32 * 4];
    // SAFETY: le tampon porte bien `stride × hauteur × 4` octets.
    let clos = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(clos, SCG_OK);

    // SAFETY: l'image est close, le réglage redevient permis.
    assert_eq!(unsafe { scg_set_grade(ctx, &neutral_grade()) }, SCG_OK);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// La résolution se pose sous le maximum de la création, et rien d'autre.
///
/// Le maximum est ce sur quoi tous les tampons ont été dimensionnés : au-delà,
/// l'accepter demanderait une réallocation, c'est-à-dire ce que « zéro
/// allocation par image » interdit.
#[test]
fn pose_une_resolution_sous_le_maximum_et_refuse_le_reste() {
    let ctx = create(&sane());

    for (width, height) in [(32u32, 16u32), (64, 32), (1, 1)] {
        // SAFETY: `ctx` est un handle vivant, employé par ce seul thread.
        assert_eq!(unsafe { scg_set_resolution(ctx, width, height) }, SCG_OK);
    }

    for (width, height) in [(65u32, 32u32), (64, 33), (0, 32), (32, 0)] {
        // SAFETY: mêmes préconditions.
        let refus = unsafe { scg_set_resolution(ctx, width, height) };
        assert_eq!(refus, SCG_ERR_INVALID_ARGUMENT, "{width}×{height}");
        assert!(last_error(ctx).contains("resolution"));
    }

    // SAFETY: le pointeur nul est le cas que la fonction doit refuser.
    let nul = unsafe { scg_set_resolution(ptr::null_mut(), 32, 16) };
    assert_eq!(nul, SCG_ERR_NULL);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Le nombre de tuiles suit la résolution, et c'est le piège d'ABI du
/// redimensionnement : un hôte qui garde le compte de l'image précédente boucle
/// au-delà de la grille.
#[test]
fn le_nombre_de_tuiles_suit_la_resolution() {
    let ctx = create(&sane());
    let mut avant = 0;
    // SAFETY: `ctx` est vivant, le compteur est local, l'image se referme plus
    // bas.
    assert_eq!(unsafe { scg_frame_begin(ctx, &mut avant) }, SCG_OK);
    let mut pixels = vec![0u8; 64 * 32 * 4];
    // SAFETY: le tampon porte `stride × hauteur × 4` octets.
    let clos = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(clos, SCG_OK);

    // SAFETY: l'image est close, le réglage est permis.
    assert_eq!(unsafe { scg_set_resolution(ctx, 32, 16) }, SCG_OK);

    let mut apres = 0;
    // SAFETY: mêmes préconditions.
    assert_eq!(unsafe { scg_frame_begin(ctx, &mut apres) }, SCG_OK);
    assert!(apres < avant, "{apres} tuiles, contre {avant} avant");

    // SAFETY: l'index vient du compte que la grille rétrécie a rendu.
    let hors = unsafe { scg_frame_tile(ctx, avant - 1, pixels.as_mut_ptr(), 32) };
    assert_eq!(hors, SCG_ERR_INVALID_ARGUMENT, "l'ancien compte est périmé");

    // SAFETY: le tampon reste assez long pour la résolution réduite.
    let reduit = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 32) };
    assert_eq!(reduit, SCG_OK);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// La résolution ne se change pas pendant une image, pour la raison qui vaut
/// pour le filtre : les tuiles se rendent depuis des threads que le moteur ne
/// connaît pas, et une grille changée au milieu ne décrirait plus rien.
#[test]
fn refuse_de_changer_la_resolution_pendant_une_image() {
    let ctx = create(&sane());
    let mut tiles = 0;
    // SAFETY: `ctx` est vivant, le compteur est local, l'image se referme plus
    // bas.
    assert_eq!(unsafe { scg_frame_begin(ctx, &mut tiles) }, SCG_OK);

    // SAFETY: mêmes préconditions.
    let pendant = unsafe { scg_set_resolution(ctx, 32, 16) };
    assert_eq!(pendant, SCG_ERR_INVALID_STATE);

    let mut pixels = vec![0u8; 64 * 32 * 4];
    // SAFETY: le tampon porte `stride × hauteur × 4` octets.
    let clos = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(clos, SCG_OK);

    // SAFETY: l'image est close, le réglage redevient permis.
    assert_eq!(unsafe { scg_set_resolution(ctx, 32, 16) }, SCG_OK);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// La caméra ne se change plus dès qu'un triangle est retenu : la soumission
/// projette tout de suite, et deux espaces écran dans la même image ne
/// décriraient rien.
///
/// Le refus tombe avec l'image : un hôte qui règle sa caméra dès le retour de
/// `scg_frame_end` est servi, sans quoi la boucle la plus naturelle — régler,
/// soumettre, rendre — serait refusée dès sa deuxième itération.
#[test]
fn refuse_de_changer_la_camera_apres_une_soumission() {
    let ctx = create(&sane());
    let camera = ScgCamera {
        position: [1.0, 0.0, 0.0],
        orientation: [0.0, 0.0, 0.0, 1.0],
        fov_y: 1.2,
        near_plane: 0.1,
    };

    // SAFETY: `ctx` est vivant, et `camera` est un local qui vit jusqu'au
    // retour de l'appel — le moteur n'en garde aucune référence.
    assert_eq!(unsafe { scg_set_camera(ctx, &camera) }, SCG_OK);
    assert_eq!(submit_scene(ctx), SCG_OK);

    // SAFETY: mêmes préconditions.
    let apres = unsafe { scg_set_camera(ctx, &camera) };
    assert_eq!(apres, SCG_ERR_INVALID_STATE);

    let mut pixels = vec![0u8; 64 * 32 * 4];
    // SAFETY: le tampon porte bien `stride × hauteur × 4` octets.
    let code = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(code, SCG_OK);

    // SAFETY: l'image est close, sa liste de dessin ne compte plus.
    assert_eq!(unsafe { scg_set_camera(ctx, &camera) }, SCG_OK);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Une coordonnée non finie refuse le lot **même sur un sommet qu'aucun
/// triangle n'indexe**, et une texture nulle le refuse aussi.
///
/// **C'est là que la façade se distingue du noyau, et nulle part ailleurs.** Le
/// cas posait d'abord le `NaN` sur les trois sommets du triangle : les deux
/// couches le refusaient, avec le même code et un message contenant « finite »
/// toutes les deux, si bien qu'il passait encore après le retrait de
/// `check_finite`. Ce que la frontière ajoute est la validation de la tranche
/// **entière** — le noyau ne voit que les sommets qu'il résout —, donc le `NaN`
/// va sur le quatrième, et le message attendu est celui de la frontière mot
/// pour mot.
#[test]
fn refuse_un_sommet_texture_non_fini() {
    let ctx = create(&sane());
    let texture = load(&desc(2, 2), &[0x40; 16]);
    let bon = ScgVertexUv {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        u: 0.0,
        v: 0.0,
    };
    let vertices = [bon, bon, bon, ScgVertexUv { x: f32::NAN, ..bon }];
    let triangles = [ScgTriangle {
        i0: 0,
        i1: 1,
        i2: 2,
        r: 0,
        g: 0,
        b: 0,
        a: 0xFF,
    }];
    let identity = ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    // SAFETY: contexte et texture vivants, tableaux de la taille annoncée.
    let code = unsafe {
        scg_submit_textured(
            ctx,
            &identity,
            vertices.as_ptr(),
            4,
            triangles.as_ptr(),
            1,
            texture,
        )
    };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert_eq!(
        last_error(ctx),
        "vertex coordinates must be finite numbers",
        "le refus doit venir de la frontière, pas du noyau"
    );

    // SAFETY: mêmes préconditions ; le handle nul est refusé, pas déréférencé.
    let null_texture = unsafe {
        scg_submit_textured(
            ctx,
            &identity,
            vertices.as_ptr(),
            4,
            triangles.as_ptr(),
            1,
            ptr::null(),
        )
    };
    assert_eq!(null_texture, SCG_ERR_NULL);

    // SAFETY: les deux handles sont vivants et détruits une seule fois.
    unsafe { scg_texture_destroy(texture) };
    // SAFETY: idem.
    unsafe { scg_destroy(ctx) };
}

/// Laisse un message dans l'emplacement par thread, et vérifie qu'il y est.
///
/// Une configuration refusée est le moyen le plus court : `scg_create` n'a pas
/// de contexte auquel rattacher son erreur, elle écrit donc là.
fn laisse_un_message() {
    let config = ScgContextConfig {
        tile_size: 7,
        ..sane()
    };
    let mut ctx = ptr::null_mut();
    // SAFETY: les deux pointeurs visent des valeurs locales vivantes.
    let code = unsafe { scg_create(&config, &mut ctx) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert!(!last_error(ptr::null()).is_empty(), "rien à vider");
}

/// Chaque point d'entrée vide l'emplacement par thread en entrant, **y compris
/// ceux qui ne rendent aucun code**.
///
/// Le contrat d'ABI l'exige sans exception, et la raison n'a rien à voir avec
/// les paniques : sur un thread de pool, une tâche qui échoue laisse son
/// message, et la tâche suivante le relit comme s'il était le sien. Les quatre
/// fonctions testées ici sont celles qui ne peuvent rien rendre — une
/// destruction, une libération — ou qui rendent une valeur plutôt qu'un code.
///
/// Chacune est vérifiée séparément : une seule qui oublierait le vidage suffit
/// à rendre la clause fausse, et c'est toujours la dernière ajoutée.
#[test]
fn les_appels_sans_code_vident_le_message_du_thread() {
    laisse_un_message();
    let ptr = scg_buffer_alloc(64);
    assert!(!ptr.is_null());
    assert_eq!(last_error(ptr::null()), "", "scg_buffer_alloc n'a pas vidé");

    laisse_un_message();
    // SAFETY: `ptr` vient de l'allocation ci-dessus, avec la même longueur.
    unsafe { scg_buffer_free(ptr, 64) };
    assert_eq!(last_error(ptr::null()), "", "scg_buffer_free n'a pas vidé");

    let ctx = create(&sane());
    laisse_un_message();
    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
    assert_eq!(last_error(ptr::null()), "", "scg_destroy n'a pas vidé");

    let pixels = [0u8; 4];
    let desc = ScgTextureDesc {
        width: 1,
        height: 1,
        format: SCG_TEXTURE_FORMAT_RGBA8,
        reserved0: 0,
        reserved1: 0,
        reserved2: 0,
    };
    let mut texture = ptr::null_mut();
    // SAFETY: la description et le bloc de pixels s'accordent, et la sortie
    // vise une variable locale vivante.
    let code = unsafe { scg_texture_load(&desc, pixels.as_ptr(), pixels.len(), &mut texture) };
    assert_eq!(code, SCG_OK);

    laisse_un_message();
    // SAFETY: le handle vient du chargement ci-dessus et n'a pas été détruit.
    unsafe { scg_texture_destroy(texture) };
    assert_eq!(
        last_error(ptr::null()),
        "",
        "scg_texture_destroy n'a pas vidé"
    );
}

/// Un pointeur nul passé à une destruction vide quand même l'emplacement.
///
/// C'est le cas limite qui se perd : ces fonctions rendent la main tout de
/// suite sur un handle nul, et il serait naturel de sortir avant l'enveloppe.
/// Le contrat ne fait pas cette exception — un hôte qui libère en boucle des
/// handles dont certains sont nuls laisserait passer un message sur deux.
#[test]
fn une_destruction_d_un_pointeur_nul_vide_aussi_le_message() {
    laisse_un_message();
    // SAFETY: le pointeur nul est explicitement permis.
    unsafe { scg_destroy(ptr::null_mut()) };
    assert_eq!(
        last_error(ptr::null()),
        "",
        "scg_destroy(NULL) n'a pas vidé"
    );

    laisse_un_message();
    // SAFETY: idem.
    unsafe { scg_texture_destroy(ptr::null_mut()) };
    assert_eq!(last_error(ptr::null()), "", "destruction nulle sans vidage");

    laisse_un_message();
    // SAFETY: idem, et la longueur d'un pointeur nul n'est pas lue.
    unsafe { scg_buffer_free(ptr::null_mut(), 0) };
    assert_eq!(last_error(ptr::null()), "", "libération nulle sans vidage");
}

/// Le sur-éclairement pose ses trois valeurs et refuse le reste.
///
/// Exporté, documenté dans le header et dans `docs/abi.md`, ce point d'entrée
/// n'était appelé par aucun test ni aucun hôte : rien ne disait qu'il traverse
/// la frontière, ni qu'un décalage de trois est refusé plutôt que tronqué à
/// deux. Un décalage silencieusement rabattu rendrait une image plus sombre que
/// ce que l'hôte a demandé, sans rien pour l'en avertir.
#[test]
fn pose_les_trois_sur_eclairements_et_refuse_le_reste() {
    let ctx = create(&sane());

    for shift in [0, 1, 2] {
        // SAFETY: `ctx` est un handle vivant, seul ce thread l'emploie.
        assert_eq!(unsafe { scg_set_overbright(ctx, shift) }, SCG_OK, "{shift}");
    }

    // SAFETY: mêmes préconditions ; la valeur n'est pas un pointeur.
    let refus = unsafe { scg_set_overbright(ctx, 3) };
    assert_eq!(refus, SCG_ERR_INVALID_ARGUMENT);
    assert!(last_error(ctx).contains("overbright"));

    // SAFETY: idem, le handle nul est refusé et non déréférencé.
    let nul = unsafe { scg_set_overbright(ptr::null_mut(), 0) };
    assert_eq!(nul, SCG_ERR_NULL);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Le sur-éclairement ne se change pas pendant une image, comme le filtre.
///
/// La clause d'`abi.md` vaut pour les huit fonctions de l'étape ; celle-ci ne
/// l'éprouvait pas, alors qu'elle écrit dans l'état que les tuiles lisent.
#[test]
fn refuse_de_changer_le_sur_eclairement_pendant_une_image() {
    let ctx = create(&sane());
    let mut tiles = 0;
    // SAFETY: `ctx` est vivant, le compteur est local, et l'image se referme
    // plus bas.
    assert_eq!(unsafe { scg_frame_begin(ctx, &mut tiles) }, SCG_OK);

    // SAFETY: mêmes préconditions.
    let pendant = unsafe { scg_set_overbright(ctx, 2) };
    assert_eq!(pendant, SCG_ERR_INVALID_STATE);

    let mut pixels = vec![0u8; 64 * 32 * 4];
    // SAFETY: le tampon porte bien `stride × hauteur × 4` octets.
    let fin = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(fin, SCG_OK);

    // SAFETY: l'image est close, le réglage redevient permis.
    assert_eq!(unsafe { scg_set_overbright(ctx, 2) }, SCG_OK);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// `scg_set_lights` accepte le plafond exact, et refuse une lumière de plus.
///
/// **Deux contrôles de plafond vivent à deux étages**, et c'est ce qui donne son
/// objet à ce cas : la frontière refuse sur la taille de son propre tableau,
/// avant tout appel au noyau, qui refuse ensuite sur le sien. Un étage rabaissé
/// d'une unité ne serait donc vu par rien — la frontière n'a jamais passé plus
/// d'une lumière, et aucune scène n'en règle plus de trois.
///
/// Le compte vient de `screengine::MAX_LIGHTS` plutôt que d'un littéral : le
/// header ne publie pas cette borne, de sorte qu'une valeur recopiée ici
/// cesserait de dire la même chose que le moteur sans que rien ne le signale.
#[test]
fn les_lumieres_s_arretent_au_plafond_du_moteur() {
    let ctx = create(&sane());
    let lumiere = |x: f32| ScgLight {
        x,
        y: 0.0,
        z: 0.0,
        radius: 32.0,
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
        // Réservé, nul obligatoire : une lumière ajoute, elle ne mélange pas.
        _reserved: 0,
    };
    let lot = |count: usize| -> Vec<ScgLight> {
        (0..count).map(|rang| lumiere(2.0 + rang as f32)).collect()
    };

    let pleine = lot(screengine::MAX_LIGHTS);
    assert_eq!(
        // SAFETY: contexte vivant, le tableau couvre le compte annoncé.
        unsafe { scg_set_lights(ctx, pleine.as_ptr(), pleine.len() as u32) },
        SCG_OK,
        "le plafond exact est refusé : {}",
        last_error(ctx)
    );

    let trop = lot(screengine::MAX_LIGHTS + 1);
    assert_eq!(
        // SAFETY: même précondition, sur un lot d'une lumière de plus.
        unsafe { scg_set_lights(ctx, trop.as_ptr(), trop.len() as u32) },
        SCG_ERR_INVALID_ARGUMENT,
        "une lumière au-delà du plafond a été acceptée"
    );

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// `scg_submit_shaded` traverse la frontière, et la normale y décide.
///
/// **Le seul point d'entrée que personne n'appelait** : aucun hôte, aucune
/// scène de conformance, aucun test. Son symbole était exporté sans que rien
/// ne le lie jamais, et sa structure de sommet — huit flottants, la seule qui
/// en porte autant — n'était écrite par personne à travers l'ABI.
///
/// Deux lots identiques sauf leur normale, l'une vers la lumière et l'autre à
/// l'opposé : le second doit sortir plus sombre. C'est la propriété que cette
/// fonction ajoute à `scg_submit_textured`, et la seule qu'un test puisse
/// vérifier sans figer une empreinte.
#[test]
fn soumet_un_lot_a_normales_et_la_normale_decide() {
    let peint = |nx: f32| -> u32 {
        let ctx = create(&sane());
        let texture = load(&desc(2, 2), &[0xFF; 16]);

        // **Entre la caméra et la géométrie**, qui est à `x = 10` : la
        // direction vers la lumière est donc le `-X`, et c'est une normale
        // tournée par là qui la reçoit.
        let lights = [ScgLight {
            x: 4.0,
            y: 0.0,
            z: 0.0,
            radius: 32.0,
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
            // Réservé, et le contrat exige zéro : une lumière ajoute, elle ne
            // mélange pas, donc l'octet n'est pas un alpha.
            _reserved: 0,
        }];
        assert_eq!(
            // SAFETY: contexte vivant, le tableau couvre le compte annoncé.
            unsafe { scg_set_lights(ctx, lights.as_ptr(), 1) },
            SCG_OK,
            "lumières refusées : {}",
            last_error(ctx)
        );

        let vertex = |y: f32, z: f32, u: f32, v: f32| ScgVertexUvN {
            x: 10.0,
            y,
            z,
            u,
            v,
            nx,
            ny: 0.0,
            nz: 0.0,
        };
        let vertices = [
            vertex(-2.0, -2.0, 0.0, 0.0),
            vertex(0.0, 2.0, 4.0, 0.0),
            vertex(2.0, -2.0, 4.0, 4.0),
        ];
        let triangles = [ScgTriangle {
            i0: 0,
            i1: 1,
            i2: 2,
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
            a: 0xFF,
        }];
        let identity = ScgMat4 {
            m: [
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
        };

        // SAFETY: contexte et texture vivants, les deux tableaux couvrent les
        // comptes annoncés.
        let code = unsafe {
            scg_submit_shaded(
                ctx,
                &identity,
                vertices.as_ptr(),
                3,
                triangles.as_ptr(),
                1,
                texture,
            )
        };
        assert_eq!(code, SCG_OK, "soumission refusée : {}", last_error(ctx));

        // SAFETY: handle vivant ; le moteur en garde sa propre référence.
        unsafe { scg_texture_destroy(texture) };

        let mut pixels = vec![0u8; 64 * 32 * 4];
        assert_eq!(
            // SAFETY: contexte vivant, tampon de la taille annoncée par le
            // stride.
            unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) },
            SCG_OK
        );
        // SAFETY: le handle est vivant et détruit une seule fois.
        unsafe { scg_destroy(ctx) };

        // Le pixel le plus clair de l'image : la géométrie est la même dans les
        // deux cas, seule sa réponse à la lumière change.
        pixels
            .chunks_exact(4)
            .map(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]))
            .max()
            .unwrap_or(0)
    };

    let face = peint(-1.0);
    let dos = peint(1.0);
    assert!(face > 0, "le lot n'a rien peint");
    assert!(
        dos < face,
        "la normale n'a rien changé : face {face}, dos {dos}"
    );
}

/// Une texture nulle est acceptée à normales, et refusée au chemin texturé.
///
/// **C'est la seconde différence entre les deux, et le contrat ne la disait
/// pas** : il annonçait « le même contrat, à une différence près ». Sur le chemin
/// texturé, un lot sans texture a `scg_submit` pour lui ; sur celui-ci il n'a
/// rien, `scg_submit` ne portant pas de normale, et c'est pour lui que la
/// tolérance existe.
///
/// Les deux moitiés tiennent l'écart dans les deux sens : aligner l'un sur
/// l'autre, dans un sens ou dans l'autre, fait rougir l'une des deux.
#[test]
fn une_texture_nulle_est_acceptee_a_normales_et_refusee_au_chemin_texture() {
    let ctx = create(&sane());
    let model = identite();
    let triangles = [ScgTriangle {
        i0: 0,
        i1: 1,
        i2: 2,
        r: 0x80,
        g: 0x40,
        b: 0x20,
        a: 0xFF,
    }];

    let corner = |y: f32, z: f32| ScgVertexUvN {
        x: 10.0,
        y,
        z,
        u: 0.0,
        v: 0.0,
        nx: -1.0,
        ny: 0.0,
        nz: 0.0,
    };
    let shaded = [corner(-2.0, -2.0), corner(0.0, 2.0), corner(2.0, -2.0)];
    // SAFETY: contexte vivant, les deux tableaux couvrent les comptes annoncés,
    // et le contrat admet une texture nulle sur ce chemin.
    let code = unsafe {
        scg_submit_shaded(
            ctx,
            &model,
            shaded.as_ptr(),
            3,
            triangles.as_ptr(),
            1,
            ptr::null(),
        )
    };
    assert_eq!(code, SCG_OK, "lot à normales refusé : {}", last_error(ctx));

    let plain = |y: f32, z: f32| ScgVertexUv {
        x: 10.0,
        y,
        z,
        u: 0.0,
        v: 0.0,
    };
    let textured = [plain(-2.0, -2.0), plain(0.0, 2.0), plain(2.0, -2.0)];
    // SAFETY: mêmes préconditions ; la texture nulle est ce que ce chemin refuse.
    let code = unsafe {
        scg_submit_textured(
            ctx,
            &model,
            textured.as_ptr(),
            3,
            triangles.as_ptr(),
            1,
            ptr::null(),
        )
    };
    assert_eq!(code, SCG_ERR_NULL);

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Pendant une image, l'état est refusé **avant** l'argument.
///
/// La convention de la frontière, et elle n'était tenue nulle part : les huit
/// réglages prennent l'exclusivité avant de convertir leur argument, les deux
/// soumissions de tracé le faisaient dans l'autre sens. Un hôte recevait donc
/// `SCG_ERR_INVALID_ARGUMENT` ici et `SCG_ERR_INVALID_STATE` là pour la même
/// faute double, et aucun document ne départageait.
///
/// **Le mode est doublement fautif**, et c'est ce qui rend le cas mordant : zéro
/// n'est pas un mode publié, donc les deux refus s'appliquent et un seul peut
/// sortir.
#[test]
fn l_etat_passe_avant_l_argument_pendant_une_image() {
    let ctx = create(&sane());
    let model = identite();
    let lines = [ligne()];
    let points = [point()];
    let mut tiles = 0;

    // SAFETY: `ctx` est vivant, le compteur est local, et l'image se referme
    // plus bas.
    assert_eq!(unsafe { scg_frame_begin(ctx, &mut tiles) }, SCG_OK);

    // SAFETY: handle vivant, pointeurs locaux, comptes exacts.
    let code = unsafe { scg_submit_lines(ctx, &model, lines.as_ptr(), 1, 0) };
    assert_eq!(
        code, SCG_ERR_INVALID_STATE,
        "lignes, mode nul pendant l'image"
    );
    // SAFETY: mêmes préconditions.
    let code = unsafe { scg_submit_points(ctx, &model, points.as_ptr(), 1, 0) };
    assert_eq!(
        code, SCG_ERR_INVALID_STATE,
        "points, mode nul pendant l'image"
    );

    let mut pixels = vec![0u8; 64 * 32 * 4];
    // SAFETY: le tampon porte bien `stride × hauteur × 4` octets.
    let fin = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(fin, SCG_OK);

    // Hors de l'image, le même mode rend bien le refus d'argument : le cas
    // n'aurait rien prouvé si l'état masquait un mode que personne ne refuse.
    // SAFETY: mêmes préconditions.
    let code = unsafe { scg_submit_lines(ctx, &model, lines.as_ptr(), 1, 0) };
    assert_eq!(
        code, SCG_ERR_INVALID_ARGUMENT,
        "hors image, l'argument sort"
    );

    // SAFETY: le handle est vivant et détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Un mode de profondeur inconnu est refusé, **et zéro en fait partie**.
///
/// La même règle que pour le mélange et l'orientation, et c'est bien une règle
/// et non trois coïncidences : un mode passé à une soumission décrit le lot, il
/// ne règle pas le contexte. Zéro ne vaut défaut que pour les réglages.
#[test]
fn refuse_un_mode_de_profondeur_inconnu() {
    let ctx = create(&sane());
    let model = identite();
    let lines = [ligne()];
    let points = [point()];

    for mode in [0, SCG_DEPTH_ALWAYS + 1, u32::MAX] {
        // SAFETY: handle vivant, pointeurs locaux, comptes exacts.
        let code =
            unsafe { scg_submit_lines(ctx, &model, lines.as_ptr(), lines.len() as u32, mode) };
        assert_eq!(code, SCG_ERR_INVALID_ARGUMENT, "mode de ligne {mode}");
        assert!(last_error(ctx).contains("depth mode"));

        // SAFETY: mêmes préconditions.
        let code =
            unsafe { scg_submit_points(ctx, &model, points.as_ptr(), points.len() as u32, mode) };
        assert_eq!(code, SCG_ERR_INVALID_ARGUMENT, "mode de point {mode}");
    }

    // Les deux valeurs publiées passent, pour les deux familles.
    for mode in [SCG_DEPTH_TESTED, SCG_DEPTH_ALWAYS] {
        // SAFETY: handle vivant, pointeurs locaux, comptes exacts.
        let code =
            unsafe { scg_submit_lines(ctx, &model, lines.as_ptr(), lines.len() as u32, mode) };
        assert_eq!(code, SCG_OK, "mode de ligne {mode}");
        // SAFETY: mêmes préconditions.
        let code =
            unsafe { scg_submit_points(ctx, &model, points.as_ptr(), points.len() as u32, mode) };
        assert_eq!(code, SCG_OK, "mode de point {mode}");
    }

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Les deux soumissions de tracé refusent un pointeur nul, et un handle nul.
///
/// Le contexte se vérifie avant tout le reste : l'enveloppe déréférencerait
/// pour rien si elle lisait la matrice d'abord.
#[test]
fn le_trace_refuse_les_pointeurs_nuls() {
    let ctx = create(&sane());
    let model = identite();
    let lines = [ligne()];

    // SAFETY: le handle nul est le cas que la fonction doit refuser.
    let code =
        unsafe { scg_submit_lines(ptr::null_mut(), &model, lines.as_ptr(), 1, SCG_DEPTH_ALWAYS) };
    assert_eq!(code, SCG_ERR_NULL);

    // SAFETY: handle vivant, matrice nulle — le cas à refuser.
    let code = unsafe { scg_submit_lines(ctx, ptr::null(), lines.as_ptr(), 1, SCG_DEPTH_ALWAYS) };
    assert_eq!(code, SCG_ERR_NULL);

    // SAFETY: handle vivant, matrice valide, tableau nul de longueur nulle —
    // un lot vide est licite et ne lit rien.
    let code = unsafe { scg_submit_lines(ctx, &model, ptr::null(), 0, SCG_DEPTH_ALWAYS) };
    assert_eq!(code, SCG_OK, "un lot vide est une scène, pas une faute");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// La capacité de tracé est **à elle** : l'épuiser ne dit rien des triangles,
/// et le message le nomme.
#[test]
fn le_trace_a_son_propre_budget() {
    let mut config = sane();
    config.max_lines = 2;
    let ctx = create(&config);
    let model = identite();
    let lines = [ligne(), ligne(), ligne()];

    // SAFETY: handle vivant, pointeurs locaux, comptes exacts.
    let code = unsafe { scg_submit_lines(ctx, &model, lines.as_ptr(), 3, SCG_DEPTH_ALWAYS) };
    assert_eq!(code, SCG_ERR_INVALID_ARGUMENT);
    assert!(last_error(ctx).contains("max_lines"));

    // Deux passent : le lot refusé n'a rien laissé derrière lui.
    // SAFETY: mêmes préconditions.
    let code = unsafe { scg_submit_lines(ctx, &model, lines.as_ptr(), 2, SCG_DEPTH_ALWAYS) };
    assert_eq!(code, SCG_OK);

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// Le tracé est refusé entre le début et la fin d'une image, comme toute
/// soumission : c'est un état du contexte que les tuiles lisent.
#[test]
fn le_trace_est_refuse_pendant_une_image() {
    let ctx = create(&sane());
    let model = identite();
    let lines = [ligne()];
    let mut tiles = 0u32;

    // SAFETY: handle vivant, paramètre de sortie local.
    assert_eq!(unsafe { scg_frame_begin(ctx, &mut tiles) }, SCG_OK);
    // SAFETY: handle vivant, pointeurs locaux.
    let code = unsafe { scg_submit_lines(ctx, &model, lines.as_ptr(), 1, SCG_DEPTH_ALWAYS) };
    assert_eq!(code, SCG_ERR_INVALID_STATE);

    let mut pixels = vec![0u8; 64 * 32 * 4];
    // SAFETY: handle vivant, tampon de la taille annoncée par le stride.
    let fin = unsafe { scg_frame_end(ctx, pixels.as_mut_ptr(), 64) };
    assert_eq!(fin, SCG_OK);
    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_destroy(ctx) };
}

/// La matrice identité des épreuves de tracé.
fn identite() -> ScgMat4 {
    ScgMat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    }
}

/// Une ligne devant la caméra par défaut, dans le champ.
fn ligne() -> ScgLine {
    ScgLine {
        ax: 4.0,
        ay: -1.0,
        az: 0.0,
        bx: 4.0,
        by: 1.0,
        bz: 0.0,
        r: 0xFF,
        g: 0xC0,
        b: 0x40,
        a: 0xFF,
    }
}

/// Un point devant la caméra par défaut.
fn point() -> ScgPoint {
    ScgPoint {
        x: 4.0,
        y: 0.0,
        z: 0.0,
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
        a: 0xFF,
    }
}
