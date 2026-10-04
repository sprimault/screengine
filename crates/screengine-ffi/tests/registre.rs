// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le registre flottant de l'hôte ne change pas un octet de ce que le moteur rend.
//!
//! **Ce que `boundary.rs` éprouve, et ce qu'il n'éprouve pas.** Il passe une fin
//! d'image sous registre hostile et vérifie deux choses : l'appel aboutit, et
//! l'hôte retrouve son registre. Rien, dans tout le dépôt, ne le faisait pour un
//! chargement de carte, un balayage ou une cuisson — alors que les trois calculent
//! en flottant hors du chemin d'image, et que toute valeur dérivée au chargement
//! entre ensuite dans le rendu.
//!
//! **Et ces trois-là peuvent être éprouvés plus fort qu'une image** : ils rendent
//! des valeurs comparables — un balayage rend des bits, une cuisson se sérialise
//! par `scg_lighting_save`. Le contrôle n'est donc pas « l'appel aboutit » mais
//! « le résultat est identique », ce qui est exactement ce que l'invariant promet
//! et ce qu'un aboutissement ne dit pas : un chargement qui dériverait d'autres
//! coordonnées sous arrondi dirigé aboutirait très bien.
//!
//! **Les trois cas ont été mesurés la garde retirée, et c'est ce qui a fixé le
//! décor.** Sur un cube droit de quatre unités, deux d'entre eux restaient verts
//! sans elle : des cotes en puissances de deux rendent produits, sommes et somme
//! de Newell exacts, et un arrondi n'a alors rien à mordre. Le décor porte donc
//! des cotes non représentables et une face oblique — ce qui n'est pas une
//! coquetterie de scène, mais la condition pour que ces cas puissent échouer.
//!
//! x86_64 seulement : `stmxcsr` n'a pas d'équivalent portable, et le chemin ARM
//! est éprouvé par l'hôte Android.

#![cfg(target_arch = "x86_64")]

use std::ffi::CStr;
use std::ptr;

use screengine_ffi::*;

/// Lit le message de l'emplacement par thread.
fn last_error() -> String {
    // SAFETY: le pointeur nul demande l'emplacement du thread, et le pointeur
    // rendu reste valide jusqu'au prochain appel — la copie a lieu avant.
    let text = unsafe { CStr::from_ptr(scg_last_error(ptr::null())) };
    text.to_str().expect("UTF-8 valide").to_owned()
}

/// Lit MXCSR sur le thread courant.
fn mxcsr() -> u32 {
    let mut control = 0u32;
    // SAFETY: `stmxcsr` écrit quatre octets dans un `u32` vivant et aligné.
    unsafe { std::arch::asm!("stmxcsr [{}]", in(reg) &mut control, options(nostack)) };
    control
}

/// Écrit MXCSR sur le thread courant.
fn set_mxcsr(control: u32) {
    // SAFETY: `ldmxcsr` relit un `u32` vivant et aligné, et toute valeur posée
    // par ce fichier dérive du registre lu juste avant.
    unsafe { std::arch::asm!("ldmxcsr [{}]", in(reg) &control, options(nostack)) };
}

/// Le registre hostile de ce fichier : arrondi vers le haut, DAZ et FZ,
/// exceptions **masquées**.
///
/// **Masquées, contrairement à celui de `boundary.rs`, et c'est le point.** Une
/// exception démasquée fait tomber le processus au premier calcul inexact :
/// mesuré avec elle, le cas redevient « l'appel aboutit », qui est exactement ce
/// que l'autre fichier couvre déjà. Ce qui est éprouvé ici est l'arrondi, la
/// seule hostilité qui change le résultat **sans rien signaler** — et la plus
/// probable en vrai, DAZ et FTZ étant posés par bien des bibliothèques audio là
/// où un hôte qui démasque les exceptions est rare.
fn hostile(default: u32) -> u32 {
    (default & !0x6000) | 0x1F80 | 0x4000 | 0x8040
}

/// Joue `body` sous registre hostile, et rend à l'hôte le registre qu'il avait.
///
/// Le registre est restauré **avant** l'assertion : un échec laisserait sinon le
/// thread en arrondi vers le haut, et les tests suivants mesureraient autre chose
/// que ce qu'ils annoncent.
fn under_hostile<T>(body: impl FnOnce() -> T) -> T {
    let default = mxcsr();
    let armed = hostile(default);
    set_mxcsr(armed);
    let out = body();
    let after = mxcsr();
    set_mxcsr(default);
    assert_eq!(after, armed, "le registre de l'hôte n'est pas rendu intact");
    out
}

