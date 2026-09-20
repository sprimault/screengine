// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Suite de conformance de Screengine.
//!
//! Rejoue les scènes de référence sans fenêtre, hache chaque tampon rendu et
//! compare l'empreinte à celle de `references/`. Les références sont
//! versionnées : chaque plateforme se compare aux mêmes fichiers, donc toutes
//! les plateformes entre elles — et, puisque chaque hôte compare la sienne au
//! chemin Rust de sa plateforme, tous les hôtes entre eux.
//!
//! Une scène se rend dans chaque configuration qui ne doit pas changer l'image,
//! et toutes se comparent à une seule référence : une couture de tuile ne se
//! voit que dans une configuration.
//!
//! `--print` rend une scène et écrit son empreinte, sans rien comparer : c'est
//! le chemin Rust natif auquel les hôtes comparent la leur.

mod hash;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{fs, io};

use screengine::{
    Affine3, BYTES_PER_PIXEL, Color, Config, Context, Frame, Rect, Rows, Triangle, Vec3,
};

/// Une façon de rendre une scène qui ne doit pas changer l'image.
///
/// Une couture de tuile ne se voit que dans une configuration : chaque scène
/// passe par toutes, et toutes se comparent à une seule référence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// Tuiles de 32, rendues par la seule fin d'image, dans l'ordre.
    Tiles32,
    /// Tuiles de 64, de même. C'est la configuration des hôtes.
    Tiles64,
    /// L'image entière d'un bloc, sans répartition : la référence des tuiles.
    Whole,
    /// Tuiles de 32, une moitié dans un ordre mélangé à graine fixe, l'autre
    /// laissée à la fin d'image.
    Shuffled,
    /// Tuiles de 32, une bande de tuiles par thread.
    Threads,
}

impl Pass {
    /// Toutes les passes, dans l'ordre où la suite les rejoue.
    const ALL: [Self; 5] = [
        Self::Tiles32,
        Self::Tiles64,
        Self::Whole,
        Self::Shuffled,
        Self::Threads,
    ];

    /// Le nom de la passe, dans un message de divergence.
    fn name(self) -> &'static str {
        match self {
            Self::Tiles32 => "tuiles de 32",
            Self::Tiles64 => "tuiles de 64",
            Self::Whole => "image entière",
            Self::Shuffled => "ordre mélangé",
            Self::Threads => "threads",
        }
    }

    /// Le côté de tuile du contexte.
    fn tile_size(self) -> u32 {
        match self {
            Self::Tiles64 | Self::Whole => 64,
            Self::Tiles32 | Self::Shuffled | Self::Threads => 32,
        }
    }

    /// Rend une image déjà commencée dans `pixels`, rangé par lignes de `width`.
    fn render(
        self,
        frame: Frame<'_>,
        pixels: &mut [u8],
        width: u32,
        height: u32,
    ) -> screengine::Result<()> {
        match self {
            Self::Tiles32 | Self::Tiles64 => frame.end(&mut Rows::new(pixels, width)),
            Self::Whole => {
                let mut color = vec![0u32; width as usize * height as usize];
                let mut depth = color.clone();
                let image = Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                };
                let mut out = Rows::new(pixels, width);
                frame.region(image, &mut color, &mut depth, &mut out)
            }
            Self::Shuffled => {
                let mut order: Vec<u32> = (0..frame.tile_count()).collect();
                // xorshift à graine fixe : un ordre qui change d'un passage à
                // l'autre ferait d'une divergence un échec qu'on ne rejoue pas.
                let mut state = 0x9E37_79B9_7F4A_7C15u64;
                for i in (1..order.len()).rev() {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    order.swap(i, (state % (i as u64 + 1)) as usize);
                }
                let mut rows = Rows::new(pixels, width);
                for &index in &order[..order.len() / 2] {
                    frame.tile(index, &mut rows)?;
                }
                frame.end(&mut rows)
            }
            Self::Threads => {
                let tile = self.tile_size();
                let columns = width.div_ceil(tile);
                let band = tile as usize * width as usize * BYTES_PER_PIXEL;
                std::thread::scope(|scope| {
                    let workers: Vec<_> = pixels
                        .chunks_mut(band)
                        .enumerate()
                        .map(|(row, chunk)| {
                            let frame = &frame;
                            scope.spawn(move || {
                                let mut rows = Rows::band(chunk, width, row as u32 * tile);
                                (0..columns).try_for_each(|column| {
                                    frame.tile(row as u32 * columns + column, &mut rows)
                                })
                            })
                        })
                        .collect();
                    workers.into_iter().try_for_each(|worker| {
                        worker
                            .join()
                            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                    })
                })?;
                frame.end(&mut Rows::new(pixels, width))
            }
        }
    }
}

