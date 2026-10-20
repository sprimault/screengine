// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Aucun paramètre de sortie n'est écrit sur un chemin d'erreur.
//!
//! **Le contrat l'énonce de toute l'ABI**, et il le fait en passant : pour
//! écarter l'idée de remplir `out_len` quand une capacité est trop courte, il
//! dit que ce serait « le seul paramètre de sortie de toute l'ABI écrit sur un
//! chemin d'erreur ». C'est donc une clause universelle, dont une liaison peut
//! dépendre — elle garde sa propre valeur dans le tampon qu'elle passe et la
//! relit sur un refus.
//!
//! **Une poignée de fonctions la tenaient par une assertion ; ce fichier la fait
//! porter par toutes**, et le compte de `APPELS` est tenu avec elles.
//! Les autres la tenaient par la forme de leur corps — valider d'abord, écrire
//! en dernier —, ce qui est la bonne forme et ne prouve rien : elle se perd au
//! premier point d'entrée écrit autrement, et le symptôme serait une liaison
//! qui lit une valeur que le moteur n'a pas voulu rendre.
//!
//! **Ce que ce fichier n'éprouve pas** : le refus tardif, celui d'un index au
//! delà de son compte ou d'une capacité trop courte, qui demande une ressource
//! valide. Celui-là se garde fonction par fonction, là où la ressource existe —
//! `mesh.rs`, `world.rs` et `sweep.rs` en portent plusieurs. Ici c'est le refus
//! que **tout** point d'entrée partage qui est passé en revue, sans exception.

use std::ffi::c_char;
use std::ptr;
use std::slice;

use screengine_ffi::*;

/// Le nombre d'appels que le passage en revue doit faire.
///
/// **Un compte attendu, pas un relevé** : un appel supprimé ou un point d'entrée
/// ajouté sans sa ligne ici laisseraient sinon le cas vert, et c'est exactement
/// ce que cette clause existe pour empêcher. Plus nombreux que les fonctions,
/// parce que six d'entre elles ont deux paramètres de sortie — un tampon et sa
/// longueur, ou deux identifiants —, chacun éprouvé à part.
const APPELS: usize = 47;

/// Le compteur des paramètres de sortie passés en revue.
struct Sorties {
    compte: usize,
}

impl Sorties {
    /// Un appel refusé laisse son paramètre de sortie exactement comme il l'a
    /// reçu.
    ///
    /// La comparaison porte sur les **octets** et non sur une égalité de valeurs :
    /// les structures de l'ABI ne dérivent pas `PartialEq`, et c'est l'absence
    /// d'écriture qu'on éprouve, pas une valeur attendue. Le code doit être
    /// **négatif** — un statut positif est un succès, et un succès n'a rien à
    /// faire ici.
    fn intact<T: Copy>(&mut self, nom: &str, sentinelle: T, appel: impl FnOnce(*mut T) -> i32) {
        let mut out = sentinelle;
        let code = appel(&mut out);
        assert!(code < 0, "{nom} : refus attendu, code rendu {code}");

        // SAFETY: les deux valeurs sont des `T` initialisés, vivants pendant la
        // comparaison, et lus en octets sans hypothèse sur leur disposition.
        let (ecrit, recu) = unsafe {
            let taille = size_of::<T>();
            (
                slice::from_raw_parts((&raw const out).cast::<u8>(), taille),
                slice::from_raw_parts((&raw const sentinelle).cast::<u8>(), taille),
            )
        };
        assert_eq!(ecrit, recu, "{nom} : le paramètre de sortie a été écrit");
        self.compte += 1;
    }
}

