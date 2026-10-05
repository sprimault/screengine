// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Les balayages de la scène de collision, et leur empreinte.
//!
//! **Une scène d'interrogation, pas une scène d'image.** Elle ne rend aucun
//! pixel, donc elle n'a ni passe ni résolution : les six configurations de
//! découpage n'ont pas d'objet ici, et l'empreinte se calcule une fois.
//!
//! **La liste des balayages se dérive d'une règle, elle ne s'écrit pas en
//! table.** Une table serait une cinquième copie d'une liste, dans quatre
//! langages, le jour où les hôtes rendront cette empreinte ; une règle tient en
//! dix lignes partout. Le treillis et les directions sont donc figés ici, et ce
//! qu'ils produisent est entièrement déterminé par eux.
//!
//! **Les résultats se hachent par leurs bits**, `to_bits` et jamais une valeur
//! quantifiée : une empreinte quantifiée serait aveugle à l'écart d'un ULP entre
//! deux cibles ou deux chemins, qui est exactement ce qu'elle existe pour
//! attraper. Deux clauses en découlent, que le noyau tient : **jamais de `NaN`,
//! jamais de `-0,0`** dans un résultat, sans quoi deux résultats égaux
//! donneraient deux empreintes.
//!
//! **On ne hache que ce que la frontière publie**, et c'est la clause qui
//! commande toutes les autres : voir [`publish`].

use screengine::{Hit, Vec3, World, sweep_reach, sweep_skin};
use screengine_conformance::collision_file;

use crate::hash;

/// Les quatre codes que `scg_world_sweep` rend, tels que `docs/abi.md` les
/// publie.
///
/// **Recopiés plutôt que partagés**, et c'est la mesure elle-même qui les garde :
/// un écart entre ces valeurs et celles du header ferait diverger cette empreinte
/// de celles des cinq hôtes, qui hachent l'entier que la frontière leur rend
/// sans le traduire. C'est ce qui leur épargne un branchement, et ce qui rend la
/// recopie détectable au lieu d'être silencieuse.
pub const STATUS_OK: u8 = 0;
/// La région examinée a été tronquée par la borne de cellules.
pub const STATUS_INCOMPLETE: u8 = 1;
/// Le départ n'était dans aucune cellule.
pub const STATUS_NO_CELL: u8 = 2;
/// La boîte partait dans le solide.
pub const STATUS_START_SOLID: u8 = 3;
/// La boîte est trop petite, là où elle se déplace, pour garder un jeu.
pub const STATUS_NO_GAP: u8 = 4;

/// Le pas du treillis de départs, en unités de monde.
const STEP: f32 = 2.0;

/// De combien le treillis est décalé du bord d'une cellule.
///
/// **Un demi-pas, et c'est ce qui décide de l'utilité de la scène.** Un treillis
/// aligné sur les coins tombe sur les murs : le premier essai partait de `(0, 0)`
/// et rendait 440 départs dans le solide et 1 760 points hors de toute cellule
/// sur 2 400 — une empreinte qui n'éprouvait presque rien, et qui se serait figée
/// aussi bien qu'une autre. C'est le rapport texte qui l'a montré, pas
/// l'empreinte.
const INSET: f32 = 1.0;

/// La longueur d'un balayage, en unités de monde.
///
/// Assez pour traverser le couloir de part en part depuis la salle, ce qui est le
/// seul cas où la traversée franchit un portail.
pub const REACH: f32 = 10.0;

/// Les demi-étendues des deux boîtes d'épreuve.
///
/// La première tient dans le couloir, la seconde non : c'est le couple qui rend
/// la constante de dilatation **mesurable** — l'une doit passer, l'autre non, et
/// une marge mal réglée casse l'un des deux.
///
/// **Ni l'une ni l'autre ne vaut exactement la demi-largeur du couloir**, et
/// c'est le résultat de la mesure, pas un contournement. Une boîte d'exactement
/// une unité dans un couloir d'exactement une unité touche ses deux murs dès
/// qu'elle est dilatée : elle part donc dans le solide et n'entre jamais. La
/// dilatation coûte cela, et le décor le dit — la petite passe, la grande non.
const HALVES: [f32; 2] = [0.45, 0.55];

