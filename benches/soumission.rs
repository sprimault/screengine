// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que coûte le parcours du tableau de sommets, et sur quels appels.
//!
//! **La question est née d'un correctif.** Un sommet non fini qu'aucun indice ne
//! nommait était refusé par la frontière C et accepté par l'API Rust : la même
//! scène passait d'un côté et pas de l'autre. L'alignement ne pouvait se faire
//! que dans un sens — la frontière ne sait pas quels sommets un triangle
//! référence, elle confie l'indexation au noyau —, et les cinq soumissions qui
//! reçoivent une tranche de l'appelant vérifient donc le tableau **entier**.
//! Restait à savoir ce que ce parcours coûte, qui n'avait jamais été mesuré.
//!
//! **Le banc le mesure par différence entre deux chemins publics**, sur la même
//! géométrie et le même tableau : [`Context::submit`] le parcourt,
//! [`Context::submit_each`] ne voit aucun tableau — elle reçoit un lecteur, et
//! c'est par elle que passe la frontière. La closure qu'on lui donne ici est le
//! corps de `submit`, recopié ligne pour ligne : la seule chose qui reste entre
//! les deux mesures est le parcours.
//!
//! Le harnais et ce que ces chiffres valent sont dans
//! [`remplissage`](../remplissage/index.html) : aucun seuil, jamais d'échec sur
//! une durée, le minimum plutôt que la moyenne.
//!
//! # Les trois cas, et ce que chacun répond
//!
//! - **Le parcours isolé.** Un grand tableau, peu de triangles, et tous derrière
//!   la caméra : le plan proche les rejette, l'image reste vide, et ce qui
//!   sépare les deux chemins n'est plus que le parcours. C'est de là que sort le
//!   coût par sommet, le seul chiffre qu'un intégrateur puisse appliquer à sa
//!   propre taille de tableau. **L'image vide est vérifiée** — un triangle qui
//!   se faufilerait ferait mesurer du remplissage sous un nom qui annonce le
//!   contraire.
//! - **Le mur indexé plein cadre.** Un lot qu'un hôte soumettrait vraiment,
//!   sommets partagés, couvrant l'écran et rendu : la part du parcours dans une
//!   image complète.
//! - **Le mur à queue non indexée.** Les mêmes triangles, dans un tableau bien
//!   plus grand dont la plus grande part n'est nommée par aucun indice. C'est le
//!   régime que le correctif a rendu payant, et le seul où le parcours puisse
//!   peser : il est la mesure du pire cas, pas celle d'un décor.
//!
//! # La mesure, prise le 2026-10-14
//!
//! ```text
//!   tableau   submit   submit_each     ecart        bruit
//!     1 024   0.004 ms    0.004 ms   0.49 ns/s    0.29 ns/s
//!     4 096   0.005 ms    0.004 ms   0.46 ns/s    0.02 ns/s
//!    16 384   0.011 ms    0.004 ms   0.41 ns/s    0.03 ns/s
//!
//!   mur indexe plein cadre, 1 089 sommets, 2 048 triangles
//!             0.619 ms    0.594 ms   0.025 ms     0.102 ms
//!
//!   mur a queue non indexee, 16 384 sommets, 2 048 triangles
//!             0.621 ms    0.613 ms   0.007 ms     0.011 ms
//! ```
//!
//! Poste Windows au repos ; deux exécutions successives concordent sur le cas
//! isolé, qui est le seul des trois à sortir du bruit.
//!
//! **Le parcours coûte quatre dixièmes de nanoseconde par sommet, et il est
//! linéaire.** Le chiffre est plat de 4 096 à 16 384 sommets ; à 1 024 il monte
//! un peu, le terme fixe de l'appel comptant encore à cette taille. Un tableau
//! de seize mille sommets paie donc **sept microsecondes**.
//!
//! **Sur une image, ce n'est pas mesurable, et le banc le démontre au lieu de
//! l'affirmer.** Les deux cas rendus donnent un écart du même ordre que leur
//! propre bruit — et sur le mur plein cadre, le bruit le dépasse cinq fois. Ce
//! qui autorise à conclure n'est donc pas leur écart, c'est que le coût prédit
//! par le cas isolé — un demi-microseconde pour mille quatre-vingt-neuf
//! sommets, sept pour seize mille — soit cent fois sous la milliseconde d'une
//! image.
//!
//! **La question est close : le parcours reste entier.** Au pire cas construit
//! pour lui — un tableau à la capacité de triangles par défaut, dont la plus
//! grande part n'est nommée par aucun indice —, il vaut un pour cent d'une
//! image ; sur un lot indexé ordinaire, moins d'un dixième de pour cent. Rien
//! là-dedans ne paie une seconde forme de soumission, ni un réglage pour le
//! sauter : ce que l'une et l'autre coûteraient à l'ABI est sans commune mesure
//! avec sept microsecondes.

