// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Changer de carte sans quitter la fenêtre : la seule méthode que l'ABI offre,
//! démontrée. `M` bascule, Échap ferme.
//!
//! **Ce que `carte.rs` ne montre pas.** Celui-là charge un décor et le parcourt ;
//! il ne le remplace jamais. Or `docs/abi.md` arrête une méthode pour modifier
//! une carte — « en construire une autre à côté et basculer » —, décrit l'ordre
//! des gestes, le coût de la double occupation et la reprise du cache de
//! lightmaps, et **aucun exemple du dépôt n'en appelait une ligne**. Une méthode
//! décrite et jamais exercée est une méthode dont on découvre les surprises chez
//! l'intégrateur.
//!
//! # Les quatre gestes, dans cet ordre
//!
//! 1. **charger** la carte nouvelle, l'ancienne étant encore vivante ;
//! 2. **cuire** ses lightmaps, ou reprendre son cache si on en garde un ;
//! 3. **basculer** ce que la boucle soumet ;
//! 4. **relâcher** l'ancienne, ce que Rust fait au `drop` du `World`.
//!
//! **L'ordre n'est pas une commodité : la cuisson peut encore refuser.** Une
//! cellule dont les surfaces ne tiennent pas dans un atlas rend une erreur que le
//! chargement n'a pas vue — `docs/abi.md`, « Le plafond d'un atlas ». Un hôte qui
//! relâcherait d'abord se retrouverait sans décor du tout, et c'est pour cela que
//! les deux cartes coexistent le temps du geste.
//!
//! # Ce que le cache rend ici, et ce qu'il ne rend pas
//!
//! L'exemple garde le cache de chaque carte et le lui redonne au retour. Le badge
//! affiche le nombre d'entrées reprises, et il dit deux choses différentes :
//!
//! - **zéro** à la première visite d'une carte, et c'est normal — il n'y a rien à
//!   reprendre. Entre deux cartes **différentes**, un cache ne sert jamais à
//!   rien : l'empreinte hache la cellule, ses voisines et ses lumières en
//!   coordonnées de monde, donc aucune entrée ne concorde ;
//! - **toutes** au retour sur une carte déjà visitée, parce que c'est la même
//!   géométrie et les mêmes lampes. C'est le cas pour lequel le cache existe, et
//!   la différence entre les deux chiffres est ce qu'il fait gagner.
//!
//! La cuisson coûte deux à trois ordres de grandeur le chargement — `benches/cuisson.rs`
//! le mesure —, donc c'est elle qu'on évite en gardant un cache, jamais le
//! décodage.

use std::sync::Arc;

use screengine_play::{
    Affine3, FreeCamera, KeyCode, Lightmaps, Play, Texture, Vec3, World, load_png,
};

/// Les deux cartes versionnées du dépôt, celles que les cinq hôtes chargent.
///
/// Deux décors franchement différents — quatre cellules contre une enfilade —
/// pour que la bascule se voie sans lire le badge.
const CARTES: [&[u8]; 2] = [
    include_bytes!("../../../hosts/salles.world"),
    include_bytes!("../../../hosts/couloir.world"),
];

/// Les noms que le badge affiche, dans l'ordre de [`CARTES`].
const NOMS: [&str; 2] = ["salles", "couloir"];

/// Où la caméra se pose, dans l'une comme dans l'autre.
///
/// **Une position unique parce que les deux décors du dépôt la partagent**, et
/// c'est vérifié plutôt que supposé : un point légal dans une carte tombe en
/// général dans le mur d'une autre, et une caméra hors de toute cellule rendrait
/// une image vide que l'on prendrait pour un défaut de la bascule. Un hôte qui
/// change de carte pour de bon pose donc sa caméra par carte, ou relocalise.
const DEPART: Vec3 = Vec3::new(2.0, 2.0, 2.0);

/// La résolution interne, celle des autres exemples.
const RESOLUTION: (u32, u32) = (640, 360);

/// La texture des murs.
const MUR: &[u8] = include_bytes!("../assets/mur-mousse.png");

/// Celle du sol et du plafond.
const SOL: &[u8] = include_bytes!("../assets/sol-pave-mousse.png");

/// Ce qu'une carte chargée coûte à l'hôte de garder.
///
/// Les trois choses se remplacent **ensemble** à la bascule, et c'est pourquoi
/// elles vivent dans la même structure : une carte, ses textures dans l'ordre de
/// ses matériaux, et ses lightmaps. Les tenir séparément est exactement ce qui
/// permet d'en dépareiller deux — ce que la frontière refuse désormais, et que le
/// chemin Rust ne peut que borner.
struct Carte {
    /// Le décor chargé.
    world: World,
    /// Une texture par matériau, dans l'ordre que la carte déclare.
    materials: Vec<Arc<Texture>>,
    /// Les lightmaps, cuites ou reprises avant la première image.
    lightmaps: Lightmaps,
    /// La cellule où la caméra se trouve, que l'hôte suit lui-même.
    cell: u32,
    /// Combien d'entrées de cache la dernière reprise a acceptées.
    reprises: u32,
}

