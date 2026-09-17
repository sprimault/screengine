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
  const module = await WebAssembly.compileStreaming(fetch("screengine_ffi.wasm"));
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