use core::hint::black_box;
use core::time::Duration;
use std::time::Instant;

use screengine::{
    Affine3, Argument, BYTES_PER_PIXEL, Color, Config, Context, Error, Triangle, Vec3,
};

/// La résolution interne de référence du projet.
const WIDTH: u32 = 640;

/// Sa hauteur.
const HEIGHT: u32 = 360;

/// Tours mesurés par cas, comme les trois autres bancs.
const TOURS: u32 = 60;

/// Le côté du mur en cases, qui en fait `COTE × COTE × 2` triangles.
///
/// Trente-deux donne deux mille quarante-huit triangles pour mille
/// quatre-vingt-neuf sommets : le rapport d'un maillage indexé ordinaire, où un
/// sommet sert quatre à six triangles.
const COTE: usize = 32;

/// La distance du mur à la caméra, en unités de monde.
///
/// Dix, avec les demi-cotes ci-dessous, pour que le mur dépasse l'écran de peu :
/// le remplissage porte alors sur l'image entière sans que la bande de garde
/// travaille, et aucun triangle ne se multiplie au clipping.
const DISTANCE: f32 = 10.0;

/// La demi-hauteur du mur. Le champ de vision par défaut en couvre 5,77 à cette
/// distance.
const DEMI_HAUTEUR: f32 = 6.3;

/// Sa demi-largeur, dans le rapport de l'image.
const DEMI_LARGEUR: f32 = 11.5;

/// La taille du tableau des deux cas qui en portent un grand.
///
/// Seize mille trois cent quatre-vingt-quatre, soit la capacité de triangles par
/// défaut : un hôte qui soumet un tableau plus grand que cela a déjà dépassé ce
/// que l'image peut porter.
const GRAND_TABLEAU: usize = 16_384;

/// Le plus court temps observé sur `TOURS` répétitions.
///
/// Deux tours à blanc d'abord : la première soumission d'un processus paie le
/// cache d'instructions et les défauts de page du tampon.
fn mesure(mut tour: impl FnMut()) -> Duration {
    tour();
    tour();

    let mut court = Duration::MAX;
    for _ in 0..TOURS {
        let debut = Instant::now();
        tour();
        court = court.min(debut.elapsed());
    }
    court
}

/// Ce que l'image d'un cas doit porter, vérifié à chaque fois.
///
/// **C'est le témoin du banc.** Une mesure de soumission se chronomètre très
/// bien sur une image que rien n'atteint, et le chiffre est alors flatteur et
/// faux. Les deux attentes se trompent dans des sens opposés : un cas qui se
/// croit vide et peint mesure du remplissage, un cas qui se croit plein et ne
/// peint rien ne mesure qu'un rejet.
enum Image {
    /// Rien n'est peint : les triangles sont rejetés avant le remplissage.
    Vide,
    /// Au moins cette part de l'image est peinte.
    Peinte(f64),
}

/// Le contexte, son tampon et la largeur de ligne qui vont ensemble.
///
/// Créés une fois et repris par tous les cas d'une même résolution : recréer un
/// contexte par mesure ferait payer ses allocations, qui sont précisément ce que
/// le moteur promet de ne pas refaire par image.
struct Banc {
    /// Le contexte de rendu.
    context: Context,
    /// Le tampon de sortie, à la résolution du contexte.
    pixels: Vec<u8>,
    /// La largeur de ligne en pixels, égale à la largeur.
    stride: u32,
    /// Les pixels de l'image, pour la part qu'exige le témoin.
    surface: f64,
}

impl Banc {
    /// Un banc à la résolution demandée, en tuiles de 64.
    fn nouveau(width: u32, height: u32) -> Self {
        let context = Context::new(Config {
            max_width: width,
            max_height: height,
            width,
            height,
            tile_size: 64,
            max_triangles: 0,
            max_lines: 0,
        })
        .expect("configuration valide");
        Self {
            pixels: vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL],
            context,
            stride: width,
            surface: f64::from(width) * f64::from(height),
        }
    }

    /// Combien de pixels l'image porte, hors fond.
    fn peints(&self) -> usize {
        self.pixels
            .chunks_exact(BYTES_PER_PIXEL)
            .filter(|p| p[..3] != [0, 0, 0])
            .count()
    }

    /// Vérifie que l'image rendue est bien celle que le cas annonce.
    fn exige(&self, quoi: &str, attendu: &Image) {
        let peints = self.peints();
        let total = self.surface;
        match attendu {
            Image::Vide => assert_eq!(
                peints, 0,
                "{quoi} : {peints} pixels peints, la mesure porte sur du remplissage"
            ),
            Image::Peinte(part) => assert!(
                peints as f64 >= total * part,
                "{quoi} : {peints} pixels peints sur {total}, la mesure ne porte sur rien"
            ),
        }
    }
}