/// Ce que la suite fait des empreintes calculées.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    /// Compare aux références et échoue sur toute divergence.
    Check,
    /// Réécrit les références. Une évolution voulue du rendu, jamais une
    /// régression qu'on ferait taire : le commit qui les met à jour est distinct.
    Update,
    /// Rend une scène et écrit son empreinte.
    Print(Scene),
    /// Écrit chaque scène en image, dans un répertoire, pour la regarder.
    ///
    /// Une empreinte ne dit pas ce qu'elle fixe. Figer une référence sans avoir
    /// vu l'image, c'est graver un fond noir ou un mur retourné et s'en
    /// apercevoir à l'étape qui s'en sert.
    Dump(PathBuf),
}

/// Les scènes que la suite sait rendre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scene {
    /// La scène de démonstration du moteur, en 640×360, que rien ne soumet.
    ///
    /// Les hôtes rendent la même configuration, en tuiles de 64 : la changer ici
    /// sans eux ferait diverger toutes les empreintes à la fois.
    Triangle,
    /// Un mur si près et si large que ses sommets sortent de la bande de garde.
    ///
    /// C'est le seul chemin qui exerce les quatre plans de garde : sans lui,
    /// une erreur dans leur découpe ne se verrait qu'à l'étape où un couloir se
    /// parcourt, c'est-à-dire trop tard pour savoir d'où elle vient.
    Guard,
    /// Un mur qui déborde largement de l'image, mais reste dans la bande de
    /// garde.
    ///
    /// Le cas complémentaire du précédent : rien n'est découpé, et ce sont les
    /// rectangles de parcours et la répartition par tuile qui bornent le
    /// remplissage. Une tuile qui rejetterait un triangle à tort troue l'image
    /// ici, et nulle part ailleurs.
    Lateral,
    /// Deux murs qui se traversent, et un triangle soumis deux fois.
    ///
    /// L'interpénétration ne se rend juste que par le tampon de profondeur ;
    /// le doublon fixe l'autre moitié du contrat, le test strict qui laisse le
    /// premier soumis gagner à profondeur égale.
    Depth,
    /// Un sol qui commence derrière la caméra et fuit vers l'horizon.
    ///
    /// Le plan proche coupe chacun de ses triangles, et c'est la découpe qui
    /// engendre le plus de sommets par triangle.
    Near,
}

/// Deux gris francs, pour les scènes où la couleur ne porte rien d'autre que
/// la diagonale.
const PLAIN: [Color; 2] = [
    Color::new(0xC0, 0xC0, 0xC0, 0xFF),
    Color::new(0x70, 0x70, 0x70, 0xFF),
];

/// Soumet un quadrilatère plan, en deux triangles qui partagent une diagonale.
///
/// Les quatre coins se donnent dans l'ordre du contour, en sens antihoraire vu
/// de la face avant : un ordre inverse en fait un dos de face, que le moteur
/// élimine, et la scène rend alors un fond noir sans se plaindre.
///
/// Une couleur par triangle, et non une pour le quadrilatère : sans la
/// diagonale, deux quadrilatères qui couvrent l'écran rendent la même image
/// quelles que soient leurs tailles, et deux scènes cessent de se distinguer.
fn quad(context: &mut Context, corners: [Vec3; 4], colors: [Color; 2]) -> screengine::Result<()> {
    context.submit(
        Affine3::IDENTITY,
        &corners,
        &[
            Triangle {
                indices: [0, 1, 2],
                color: colors[0],
            },
            Triangle {
                indices: [0, 2, 3],
                color: colors[1],
            },
        ],
    )
}