/// Charge une carte, l'habille, et prépare ses lightmaps.
///
/// `cache` est celui que cette carte avait laissé, s'il y en a un. Les entrées
/// qui concordent sont reprises, les autres écartées **sans erreur**, et ce qui
/// reste absent est cuit : c'est le patron que `docs/abi.md` décrit pour une
/// carte rechargée, et il vaut tel quel pour une carte qu'on retrouve.
fn charger(
    bytes: &[u8],
    mur: &Arc<Texture>,
    sol: &Arc<Texture>,
    depart: Vec3,
    cache: Option<&[u8]>,
) -> Carte {
    let world = World::load(bytes).expect("carte du dépôt valide");
    let materials = (0..world.material_count())
        .map(|rank| match world.material_name(rank) {
            Some("mur") => Arc::clone(mur),
            _ => Arc::clone(sol),
        })
        .collect();

    let mut lightmaps = Lightmaps::new(&world).expect("porteur");
    let reprises = match cache {
        // Le bloc vient de cette carte-ci, mais le moteur ne s'y fie pas : il
        // confronte chaque entrée à la carte chargée. C'est ce qui rend l'appel
        // sûr même sur un cache qu'on aurait gardé d'un décor modifié depuis.
        Some(bytes) => lightmaps.restore(&world, bytes).expect("cache lisible"),
        None => 0,
    };
    // Ce que la reprise n'a pas donné se cuit. Toutes les cellules avant la
    // première image, et non celles que la caméra voit : une lightmap est un
    // cache de la carte, pas de la vue.
    for index in 0..world.cell_count() {
        if let Some(id) = world.cell_id(index) {
            lightmaps.build(&world, id).expect("cuisson possible");
        }
    }

    let cell = world.locate(depart);
    Carte {
        world,
        materials,
        lightmaps,
        cell,
        reprises,
    }
}

/// Le cache d'une carte, prêt à être rangé par l'hôte.
///
/// Deux temps, comme à travers l'ABI : la longueur d'abord, le remplissage
/// ensuite. L'appel de mesure ne sérialise rien.
fn cache(carte: &Carte) -> Vec<u8> {
    let len = carte
        .lightmaps
        .save_len(&carte.world)
        .expect("longueur du cache");
    let mut bytes = vec![0u8; len];
    carte
        .lightmaps
        .save(&carte.world, &mut bytes)
        .expect("écriture du cache");
    bytes
}

/// Ce que la boucle garde entre deux images.
struct Scene {
    /// La caméra dirigée au clavier.
    camera: FreeCamera,
    /// La carte courante, avec tout ce qui en dépend.
    carte: Carte,
    /// Le rang de cette carte dans [`CARTES`].
    rang: usize,
    /// Le cache que chaque carte a laissé, ou rien si on ne l'a pas visitée.
    caches: [Option<Vec<u8>>; 2],
    /// Les deux textures, partagées par les deux cartes.
    textures: (Arc<Texture>, Arc<Texture>),
    /// Où la caméra était à l'image précédente, ce dont le suivi a besoin.
    previous: Vec3,
    /// La touche de bascule est-elle encore tenue depuis l'image d'avant ?
    ///
    /// **L'étage d'accueil ne donne que l'état d'une touche**, jamais son front :
    /// `pressed` reste vrai tant qu'on appuie, et une bascule branchée dessus
    /// rechargerait les deux cartes à chaque image. Le front se tient donc ici,
    /// et c'est le genre de détail qu'un exemple existe pour montrer.
    bascule_tenue: bool,
}

impl Scene {
    /// Passe à l'autre carte, en gardant le cache de celle qu'on quitte.
    ///
    /// **Les quatre gestes sont ici, et dans l'ordre.** L'ancienne carte vit
    /// jusqu'à la fin de cette fonction — elle est encore dans `self.carte`
    /// pendant que la nouvelle se charge et se cuit —, et elle n'est relâchée
    /// qu'à l'affectation, quand la nouvelle est prête. C'est ce que « charger à
    /// côté, puis basculer » veut dire, et le `drop` de Rust le rend implicite
    /// là où un hôte C appelle `scg_world_destroy`.
    fn basculer(&mut self) {
        self.caches[self.rang] = Some(cache(&self.carte));
        let rang = 1 - self.rang;
        let carte = charger(
            CARTES[rang],
            &self.textures.0,
            &self.textures.1,
            DEPART,
            self.caches[rang].as_deref(),
        );

        self.carte = carte;
        self.rang = rang;
        self.camera = FreeCamera::new(DEPART);
        self.previous = DEPART;
    }
}