/// Un mur plan subdivisé en `COTE × COTE` cases, sommets partagés.
///
/// Face à la caméra par défaut, qui regarde le +X : le mur est dans le plan
/// `x = DISTANCE`, et ses sommets sont donc ordonnés sur `y` et `z`.
fn mur() -> (Vec<Vec3>, Vec<Triangle>) {
    let mut sommets = Vec::with_capacity((COTE + 1) * (COTE + 1));
    for j in 0..=COTE {
        for i in 0..=COTE {
            let t = i as f32 / COTE as f32;
            let s = j as f32 / COTE as f32;
            // **La hauteur décroît avec `j`**, et ce n'est pas un détail de
            // repère : c'est l'ordre des sommets qui décide de la face, et pris
            // dans l'autre sens le mur est un dos que le moteur élimine. Le
            // témoin de ce banc l'a dit du premier coup.
            sommets.push(Vec3::new(
                DISTANCE,
                DEMI_LARGEUR * (2.0 * t - 1.0),
                DEMI_HAUTEUR * (1.0 - 2.0 * s),
            ));
        }
    }

    let largeur = (COTE + 1) as u32;
    let mut triangles = Vec::with_capacity(COTE * COTE * 2);
    for j in 0..COTE as u32 {
        for i in 0..COTE as u32 {
            let a = j * largeur + i;
            let (b, c, d) = (a + 1, a + largeur, a + largeur + 1);
            // Deux teintes alternées : un aplat se compterait pareil, mais une
            // case qui manque se verrait à l'oeil sur une capture.
            let couleur = if (i ^ j) & 1 == 0 {
                Color::new(0x40, 0x80, 0xC0, 0xFF)
            } else {
                Color::new(0xC0, 0x80, 0x40, 0xFF)
            };
            triangles.push(Triangle {
                indices: [a, b, c],
                color: couleur,
            });
            triangles.push(Triangle {
                indices: [b, d, c],
                color: couleur,
            });
        }
    }
    (sommets, triangles)
}

/// Le même mur, suivi de sommets qu'aucun indice ne nomme.
///
/// La queue reprend le premier sommet du mur plutôt qu'une valeur arbitraire :
/// ce qu'on mesure est un parcours, et une coordonnée démesurée y ferait entrer
/// un sous-normal ou un infini dont le coût de lecture n'est pas celui d'un
/// flottant ordinaire.
fn mur_a_queue(total: usize) -> (Vec<Vec3>, Vec<Triangle>) {
    let (mut sommets, triangles) = mur();
    let premier = sommets[0];
    sommets.resize(total, premier);
    (sommets, triangles)
}

/// Un lot que le plan proche rejette : un grand tableau, peu de triangles.
///
/// Les sommets sont **derrière** la caméra, qui regarde le +X depuis l'origine.
/// Le clipping les écarte avant toute préparation, et l'image reste vide — c'est
/// ce que `Image::Vide` vérifie.
fn derriere(total: usize, triangles: usize) -> (Vec<Vec3>, Vec<Triangle>) {
    let sommets = vec![Vec3::new(-DISTANCE, 0.0, 0.0); total];
    let lot = (0..triangles as u32)
        .map(|i| Triangle {
            indices: [i % 3, (i + 1) % 3, (i + 2) % 3],
            color: Color::new(0xFF, 0xFF, 0xFF, 0xFF),
        })
        .collect();
    (sommets, lot)
}

/// Soumet le lot par `Context::submit`, qui parcourt le tableau entier.
fn par_tranche(banc: &mut Banc, sommets: &[Vec3], triangles: &[Triangle]) {
    banc.context
        .submit(Affine3::IDENTITY, black_box(sommets), black_box(triangles))
        .expect("lot valide");
}

/// Soumet le même lot par `Context::submit_each`, qui ne voit aucun tableau.
///
/// **La closure est le corps de `Context::submit`, recopié**, y compris son
/// `get` et son erreur d'indice : c'est ce qui laisse le parcours comme seule
/// différence entre les deux mesures. L'écrire autrement — une indexation
/// directe, un `unwrap` — mesurerait aussi l'écart entre deux façons de lire un
/// tableau.
fn par_lecteur(banc: &mut Banc, sommets: &[Vec3], triangles: &[Triangle]) {
    let sommets = black_box(sommets);
    let triangles = black_box(triangles);
    banc.context
        .submit_each(Affine3::IDENTITY, triangles.len(), |i| {
            let triangle = triangles[i];
            let mut corners = [Vec3::ZERO; 3];
            for (corner, &index) in corners.iter_mut().zip(&triangle.indices) {
                *corner = *sommets
                    .get(index as usize)
                    .ok_or(Error::InvalidArgument(Argument::VertexIndex))?;
            }
            Ok::<_, Error>((corners, triangle.color))
        })
        .expect("lot valide");
}