/// Les dix directions balayées, normalisées par construction du treillis.
///
/// Les six axes, puis les quatre diagonales horizontales : ce sont elles qui font
/// travailler les prismes d'arêtes, qu'un balayage aligné sur un axe ne rencontre
/// jamais de biais.
pub const DIRECTIONS: [[f32; 3]; 10] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [-1.0, -1.0, 0.0],
];

/// Un balayage à jouer : sa boîte, son départ, son arrivée.
pub struct Sweep {
    /// Les demi-étendues de la boîte.
    pub half: Vec3,
    /// Le point de départ.
    pub from: Vec3,
    /// Le point d'arrivée.
    pub to: Vec3,
}

/// Tous les balayages de la scène, dans l'ordre où l'empreinte les prend.
///
/// L'ordre est contractuel : c'est lui qui fait qu'une empreinte se compare, et
/// le changer déplacerait la référence sans que rien n'ait bougé du moteur.
pub fn all() -> Vec<Sweep> {
    let starts = starts();

    let mut sweeps = Vec::new();
    for half in HALVES {
        let half = Vec3::new(half, half, half);
        for from in &starts {
            for direction in DIRECTIONS {
                sweeps.push(Sweep {
                    half,
                    from: *from,
                    to: Vec3::new(
                        from.x + direction[0] * REACH,
                        from.y + direction[1] * REACH,
                        from.z + direction[2] * REACH,
                    ),
                });
            }
        }
    }
    sweeps.push(saturating());
    sweeps
}

/// Le trajet qui épuise le budget de cellules du balayage.
///
/// **En queue de la liste, et c'est ce qui rend l'ajout lisible** : les cas du
/// treillis gardent leur rang, donc leurs octets, et la référence ne se déplace
/// que de l'enregistrement ajouté.
///
/// Il traverse l'enfilade de bout en bout, là où la portée du treillis vaut dix
/// unités — trop court pour toucher plus de quelques cellules. Ce que l'empreinte
/// retient de lui est le **statut**, `SCG_STATUS_INCOMPLETE`, qu'aucun décor
/// versionné n'atteignait et qu'aucun hôte n'avait donc jamais reçu.
fn saturating() -> Sweep {
    let entry = collision_file::chain_entry();
    let exit = collision_file::chain_exit();
    Sweep {
        // La boîte tient largement dans un cube de l'enfilade : ce qui sature est
        // le nombre de cellules que le trajet touche, jamais la taille de la
        // boîte.
        half: Vec3::new(0.5, 0.5, 0.5),
        from: Vec3::new(entry[0], entry[1], entry[2]),
        to: Vec3::new(exit[0], exit[1], exit[2]),
    }
}

