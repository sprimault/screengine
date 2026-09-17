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

use screengine::{BYTES_PER_PIXEL, Config, Context, Frame, Rect, Rows};

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
                let mut scratch = vec![0u32; width as usize * height as usize];
                let image = Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                };
                frame.region(image, &mut scratch, &mut Rows::new(pixels, width))
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
}

/// Les scènes que la suite sait rendre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scene {
    /// Le triangle en dur de l'étape 0, en 640×360.
    ///
    /// Les hôtes rendent la même configuration, en tuiles de 64 : la changer ici
    /// sans eux ferait diverger toutes les empreintes à la fois.
    Triangle,
}

impl Scene {
    /// Toutes les scènes, dans l'ordre où `--check` les rejoue.
    const ALL: [Self; 1] = [Self::Triangle];

    /// La passe que `--print` utilise, celle des hôtes.
    const HOST_PASS: Pass = Pass::Tiles64;

    /// Le nom de la scène, qui est aussi celui de sa référence.
    fn name(self) -> &'static str {
        match self {
            Self::Triangle => "triangle",
        }
    }

    /// Reconnaît une scène par son nom.
    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scene| scene.name() == name)
    }

    /// Rend la scène par `pass` et rend son empreinte.
    fn render(self, pass: Pass) -> Result<u64, screengine::Error> {
        match self {
            Self::Triangle => {
                let (width, height) = (640, 360);
                let mut context = Context::new(Config {
                    max_width: width,
                    max_height: height,
                    width,
                    height,
                    tile_size: pass.tile_size(),
                })?;
                let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];
                pass.render(context.frame_begin()?, &mut pixels, width, height)?;
                Ok(hash::image(&pixels, width, height, width))
            }
        }
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

/// Lit le mode dans les arguments, programme exclu.
///
/// Exactement un mode est attendu. Sans lui, la suite ne choisit pas à la
/// place de l'appelant : réécrire des références par défaut effacerait une
/// régression au lieu de la signaler.
fn parse_mode(args: &[String]) -> Result<Mode, String> {
    let usage = "usage : screengine-conformance --check | --update | --print <scène>";
    match args {
        [only] if only == "--check" => Ok(Mode::Check),
        [only] if only == "--update" => Ok(Mode::Update),
        [print, name] if print == "--print" => Scene::named(name)
            .map(Mode::Print)
            .ok_or_else(|| format!("scène inconnue : {name}")),
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

    let dir = references();
    let step: fn(Scene, &Path) -> Result<String, String> = match mode {
        Mode::Check => check,
        Mode::Update => update,
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
