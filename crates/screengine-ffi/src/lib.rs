// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

//! Frontière C de Screengine.
//!
//! Ce crate convertit des types, enveloppe chaque point d'entrée de
//! `catch_unwind` et traduit les erreurs en codes. Il ne calcule rien : un
//! calcul écrit ici serait fait autrement par la conformance, qui appelle le
//! noyau sans passer par lui.
//!
//! Le contrat est dans `docs/abi.md`. `include/screengine.h` est généré à partir
//! de ce crate seul.