/// Les départs du treillis, partagés avec la scène d'interrogation.
///
/// **Le même treillis pour les deux scènes**, et non deux règles voisines : ce
/// qui fait l'intérêt de ces départs — ils tombent dans les cellules et non dans
/// les murs — vaut autant pour un rayon que pour une boîte, et deux règles à
/// tenir d'accord auraient fini par diverger sur le décalage d'un demi-pas, qui
/// est précisément ce qui décide de leur utilité.
/// Les départs **choisis**, que le treillis ne peut pas produire.
///
/// **Un treillis régulier rate les bandes étroites**, et deux l'ont montré. Son
/// pas vaut deux unités, là où les cas qui comptent se mesurent en fractions
/// d'unité : rien ne garantit qu'un départ y tombe, et une empreinte prise sans en
/// avoir un ne garde rien du tout.
///
/// Les deux bandes, et ce que chacune éprouve :
///
/// - **le long de l'arête amont de la rampe**, à une demi-extension du haut : le
///   domaine d'une face obliquese décalait de cette largeur, si bien qu'une boîte
///   y traversait le sol. Les cotes suivent la pente, puisque le sol monte ;
/// - **dans la bande de dilatation d'un sol plat**, à une fraction de la marge
///   au-dessus de lui : la vraie boîte n'y touche rien et la boîte dilatée y
///   touche déjà, ce qui est exactement l'état qu'un balayage précédent laisse
///   derrière lui. C'est la bande que la doctrine de test disait non couverte.
fn chosen() -> Vec<Vec3> {
    let mut chosen = Vec::new();
    // La rampe monte d'une unité par unité : son sol est à la cote de `y`.
    for tenths in [60u32, 65, 70, 72, 75] {
        let y = f32::from(tenths as u16) / 10.0;
        let floor = collision_file::FLOOR_Z + y * collision_file::RAMP_RISE;
        // L'abscisse suit l'origine du décor : écrite en dur, elle tomberait hors
        // cellule et le cas de la face oblique se perdrait sans rien signaler.
        chosen.push(Vec3::new(collision_file::ORIGIN_X + 28.0, y, floor + 1.0));
    }
    // **Posés au sol et face au portail**, et les deux conditions comptent : la
    // bande de dilatation seule ne dit rien si rien n'y franchit de seuil, et
    // c'est l'arête du seuil qui portait un prisme faute de voir la cellule d'en
    // face. Les deux boîtes de la scène ont des marges différentes, donc les deux
    // cotes servent, et la troisième sort de la bande pour témoin.
    for lift in [0.0002f32, 0.0005, 0.002] {
        let x = collision_file::ORIGIN_X + 6.0;
        chosen.push(Vec3::new(x, 2.5, collision_file::FLOOR_Z + 0.45 + lift));
        chosen.push(Vec3::new(x, 2.5, collision_file::FLOOR_Z + 0.55 + lift));
    }
    // **Posés au contact d'un mur du couloir**, à la distance exacte que le
    // balayage rend : `demi-étendue + marge`. C'est la seule pose qui éprouve ce
    // qu'un mobile fait en permanence — suivre une paroi —, et le treillis ne
    // peut pas la produire, son pas valant deux unités là où la marge se mesure
    // en dixièmes de millième.
    //
    // Les deux directions qui longent le mur portent le cas : elles mènent la
    // boîte jusqu'au **bout du panneau**, là où le couloir débouche, et c'est
    // l'arête qui le termine qui l'arrêtait. Les huit autres directions viennent
    // avec, et deux d'entre elles entrent dans le mur — ce qui garde au départ sa
    // valeur de témoin.
    //
    // **Aux trois quarts de la marge, et pas à la marge pleine.** Posée pile à la
    // distance de contact, la boîte tombe du bon côté du bord quoi qu'il arrive et
    // le cas reste vert sur le code fautif : il ne garderait rien. Ce qui le rend
    // discriminant, c'est de viser la **bande** — entre la demi-marge, où le bord
    // s'arrête désormais, et la marge, où il s'arrêtait. C'est là qu'un balayage
    // précédent laisse le mobile, à l'arrondi de la fraction près.
    //
    // **Une seule pose, celle de la petite boîte**, et c'est mesuré : le couloir
    // n'a qu'une unité de large, si bien qu'une pose au contact pour la grande
    // place la petite contre le mur d'en face. Les deux y partent dans le solide,
    // et vingt balayages n'y disent rien que le décor ne dise déjà. La grande joue
    // tout de même celle-ci et y part solide, ce qui est le témoin que le décor
    // porte déjà : la petite passe dans le couloir, la grande non.
    {
        let half = HALVES[0];
        chosen.push(Vec3::new(
            collision_file::ORIGIN_X + 12.0,
            collision_file::CORRIDOR[0][1] + half + half * 3.0 / 4096.0,
            collision_file::FLOOR_Z + 2.0,
        ));
    }
    // **Un départ par marche de la cage**, au milieu du plat et à une unité
    // au-dessus. Le treillis ne l'atteint pas : il tire ses cotes entre le sol et
    // le plafond des cellules basses, quand la cage monte sur deux étages — et une
    // cellule qu'aucun départ ne touche est une géométrie que l'empreinte ne voit
    // pas, ce que ce décor a déjà laissé passer une fois.
    //
    // Chacun joue les dix directions et les deux boîtes : la descente éprouve le
    // plat, l'horizontale la contremarche, et les obliques le nez de marche —
    // l'arête saillante qui garde son prisme là où une arête rentrante le perd.
    for step in 0..collision_file::STEPS {
        let along = step as f32;
        chosen.push(Vec3::new(
            collision_file::ORIGIN_X + along + 0.5,
            collision_file::stair_middle(),
            collision_file::FLOOR_Z + along + 1.0,
        ));
    }
    chosen
}

