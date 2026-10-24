// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Zéro allocation par image, prouvé plutôt que relu.
//!
//! Un allocateur global compte les allocations du thread qui l'a armé. Le
//! contexte se crée librement — c'est un appel nommé —, puis aucune image ne
//! doit allouer, la première comprise : c'est là qu'un mipmap généré à la
//! demande ou un atlas agrandi se cacherait.
//!
//! Un binaire de test à part, parce qu'un allocateur global vaut pour tout le
//! binaire. Le compte est par thread, parce que le harnais de test alloue sur
//! les siens pendant que le test tourne.
//!
//! C'est le seul `unsafe` de la conformance : `GlobalAlloc` est un trait
//! `unsafe`, et il n'existe pas d'autre façon de voir une allocation.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use screengine::{
    Affine3, Angle, BYTES_PER_PIXEL, Camera, Color, Config, Context, DepthMode, Filter, Light,
    Lightmaps, Line, Mesh, Point, Quat, Rect, Rows, SimdPath, Sprite, SpriteOrientation, Texture,
    Triangle, Vec3, VertexUv, VertexUv2, World,
};

/// Le décor versionné que les hôtes parcourent, intégré à la compilation.
///
/// Celui du dépôt plutôt qu'un décor écrit ici : c'est lui que la traversée
/// rencontre en vrai, avec ses quatre cellules, son portail oblique et son
/// étage — un couloir d'une cellule ne remplirait ni la liste des visites ni
/// l'atlas de lightmaps, donc ne mesurerait rien.
const SALLES: &[u8] = include_bytes!("../../../hosts/salles.world");

/// Le quadrilatère de la scène de référence, resoumis à chaque image.
///
/// La soumission est mesurée avec le rendu : elle est un appel nommé, mais
/// c'est le contexte qui a réservé sa capacité à la création, et une liste de
/// dessin qui grandirait au premier lot vidé serait exactement le défaut que ce
/// fichier existe pour voir.
const VERTICES: [Vec3; 4] = [
    Vec3::new(2.0, 2.5, 1.6),
    Vec3::new(3.5, -2.5, 1.6),
    Vec3::new(3.5, -2.5, -1.6),
    Vec3::new(2.0, 2.5, -1.6),
];

/// Ses deux triangles, qui partagent l'arête des sommets 0 et 2.
const TRIANGLES: [Triangle; 2] = [
    Triangle {
        indices: [0, 2, 1],
        color: Color::new(0xE0, 0xA0, 0x30, 0xFF),
    },
    Triangle {
        indices: [0, 3, 2],
        color: Color::new(0xA0, 0xE0, 0x30, 0xFF),
    },
];

/// Le fichier de maillage que la mesure soumet : le même quadrilatère, chargé.
///
/// Les octets s'écrivent ici plutôt que par le constructeur de la scène : ce
/// binaire de test ne voit pas les modules du binaire de conformance, et un
/// quadrilatère est assez court pour que la recopie coûte moins qu'une
/// bibliothèque ouverte pour lui.
fn quad_mesh(frames: u32) -> Vec<u8> {
    // Deux sections : les poses animent, les coordonnées de texture non.
    let mut poses = frames.to_le_bytes().to_vec();
    let mut uvs = Vec::new();
    for (i, position) in VERTICES.iter().enumerate() {
        for value in [position.x, position.y, position.z] {
            poses.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32, 0.0, 1.0] {
            poses.extend_from_slice(&value.to_le_bytes());
        }
        for value in [((i & 1) * 64) as f32, ((i >> 1) * 64) as f32] {
            uvs.extend_from_slice(&value.to_le_bytes());
        }
    }
    // Les trames suivantes déplacent le quadrilatère : l'interpolation ne
    // s'éprouve pas entre deux poses identiques, que le moteur rendrait comme
    // la soumission statique.
    for frame in 1..frames {
        for position in VERTICES {
            for value in [position.x + frame as f32 * 0.25, position.y, position.z] {
                poses.extend_from_slice(&value.to_le_bytes());
            }
            for value in [0.0f32, 0.0, 1.0] {
                poses.extend_from_slice(&value.to_le_bytes());
            }
        }
    }

    let mut triangles = Vec::new();
    for triangle in TRIANGLES {
        for index in triangle.indices {
            triangles.extend_from_slice(&index.to_le_bytes());
        }
        let color = triangle.color;
        triangles.extend_from_slice(&[color.r, color.g, color.b, color.a]);
    }

    let mut groups = Vec::new();
    for value in [1u32, 0, TRIANGLES.len() as u32, 0] {
        groups.extend_from_slice(&value.to_le_bytes());
    }

    let mut names = 3u16.to_le_bytes().to_vec();
    names.extend_from_slice(b"mur");

    let sections = [
        (*b"FRMS", poses.as_slice()),
        (*b"SURF", groups.as_slice()),
        (*b"TEXN", names.as_slice()),
        (*b"TRIS", triangles.as_slice()),
        (*b"VTXS", uvs.as_slice()),
    ];
    let first = 20 + 12 * sections.len();
    let total = first + sections.iter().map(|(_, body)| body.len()).sum::<usize>();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"SCG\x1a");
    bytes.extend_from_slice(b"MESH");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(sections.len() as u32).to_le_bytes());

    let mut offset = first;
    for (tag, body) in &sections {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        offset += body.len();
    }
    for (_, body) in &sections {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// L'allocateur du système, qui compte au passage.
struct Counting;

thread_local! {
    /// Vrai pendant la mesure, sur le thread qui mesure.
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// Les allocations vues depuis que le thread a armé la mesure.
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Note une allocation si la mesure est armée sur ce thread.
///
/// `try_with` : pendant la destruction du stockage local d'un thread, l'accès
/// échouerait, et l'allocateur ne doit jamais paniquer.
fn note() {
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            COUNT.with(|count| count.set(count.get() + 1));
        }
    });
}