/// Soumet un mur perpendiculaire au regard, à `distance` de la caméra, large de
/// `half_y` et haut de `half_z`.
///
/// Sa face avant regarde la caméra. Deux scènes n'en diffèrent que par ces
/// nombres, et c'est voulu : ce qui les sépare est la seule chose qu'elles
/// éprouvent, le franchissement de la bande de garde. Elles laissent l'une
/// comme l'autre du fond visible au-dessus et au-dessous — un mur qui
/// couvrirait tout rendrait la même image, qu'il ait été découpé correctement
/// ou pas du tout.
fn wall(context: &mut Context, distance: f32, half_y: f32, half_z: f32) -> screengine::Result<()> {
    quad(
        context,
        [
            Vec3::new(distance, half_y, -half_z),
            Vec3::new(distance, -half_y, -half_z),
            Vec3::new(distance, -half_y, half_z),
            Vec3::new(distance, half_y, half_z),
        ],
        PLAIN,
    )
}

impl Scene {
    /// Toutes les scènes, dans l'ordre où `--check` les rejoue.
    const ALL: [Self; 5] = [
        Self::Triangle,
        Self::Guard,
        Self::Lateral,
        Self::Depth,
        Self::Near,
    ];

    /// La passe que `--print` utilise, celle des hôtes.
    const HOST_PASS: Pass = Pass::Tiles64;

    /// La résolution de toutes les scènes, qui est celle des hôtes.
    const RESOLUTION: (u32, u32) = (640, 360);

    /// Le nom de la scène, qui est aussi celui de sa référence.
    fn name(self) -> &'static str {
        match self {
            Self::Triangle => "triangle",
            Self::Guard => "garde",
            Self::Lateral => "lateral",
            Self::Depth => "profondeur",
            Self::Near => "proche",
        }
    }

    /// Soumet la scène, la caméra restant celle du contexte neuf.
    ///
    /// Les coordonnées sont celles du monde — X vers l'est, Z en haut —, et la
    /// caméra neutre regarde le +X depuis l'origine. Rien n'est soumis pour
    /// [`Scene::Triangle`] : c'est le moteur qui rend alors sa scène de
    /// démonstration, et c'est ce qui garde son empreinte comparable à celle
    /// des hôtes.
    fn submit(self, context: &mut Context) -> screengine::Result<()> {
        match self {
            Self::Triangle => Ok(()),
            // À une demi-unité de la caméra et dix de large : ses bords partent
            // à plus de six mille pixels du centre, loin au-delà des 4096 de la
            // bande de garde.
            Self::Guard => wall(context, 0.5, 10.0, 0.15),
            // À trois unités et vingt de large : deux mille pixels, donc bien
            // hors de l'image, mais dans la bande — rien n'est découpé ici.
            Self::Lateral => wall(context, 3.0, 20.0, 1.0),
            Self::Depth => {
                // Deux murs inclinés qui se croisent en leur milieu : aucune
                // arête ne marque l'intersection, seule la profondeur la trace.
                quad(
                    context,
                    [
                        Vec3::new(6.0, -4.0, 3.0),
                        Vec3::new(14.0, 4.0, 3.0),
                        Vec3::new(14.0, 4.0, -3.0),
                        Vec3::new(6.0, -4.0, -3.0),
                    ],
                    [
                        Color::new(0xE0, 0x40, 0x40, 0xFF),
                        Color::new(0xA0, 0x30, 0x30, 0xFF),
                    ],
                )?;
                quad(
                    context,
                    [
                        Vec3::new(14.0, -4.0, 3.0),
                        Vec3::new(6.0, 4.0, 3.0),
                        Vec3::new(6.0, 4.0, -3.0),
                        Vec3::new(14.0, -4.0, -3.0),
                    ],
                    [
                        Color::new(0x40, 0x40, 0xE0, 0xFF),
                        Color::new(0x30, 0x30, 0xA0, 0xFF),
                    ],
                )?;
                // Exactement les mêmes sommets, donc exactement la même
                // profondeur : le test strict garde le premier, et un test
                // relâché ferait apparaître le second.
                let corners = [
                    Vec3::new(9.0, -1.0, -1.0),
                    Vec3::new(9.0, 1.0, -1.0),
                    Vec3::new(9.0, 0.0, 1.0),
                ];
                for color in [
                    Color::new(0x20, 0xE0, 0x20, 0xFF),
                    Color::new(0xE0, 0xE0, 0x20, 0xFF),
                ] {
                    context.submit(
                        Affine3::IDENTITY,
                        &corners,
                        &[Triangle {
                            indices: [0, 2, 1],
                            color,
                        }],
                    )?;
                }
                Ok(())
            }
            // Le sol commence derrière la caméra : chaque triangle traverse le
            // plan proche, et aucun n'en sort entier.
            Self::Near => quad(
                context,
                [
                    Vec3::new(-6.0, -12.0, -1.0),
                    Vec3::new(40.0, -12.0, -1.0),
                    Vec3::new(40.0, 12.0, -1.0),
                    Vec3::new(-6.0, 12.0, -1.0),
                ],
                [
                    Color::new(0x60, 0x90, 0x60, 0xFF),
                    Color::new(0x40, 0x68, 0x40, 0xFF),
                ],
            ),
        }
    }

    /// Reconnaît une scène par son nom.
    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scene| scene.name() == name)
    }

    /// Rend la scène par `pass` et rend son empreinte.
    ///
    /// Toutes les scènes partagent la résolution des hôtes : une scène qui
    /// rendrait ailleurs ne se comparerait plus à rien de ce qu'ils mesurent.
    fn render(self, pass: Pass) -> Result<u64, screengine::Error> {
        let (width, height) = Self::RESOLUTION;
        let pixels = self.render_pixels(pass)?;
        Ok(hash::image(&pixels, width, height, width))
    }

    /// Rend la scène par `pass` et rend le tampon lui-même.
    ///
    /// L'empreinte ne dit pas si une image est noire, et une scène soumise à
    /// l'envers est éliminée comme dos de face sans un mot : c'est par ce
    /// chemin que les tests vérifient qu'il y a quelque chose à hacher.
    fn render_pixels(self, pass: Pass) -> Result<Vec<u8>, screengine::Error> {
        let (width, height) = Self::RESOLUTION;
        let mut context = Context::new(Config {
            max_width: width,
            max_height: height,
            width,
            height,
            tile_size: pass.tile_size(),
        })?;
        self.submit(&mut context)?;
        let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];
        pass.render(context.frame_begin()?, &mut pixels, width, height)?;
        Ok(pixels)
    }

    /// Rend la scène par chaque passe, et rend l'empreinte commune.
    ///
    /// Deux passes qui divergent sont une erreur du moteur, pas une
    /// différence à départager : l'image ne dépend ni du découpage, ni de
    /// l'ordre, ni des threads.
    fn render_all(self) -> Result<u64, String> {
        let mut common: Option<(Pass, u64)> = None;
        for pass in Pass::ALL {
            let hash = self.render(pass).map_err(|error| {
                format!(
                    "{} ({}) : le moteur a refusé la scène : {error:?}",
                    self.name(),
                    pass.name()
                )
            })?;
            match common {
                None => common = Some((pass, hash)),
                Some((first, expected)) if expected != hash => {
                    return Err(format!(
                        "{} : {} et {} divergent ({} contre {})",
                        self.name(),
                        first.name(),
                        pass.name(),
                        hash::format(expected),
                        hash::format(hash)
                    ));
                }
                Some(_) => {}
            }
        }
        // Pass::ALL n'est pas vide : la boucle a fixé l'empreinte.
        Ok(common.map_or(0, |(_, hash)| hash))
    }
}