/// Un refus n'écrit aucun paramètre de sortie, sur tous les points d'entrée qui
/// en ont un.
///
/// Le refus choisi est celui qu'ils partagent tous et qui ne demande aucune
/// ressource : un handle ou un pointeur d'entrée nul, et pour les quatre
/// fonctions qui n'en prennent pas, un argument que leur contrat refuse.
#[test]
fn aucun_refus_n_ecrit_sa_sortie() {
    let mut v = Sorties { compte: 0 };
    let mut jetable = 0u32;
    let mut jetable_len = 0usize;
    let mut jetable_mots = [0u32; 4];
    let mut jetable_pixels = [0u8; 16];

    // SAFETY: un seul bloc pour tous les appels qui suivent, et non une phrase
    // par appel : la précondition est la même partout et tient par construction
    // — tout handle passé est nul, ce que chaque point d'entrée documente
    // accepter et refuser, et tout autre pointeur vise une locale vivante de ce
    // cas. Quarante-quatre fois la même phrase ne dirait rien de plus.
    unsafe {
        // La création : sans contexte, et c'est l'appel où un intégrateur se trompe
        // de configuration.
        v.intact("scg_create", ptr::null_mut::<ScgContext>(), |out| {
            scg_create(ptr::null(), out)
        });
        v.intact("scg_frame_begin", 0xdead_u32, |out| {
            scg_frame_begin(ptr::null_mut(), out)
        });
        // Les deux tampons de travail d'une région sont des paramètres de sortie
        // comme les autres — le moteur les écrit —, donc un refus n'y touche
        // pas. Ils s'éprouvent un par un, l'autre recevant un tampon jetable.
        v.intact("scg_frame_region color", [0xdead_u32; 4], |out| {
            scg_frame_region(
                ptr::null_mut(),
                0,
                0,
                2,
                2,
                out.cast::<u32>(),
                jetable_mots.as_mut_ptr(),
                4,
                jetable_pixels.as_mut_ptr(),
                2,
            )
        });
        v.intact("scg_frame_region depth", [0xdead_u32; 4], |out| {
            scg_frame_region(
                ptr::null_mut(),
                0,
                0,
                2,
                2,
                jetable_mots.as_mut_ptr(),
                out.cast::<u32>(),
                4,
                jetable_pixels.as_mut_ptr(),
                2,
            )
        });

        // Les chargements de ressources, qui rendent un handle.
        v.intact("scg_texture_load", ptr::null_mut::<ScgTexture>(), |out| {
            scg_texture_load(ptr::null(), ptr::null(), 0, out)
        });
        v.intact("scg_mesh_load", ptr::null_mut::<ScgMesh>(), |out| {
            scg_mesh_load(ptr::null(), 0, out)
        });
        v.intact("scg_world_load", ptr::null_mut::<ScgWorld>(), |out| {
            scg_world_load(ptr::null(), 0, out)
        });
        v.intact(
            "scg_lighting_create",
            ptr::null_mut::<ScgLighting>(),
            |out| scg_lighting_create(ptr::null(), out),
        );

        // Les accesseurs scalaires d'un maillage.
        v.intact("scg_mesh_texture_count", 0xdead_u32, |out| {
            scg_mesh_texture_count(ptr::null(), out)
        });
        v.intact("scg_mesh_triangle_count", 0xdead_u32, |out| {
            scg_mesh_triangle_count(ptr::null(), out)
        });
        v.intact("scg_mesh_frame_count", 0xdead_u32, |out| {
            scg_mesh_frame_count(ptr::null(), out)
        });
        v.intact("scg_mesh_bounds", [0xdead_u32 as f32; 6], |out| {
            scg_mesh_bounds(ptr::null(), 0, out.cast::<f32>())
        });
        v.intact("scg_mesh_texture_name buf", [0xaa_u8; 8], |out| {
            scg_mesh_texture_name(ptr::null(), 0, out.cast::<c_char>(), 8, &mut jetable_len)
        });
        v.intact("scg_mesh_texture_name out_len", usize::MAX, |out| {
            scg_mesh_texture_name(ptr::null(), 0, ptr::null_mut(), 0, out)
        });

        // Les accesseurs scalaires d'une carte.
        v.intact("scg_world_material_count", 0xdead_u32, |out| {
            scg_world_material_count(ptr::null(), out)
        });
        v.intact("scg_world_triangle_count", 0xdead_u32, |out| {
            scg_world_triangle_count(ptr::null(), out)
        });
        v.intact("scg_world_cell_count", 0xdead_u32, |out| {
            scg_world_cell_count(ptr::null(), out)
        });
        v.intact("scg_world_cell_id", 0xdead_u32, |out| {
            scg_world_cell_id(ptr::null(), 0, out)
        });
        v.intact("scg_world_cell_luxel_count", 0xdead_u32, |out| {
            scg_world_cell_luxel_count(ptr::null(), 0, out)
        });
        v.intact("scg_world_light_count", 0xdead_u32, |out| {
            scg_world_light_count(ptr::null(), out)
        });
        v.intact("scg_world_light_id", 0xdead_u32, |out| {
            scg_world_light_id(ptr::null(), 0, out)
        });
        v.intact("scg_world_material_id", 0xdead_u32, |out| {
            scg_world_material_id(ptr::null(), 0, out)
        });
        v.intact("scg_world_entity_count", 0xdead_u32, |out| {
            scg_world_entity_count(ptr::null(), out)
        });
        v.intact("scg_world_surface_material", 0xdead_u32, |out| {
            scg_world_surface_material(ptr::null(), 0, out)
        });
        v.intact("scg_world_material_name buf", [0xaa_u8; 8], |out| {
            scg_world_material_name(ptr::null(), 0, out.cast::<c_char>(), 8, &mut jetable_len)
        });
        v.intact("scg_world_material_name out_len", usize::MAX, |out| {
            scg_world_material_name(ptr::null(), 0, ptr::null_mut(), 0, out)
        });

        // Les entités : une structure que l'hôte possède pour la lumière, des
        // tableaux pour la pose, deux identifiants qui s'éprouvent un par un.
        v.intact("scg_world_light", light_sentinelle(), |out| {
            scg_world_light(ptr::null(), 0, out)
        });
        v.intact("scg_world_entity_pose", [0xdead_u32 as f32; 7], |out| {
            scg_world_entity_pose(ptr::null(), 0, out.cast::<f32>())
        });
        v.intact("scg_world_entity_ids id", 0xdead_u32, |out| {
            scg_world_entity_ids(ptr::null(), 0, out, &mut jetable)
        });
        v.intact("scg_world_entity_ids cell", 0xdead_u32, |out| {
            scg_world_entity_ids(ptr::null(), 0, &mut jetable, out)
        });
        v.intact("scg_world_entity_class buf", [0xaa_u8; 8], |out| {
            scg_world_entity_class(ptr::null(), 0, out.cast::<c_char>(), 8, &mut jetable_len)
        });
        v.intact("scg_world_entity_class out_len", usize::MAX, |out| {
            scg_world_entity_class(ptr::null(), 0, ptr::null_mut(), 0, out)
        });
        v.intact("scg_world_entity_data buf", [0xaa_u8; 8], |out| {
            scg_world_entity_data(ptr::null(), 0, out.cast::<u8>(), 8, &mut jetable_len)
        });
        v.intact("scg_world_entity_data out_len", usize::MAX, |out| {
            scg_world_entity_data(ptr::null(), 0, ptr::null_mut(), 0, out)
        });

        // Le suivi de la cellule de la caméra.
        v.intact("scg_world_locate", 0xdead_u32, |out| {
            scg_world_locate(ptr::null(), POSITION.as_ptr(), out)
        });
        v.intact("scg_world_track", 0xdead_u32, |out| {
            scg_world_track(ptr::null(), 1, POSITION.as_ptr(), POSITION.as_ptr(), out)
        });

        // Les lightmaps calculées et leur cache.
        v.intact("scg_lighting_state", 0xdead_u32, |out| {
            scg_lighting_state(ptr::null(), 0, out)
        });
        v.intact("scg_lighting_restore", 0xdead_u32, |out| {
            scg_lighting_restore(ptr::null_mut(), ptr::null(), 0, out)
        });
        v.intact("scg_lighting_save buf", [0xaa_u8; 8], |out| {
            scg_lighting_save(ptr::null(), out.cast::<u8>(), 8, &mut jetable_len)
        });
        v.intact("scg_lighting_save out_len", usize::MAX, |out| {
            scg_lighting_save(ptr::null(), ptr::null_mut(), 0, out)
        });

        // La collision et l'interrogation, qui remplissent la structure de l'hôte.
        v.intact("scg_world_sweep", hit_sentinelle(), |out| {
            scg_world_sweep(
                ptr::null(),
                1,
                POSITION.as_ptr(),
                POSITION.as_ptr(),
                POSITION.as_ptr(),
                out,
            )
        });
        v.intact("scg_world_sweep_brute", hit_sentinelle(), |out| {
            scg_world_sweep_brute(
                ptr::null(),
                POSITION.as_ptr(),
                POSITION.as_ptr(),
                POSITION.as_ptr(),
                out,
            )
        });
        v.intact("scg_world_pick", hit_sentinelle(), |out| {
            scg_world_pick(
                ptr::null(),
                1,
                POSITION.as_ptr(),
                POSITION.as_ptr(),
                SCG_PICK_SOLID,
                out,
            )
        });
        v.intact("scg_world_pick_brute", hit_sentinelle(), |out| {
            scg_world_pick_brute(
                ptr::null(),
                POSITION.as_ptr(),
                POSITION.as_ptr(),
                SCG_PICK_SOLID,
                out,
            )
        });

        // Les quatre qui ne prennent aucun handle : leur refus vient de ce que leur
        // contrat exige de leurs arguments, jamais d'un pointeur de ressource.
        v.intact("scg_sweep_skin", 0xdead_u32 as f32, |out| {
            scg_sweep_skin(NEGATIF.as_ptr(), out)
        });
        v.intact("scg_sweep_reach", 0xdead_u32 as f32, |out| {
            scg_sweep_reach(NEGATIF.as_ptr(), out)
        });
        v.intact("scg_lightmap_fault", 0xdead_u32, |out| {
            // Moins de trois coins : la cinquième clause du repère se tromperait
            // dans les deux sens, et le contrat refuse l'appel.
            scg_lightmap_fault(
                POSITION.as_ptr(),
                POSITION.as_ptr(),
                POSITION.as_ptr(),
                POSITION.as_ptr(),
                0,
                out,
            )
        });
    }

    assert_eq!(
        v.compte, APPELS,
        "le passage en revue ne couvre plus ce qu'il annonce"
    );
}

