// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! L'analyse de la ligne de commande de la conformance.
//!
//! Le reste — les scènes et leurs empreintes — n'existe pas encore : il n'y a
//! rien à rendre au-delà du triangle en dur, qui est celui des hôtes.

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