/// Les octets d'une suite d'entiers.
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Les octets d'une suite de flottants.
fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Les octets d'une surface opaque, ses deux repères compris.
///
/// L'origine est nulle sur toutes les faces : le chargement exige que ses
/// projections sur les deux axes soient entières, et celles d'une origine nulle le
/// sont quelle que soit la direction des axes — ce qui laisse incliner une face
/// sans avoir à replacer son repère sur une grille.
fn surface(id: u32, indices: &[u32], u: [f32; 3], v: [f32; 3]) -> Vec<u8> {
    let mut frame = floats(&[0.0, 0.0, 0.0]);
    frame.extend_from_slice(&floats(&u));
    frame.extend_from_slice(&floats(&v));

    let mut bytes = words(&[id, 0, 1, indices.len() as u32]);
    bytes.extend_from_slice(&words(indices));
    bytes.extend_from_slice(&frame);
    bytes.extend_from_slice(&frame);
    bytes
}

/// Un volume fermé de quatre unités au toit incliné, un matériau, et sa lumière
/// dessous si `lit`.
///
/// **Fermé, et c'est la cuisson qui l'exige** : le signe du volume d'une cellule
/// vient de la somme sur ses faces, donc une cellule ouverte n'orienterait pas
/// ses normales. Les six faces portent leurs sommets dans l'ordre qui sort du
/// volume.
///
/// **Les cotes sortent de la grille des représentables, et c'est l'arrondi qui
/// l'exige.** Un cube droit de quatre unités a tout en puissances de deux : la
/// somme de Newell y est exacte, et comme c'est elle que le chargement garde et
/// que le balayage relit sans jamais la resommer, aucun mode d'arrondi ne la
/// change. Mesurés sur un tel décor, deux des trois cas passaient **la garde
/// d'environnement retirée**, verts pour n'avoir rien eu à arrondir. Une face
/// oblique seule n'y suffit pas : sur des cotes entières, sa normale brute est
/// exacte elle aussi.
///
/// Le volume sans lumière ne sert qu'au témoin de la cuisson : il donne le cache
/// auquel comparer celui du volume éclairé, et c'est leur écart qui dit que le
/// cache porte le calcul plutôt qu'un en-tête.
fn prism(lit: bool) -> Vec<u8> {
    let points: [[f32; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [3.7, 0.0, 0.0],
        [3.7, 4.3, 0.0],
        [0.0, 4.3, 0.0],
        [0.0, 0.0, 3.9],
        [3.7, 0.0, 1.1],
        [3.7, 4.3, 1.1],
        [0.0, 4.3, 3.9],
    ];
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    // Dans le plan du toit et perpendiculaire à `y`, donc un repère valide : c'est
    // le quart du vecteur `(3,7 ; 0 ; −2,8)` qui joint ses deux arêtes, et le quart
    // pour que la face porte quelques luxels plutôt qu'un seul.
    let slope = [0.925, 0.0, -0.7];
    let faces = [
        surface(11, &[0, 3, 2, 1], x, y),
        surface(12, &[4, 5, 6, 7], slope, y),
        surface(13, &[0, 1, 5, 4], x, z),
        surface(14, &[3, 7, 6, 2], x, z),
        surface(15, &[0, 4, 7, 3], y, z),
        surface(16, &[1, 2, 6, 5], y, z),
    ];

    let mut body = words(&[7, 0, points.len() as u32, faces.len() as u32, 0]);
    for point in &points {
        body.extend_from_slice(&floats(point));
    }
    for face in &faces {
        body.extend_from_slice(face);
    }
    let mut cells = words(&[body.len() as u32]);
    cells.extend_from_slice(&body);

    // La lumière est dans le volume, sous le toit : elle porte sur les six faces,
    // donc la cuisson écrit des luxels sur chacune. Posée dehors, elle les
    // laisserait noirs et le cache ne dirait plus rien du calcul.
    let mut lights = Vec::new();
    if lit {
        lights.extend_from_slice(&words(&[21]));
        lights.extend_from_slice(&floats(&[1.7, 2.3, 1.1, 9.0]));
        lights.extend_from_slice(&[0xC0, 0xB0, 0x90, 0x00]);
    }

    let mut mats = words(&[1]);
    mats.extend_from_slice(&3u16.to_le_bytes());
    mats.extend_from_slice(b"mur");

    file(&[(*b"CELL", &cells), (*b"LGTS", &lights), (*b"MATS", &mats)])
}

/// Un fichier de carte : en-tête, table de sections, sections.
///
/// Les genres se rangent par ordre croissant, ce que le décodeur vérifie avant de
/// lire un seul champ. Une section vide s'omet plutôt que de s'écrire creuse : le
/// volume sans lumière n'a pas de `LGTS`.
fn file(table: &[([u8; 4], &[u8])]) -> Vec<u8> {
    let sections: Vec<([u8; 4], &[u8])> = table
        .iter()
        .copied()
        .filter(|(_, body)| !body.is_empty())
        .collect();
    let sections = &sections[..];
    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"WRLD");
    bytes.extend_from_slice(&words(&[1, total as u32, sections.len() as u32]));

    let mut offset = first as u32;
    for (kind, body) in sections {
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(&words(&[offset, body.len() as u32]));
        offset += body.len() as u32;
    }
    for (_, body) in sections {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// Charge la carte et rend son handle.
fn load(bytes: &[u8]) -> *mut ScgWorld {
    let mut out = ptr::null_mut();
    // SAFETY: le bloc et la sortie sont des valeurs locales vivantes.
    let code = unsafe { scg_world_load(bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(code, SCG_OK, "chargement refusé : {}", last_error());
    out
}

/// Balaie le volume vers son toit et rend les bits du contact.
///
/// Les bits plutôt que les valeurs : deux zéros de signes opposés se comparent
/// égaux en flottant, et c'est précisément l'écart qu'un arrondi dirigé produit.
/// Oblique à dessein — un pas axial annule deux composantes de chaque produit
/// scalaire, donc il mesure moins d'arithmétique.
fn sweep_bits(world: *const ScgWorld) -> Vec<u8> {
    let mut hit = ScgSweepHit {
        fraction: -1.0,
        normal: [9.0; 3],
        point: [9.0; 3],
        surface_id: 999,
        cell_id: 999,
        reserved0: 0,
        reserved1: 0,
    };
    let half = [0.5f32; 3];
    let from = [1.0f32, 1.0, 1.0];
    // Vers le toit : c'est la seule face dont la normale a ses trois composantes
    // non nulles, donc celle dont le plan reconstruit arrondit le plus. Une paroi
    // axiale garde deux composantes exactement nulles.
    let to = [1.3f32, 2.2, 3.1];

    // SAFETY: carte vivante, trois tableaux de trois flottants, sortie
    // inscriptible.
    let code = unsafe {
        scg_world_sweep(
            world,
            7,
            half.as_ptr(),
            from.as_ptr(),
            to.as_ptr(),
            &mut hit,
        )
    };
    assert!(code >= 0, "balayage refusé : {}", last_error());

    // Le témoin de l'instrument : sans contact, la fraction vaut `1`, la normale
    // est nulle et il n'y a plus que des constantes à comparer — des cas verts qui
    // n'éprouvent rien. L'identifiant est celui du toit, et pas seulement un
    // identifiant quelconque : touchée ailleurs, la boîte heurte un plan exact.
    assert!(hit.fraction < 1.0, "le balayage ne touche rien");
    assert_eq!(hit.surface_id, 12, "le balayage ne touche pas le toit");

    let mut bits = hit.fraction.to_bits().to_le_bytes().to_vec();
    for value in hit.normal.iter().chain(hit.point.iter()) {
        bits.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    bits.extend_from_slice(&hit.surface_id.to_le_bytes());
    bits.extend_from_slice(&hit.cell_id.to_le_bytes());
    bits
}

/// Cuit la cellule du volume et rend les octets de son cache.
///
/// `scg_lighting_save` sérialise ce que le calcul a écrit, luxel par luxel :
/// c'est l'empreinte de la cuisson, et deux arrondis différents ne la rendent
/// pas. Seule la cuisson passe sous registre hostile quand `armed` — la création
/// et la sauvegarde restent saines, pour que ce qui est comparé soit le calcul.
fn bake_bytes(world: *const ScgWorld, armed: bool) -> Vec<u8> {
    let mut lighting = ptr::null_mut();
    // SAFETY: carte vivante, sortie inscriptible.
    let code = unsafe { scg_lighting_create(world, &mut lighting) };
    assert_eq!(code, SCG_OK, "création refusée : {}", last_error());

    // SAFETY: handle vivant, et 7 est l'identifiant de la cellule du volume.
    let build = || unsafe { scg_lighting_build(lighting, 7) };
    let built = if armed { under_hostile(build) } else { build() };
    assert_eq!(built, SCG_OK, "cuisson refusée : {}", last_error());

    let mut len = 0usize;
    // SAFETY: handle vivant ; tampon nul et capacité nulle demandent la mesure.
    let code = unsafe { scg_lighting_save(lighting, ptr::null_mut(), 0, &mut len) };
    assert_eq!(code, SCG_OK, "mesure refusée : {}", last_error());
    assert!(len > 0, "le cache d'une cellule cuite est vide");

    let mut block = vec![0u8; len];
    // SAFETY: le tampon porte exactement la longueur que la mesure a rendue.
    let code = unsafe { scg_lighting_save(lighting, block.as_mut_ptr(), len, &mut len) };
    assert_eq!(code, SCG_OK, "sauvegarde refusée : {}", last_error());

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_lighting_destroy(lighting) };
    block
}

/// Une carte chargée sous registre hostile rend les mêmes bits qu'une saine.
///
/// **C'est le chemin où un registre changé se verrait le moins, et où il
/// coûterait le plus** : le chargement somme la normale de Newell de chaque
/// surface et la garde telle quelle, et c'est elle que le balayage relit sans
/// jamais la resommer. Un arrondi dirigé au chargement déplace donc toutes les
/// collisions d'un hôte qui a armé son registre, sans qu'aucun code de retour le
/// dise — et le cache de lightmaps ne le périme pas, son empreinte ne hachant
/// aucune valeur dérivée.
#[test]
fn une_carte_chargee_sous_registre_hostile_rend_les_memes_bits() {
    let bytes = prism(true);
    let sane = load(&bytes);
    let expected = sweep_bits(sane);
    let baked = bake_bytes(sane, false);

    let armed = under_hostile(|| load(&bytes));
    // Balayage et cuisson se font tous deux sous registre **sain** : ce qui est
    // comparé est ce que le chargement a dérivé, pas ce qu'eux calculent.
    assert_eq!(
        sweep_bits(armed),
        expected,
        "le chargement a dérivé d'autres normales"
    );
    assert_eq!(
        bake_bytes(armed, false),
        baked,
        "le chargement a dérivé d'autres repères de lightmap"
    );

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_world_destroy(sane);
        scg_world_destroy(armed);
    }
}

/// Un balayage sous registre hostile rend les mêmes bits.
///
/// C'est le calcul le plus sensible à l'arrondi de tout ce que l'ABI expose hors
/// du pipeline d'image : une division par plan franchi, et une fraction que
/// l'hôte rejoue pour placer son mobile.
#[test]
fn un_balayage_sous_registre_hostile_rend_les_memes_bits() {
    let world = load(&prism(true));
    let expected = sweep_bits(world);
    let obtained = under_hostile(|| sweep_bits(world));

    assert_eq!(obtained, expected, "le balayage a rendu un autre contact");

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Une cuisson sous registre hostile écrit les mêmes luxels.
///
/// Le cache est comparé octet pour octet : il porte les luxels eux-mêmes et une
/// empreinte par cellule. Un sous-normal mis à zéro par DAZ dans l'atténuation s'y
/// voit, là où une image l'aurait noyé dans la quantification.
#[test]
fn une_cuisson_sous_registre_hostile_ecrit_les_memes_luxels() {
    let world = load(&prism(true));
    let expected = bake_bytes(world, false);
    let obtained = bake_bytes(world, true);

    assert_eq!(
        obtained, expected,
        "la cuisson a écrit d'autres luxels sous registre hostile"
    );

    // SAFETY: handle vivant, détruit une seule fois.
    unsafe { scg_world_destroy(world) };
}

/// Le témoin du cas ci-dessus : le cache porte bien ce que la lumière a produit.
///
/// Sans lui, une cuisson qui n'écrirait que des luxels noirs — lumière hors de
/// portée, repère refusé en silence, format changé — rendrait deux caches
/// identiques pour la seule raison qu'ils ne portent rien, et le cas passerait au
/// vert sans avoir mesuré un seul arrondi.
#[test]
fn le_cache_cuit_depend_de_la_lumiere() {
    let lit = load(&prism(true));
    let dark = load(&prism(false));

    assert_ne!(
        bake_bytes(lit, false),
        bake_bytes(dark, false),
        "le cache est le même avec et sans lumière : il ne porte pas la cuisson"
    );

    // SAFETY: handles vivants, détruits une seule fois.
    unsafe {
        scg_world_destroy(lit);
        scg_world_destroy(dark);
    }
}
