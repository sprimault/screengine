// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * @file Affiche le triangle dans le navigateur.
 *
 * La mise à l'échelle appartient à l'hôte : la page dessine à la résolution
 * interne, et c'est le CSS qui l'agrandit sans lissage.
 */

import * as scg from "./screengine.js";

/** Largeur de la scène, celle de la conformance. */
const WIDTH = 640;

/** Hauteur de la scène. */
const HEIGHT = 360;

/** Côté de tuile. */
const TILE = 64;

/**
 * Écrit une ligne d'état sous l'image.
 *
 * @param {string} text
 */
function status(text) {
  document.getElementById("status").textContent = text;
}

/** Charge le module, rend une image et la dessine. */
async function main() {
  const module = await WebAssembly.compileStreaming(fetch("screengine.wasm"));
  const engine = await scg.Screengine.instantiate(module);
  if (engine.abiVersion() !== scg.SCG_ABI_VERSION) {
    status(`ABI ${engine.abiVersion()}, attendue ${scg.SCG_ABI_VERSION}`);
    return;
  }

  const e = engine.exports;
  const len = WIDTH * HEIGHT * scg.BYTES_PER_PIXEL;
  const pixels = engine.alloc(len);
  const config = engine.alloc(scg.CONFIG_SIZE);
  const out = engine.alloc(4);
  engine.writeConfig(config, { maxWidth: WIDTH, maxHeight: HEIGHT, width: WIDTH, height: HEIGHT, tileSize: TILE });

  if (e.scg_create(config, out) !== scg.SCG_OK) {
    status(engine.lastError(0));
    return;
  }
  const ctx = engine.readU32(out);

  // La même scène que la conformance : deux triangles qui partagent une arête,
  // vus de biais par la caméra par défaut, en coordonnées de monde.
  const model = engine.alloc(scg.MAT4_SIZE);
  const vertices = engine.alloc(4 * scg.VERTEX_SIZE);
  const triangles = engine.alloc(2 * scg.TRIANGLE_SIZE);
  engine.writeIdentity(model);
  engine.writeVertices(vertices, [
    [2.0, 2.5, 1.6],
    [3.5, -2.5, 1.6],
    [3.5, -2.5, -1.6],
    [2.0, 2.5, -1.6],
  ]);
  engine.writeTriangles(triangles, [
    { indices: [0, 2, 1], color: [0xe0, 0xa0, 0x30, 0xff] },
    { indices: [0, 3, 2], color: [0xa0, 0xe0, 0x30, 0xff] },
  ]);
  if (e.scg_submit(ctx, model, vertices, 4, triangles, 2) !== scg.SCG_OK) {
    status(engine.lastError(ctx));
    return;
  }

  let code;
  try {
    code = e.scg_frame_end(ctx, pixels, WIDTH);
  } catch (error) {
    // Un trap : le module ne se rappelle plus, on lit ce que le crochet de
    // panique a laissé.
    status(`${error.message} : ${engine.trapMessage()}`);
    return;
  }
  if (code !== scg.SCG_OK) {
    status(engine.lastError(ctx));
    return;
  }

  // La vue se construit après l'appel, jamais avant : il a pu agrandir la
  // mémoire et détacher l'ancienne.
  const image = new ImageData(new Uint8ClampedArray(engine.memory.buffer, pixels, len).slice(), WIDTH, HEIGHT);
  const canvas = document.getElementById("image");
  canvas.width = WIDTH;
  canvas.height = HEIGHT;
  canvas.getContext("2d", { alpha: false }).putImageData(image, 0, 0);
  status(`empreinte ${engine.fingerprint(pixels, WIDTH, HEIGHT, WIDTH)}`);

  e.scg_destroy(ctx);
  engine.free(out, 4);
  engine.free(config, scg.CONFIG_SIZE);
  engine.free(pixels, len);
}

main().catch((error) => status(String(error)));
