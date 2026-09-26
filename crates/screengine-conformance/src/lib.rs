// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce que la conformance partage entre son binaire, ses tests et les hôtes.
//!
//! Le binaire rend les scènes ; les hôtes chargent le même fichier de maillage
//! pour prouver que leur point d'entrée franchit réellement la frontière. Le
//! fichier est versionné dans `hosts/`, et **il n'est jamais la source de
//! vérité** : c'est ce module qui l'engendre, un test le compare octet pour
//! octet, et `make mesh` le réécrit. Même règle que le header — un fichier
//! périmé échoue franchement au lieu de dériver.

pub mod mesh_file;
pub mod rooms_file;
pub mod world_file;