// SAFETY: chaque méthode délègue à `System` avec les arguments reçus, dont les
// préconditions sont celles de `GlobalAlloc` que l'appelant tient déjà ; le
// compte n'alloue rien et ne touche pas aux blocs.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: précondition de `GlobalAlloc::alloc`, transmise telle quelle.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: précondition de `GlobalAlloc::alloc_zeroed`, transmise telle quelle.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: précondition de `GlobalAlloc::realloc`, transmise telle quelle.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // **La libération compte**, et c'est une décision prise le 2026-10-20 :
        // l'invariant ne disait qu'« allocation », mais `free` prend le même
        // verrou que `malloc` et rendre plusieurs mégaoctets au début d'une
        // image coûte à cette image-là. Sans cette ligne, la moitié du coût
        // était hors de la mesure.
        note();
        // SAFETY: précondition de `GlobalAlloc::dealloc`, transmise telle quelle.
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// L'allocateur de ce binaire de test.
#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Compte les allocations de `f` sur le thread courant.
fn allocations(f: impl FnOnce()) -> usize {
    COUNT.with(|count| count.set(0));
    ARMED.with(|armed| armed.set(true));
    f();
    ARMED.with(|armed| armed.set(false));
    COUNT.with(Cell::get)
}

/// Le compteur voit une allocation : sans ce témoin, le test suivant passerait
/// aussi avec un allocateur qui ne compte rien.
#[test]
fn le_compteur_voit_une_allocation() {
    let seen = allocations(|| {
        std::hint::black_box(Vec::<u8>::with_capacity(64));
    });
    assert!(seen >= 1, "aucune allocation vue");
}

/// Le compteur voit une libération : même témoin que ci-dessus, pour la moitié
/// de la mesure que l'invariant a gagnée le 2026-10-20.
///
/// Sans lui, un `dealloc` qui cesserait de compter rendrait vertes toutes les
/// mesures de libération sans qu'aucune ne le dise — et c'est le mode de panne
/// qu'un instrument non vérifié a toujours.
#[test]
fn le_compteur_voit_une_liberation() {
    let seen = allocations(|| {
        drop(std::hint::black_box(Vec::<u8>::with_capacity(64)));
    });
    assert!(
        seen >= 2,
        "{seen} : l'allocation et sa libération attendues"
    );
}

/// Le compteur voit une réallocation.
///
/// **C'est le bras qui porte la classe de défaut que ce fichier existe pour
/// attraper** : une `Vec` qui grandit en cours d'image n'appelle ni `alloc` ni
/// `dealloc`, elle appelle `realloc`. Sans ce témoin, les mesures voisines
/// resteraient vertes alors que le seul bras qui les intéresse aurait cessé de
/// compter.
///
/// La capacité est prise hors mesure et la croissance demandée dedans, pour que
/// le compte soit exactement l'appel qu'on cherche. Un compte de deux désignerait
/// un allocateur qui alloue et recopie au lieu de réallouer : le témoin rougirait
/// en disant quoi chercher.
#[test]
fn le_compteur_voit_une_reallocation() {
    let mut buffer = Vec::<u8>::with_capacity(64);
    let seen = allocations(|| {
        buffer.reserve(4096);
    });
    std::hint::black_box(&buffer);
    assert_eq!(seen, 1, "{seen} : la réallocation attendue, et elle seule");
}

/// Le compteur voit une allocation mise à zéro.
///
/// Quatrième et dernier bras de `GlobalAlloc`. Un `vec![0u8; n]` l'emprunte par
/// la spécialisation que la bibliothèque standard réserve aux éléments nuls, et
/// c'est par là que passent les tampons du contexte : le tampon de couleur, le
/// z-buffer, les lignes de l'atlas.
///
/// L'égalité à un vaut preuve que ce bras compte **et** que c'est bien celui-là
/// qui est emprunté : par `alloc`, le compte serait le même, donc c'est la mise
/// à zéro retirée du compteur qui rougit ici, vérifiée en la retirant.
#[test]
fn le_compteur_voit_une_allocation_mise_a_zero() {
    let mut buffer = Vec::new();
    let seen = allocations(|| {
        buffer = vec![0u8; 4096];
    });
    std::hint::black_box(&buffer);
    assert_eq!(seen, 1, "{seen} : la mise à zéro attendue, et elle seule");
}

/// Une texture et toute sa chaîne de mipmaps tiennent dans une allocation, et
/// une seule.
///
/// Le chargement est un appel nommé : il a le droit d'allouer. Ce qu'on vérifie
/// est qu'il alloue **une fois**, donc que la chaîne entière est engendrée
/// maintenant. Un niveau produit plus tard, au premier affichage, n'est pas
/// visible dans le test des images tant qu'aucune n'échantillonne de texture —
/// mais il se verrait ici, en niveaux comptés séparément.
#[test]
fn une_texture_se_charge_en_une_seule_allocation() {
    let (width, height) = (64, 64);
    let pixels = vec![0x80u8; width as usize * height as usize * BYTES_PER_PIXEL];

    let mut texture = None;
    let seen = allocations(|| {
        texture = Some(Texture::load(width, height, &pixels).expect("texture valide"));
    });

    let texture = texture.expect("texture valide");
    assert_eq!(texture.level_count(), 7, "chaîne incomplète");
    assert_eq!(seen, 1, "{seen} allocation(s) pour une texture");
}

