// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * @file Exécute un binaire wasm WASI, pour que cargo puisse lancer les tests du
 * noyau et la conformance sur une cible wasm.
 *
 * **Ce n'est pas l'hôte web**, qui vit dans les autres fichiers de ce
 * répertoire : il charge la bibliothèque et appelle l'ABI C, là où ce script
 * exécute un programme complet. Il est ici parce que c'est le seul endroit du
 * dépôt qui porte de l'outillage Node, comme `serve.js`, et qu'un répertoire
 * pour un fichier coûterait plus qu'il ne range.
 *
 * Usage, par le runner de cargo : node --experimental-wasi-unstable-preview1
 * wasi-run.mjs <module.wasm> [arguments du programme]
 *
 * `wasm32-unknown-unknown` ne peut rien exécuter : son `std` n'a ni arguments,
 * ni sortie standard, ni système de fichiers. WASI les donne, et c'est tout ce
 * que ce script ajoute.
 */

import { WASI } from "node:wasi";
import { readFile } from "node:fs/promises";
import { argv, cwd, env, exit, stderr } from "node:process";

const [module_path, ...args] = argv.slice(2);
if (module_path === undefined) {
  stderr.write("usage : wasi-run.mjs <module.wasm> [arguments]\n");
  exit(2);
}

/**
 * Le répertoire courant, monté à la racine de la vue du programme.
 *
 * La conformance lit ses références par un chemin relatif au dépôt, et cargo la
 * lance depuis la racine : sans ce montage, chaque ouverture échouerait sur un
 * fichier introuvable, et le contrôle rendrait une erreur étrangère à ce qu'il
 * vérifie.
 */
const wasi = new WASI({
  version: "preview1",
  // Le programme s'attend à recevoir son propre nom en premier argument, comme
  // tout binaire : le harnais de test de Rust y lit celui qu'il affiche, et un
  // décalage ferait prendre le premier filtre pour le nom du binaire.
  args: [module_path, ...args],
  env,
  preopens: { "/": cwd() },
  returnOnExit: true,
});

const wasm = await WebAssembly.compile(await readFile(module_path));
const instance = await WebAssembly.instantiate(wasm, wasi.getImportObject());

// **`returnOnExit` rend le code au lieu de terminer Node**, et c'est ce qui le
// rend transmissible : sans lui, un `exit(1)` du programme sortirait de Node
// avec ce code par un chemin que rien n'intercepte, et un `exit(0)` masquerait
// tout ce qui suit. Le relayer explicitement est la seule forme où l'échec d'un
// test devient un échec de `make`.
exit(wasi.start(instance));
