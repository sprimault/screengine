// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Suite de conformance de Screengine.
//!
//! Rejoue les scènes de référence sans fenêtre, hache chaque tampon rendu et
//! compare l'empreinte à celle de `references/`. Les références sont
//! versionnées : chaque plateforme se compare aux mêmes fichiers, donc toutes
//! les plateformes entre elles.
//!
//! `--print` rend une scène et écrit son empreinte, sans rien comparer : c'est
//! le chemin Rust natif auquel les hôtes comparent la leur.

mod hash;

use std::process::ExitCode;

use screengine::{BYTES_PER_PIXEL, Config, Context};

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
    /// Le triangle en dur de l'étape 0, en 640×360 et tuiles de 64.
    ///
    /// Les hôtes rendent la même configuration : la changer ici sans eux
    /// ferait diverger toutes les empreintes à la fois.
    Triangle,
}

impl Scene {
    /// Reconnaît une scène par son nom.
    fn named(name: &str) -> Option<Self> {
        match name {
            "triangle" => Some(Self::Triangle),
            _ => None,
        }
    }

    /// Rend la scène et rend son empreinte.
    fn render(self) -> Result<u64, screengine::Error> {
        match self {
            Self::Triangle => {
                let (width, height) = (640, 360);
                let mut context = Context::new(Config {
                    max_width: width,
                    max_height: height,
                    width,
                    height,
                    tile_size: 64,
                })?;
                let mut pixels = vec![0u8; width as usize * height as usize * BYTES_PER_PIXEL];
                context.frame_end(&mut pixels, width)?;
                Ok(hash::image(&pixels, width, height, width))
            }
        }
    }
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

/// Point d'entrée : rend 2 sur un usage invalide, 1 sur une divergence ou un
/// refus du moteur.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match parse_mode(&args) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    match mode {
        Mode::Check => println!("aucune scène de référence : rien à comparer"),
        Mode::Update => println!("aucune scène de référence : rien à réécrire"),
        Mode::Print(scene) => match scene.render() {
            Ok(hash) => println!("{}", hash::format(hash)),
            Err(error) => {
                eprintln!("le moteur a refusé la scène : {error:?}");
                return ExitCode::FAILURE;
            }
        },
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