/// Aucune image texturée n'alloue non plus, dans les deux filtrages.
///
/// Le chemin sans texture ne suffit pas : `submit_textured` passe par la table
/// de textures du contexte, que `submit` ne touche jamais. Une table qui
/// grandirait au premier lot serait une allocation par image, et la mesure
/// voisine ne la verrait pas — alors que c'est le chemin que tout décor
/// emprunte depuis l'étape 2, et celui par lequel l'étape 3 a fait entrer les
/// lightmaps.
///
/// Les deux filtrages sont mesurés parce qu'ils ne lisent pas la texture de la
/// même façon : le bilinéaire prend quatre texels au lieu d'un, et rien ne dit
/// a priori qu'aucun des deux n'a besoin d'un tampon.
#[test]
fn aucune_image_texturee_n_alloue() {
    let (width, height) = (640, 360);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Hors mesure : charger une ressource est un appel nommé, qui alloue.
    let texture = Arc::new(
        Texture::load(64, 64, &vec![0x80u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );
    let vertices: Vec<VertexUv> = VERTICES
        .iter()
        .enumerate()
        .map(|(i, position)| VertexUv {
            position: *position,
            u: ((i & 1) * 64) as f32,
            v: ((i >> 1) * 64) as f32,
        })
        .collect();

    for filter in [Filter::Dither, Filter::Bilinear] {
        context.set_filter(filter).expect("hors image");
        let seen = allocations(|| {
            for _ in 0..3 {
                context
                    .submit_textured(Affine3::IDENTITY, &vertices, &TRIANGLES, &texture)
                    .expect("scène soumise");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        assert_eq!(seen, 0, "{seen} allocation(s) en {filter:?}");
    }
}

/// Une texture que l'hôte a relâchée entre deux images ne se libère pas dans
/// l'image suivante.
///
/// **C'est le cas que la décision du 2026-10-20 a rendu mesurable**, et il n'est
/// pas celui des mesures voisines : elles gardent leurs textures vivantes du
/// début à la fin, si bien que le vidage de la table du contexte ne relâche
/// jamais la dernière référence et qu'aucune libération n'a lieu. Ici l'hôte
/// détruit la sienne après l'image qui l'a soumise — ce que le contrat lui
/// permet, « détruire une texture qu'une image référence encore est sans effet
/// sur elle » —, donc le moteur tient seul les mégaoctets de la chaîne de
/// mipmaps, et c'est lui qui les rend.
///
/// Ce qu'il ne faut pas en conclure : que le contexte doive relâcher plus tôt.
/// La référence doit vivre jusqu'à la fin de l'image qui la soumet, le contrat
/// le promet. Ce qui est en jeu est **quand** elle meurt après cela, et la
/// réponse ne peut pas être « au début de l'image suivante ».
#[test]
fn une_texture_relachee_par_l_hote_ne_se_libere_pas_dans_l_image_suivante() {
    let (width, height) = (320, 180);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Hors mesure : le chargement est un appel nommé, et la chaîne de mipmaps
    // d'une texture de 256 pèse assez pour qu'une libération se voie.
    let texture = Arc::new(
        Texture::load(256, 256, &vec![0x80u8; 256 * 256 * BYTES_PER_PIXEL])
            .expect("texture valide"),
    );
    let vertices: Vec<VertexUv> = VERTICES
        .iter()
        .enumerate()
        .map(|(i, position)| VertexUv {
            position: *position,
            u: ((i & 1) * 64) as f32,
            v: ((i >> 1) * 64) as f32,
        })
        .collect();

    // L'image qui soumet la texture, puis l'hôte qui s'en défait : le contexte
    // reste alors seul à la tenir.
    context
        .submit_textured(Affine3::IDENTITY, &vertices, &TRIANGLES, &texture)
        .expect("scène soumise");
    context.frame_end(&mut pixels, width).expect("image rendue");
    drop(texture);

    // L'image suivante, mesurée : elle ne soumet rien et ne doit rien rendre à
    // l'allocateur.
    //
    // **Elle ne porte aucune géométrie, et ce n'est pas un raccourci** : une
    // image ne peut pas à la fois travailler et porter une libération en
    // attente, puisque la soumission qui lui donnerait de la géométrie appelle
    // le règlement et solde la libération avant qu'elle commence. C'est le seul
    // régime où la mesure a un sujet.
    let seen = allocations(|| {
        context.frame_end(&mut pixels, width).expect("image rendue");
    });
    assert_eq!(seen, 0, "{seen} mouvement(s) d'allocateur dans l'image");

    // **Et la libération a bien lieu, juste après.** Sans cette seconde moitié,
    // le zéro ci-dessus serait indistinguable d'un zéro obtenu parce qu'il n'y
    // avait rien à rendre : le cas passerait sur un moteur qui ne libère jamais,
    // comme sur un moteur qui libère au bon moment. Le premier réglage hors
    // image règle la table close, et c'est là que les mégaoctets partent.
    let apres = allocations(|| {
        context.set_filter(Filter::Dither).expect("hors image");
    });
    assert!(
        apres >= 1,
        "la libération n'a eu lieu nulle part : rien n'était en attente, et la mesure ci-dessus ne portait sur rien"
    );
}

/// Une soumission **refusée** pendant une image ne rend rien à l'allocateur.
///
/// C'est le régime que la mesure voisine déclarait impossible — « une image ne
/// peut pas à la fois travailler et porter une libération en attente » —, et il
/// l'était pour une soumission acceptée, qui solde la table avant que l'image
/// commence. Un appel **refusé** l'atteint : les deux soumissions de ressource
/// réglaient avant de vérifier l'état, si bien que les mégaoctets d'une chaîne
/// de mipmaps partaient entre le début et la fin d'une image, sur un appel que
/// le contexte refusait par ailleurs.
///
/// Le maillage est quelconque : ce qui est mesuré n'est pas son contenu mais
/// l'ordre de la façade, et un maillage vide est éprouvé par le noyau, où il
/// révèle le second défaut du même ordre — l'appel y passait tout court.
#[test]
fn une_soumission_refusee_pendant_une_image_ne_libere_rien() {
    let (width, height) = (320, 180);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Hors mesure, comme ci-dessus : une chaîne de 256 pèse assez pour qu'une
    // libération se voie, et l'habillage du maillage doit survivre à la mesure
    // pour que la fermeture ne soit pas ce qui la fait échouer.
    let texture = Arc::new(
        Texture::load(256, 256, &vec![0x80u8; 256 * 256 * BYTES_PER_PIXEL])
            .expect("texture valide"),
    );
    let habillage = Arc::new(
        Texture::load(64, 64, &vec![0x40u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );
    let mesh = Mesh::load(&quad_mesh(1)).expect("maillage valide");
    let vertices: Vec<VertexUv> = VERTICES
        .iter()
        .enumerate()
        .map(|(i, position)| VertexUv {
            position: *position,
            u: ((i & 1) * 64) as f32,
            v: ((i >> 1) * 64) as f32,
        })
        .collect();

    // L'image qui soumet la texture, puis l'hôte qui s'en défait : le contexte
    // tient seul la chaîne, et c'est elle qui est en attente de règlement.
    context
        .submit_textured(Affine3::IDENTITY, &vertices, &TRIANGLES, &texture)
        .expect("scène soumise");
    context.frame_end(&mut pixels, width).expect("image rendue");
    drop(texture);

    context.begin().expect("image commencée");
    let seen = allocations(|| {
        context
            .submit_mesh(Affine3::IDENTITY, &mesh, |_| Some(&habillage))
            .expect_err("une soumission pendant une image est refusée");
    });
    assert_eq!(seen, 0, "{seen} mouvement(s) d'allocateur dans l'image");

    // La même seconde moitié que la mesure voisine, et pour la même raison : le
    // zéro ci-dessus passerait aussi sur un moteur qui ne libère jamais.
    context.frame_end(&mut pixels, width).expect("image rendue");
    let apres = allocations(|| {
        context.set_filter(Filter::Dither).expect("hors image");
    });
    assert!(
        apres >= 1,
        "la libération n'a eu lieu nulle part : rien n'était en attente, et la mesure ci-dessus ne portait sur rien"
    );
}

/// Aucune image soumise depuis un maillage n'alloue.
///
/// Le maillage se charge **hors de la mesure** — un chargement est un appel
/// nommé, qui a le droit d'allouer — et se soumet **dedans**. C'est là que la
/// promesse se joue : la soumission lit un tableau de textures qu'un décodeur
/// naïf recopierait dans un tampon, et parcourt des groupes dont un chemin
/// distrait ferait une liste intermédiaire.
///
/// Avec texture puis sans, comme pour les lots éclairés : les deux chemins ne
/// touchent pas la même table.
#[test]
fn aucune_image_de_maillage_n_alloue() {
    let (width, height) = (640, 360);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Hors mesure : le chargement d'une ressource est un appel nommé.
    let mesh = Mesh::load(&quad_mesh(1)).expect("maillage valide");
    let texture = Arc::new(
        Texture::load(64, 64, &vec![0x80u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );

    for avec_texture in [true, false] {
        let seen = allocations(|| {
            for _ in 0..3 {
                context
                    .submit_mesh(Affine3::IDENTITY, &mesh, |_| {
                        avec_texture.then_some(&texture)
                    })
                    .expect("maillage soumis");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        assert_eq!(seen, 0, "{seen} allocation(s), texture : {avec_texture}");
    }
}

/// Aucune image n'alloue, la première comprise, une fois le contexte créé et le
/// tampon de l'hôte fourni.
#[test]
fn aucune_image_n_alloue() {
    let (width, height) = (640, 360);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    let seen = allocations(|| {
        for _ in 0..3 {
            context
                .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
                .expect("scène soumise");
            context.frame_end(&mut pixels, width).expect("image rendue");
        }
    });
    assert_eq!(seen, 0, "{seen} allocation(s) pendant trois images");
}

/// Changer de résolution sous le maximum n'alloue pas, aller comme retour.
///
/// C'est la clause que l'ABI promet et que seul ce test peut prouver : tout ce
/// que l'image consomme est dimensionné sur la résolution maximale, et la
/// grille se reconstruit dans de la capacité déjà réservée. Une seule de ces
/// deux réserves prise sur la résolution **courante** ferait réallouer au
/// premier agrandissement, chez un hôte qui ajuste sa résolution en cours de
/// partie — donc image après image.
///
/// Le tampon est alloué une fois pour le maximum et réutilisé : le
/// redimensionner dans la mesure ferait compter l'allocation du test.
#[test]
fn aucun_changement_de_resolution_n_alloue() {
    let (max_width, max_height) = (640u32, 360u32);
    let mut context = Context::new(Config {
        max_width,
        max_height,
        width: 320,
        height: 180,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; max_width as usize * max_height as usize * BYTES_PER_PIXEL];

    // Hors mesure : la première image de ce contexte, pour que rien de
    // paresseux ne vienne s'imputer au redimensionnement.
    context
        .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
        .expect("scène soumise");
    context.frame_end(&mut pixels, 320).expect("image rendue");

    let seen = allocations(|| {
        for (width, height) in [(640, 360), (320, 180), (480, 270), (640, 360)] {
            context
                .set_resolution(width, height)
                .expect("sous le maximum");
            context
                .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
                .expect("scène soumise");
            context.frame_end(&mut pixels, width).expect("image rendue");
        }
    });
    assert_eq!(seen, 0, "{seen} allocation(s) en changeant de résolution");
}

/// Aucun réglage reposé entre deux images n'alloue.
///
/// Les mesures voisines tournent à état figé : elles soumettent et rendent, et
/// aucune ne traverse les réglages qu'un hôte repose dans sa boucle — neuf
/// appels, dont seuls `set_lights` et `set_resolution` étaient mesurés. Deux des
/// sept autres remplissent une table de mille vingt-quatre entrées, le
/// brouillard et la courbe de sortie, et ce sont les deux dont une version qui
/// reconstruirait son `Vec` au lieu de le vider coûterait une image.
///
/// **`set_camera` est le premier de la liste, et pour une raison qui lui est
/// propre** : il refait la table du brouillard quand le plan proche a bougé,
/// donc c'est le seul chemin par lequel un remplissage de table peut arriver en
/// pleine boucle sans que l'hôte ait reposé de brouillard. Le brouillard est
/// donc réglé avant lui, et le plan proche change à chaque tour : sans cela, le
/// refait ne serait pas traversé.
#[test]
fn aucun_reglage_n_alloue() {
    let (width, height) = (320, 180);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Hors mesure : la première image de ce contexte, pour que rien de
    // paresseux ne vienne s'imputer aux réglages.
    context
        .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
        .expect("scène soumise");
    context.frame_end(&mut pixels, width).expect("image rendue");

    let seen = allocations(|| {
        for tour in 0..3 {
            context
                .set_fog(Color::new(0x40, 0x50, 0x60, 0xFF), 4.0, 24.0)
                .expect("hors image");
            context
                .set_camera(Camera {
                    position: Vec3::new(1.0, 0.0, 0.5),
                    orientation: Quat::IDENTITY,
                    fov_y: 1.0,
                    near: 0.125 + tour as f32 * 0.0625,
                })
                .expect("hors image");
            context
                .set_grade(2.2, [1.0, 0.9, 1.1], [0.0, 0.0, 0.0])
                .expect("hors image");
            context.set_overbright(1).expect("sous le maximum");
            context.set_filter(Filter::Bilinear).expect("hors image");
            context.set_simd(SimdPath::Scalar).expect("toujours portée");
            context
                .pixel_ray(width / 2, height / 2)
                .expect("dans l'image");
            context.clear_fog().expect("hors image");
            context.clear_grade().expect("hors image");

            context
                .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
                .expect("scène soumise");
            context.frame_end(&mut pixels, width).expect("image rendue");
        }
    });
    assert_eq!(seen, 0, "{seen} allocation(s) en reposant les réglages");
}

/// Des tuiles rendues sur d'autres threads n'allouent pas davantage : chaque
/// thread arme sa propre mesure autour de ses tuiles, puisque la création
/// des threads, elle, alloue.
#[test]
fn aucune_tuile_n_alloue_sur_un_autre_thread() {
    let (width, height) = (640u32, 360u32);
    let tile = 32;
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: tile,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];
    let columns = width.div_ceil(tile);
    let band = tile as usize * width as usize * BYTES_PER_PIXEL;

    for _ in 0..2 {
        let mut frame = None;
        let begun = allocations(|| {
            context
                .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
                .expect("scène soumise");
            frame = Some(context.frame_begin().expect("début"));
        });
        let frame = frame.expect("début");
        assert_eq!(
            begun, 0,
            "{begun} allocation(s) à la soumission ou au début"
        );

        let seen: usize = std::thread::scope(|scope| {
            let workers: Vec<_> = pixels
                .chunks_mut(band)
                .enumerate()
                .map(|(row, chunk)| {
                    let frame = &frame;
                    scope.spawn(move || {
                        let mut rows = Rows::band(chunk, width, row as u32 * tile);
                        allocations(|| {
                            for column in 0..columns {
                                frame
                                    .tile(row as u32 * columns + column, &mut rows)
                                    .expect("tuile");
                            }
                        })
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().expect("thread de rendu"))
                .sum()
        });
        assert_eq!(seen, 0, "{seen} allocation(s) dans les threads de rendu");
    }
}

/// Une image éclairée n'alloue pas davantage : lightmap **et** lumières
/// dynamiques.
///
/// Ce que l'étape 3 a ajouté au contexte — le tableau annexe des plans
/// d'éclairage et la liste des lumières placées — n'était écrit sous aucune
/// mesure. L'invariant tenait, les deux étant dimensionnés par `Context::new`,
/// mais rien ne le prouvait : c'est exactement la forme du défaut que ce
/// fichier existe pour voir, une réserve qui grandit au premier lot qui s'en
/// sert.
///
/// **`set_lights` est dans la mesure**, et c'est le point : la lightmap est une
/// ressource, donc son chargement est un appel nommé qui a le droit d'allouer,
/// alors que régler des lumières se fait entre deux images, autant de fois que
/// le jeu veut déplacer une torche.
#[test]
fn aucune_image_eclairee_n_alloue() {
    let (width, height) = (640, 360);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Hors mesure : deux ressources, donc deux appels nommés.
    let texture = Arc::new(
        Texture::load(64, 64, &vec![0x80u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );
    let lightmap = Arc::new(
        Texture::load(16, 16, &vec![0xC0u8; 16 * 16 * BYTES_PER_PIXEL]).expect("lightmap valide"),
    );

    let vertices: Vec<VertexUv2> = VERTICES
        .iter()
        .enumerate()
        .map(|(i, position)| VertexUv2 {
            position: *position,
            u: ((i & 1) * 64) as f32,
            v: ((i >> 1) * 64) as f32,
            u2: ((i & 1) * 16) as f32,
            v2: ((i >> 1) * 16) as f32,
            normal: Vec3::ZERO,
        })
        .collect();

    let lights = [Light {
        position: Vec3::new(2.5, 0.0, 0.5),
        radius: 8.0,
        color: Color::new(0xFF, 0xC0, 0x60, 0xFF),
    }];

    // Avec texture puis sans : un mur uni éclairé est le premier cas de
    // l'étape, et il ne passe pas par la table de textures.
    for avec_texture in [true, false] {
        let seen = allocations(|| {
            for _ in 0..3 {
                context.set_lights(&lights).expect("hors image");
                context
                    .submit_lit(
                        Affine3::IDENTITY,
                        &vertices,
                        &TRIANGLES,
                        avec_texture.then_some(&texture),
                        &lightmap,
                    )
                    .expect("scène soumise");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        assert_eq!(seen, 0, "{seen} allocation(s), texture : {avec_texture}");
    }
}

/// Aucune image n'alloue sur les chemins de l'étape 6 : quadrilatères orientés,
/// maillage interpolé, surface modulée.
///
/// **Aucun des trois n'était mesuré**, et chacun a de quoi allouer sans qu'on
/// le voie : le premier construit sa base et ses quatre sommets par sprite, le
/// deuxième mêle deux poses sommet par sommet, le troisième passe par une
/// seconde table de textures. Un tampon intermédiaire y serait invisible à la
/// lecture et ne coûterait qu'une image sur soixante.
#[test]
fn aucune_image_de_l_etape_6_n_alloue() {
    let (width, height) = (640, 360);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Hors mesure : trois ressources, donc trois appels nommés. La texture
    // masquée est du lot — c'est elle qui engendre sa chaîne de mipmaps, et un
    // niveau construit au premier affichage serait le défaut type.
    let mesh = Mesh::load(&quad_mesh(2)).expect("maillage valide");
    let texture = Arc::new(
        Texture::load(64, 64, &vec![0x80u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );
    let masked = Arc::new(
        Texture::load_masked(64, 64, &vec![0xC0u8; 64 * 64 * BYTES_PER_PIXEL])
            .expect("texture masquée valide"),
    );

    let sprites = [Sprite {
        center: Vec3::new(3.0, 0.0, 0.0),
        half_width: 0.6,
        half_height: 0.8,
        u0: 0.0,
        v0: 0.0,
        u1: 64.0,
        v1: 64.0,
        roll: Angle(1 << 29),
        color: Color::new(0xFF, 0xFF, 0xFF, 0xFF),
    }];
    let blended: Vec<VertexUv> = VERTICES
        .iter()
        .enumerate()
        .map(|(i, position)| VertexUv {
            position: *position,
            u: ((i & 1) * 64) as f32,
            v: ((i >> 1) * 64) as f32,
        })
        .collect();

    // Les deux orientations, parce que leurs bases ne se construisent pas de la
    // même façon, et un facteur d'interpolation strictement entre les deux
    // trames, le seul qui mêle vraiment les poses.
    for orientation in [SpriteOrientation::Axial, SpriteOrientation::Facing] {
        let seen = allocations(|| {
            for _ in 0..3 {
                context
                    .submit_mesh_frame(Affine3::IDENTITY, &mesh, |_| Some(&texture), 0, 1, 0.35)
                    .expect("maillage animé soumis");
                context
                    .submit_sprites(Affine3::IDENTITY, &sprites, Some(&masked), orientation)
                    .expect("quadrilatères soumis");
                context
                    .submit_blended(Affine3::IDENTITY, &blended, &TRIANGLES, Some(&texture))
                    .expect("surface modulée soumise");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        assert_eq!(
            seen, 0,
            "{seen} allocation(s), orientation : {orientation:?}"
        );
    }
}

/// Aucune image de traversée n'alloue, avec ses lightmaps et sur un vrai décor.
///
/// **C'est l'invariant le plus exposé de l'étape 5.** La traversée remplit une
/// liste de visites à chaque image, la soumission lit l'atlas de la cellule, et
/// `docs/rust.md` nomme lui-même l'atlas agrandi au premier affichage comme le
/// défaut type — section « Allocation ». La cuisson, elle, alloue et le dit :
/// elle est hors mesure, comme tout appel nommé.
///
/// Les cinq images ne partent pas de la même cellule : une seule ne remplirait
/// la liste des visites qu'une fois, et une liste qui grandirait au deuxième
/// décor visité ne se verrait pas.
#[test]
fn aucune_image_de_traversee_n_alloue() {
    let (width, height) = (640, 360);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Hors mesure : le décor, ses textures et la cuisson de toutes ses cellules.
    let world = Arc::new(World::load(SALLES).expect("décor versionné valide"));
    let texture = Arc::new(
        Texture::load(64, 64, &vec![0x80u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );
    let mut lightmaps = Lightmaps::new(Arc::clone(&world)).expect("atlas alloués");
    let mut cells = Vec::new();
    for index in 0..world.cell_count() {
        let id = world.cell_id(index).expect("rang dans le compte");
        lightmaps.build(id).expect("cuisson");
        cells.push(id);
    }

    for lit in [true, false] {
        let seen = allocations(|| {
            for &cell in &cells {
                context
                    .submit_world_visible(
                        Affine3::IDENTITY,
                        &world,
                        cell,
                        lit.then_some(&lightmaps),
                        |_| Some(&texture),
                    )
                    .expect("décor soumis");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        assert_eq!(seen, 0, "{seen} allocation(s), éclairé : {lit}");
    }
}

/// **Aucun balayage n'alloue, le premier compris.**
///
/// L'invariant est vrai aujourd'hui parce que rien du balayage ne retient quoi
/// que ce soit : la pile de cellules et l'ensemble des visitées sont des tableaux
/// de taille fixe sur la pile de l'appel, et l'intersection se calcule surface
/// par surface sans garder de liste de candidates. **C'est précisément le genre
/// d'invariant qui se perd au premier ajout** — un tampon de travail qu'on
/// croirait anodin —, et rien d'autre que ce test ne le verrait : un balayage qui
/// allouerait ne changerait aucune empreinte et ne ferait rougir aucun contrôle.
///
/// **Le premier balayage compte autant que les suivants**, comme la première
/// image : c'est là qu'une table dimensionnée à la demande se cacherait.
#[test]
fn aucun_balayage_n_alloue() {
    let world = World::load(SALLES).expect("décor du dépôt valide");

    // Le chargement alloue, et il en a le droit : c'est un appel nommé. La mesure
    // s'arme après lui.
    let half = Vec3::new(0.4, 0.4, 0.9);
    let from = Vec3::new(0.0, 0.0, 1.0);
    let cell = world.locate(from);
    assert_ne!(cell, 0, "le départ est dans une cellule");

    let seen = allocations(|| {
        for step in 0..8 {
            let reach = 1.0 + step as f32;
            for direction in [
                Vec3::new(reach, 0.0, 0.0),
                Vec3::new(-reach, 0.0, 0.0),
                Vec3::new(0.0, reach, 0.0),
                Vec3::new(0.0, 0.0, -reach),
            ] {
                let to = Vec3::new(
                    from.x + direction.x,
                    from.y + direction.y,
                    from.z + direction.z,
                );
                std::hint::black_box(world.sweep(cell, half, from, to));
            }
        }
    });
    assert_eq!(seen, 0, "{seen} allocation(s) pendant les balayages");
}

/// Le chemin de force brute n'alloue pas davantage.
///
/// Il visite toutes les cellules au lieu de suivre les portails, donc c'est lui
/// qui rencontrerait le plus de géométrie — et une liste qui grandirait avec le
/// nombre de surfaces s'y verrait en premier.
#[test]
fn aucun_balayage_de_force_brute_n_alloue() {
    let world = World::load(SALLES).expect("décor du dépôt valide");
    let half = Vec3::new(0.4, 0.4, 0.9);
    let from = Vec3::new(0.0, 0.0, 1.0);

    let seen = allocations(|| {
        for step in 0..8 {
            let reach = 1.0 + step as f32;
            std::hint::black_box(world.sweep_brute(
                half,
                from,
                Vec3::new(from.x + reach, from.y, from.z),
            ));
        }
    });
    assert_eq!(seen, 0, "{seen} allocation(s) pendant la force brute");
}

/// Aucune image qui trace n'alloue, et le redimensionnement n'y change rien.
///
/// **Les deux familles de tracé n'étaient sous aucune mesure.** Elles ont leur
/// propre répartition par tuiles, dont `build_bounds` redimensionne trois
/// tampons à chaque image : la capacité les couvre par construction — `refs` est
/// réservée pour `max_lines` fois le plafond de tuiles d'une primitive petite —
/// mais rien ne le prouvait, et une capacité qui cesserait de suffire ne se
/// verrait nulle part.
///
/// **La résolution baisse au milieu**, ce qu'aucune autre mesure ne fait : les
/// tampons de répartition sont dimensionnés sur le maximum, et une réservation
/// faite sur la résolution courante ne se verrait qu'après un changement — là où
/// un contexte qui ne change jamais de taille reste vert pour toujours.
///
/// Les deux modes de profondeur, parce qu'ils ne prennent pas le même chemin
/// dans la tuile, et trois images par mode, pour que la première ne soit pas la
/// seule mesurée.
#[test]
fn aucune_image_tracee_n_alloue() {
    let (width, height) = (640, 360);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Une brisure qui traverse l'image en biais : elle touche beaucoup de
    // tuiles, donc elle remplit la répartition plutôt que de l'effleurer.
    let lines: Vec<Line> = (0..64)
        .map(|i| {
            let t = i as f32;
            Line {
                a: Vec3::new(10.0, -4.0 + t * 0.1, -2.0),
                b: Vec3::new(10.0, 4.0 - t * 0.1, 2.0),
                color: Color::new(0xFF, 0xFF, 0xFF, 0xFF),
            }
        })
        .collect();
    let points: Vec<Point> = (0..64)
        .map(|i| Point {
            at: Vec3::new(10.0, -2.0 + i as f32 * 0.06, 0.0),
            color: Color::new(0xFF, 0x00, 0x00, 0xFF),
        })
        .collect();

    for depth in [DepthMode::Tested, DepthMode::Always] {
        let seen = allocations(|| {
            for _ in 0..3 {
                context
                    .submit_lines(Affine3::IDENTITY, &lines, depth)
                    .expect("brisure soumise");
                context
                    .submit_points(Affine3::IDENTITY, &points, depth)
                    .expect("points soumis");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        assert_eq!(seen, 0, "{seen} allocation(s) en {depth:?}");
    }

    // Sous le maximum : les tampons de répartition ne se redimensionnent que
    // vers le bas, et c'est le cas qu'aucune autre mesure ne joue.
    context
        .set_resolution(320, 180)
        .expect("sous le maximum, hors image");
    let seen = allocations(|| {
        for _ in 0..3 {
            context
                .submit_lines(Affine3::IDENTITY, &lines, DepthMode::Tested)
                .expect("brisure soumise");
            context.frame_end(&mut pixels, width).expect("image rendue");
        }
    });
    assert_eq!(seen, 0, "{seen} allocation(s) après redimensionnement");
}

/// La cuisson d'une cellule alloue par surface, jamais par rayon d'ombre.
///
/// **C'est un appel nommé : il a le droit d'allouer**, et la mesure ne lui
/// demande pas zéro. Ce qu'elle lui refuse est d'allouer *par rayon* — donc par
/// luxel, par échantillon et par lumière —, ce que faisait le contour de la
/// surface éclairée en se reconstruisant à chaque test d'occultation.
///
/// Le seuil est le nombre de luxels de la cellule, et il est choisi parce qu'il
/// sépare franchement les deux régimes : une cuisson qui alloue par surface
/// reste deux ordres de grandeur en dessous, une cuisson qui alloue par rayon le
/// dépasse d'autant. Un compte exact serait un chiffre à tenir à jour sans que
/// rien ne le vérifie.
#[test]
fn la_cuisson_n_alloue_pas_par_rayon() {
    let world = Arc::new(World::load(SALLES).expect("décor versionné valide"));
    let mut lightmaps = Lightmaps::new(Arc::clone(&world)).expect("atlas alloués");
    let id = world.cell_id(0).expect("le décor a au moins une cellule");
    let luxels = world.cell_luxel_count(id).expect("cellule connue") as usize;
    assert!(
        luxels > 0,
        "la cellule ne porte aucun luxel : la mesure ne porterait sur rien"
    );

    let seen = allocations(|| {
        lightmaps.build(id).expect("cuisson");
    });
    assert!(
        seen < luxels,
        "{seen} mouvement(s) d'allocateur pour {luxels} luxels : la cuisson alloue par rayon"
    );
}

/// La façade ombrée sur tranches n'alloue pas, et rien ne la mesurait.
///
/// **Les autres soumissions non mesurées partagent leur corps avec une façade
/// qui l'est** : `submit_uv` celui de `submit_textured`, les trois soumissions
/// de décor celui de `submit_world_visible`, et chaque forme `submit_each_*`
/// celui de sa jumelle sur tranche. **Mesuré plutôt que supposé** : une faute
/// placée dans `submit_each_shaded` fait aussi rougir le maillage et l'étape 6,
/// qui l'empruntent — ce qui n'était gardé par rien, c'est cette façade-ci, son
/// contrôle de finitude et l'assemblage de ses coins.
///
/// **Avec texture puis sans** : le second cas est celui qu'un pan uni éclairé
/// emprunte, et c'est aussi celui où le contrat impose une normale faute de
/// texture à moduler.
#[test]
fn aucune_image_ombree_n_alloue() {
    let (width, height) = (640, 360);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 64,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];

    // Hors mesure : les appels nommés allouent, c'est leur droit.
    let texture = Arc::new(
        Texture::load(64, 64, &vec![0x80u8; 64 * 64 * BYTES_PER_PIXEL]).expect("texture valide"),
    );
    let lights = [Light {
        position: Vec3::new(2.5, 0.0, 0.5),
        radius: 8.0,
        color: Color::new(0xFF, 0xC0, 0x60, 0xFF),
    }];
    context.set_lights(&lights).expect("hors image");

    // La normale pointe vers la caméra, qui regarde le +X depuis l'origine : un
    // pan éteint ne ferait travailler ni l'atténuation ni la saturation.
    let vertices: Vec<VertexUv2> = VERTICES
        .iter()
        .enumerate()
        .map(|(i, position)| VertexUv2 {
            position: *position,
            u: ((i & 1) * 64) as f32,
            v: ((i >> 1) * 64) as f32,
            u2: 0.0,
            v2: 0.0,
            normal: Vec3::new(-1.0, 0.0, 0.0),
        })
        .collect();

    for bound in [Some(&texture), None] {
        let seen = allocations(|| {
            for _ in 0..3 {
                context
                    .submit_shaded(Affine3::IDENTITY, &vertices, &TRIANGLES, bound)
                    .expect("scène soumise");
                context.frame_end(&mut pixels, width).expect("image rendue");
            }
        });
        let texturee = bound.is_some();
        assert_eq!(
            seen, 0,
            "{seen} allocation(s) pour trois images ombrées, texturée : {texturee}"
        );
    }
}

/// Le rendu par région n'alloue pas, et c'était le seul chemin sans mesure.
///
/// Les trois autres — le début d'image, la tuile et la fin — sont mesurés
/// ailleurs dans ce fichier. Celui-ci reçoit ses deux tampons de travail de
/// l'hôte, donc il n'a **rien** à prendre : une allocation ici serait un tampon
/// interne qu'on ne lui a pas demandé, et c'est exactement ce que l'invariant
/// interdit.
///
/// **Quatre régions désalignées plutôt qu'une sur l'image entière** : une seule
/// région pleine ne distingue pas un tampon dimensionné sur la région d'un
/// tampon dimensionné sur l'image, et c'est la première qui grandirait.
#[test]
fn aucun_rendu_par_region_n_alloue() {
    let (width, height) = (203, 150);
    let mut context = Context::new(Config {
        max_width: width,
        max_height: height,
        width,
        height,
        tile_size: 32,
        max_triangles: 0,
        max_lines: 0,
    })
    .expect("configuration valide");
    let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];
    let cells = width as usize * height as usize;
    let mut color = vec![0u32; cells];
    let mut depth = vec![0u32; cells];

    // Un pavage dont aucune découpe n'épouse un bord de tuile de 32.
    const DECOUPES: [(u32, u32, u32, u32); 4] = [
        (0, 0, 100, 70),
        (100, 0, 103, 70),
        (0, 70, 100, 80),
        (100, 70, 103, 80),
    ];

    let seen = allocations(|| {
        for _ in 0..3 {
            context
                .submit(Affine3::IDENTITY, &VERTICES, &TRIANGLES)
                .expect("scène soumise");
            let frame = context.frame_begin().expect("début");
            for (x, y, w, h) in DECOUPES {
                let rect = Rect {
                    x,
                    y,
                    width: w,
                    height: h,
                };
                let mut rows = Rows::band(&mut pixels, width, 0);
                frame
                    .region(rect, &mut color, &mut depth, &mut rows)
                    .expect("région rendue");
            }
            frame.end(&mut rows_of(&mut pixels, width)).expect("fin");
        }
    });
    assert_eq!(
        seen, 0,
        "{seen} allocation(s) pour douze régions et trois images"
    );
}

/// Le tampon de l'hôte vu comme des lignes, à l'origine de l'image.
///
/// Un emprunt séparé par appel : la région et la fin écrivent dans le même
/// tampon, et les garder tous deux vivants ne compilerait pas.
fn rows_of(pixels: &mut [u8], width: u32) -> Rows<'_> {
    Rows::band(pixels, width, 0)
}