/// Mesure un cas par les deux chemins et écrit ses quatre lignes.
///
/// **La quatrième est le témoin de bruit**, et sans elle les trois premières
/// mentent : `submit` mesuré deux fois de suite donne l'écart que la machine
/// introduit à elle seule. Un écart entre chemins qui ne le dépasse pas n'est
/// pas un coût, c'est une fluctuation — ce qui s'est produit du premier coup sur
/// le mur plein cadre, où un demi-pour-cent de bruit sur une image d'une
/// demi-milliseconde se lisait comme vingt-six nanosecondes par sommet.
///
/// L'écart est rendu en nanosecondes par sommet du tableau : c'est le seul
/// chiffre transposable, puisqu'il ne dépend ni du nombre de triangles, ni de la
/// résolution.
fn cas(quoi: &str, banc: &mut Banc, sommets: &[Vec3], triangles: &[Triangle], attendu: Image) {
    println!(
        "{quoi} — {} sommets, {} triangles",
        sommets.len(),
        triangles.len()
    );

    let tranche = mesure(|| {
        par_tranche(banc, sommets, triangles);
        rendre(banc);
    });
    banc.exige(quoi, &attendu);

    let lecteur = mesure(|| {
        par_lecteur(banc, sommets, triangles);
        rendre(banc);
    });
    banc.exige(quoi, &attendu);

    let repete = mesure(|| {
        par_tranche(banc, sommets, triangles);
        rendre(banc);
    });

    ligne("  submit, qui parcourt", tranche);
    ligne("  submit_each, qui ne parcourt pas", lecteur);
    par_sommet("  ecart", tranche.saturating_sub(lecteur), sommets.len());
    par_sommet(
        "  bruit, submit contre lui-meme",
        tranche.abs_diff(repete),
        sommets.len(),
    );
    println!();
}

/// Clôt l'image, ce qui vide la liste de dessin pour le tour suivant.
///
/// Les deux chemins la paient à l'identique, et l'écart la retire : elle est
/// dans la boucle parce qu'une soumission qui ne se vide pas finit par dépasser
/// la capacité, pas parce qu'on la mesure.
fn rendre(banc: &mut Banc) {
    let stride = banc.stride;
    banc.context
        .frame_end(&mut banc.pixels, stride)
        .expect("image rendue");
}

/// Écrit une ligne de résultat : le coût par tour.
fn ligne(quoi: &str, duree: Duration) {
    println!("{quoi:<38} {:>7.3} ms", duree.as_secs_f64() * 1e3);
}

/// Écrit une durée rapportée au tableau, en nanosecondes par sommet.
fn par_sommet(quoi: &str, duree: Duration, sommets: usize) {
    println!(
        "{quoi:<38} {:>7.3} ms   {:>5.2} ns/sommet",
        duree.as_secs_f64() * 1e3,
        duree.as_secs_f64() * 1e9 / sommets as f64
    );
}

/// Joue les trois cas dans l'ordre et écrit leur tableau.
///
/// Le parcours isolé d'abord, parce que c'est lui qui donne le coût par sommet
/// auquel les deux suivants se comparent.
fn main() {
    println!("screengine — soumission, minimum sur {TOURS} tours\n");

    // **Trois tailles et non une** : un seul point ne distingue pas un coût
    // linéaire d'un coût fixe, et c'est la platitude du nanoseconde par sommet
    // d'une taille à l'autre qui autorise à transposer le chiffre.
    let mut vide = Banc::nouveau(64, 64);
    for tableau in [1_024, 4_096, GRAND_TABLEAU] {
        let (sommets, triangles) = derriere(tableau, 64);
        cas(
            "parcours isole, lot rejete",
            &mut vide,
            &sommets,
            &triangles,
            Image::Vide,
        );
    }

    let mut plein = Banc::nouveau(WIDTH, HEIGHT);
    let (sommets, triangles) = mur();
    cas(
        "mur indexe plein cadre",
        &mut plein,
        &sommets,
        &triangles,
        Image::Peinte(0.95),
    );

    let (sommets, triangles) = mur_a_queue(GRAND_TABLEAU);
    cas(
        "mur a queue non indexee",
        &mut plein,
        &sommets,
        &triangles,
        Image::Peinte(0.95),
    );
}