/// Ouvre la fenêtre ; `M` bascule, Échap ferme.
fn main() -> Result<(), screengine_play::Error> {
    let mur = Arc::new(load_png(MUR)?);
    let sol = Arc::new(load_png(SOL)?);
    let carte = charger(CARTES[0], &mur, &sol, DEPART, None);

    let scene = Scene {
        camera: FreeCamera::new(DEPART),
        carte,
        rang: 0,
        caches: [None, None],
        textures: (mur, sol),
        previous: DEPART,
        bascule_tenue: false,
    };

    Play::new()
        .title("Screengine — changer de carte")
        .resolution(RESOLUTION.0, RESOLUTION.1)
        .stats(true)
        .run(
            scene,
            |scene, tick| {
                if tick.input().pressed(KeyCode::Escape) {
                    tick.exit();
                }
                let tenue = tick.input().pressed(KeyCode::KeyM);
                if tenue && !scene.bascule_tenue {
                    scene.basculer();
                }
                scene.bascule_tenue = tenue;

                scene.camera.update(tick);
                let position = scene.camera.camera().position;
                // Le moteur ne se relocalise jamais : le suivi rend zéro dès
                // qu'on sort, et c'est à l'hôte de reprendre le fil. Ici on
                // garde la dernière cellule connue plutôt que d'écraser avec
                // zéro, ce qui ferait disparaître le décor.
                let suivie = scene
                    .carte
                    .world
                    .track(scene.carte.cell, scene.previous, position);
                if suivie != 0 {
                    scene.carte.cell = suivie;
                }
                scene.previous = position;

                tick.set_title(&format!(
                    "Screengine — {} : {} cellules, {} entrées de cache reprises (M pour basculer)",
                    NOMS[scene.rang],
                    scene.carte.world.cell_count(),
                    scene.carte.reprises,
                ));
            },
            |scene, context| {
                let _ = context.set_camera(scene.camera.camera());
                let _ = context.submit_world_visible(
                    Affine3::IDENTITY,
                    &scene.carte.world,
                    scene.carte.cell,
                    Some(&scene.carte.lightmaps),
                    |material| scene.carte.materials.get(material as usize),
                );
            },
        )
}

/// Ce que la fenêtre montrerait, vérifié sans fenêtre.
///
/// **Le test vit dans l'exemple pour la raison de `hello.rs`** : il appelle
/// `charger` et `cache`, les fonctions mêmes que la bascule emploie, là où un
/// test rangé ailleurs aurait éprouvé sa propre copie de la séquence. Et il garde
/// ce qu'aucun contrôle du dépôt ne gardait — l'ordre des gestes d'un changement
/// de carte, qui n'était décrit que par la prose.
#[cfg(test)]
mod tests {
    use super::{CARTES, DEPART, cache, charger};
    use screengine::Texture;
    use std::sync::Arc;

    /// Une texture d'un texel, puisque le test ne regarde aucune image.
    fn plate() -> Arc<Texture> {
        Arc::new(Texture::load(1, 1, &[0xFF, 0xFF, 0xFF, 0xFF]).expect("texel valide"))
    }

    /// Un cache ne se reprend qu'entre deux chargements de la **même** carte.
    ///
    /// **C'est la clause que l'exemple existe pour montrer**, et elle se mesure
    /// par un compte : aucune entrée reprise d'une carte à l'autre, toutes au
    /// retour. Le premier zéro n'est pas un échec — l'empreinte hache la
    /// géométrie et les lampes en coordonnées de monde, et deux décors
    /// différents n'en partagent aucune.
    #[test]
    fn le_cache_ne_se_reprend_que_sur_sa_carte() {
        let (mur, sol) = (plate(), plate());
        let premiere = charger(CARTES[0], &mur, &sol, DEPART, None);
        assert_eq!(
            premiere.reprises, 0,
            "rien à reprendre à la première visite"
        );
        let garde = cache(&premiere);
        assert!(!garde.is_empty(), "une carte cuite laisse un cache");

        // Le cache de la première carte, donné à la seconde : chaque entrée est
        // confrontée à la carte chargée, et aucune ne concorde.
        let seconde = charger(CARTES[1], &mur, &sol, DEPART, Some(&garde));
        assert_eq!(
            seconde.reprises, 0,
            "deux cartes différentes ne partagent aucune empreinte"
        );

        // Le retour sur la première, avec son propre cache : tout est reprise,
        // et c'est le gain que le cache existe pour donner.
        let retour = charger(CARTES[0], &mur, &sol, DEPART, Some(&garde));
        assert_eq!(
            retour.reprises,
            premiere.world.cell_count(),
            "toutes les cellules se reprennent sur leur propre carte"
        );
    }

    /// Le départ tombe dans une cellule de **chacune** des deux cartes.
    ///
    /// **C'est la supposition que l'exemple fait, donc celle qu'il doit
    /// vérifier** : une position hors de toute cellule rend zéro, la traversée ne
    /// dessine rien, et la fenêtre serait vide sans qu'aucun appel n'échoue. Elle
    /// a d'ailleurs été fausse à l'écriture — deux positions inventées, dont une
    /// dans un mur —, et c'est ce cas qui l'a dit.
    #[test]
    fn chaque_depart_est_dans_une_cellule() {
        let (mur, sol) = (plate(), plate());
        for (rang, bytes) in CARTES.iter().enumerate() {
            let carte = charger(bytes, &mur, &sol, DEPART, None);
            assert_ne!(
                carte.cell, 0,
                "le départ de la carte {rang} est hors de toute cellule"
            );
        }
    }
}