pub fn starts() -> Vec<Vec3> {
    let mut starts = Vec::new();
    // **Un treillis par cellule, dans sa propre boîte englobante**, plutôt qu'un
    // treillis unique sur le décor entier : le couloir ne fait qu'une unité de
    // large, et un pas de deux unités sur une boîte commune ne serait jamais
    // tombé dedans. La règle suit la carte, et les deux ne peuvent pas dériver.
    for footprint in [
        &collision_file::ROOM[..],
        &collision_file::CORRIDOR[..],
        &collision_file::BRANCHES[..],
    ] {
        let (low, high) = bounds(footprint);
        for z in axis(collision_file::FLOOR_Z, collision_file::CEILING_Z) {
            for y in axis(low[1], high[1]) {
                for x in axis(low[0], high[0]) {
                    starts.push(Vec3::new(x, y, z));
                }
            }
        }
    }
    // La rampe n'entre pas dans la boucle ci-dessus : son sol monte, donc une cote
    // commune y tomberait sous le sol d'un bout et sous le plafond de l'autre. Ses
    // départs sont choisis, et ils suivent la pente.
    starts.extend(chosen());
    starts
}

/// La magie du fichier de balayages, en tête de celui-ci.
///
/// **Elle ne précède aucun numéro de version**, et c'est délibéré : ce n'est pas
/// un format du moteur, qui ne le lit jamais, mais un fichier d'épreuve versionné
/// à côté des hôtes qui le lisent et comparé octet pour octet par un test. Une
/// version sert à négocier une évolution entre deux artefacts livrés séparément,
/// ce qui n'arrive pas quand le fichier et ses lecteurs sont dans le même commit.
/// La magie, elle, reste utile : elle fait refuser franchement un mauvais chemin
/// passé par le `Makefile`, au lieu de hacher des ordures.
const MAGIC: &[u8; 8] = b"SCGSWEEP";

