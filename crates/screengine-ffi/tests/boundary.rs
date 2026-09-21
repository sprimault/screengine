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
        reserved1: 0,
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
    let mut config = sane();
    config.reserved1 = 1;

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