/// Le répertoire des références, à côté du `Cargo.toml` de la suite et non du
/// répertoire courant : `make conform` la lance depuis la racine.
fn references() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("references")
}

/// Le contenu d'un fichier de référence : l'empreinte et un saut de ligne, ce
/// que `--update` écrit et que `--check` compare octet pour octet.
fn reference_text(hash: u64) -> String {
    format!("{}\n", hash::format(hash))
}

/// Compare une scène à sa référence ; rend le compte rendu d'une ligne.
fn check(scene: Scene, dir: &Path) -> Result<String, String> {
    let rendered = scene.render_all()?;
    let path = dir.join(scene.name());
    let expected = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!(
                "{} : référence absente, {} ; make conform-update si la scène est nouvelle",
                scene.name(),
                path.display()
            ));
        }
        Err(error) => return Err(format!("{} : {error}", path.display())),
    };
    if expected == reference_text(rendered) {
        Ok(format!(
            "{} : {}, conforme",
            scene.name(),
            hash::format(rendered)
        ))
    } else {
        Err(format!(
            "{} : rendu {}, référence {}",
            scene.name(),
            hash::format(rendered),
            expected.trim_end()
        ))
    }
}

/// Réécrit la référence d'une scène.
fn update(scene: Scene, dir: &Path) -> Result<String, String> {
    let rendered = scene.render_all()?;
    let path = dir.join(scene.name());
    fs::create_dir_all(dir)
        .and_then(|()| fs::write(&path, reference_text(rendered)))
        .map_err(|error| format!("{} : {error}", path.display()))?;
    Ok(format!(
        "{} : {}, écrite",
        scene.name(),
        hash::format(rendered)
    ))
}