/// Trois flottants finis, ce que les fonctions sans handle attendent.
const POSITION: [f32; 3] = [0.0, 0.0, 0.0];

/// Une demi-étendue négative, que le balayage et ses deux mesures refusent.
const NEGATIF: [f32; 3] = [-1.0, 1.0, 1.0];

/// Une lumière dont chaque octet est reconnaissable.
///
/// Pas de `Default` : ce qu'on veut est une valeur qu'une écriture du moteur
/// déplacerait à coup sûr, et un champ laissé à zéro ne le dirait pas.
fn light_sentinelle() -> ScgLight {
    ScgLight {
        x: 1.0,
        y: 2.0,
        z: 3.0,
        radius: 4.0,
        r: 0xaa,
        g: 0xbb,
        b: 0xcc,
        _reserved: 0xdd,
    }
}

/// Un résultat de balayage dont chaque champ est reconnaissable, pour la même
/// raison.
fn hit_sentinelle() -> ScgSweepHit {
    ScgSweepHit {
        fraction: 0.5,
        normal: [1.0, 2.0, 3.0],
        point: [4.0, 5.0, 6.0],
        surface_id: 0xaaaa,
        cell_id: 0xbbbb,
        reserved0: 0xcccc,
        reserved1: 0xdddd,
    }
}