/// La liste des balayages, telle que les hôtes la lisent.
///
/// **Ils lisent une liste, ils ne reportent pas la règle.** Quatre treillis
/// écrits dans chaque langage prouveraient qu'autant de programmeurs ont su
/// reporter la même géométrie, ce qui n'est pas ce qu'une empreinte d'hôte existe
/// pour établir. Un cinquième hôte n'a ainsi que sa boucle d'appel à écrire.
///
/// Neuf flottants par balayage, octet de poids faible en tête, dans l'ordre où
/// [`all`] les rend.
pub fn file_bytes() -> Vec<u8> {
    let sweeps = all();
    let mut bytes = Vec::with_capacity(MAGIC.len() + 4 + sweeps.len() * 36);

    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(sweeps.len() as u32).to_le_bytes());
    for sweep in &sweeps {
        for vector in [sweep.half, sweep.from, sweep.to] {
            for value in [vector.x, vector.y, vector.z] {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
    }
    bytes
}

/// Les positions de départ sur un axe, retirées des deux bords.
///
/// **Un seul point, au centre, quand l'étendue est trop étroite pour le
/// retrait.** Le couloir fait une unité de large : un treillis qui exigerait une
/// unité de marge de chaque côté n'y aurait placé aucun départ, et la scène
/// n'aurait jamais balayé la seule cellule qu'elle a construite pour cela.
fn axis(low: f32, high: f32) -> Vec<f32> {
    if high - low <= 2.0 * INSET {
        return vec![(low + high) * 0.5];
    }
    let mut values = Vec::new();
    let mut value = low + INSET;
    while value < high - INSET * 0.5 {
        values.push(value);
        value += STEP;
    }
    values
}

/// Les deux coins de la boîte d'une empreinte, par comparaisons écrites.
fn bounds(footprint: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    let mut low = [f32::MAX; 2];
    let mut high = [f32::MIN; 2];
    for point in footprint {
        for axis in 0..2 {
            if point[axis] < low[axis] {
                low[axis] = point[axis];
            }
            if point[axis] > high[axis] {
                high[axis] = point[axis];
            }
        }
    }
    (low, high)
}

/// Joue tous les balayages et rend leur empreinte.
///
/// **Chaque balayage est joué deux fois**, par la traversée et par le chemin de
/// force brute, et ce qu'ils se doivent est dans [`agree`] : l'égalité au bit
/// près tant que la traversée a tout examiné, le conservatisme de sa fraction
/// quand elle annonce avoir tronqué. C'est le théorème de l'étape, l'analogue de
/// l'égalité entre la traversée de rendu et son chemin brut : il n'attrape pas
/// une fenêtre trop large, et c'est normal — une traversée trop large ne change
/// pas le résultat.
pub fn digest() -> Result<u64, String> {
    let world = World::load(&collision_file::bytes())
        .map_err(|error| format!("collision : le décor est refusé : {error:?}"))?;

    let mut bytes = Vec::new();
    for (index, sweep) in all().into_iter().enumerate() {
        let cell = world.locate(sweep.from);
        let fast = if cell == 0 {
            None
        } else {
            world.sweep(cell, sweep.half, sweep.from, sweep.to)
        };
        let slow = world.sweep_brute(sweep.half, sweep.from, sweep.to);

        if let Some(fast) = &fast
            && let Err(cause) = agree(fast, &slow)
        {
            return Err(format!(
                "collision : balayage {index}, {cause}\n  \
                 traversée {fast:?}\n  brute     {slow:?}"
            ));
        }

        // Le départ sans cellule entre dans l'empreinte comme les autres : c'est
        // un cas que la carte produit, pas un trou dans la liste.
        let (published, status) = publish(fast, sweep.to);
        absorb(&published, status, &mut bytes);
        absorb_margins(sweep.half, &mut bytes);
    }
    Ok(hash::of(&bytes))
}

/// Ce que la frontière publie d'un balayage : un résultat, et un statut.
///
/// **Une empreinte ne peut hacher que cela**, et c'est ce que l'écriture du
/// premier hôte a montré. Le noyau porte trois drapeaux indépendants —
/// `start_solid`, `incomplete` et `no_gap` ; l'ABI n'en publie qu'un code de
/// retour, et garde le plus actionnable quand plusieurs s'appliquent — un départ
/// dans le solide demande à l'hôte de se dégager, une région tronquée ne lui
/// laisse aucun levier, la borne n'étant pas réglable, et un jeu perdu décrit un
/// état plutôt que ce contact-ci. Hacher les drapeaux revenait à valider le noyau
/// contre lui-même, sur un état qu'aucun hôte ne peut observer.
///
/// **La règle de priorité entre donc dans la référence.** Le jour où elle serait
/// remise en cause, l'empreinte bougerait sans que la géométrie ni le rendu aient
/// changé, et c'est ici qu'il faudra le lire.
///
/// `None` est le départ hors de toute cellule, que la frontière traite avant le
/// noyau : elle rend un déplacement libre au point demandé, et la conformance
/// rend le même plutôt qu'une marque à elle. **Un enregistrement de taille unique
/// épargne un cas particulier à chacun des cinq hôtes** — autant d'occasions de
/// le porter de travers, pour une distinction dont l'empreinte n'a que faire.
fn publish(hit: Option<Hit>, to: Vec3) -> (Hit, u8) {
    match hit {
        None => (free(to), STATUS_NO_CELL),
        Some(hit) if hit.start_solid => (hit, STATUS_START_SOLID),
        Some(hit) if hit.incomplete => (hit, STATUS_INCOMPLETE),
        // En dernier, et c'est l'ordre de la frontière : les deux précédents
        // disent que **ce** déplacement est faux ou bloqué, celui-ci que la
        // **repose** perdra son jeu. Aucun départ de cette scène ne le lève — les
        // boîtes y sont sept fois au-dessus du seuil —, et c'est pourquoi il
        // n'entre dans aucune empreinte.
        Some(hit) if hit.no_gap => (hit, STATUS_NO_GAP),
        Some(hit) => (hit, STATUS_OK),
    }
}

/// Le déplacement libre que rend un départ hors de toute cellule.
fn free(to: Vec3) -> Hit {
    Hit {
        fraction: 1.0,
        normal: Vec3::ZERO,
        point: to,
        surface: 0,
        cell: 0,
        start_solid: false,
        incomplete: false,
        no_gap: false,
    }
}

/// Écrit un résultat publié dans le tampon à hacher, octet de poids faible en
/// tête. Trente-sept octets, quel que soit le cas.
///
/// Tout par `to_bits`, sans une seule opération flottante : ce qui est haché est
/// ce que le moteur a écrit, et non ce qu'un formatage en aurait fait.
/// La marge et la portée de la boîte de ce balayage, dans l'empreinte.
///
/// **C'est le seul endroit du dépôt où `sweep_skin` et `sweep_reach` traversent
/// l'ABI.** Les tests de frontière les couvrent côté Rust, et l'hôte web
/// déclarait leurs symboles dans sa liste d'exports — donc `make test-wasm`
/// prouvait qu'ils existent, et rien ne prouvait qu'ils rendent la bonne valeur
/// à travers la frontière. Deux points d'entrée publiés qu'aucun hôte
/// n'empruntait, et ce sont les deux plus jeunes de l'ABI.
///
/// **Elles sont ici plutôt que dans une scène à elles**, parce qu'une scène
/// n'entre dans `HOST_SCENES` que pour un chemin d'ABI que les autres
/// n'empruntent pas — et celui-ci se greffe sur une boucle qui tient déjà les
/// demi-étendues qu'il lui faut. Une scène de plus aurait coûté cinq
/// descriptions pour rejouer la même liste.
///
/// Ni l'une ni l'autre ne dépend du décor ni du trajet : à demi-étendues égales
/// elles rendent les mêmes bits, et la répétition dans la liste est sans effet
/// sur ce que l'empreinte prouve.
fn absorb_margins(half: Vec3, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&sweep_skin(half).to_bits().to_le_bytes());
    bytes.extend_from_slice(&sweep_reach(half).to_bits().to_le_bytes());
}

