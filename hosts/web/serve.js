// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * @file Sert la page de l'hôte web en local, sans dépendance.
 *
 * `fetch` ne lit pas un fichier en `file://`, et `instantiateStreaming` exige
 * le type `application/wasm`. Écoute sur 127.0.0.1 seulement : c'est un
 * outil de développement, pas un serveur.
 *
 * Usage : node serve.js <répertoire> [port]
 */

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import process from "node:process";

/**
 * Les seuls types servis : ce que la page charge.
 *
 * Une extension absente de cette table n'est pas servie, et le `fetch` rend
 * alors le corps d'une 404 — que le moteur refuse comme un fichier tronqué,
 * sans rapport apparent avec le serveur. Vu en ouvrant la page, et nulle part
 * ailleurs : aucun test n'y passe.
 */
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".mesh": "application/octet-stream",
  ".world": "application/octet-stream",
};

/** Le répertoire servi, et rien au-dessus. */
const root = resolve(process.argv[2] ?? ".");

/** Le port d'écoute. */
const port = Number(process.argv[3] ?? 8080);

createServer(async (request, response) => {
  const path = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
  const file = resolve(root, "." + (path === "/" ? "/index.html" : path));
  const type = TYPES[extname(file)];

  // Un chemin qui remonterait hors du répertoire, ou un type que la page ne
  // charge pas, n'est pas servi.
  if (!file.startsWith(root + sep) || type === undefined) {
    response.writeHead(404).end();
    return;
  }
  try {
    const body = await readFile(file);
    response.writeHead(200, { "Content-Type": type }).end(body);
  } catch {
    response.writeHead(404).end();
  }
}).listen(port, "127.0.0.1", () => {
  process.stdout.write(`http://127.0.0.1:${port}/\n`);
});
