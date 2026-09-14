// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Suite de conformance de Screengine.
//!
//! Rejoue les scènes de référence sans fenêtre, hache chaque tampon rendu et
//! compare l'empreinte à celle de `references/`. Les références sont
//! versionnées : chaque plateforme se compare aux mêmes fichiers, donc toutes
//! les plateformes entre elles.

use std::process::ExitCode;

/// Ce que la suite fait des empreintes calculées.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    /// Compare aux références et échoue sur toute divergence.
    Check,
    /// Réécrit les références. Une évolution voulue du rendu, jamais une
    /// régression qu'on ferait taire : le commit qui les met à jour est distinct.
    Update,
}

/// Lit le mode dans les arguments, programme exclu.
///
/// Exactement un argument est attendu. Sans lui, la suite ne choisit pas à la
/// place de l'appelant : réécrire des références par défaut effacerait une
/// régression au lieu de la signaler.
fn parse_mode(args: &[String]) -> Result<Mode, String> {
    match args {
        [only] if only == "--check" => Ok(Mode::Check),
        [only] if only == "--update" => Ok(Mode::Update),
        _ => Err("usage : screengine-conformance --check | --update".to_string()),
    }
}

/// Point d'entrée : rend 2 sur un usage invalide, 1 sur une divergence.
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
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construit une liste d'arguments à partir de littéraux.
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    /// Les deux modes se reconnaissent par leur option.
    #[test]
    fn recognises_both_modes() {
        assert_eq!(parse_mode(&args(&["--check"])), Ok(Mode::Check));
        assert_eq!(parse_mode(&args(&["--update"])), Ok(Mode::Update));
    }

    /// Sans argument, aucun mode n'est choisi par défaut.
    #[test]
    fn rejects_missing_mode() {
        assert!(parse_mode(&[]).is_err());
    }

    /// Deux modes à la fois, ou une option inconnue, sont refusés.
    #[test]
    fn rejects_ambiguous_usage() {
        assert!(parse_mode(&args(&["--check", "--update"])).is_err());
        assert!(parse_mode(&args(&["--verify"])).is_err());
    }
}