fn absorb(hit: &Hit, status: u8, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&hit.fraction.to_bits().to_le_bytes());
    for value in [hit.normal.x, hit.normal.y, hit.normal.z] {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    for value in [hit.point.x, hit.point.y, hit.point.z] {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    bytes.extend_from_slice(&hit.surface.to_le_bytes());
    bytes.extend_from_slice(&hit.cell.to_le_bytes());
    bytes.push(status);
}

/// Ce que les deux chemins se doivent, selon que la traversée a tout examiné.
///
/// **L'égalité au bit près n'est exigible que d'un résultat complet.** La
/// traversée s'arrête à `SCG_SWEEP_CELLS` cellules, la force brute n'a pas de
/// borne : sur un trajet qui sature, les deux divergent nécessairement, et la
/// première annonce elle-même qu'elle n'a pas tout regardé.
///
/// **Ce qu'un résultat tronqué doit prouver est sa cohérence interne**, et non
/// une comparaison avec un chemin qui n'a pas la même borne. Son statut et sa
/// fraction doivent dire la même chose : une fraction de `1` annoncerait une
/// limite tout en rendant un déplacement libre, et un hôte qui lit le statut
/// comme un avertissement ferait traverser le mur que le moteur n'a pas eu le
/// temps de regarder.
///
/// **Écartée, et c'est l'erreur qu'il fallait éviter ici : l'inégalité des deux
/// fractions.** Elle se lit comme un conservatisme vérifié et ne vérifie rien —
/// un trajet qui sature est dégagé par construction, donc la force brute y rend
/// toujours `1`, et toute fraction lui est inférieure. Un contrôle qui ne peut
/// pas rougir est pire qu'un contrôle absent : il se lit comme une garantie.
fn agree(fast: &Hit, slow: &Hit) -> Result<(), &'static str> {
    if !fast.incomplete {
        return if same(fast, slow) {
            Ok(())
        } else {
            Err("la traversée et la force brute divergent")
        };
    }
    // Comparaison écrite, sur une valeur dont la finitude est acquise : une
    // fraction ne sort jamais de `[0, 1]`.
    if fast.fraction < 1.0 {
        Ok(())
    } else {
        Err("la traversée annonce une troncature et rend le déplacement entier")
    }
}