/// Écrit une image en BMP 24 bits, non compressé.
///
/// Un format écrit à la main plutôt qu'une bibliothèque d'images : la suite de
/// conformance n'a pas de dépendance, et n'en prend pas une pour un aperçu.
/// BMP et non PPM parce qu'un poste de travail l'ouvre sans rien installer.
fn write_bmp(path: &Path, pixels: &[u8], width: u32, height: u32) -> io::Result<()> {
    // 640 × 3 est déjà un multiple de quatre ; le calcul reste écrit pour que
    // la fonction survive au jour où une scène changera de largeur.
    let row = width as usize * 3;
    let padding = (4 - row % 4) % 4;
    let data = (row + padding) * height as usize;
    let mut file = Vec::with_capacity(54 + data);

    file.extend_from_slice(b"BM");
    file.extend_from_slice(&((54 + data) as u32).to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&54u32.to_le_bytes());
    file.extend_from_slice(&40u32.to_le_bytes());
    file.extend_from_slice(&(width as i32).to_le_bytes());
    // Hauteur positive : les lignes se rangent du bas vers le haut.
    file.extend_from_slice(&(height as i32).to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&24u16.to_le_bytes());
    file.extend_from_slice(&[0u8; 24]);

    for y in (0..height as usize).rev() {
        let line = &pixels[y * width as usize * BYTES_PER_PIXEL..];
        for x in 0..width as usize {
            let p = &line[x * BYTES_PER_PIXEL..];
            file.extend_from_slice(&[p[2], p[1], p[0]]);
        }
        file.extend(core::iter::repeat_n(0u8, padding));
    }
    fs::write(path, file)
}

/// Écrit toutes les scènes en images dans `dir` ; rend le compte rendu.
fn dump(scene: Scene, dir: &Path) -> Result<String, String> {
    let (width, height) = Scene::RESOLUTION;
    let pixels = scene
        .render_pixels(Scene::HOST_PASS)
        .map_err(|error| format!("{} : le moteur a refusé la scène : {error:?}", scene.name()))?;
    let path = dir.join(format!("{}.bmp", scene.name()));
    fs::create_dir_all(dir)
        .and_then(|()| write_bmp(&path, &pixels, width, height))
        .map_err(|error| format!("{} : {error}", path.display()))?;
    Ok(format!("{} : {}", scene.name(), path.display()))
}

/// Lit le mode dans les arguments, programme exclu.
///
/// Exactement un mode est attendu. Sans lui, la suite ne choisit pas à la
/// place de l'appelant : réécrire des références par défaut effacerait une
/// régression au lieu de la signaler.
fn parse_mode(args: &[String]) -> Result<Mode, String> {
    let usage =
        "usage : screengine-conformance --check | --update | --print <scène> | --dump <répertoire>";
    match args {
        [only] if only == "--check" => Ok(Mode::Check),
        [only] if only == "--update" => Ok(Mode::Update),
        [print, name] if print == "--print" => Scene::named(name)
            .map(Mode::Print)
            .ok_or_else(|| format!("scène inconnue : {name}")),
        [dump, dir] if dump == "--dump" => Ok(Mode::Dump(PathBuf::from(dir))),
        _ => Err(usage.to_string()),
    }
}

/// Point d'entrée : rend 2 sur un usage invalide, 1 sur une divergence, une
/// référence absente ou un refus du moteur.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match parse_mode(&args) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    let mut dir = references();
    let step: fn(Scene, &Path) -> Result<String, String> = match mode {
        Mode::Check => check,
        Mode::Update => update,
        Mode::Dump(into) => {
            dir = into;
            dump
        }
        Mode::Print(scene) => {
            return match scene.render(Scene::HOST_PASS) {
                Ok(hash) => {
                    println!("{}", hash::format(hash));
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("le moteur a refusé la scène : {error:?}");
                    ExitCode::FAILURE
                }
            };
        }
    };

    let mut failed = false;
    for scene in Scene::ALL {
        match step(scene, &dir) {
            Ok(line) => println!("{line}"),
            Err(line) => {
                eprintln!("{line}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests;