/// Deux résultats portent-ils exactement les mêmes bits ?
///
/// Par les bits et non par `==` : deux zéros de signes opposés se comparent
/// égaux en flottant et donnent deux empreintes différentes. C'est précisément
/// l'écart qu'une comparaison naïve laisserait passer ici.
///
/// **La comparaison exige la même surface, et c'est au décor de rester hors du
/// seul cas où cela ne tient pas** : un départ dans le solide qui touche des
/// surfaces de plusieurs cellules. La plus superficielle gagne — jamais l'ordre
/// du fichier entre cellules —, et elle peut vivre dans une cellule que la
/// traversée n'atteint pas. Les deux chemins s'accordent alors sur la fraction et
/// divergent sur la surface, sans qu'aucune formule soit en cause ; relâcher la
/// comparaison coûterait plus que choisir les départs. Un intégrateur l'a cherché
/// une demi-journée comme un défaut du moteur, d'où cette clause et celle de
/// `collide::brute`.
fn same(a: &Hit, b: &Hit) -> bool {
    a.fraction.to_bits() == b.fraction.to_bits()
        && a.normal.x.to_bits() == b.normal.x.to_bits()
        && a.normal.y.to_bits() == b.normal.y.to_bits()
        && a.normal.z.to_bits() == b.normal.z.to_bits()
        && a.point.x.to_bits() == b.point.x.to_bits()
        && a.point.y.to_bits() == b.point.y.to_bits()
        && a.point.z.to_bits() == b.point.z.to_bits()
        && a.surface == b.surface
        && a.cell == b.cell
        && a.start_solid == b.start_solid
        && a.incomplete == b.incomplete
        // **Celui-ci ne peut pas rougir aujourd'hui, et il est là pour demain.**
        // Les deux chemins posent `no_gap` par le même appel à une fonction pure
        // de leurs arguments, donc l'égalité est acquise par construction — comme
        // elle le serait de `grown` ou d'`in_unit` si l'oracle les comparait.
        // Ce qu'il garde est le jour où ce calcul entrerait dans le parcours :
        // une propriété posée sur un seul des deux chemins les fait diverger
        // partout où elle s'applique, et l'oracle accuserait la traversée d'un
        // écart qui vient de lui.
        && a.no_gap == b.no_gap
}

#[cfg(test)]
mod tests;

/// Le texte que `make conform-images` écrit, un balayage par ligne.
///
/// **C'est le pendant de « regarder les images » pour une étape qui n'en produit
/// pas.** Une empreinte dit qu'un résultat a changé, jamais qu'il est juste : une
/// scène où tout traverse tout se fige aussi bien qu'une autre.
pub fn report() -> Result<String, String> {
    let world = World::load(&collision_file::bytes())
        .map_err(|error| format!("collision : le décor est refusé : {error:?}"))?;

    let mut text = String::from(
        "# balayage : depart -> arrivee | boite | fraction | normale | surface | cellule | etat \
         | marge | portee\n",
    );
    for (index, sweep) in all().into_iter().enumerate() {
        let cell = world.locate(sweep.from);
        let found = if cell == 0 {
            None
        } else {
            world.sweep(cell, sweep.half, sweep.from, sweep.to)
        };
        // Le rapport montre ce que l'empreinte hache, statut compris : un texte
        // qui dirait autre chose ne servirait plus à instruire un écart.
        let (hit, status) = publish(found, sweep.to);
        let state = match status {
            STATUS_START_SOLID => "depart-solide",
            STATUS_INCOMPLETE => "tronque",
            STATUS_NO_CELL => "hors-cellule",
            STATUS_NO_GAP => "sans-jeu",
            _ => "-",
        };
        text.push_str(&format!(
            "{index:4} : ({:.2}, {:.2}, {:.2}) -> ({:.2}, {:.2}, {:.2}) | {:.2} | {:.4} | \
             ({:.3}, {:.3}, {:.3}) | {} | {} | {state} | {:.6} | {:.1}\n",
            sweep.from.x,
            sweep.from.y,
            sweep.from.z,
            sweep.to.x,
            sweep.to.y,
            sweep.to.z,
            sweep.half.x,
            hit.fraction,
            hit.normal.x,
            hit.normal.y,
            hit.normal.z,
            hit.surface,
            hit.cell,
            sweep_skin(sweep.half),
            sweep_reach(sweep.half)
        ));
    }
    Ok(text)
}
