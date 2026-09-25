// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * @file Parcourt un décor dans le navigateur, au clavier.
 *
 * **L'hôte de démonstration du web, et le patron des trois autres** : il n'a
 * aucune dépendance de fenêtrage — un canvas, des événements clavier, une
 * boucle d'animation — si bien que ce qu'il montre du moteur se transpose
 * partout sans rien démêler d'une bibliothèque d'accueil.
 *
 * Le décor vient de `couloir.world`, le fichier versionné que la conformance
 * engendre : la page ne construit aucune géométrie, elle charge un bloc
 * d'octets et le soumet. C'est tout ce que l'étape des données a pour objet.
 *
 * La mise à l'échelle appartient à l'hôte : la page dessine à la résolution
 * interne, et c'est le CSS qui l'agrandit sans lissage.
 */

import * as scg from "./screengine.js";

/** Largeur interne, celle de la conformance. */
const WIDTH = 640;

/** Hauteur interne. */
const HEIGHT = 360;

/** Côté de tuile. */
const TILE = 64;

/** Côté des damiers, en texels. */
const TEXTURE_SIDE = 64;

/** Vitesse de déplacement, en unités de monde par seconde. */
const SPEED = 6.0;

/** Vitesse de rotation, en radians par seconde. */
const TURN = 2.0;

/**
 * Écrit une ligne d'état sous l'image.
 *
 * @param {string} text
 */
function status(text) {
  document.getElementById("status").textContent = text;
}

/**
 * Un damier de `side` texels, ses cases de `cell`.
 *
 * Le même motif que la suite de conformance, teinte pour teinte : la page
 * montre le décor de la scène de référence, et une teinte inventée ici en
 * ferait une autre image.
 *
 * @param {number} side côté en texels
 * @param {number} cell côté d'une case
 * @returns {Uint8Array}
 */
function makeChecker(side, cell) {
  const texels = new Uint8Array(side * side * 4);
  for (let v = 0; v < side; v++) {
    for (let u = 0; u < side; u++) {
      const base = (v * side + u) * 4;
      const edge = u % cell === 0 || v % cell === 0;
      const dark = (Math.floor(u / cell) + Math.floor(v / cell)) % 2 === 0;
      const [r, g, b] = edge
        ? [0xf0, 0xe0, 0xa0]
        : dark
          ? [0x30, 0x38, 0x50]
          : [0x90, 0x70, 0x50];
      texels[base] = r;
      texels[base + 1] = g;
      texels[base + 2] = b;
      texels[base + 3] = 0xff;
    }
  }
  return texels;
}

/**
 * Charge une texture dans le moteur et rend son handle.
 *
 * @param {scg.Screengine} engine
 * @param {Uint8Array} texels
 * @returns {number}
 */
function loadTexture(engine, texels) {
  const e = engine.exports;
  const desc = engine.alloc(scg.TEXTURE_DESC_SIZE);
  const block = engine.alloc(texels.length);
  const out = engine.alloc(4);

  engine.writeTextureDesc(desc, TEXTURE_SIDE, TEXTURE_SIDE);
  engine.bytes().set(texels, block);
  if (e.scg_texture_load(desc, block, texels.length, out) !== scg.SCG_OK) {
    throw new Error(`texture refusée : ${engine.lastError(0)}`);
  }
  const texture = engine.readU32(out);

  engine.free(out, 4);
  engine.free(block, texels.length);
  engine.free(desc, scg.TEXTURE_DESC_SIZE);
  return texture;
}

/**
 * L'état du clavier : les touches maintenues.
 *
 * Un ensemble et non des drapeaux nommés : la boucle interroge ce que la touche
 * commande, et une disposition de clavier différente ne change qu'ici.
 *
 * @returns {Set<string>}
 */
function trackKeys() {
  const held = new Set();
  const commands = new Set([
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "KeyW",
    "KeyS",
    "KeyA",
    "KeyD",
  ]);
  addEventListener("keydown", (event) => {
    if (commands.has(event.code)) {
      held.add(event.code);
      // Les flèches font défiler la page sans cela, et le décor part avec.
      event.preventDefault();
    }
  });
  addEventListener("keyup", (event) => held.delete(event.code));
  // Une touche relâchée hors de la page ne rend jamais son `keyup` : sans ceci,
  // le déplacement continue tout seul quand on revient.
  addEventListener("blur", () => held.clear());
  return held;
}

/**
 * Où sont posées les caisses : abscisse, ordonnée, et l'angle qui les tourne.
 *
 * Le décor et les accessoires sont deux ressources différentes, chargées
 * séparément et soumises séparément : la carte porte les murs, le maillage ce
 * qu'on y pose. C'est ce que l'étape des données a construit, et un couloir vide
 * n'en montrerait que la moitié.
 */
const CRATES = [
  [6.0, -1.2, 0.4],
  [11.0, 1.4, -0.7],
  [17.0, -0.6, 1.1],
];

/**
 * L'échelle des caisses.
 *
 * **Le maillage fait deux unités de côté**, et le couloir six de large : posée
 * telle quelle, une caisse en occupe le tiers. Le fichier ne se redimensionne
 * pas — c'est celui de la scène de conformance, et son empreinte est figée —,
 * donc l'échelle va dans la matrice de modèle, qui est faite pour ça.
 */
const CRATE_SCALE = 0.5;

/** La cote du centre d'une caisse : sa demi-hauteur au-dessus du sol, à -1,5. */
const CRATE_Z = -1.0;

/**
 * La matrice d'une caisse : une rotation autour de la verticale mise à
 * l'échelle, puis une translation. Par colonnes, comme l'ABI l'attend.
 *
 * Les coefficients viennent de `Math`, et c'est permis ici : une démonstration
 * n'est comparée à aucune empreinte, là où une scène de conformance exige les
 * mêmes bits sur les quatre cibles.
 *
 * @param {number[]} placement abscisse, ordonnée, angle
 * @returns {number[]} les seize coefficients
 */
function crateModel([x, y, angle]) {
  const c = Math.cos(angle) * CRATE_SCALE;
  const s = Math.sin(angle) * CRATE_SCALE;
  return [c, s, 0, 0, -s, c, 0, 0, 0, 0, CRATE_SCALE, 0, x, y, CRATE_Z, 1];
}

/**
 * Le quaternion d'une rotation de `angle` autour de l'axe vertical.
 *
 * Écrit ici plutôt que demandé au moteur : ses tables trigonométriques ne
 * traversent pas l'ABI, et une caméra est une valeur d'hôte — c'est lui qui
 * décide où elle regarde.
 *
 * @param {number} angle en radians
 * @returns {number[]} `x, y, z, w`, la partie réelle en dernier
 */
function yaw(angle) {
  return [0, 0, Math.sin(angle / 2), Math.cos(angle / 2)];
}

/** Charge le module et le décor, puis parcourt le couloir. */
async function main() {
  const module = await WebAssembly.compileStreaming(fetch("screengine.wasm"));
  const engine = await scg.Screengine.instantiate(module);
  if (engine.abiVersion() !== scg.SCG_ABI_VERSION) {
    status(`ABI ${engine.abiVersion()}, attendue ${scg.SCG_ABI_VERSION}`);
    return;
  }

  const e = engine.exports;
  const out = engine.alloc(4);
  const config = engine.alloc(scg.CONFIG_SIZE);
  engine.writeConfig(config, {
    maxWidth: WIDTH,
    maxHeight: HEIGHT,
    width: WIDTH,
    height: HEIGHT,
    tileSize: TILE,
  });
  if (e.scg_create(config, out) !== scg.SCG_OK) {
    status(engine.lastError(0));
    return;
  }
  const ctx = engine.readU32(out);

  // Le décor : un bloc d'octets, que le moteur copie. La page ne garde pas le
  // sien — c'est toute la raison pour laquelle le chargement prend des octets
  // et non un chemin.
  const file = new Uint8Array(await (await fetch("couloir.world")).arrayBuffer());
  const block = engine.alloc(file.length);
  engine.bytes().set(file, block);
  if (e.scg_world_load(block, file.length, out) !== scg.SCG_OK) {
    status(`carte refusée : ${engine.lastError(0)}`);
    return;
  }
  const world = engine.readU32(out);
  engine.free(block, file.length);

  // Une texture par matériau, dans l'ordre que la carte déclare : l'hôte lit
  // les noms, charge ce qu'il veut, et passe les handles dans cet ordre.
  e.scg_world_material_count(world, out);
  const materials = engine.readU32(out);
  const slots = engine.alloc(materials * 4);
  const textures = [];
  for (let i = 0; i < materials; i++) {
    const len = engine.alloc(4);
    e.scg_world_material_name(world, i, 0, 0, len);
    const size = engine.readU32(len);
    const name = engine.alloc(size + 1);
    e.scg_world_material_name(world, i, name, size + 1, len);
    const text = new TextDecoder().decode(engine.bytes().subarray(name, name + size));
    engine.free(name, size + 1);
    engine.free(len, 4);

    // Le nom décide du motif, et c'est l'hôte qui en décide : le moteur ne
    // connaît que des emplacements à remplir.
    textures.push(loadTexture(engine, makeChecker(TEXTURE_SIDE, text === "mur" ? 16 : 8)));
  }
  // La vue se construit après les chargements : chacun a pu agrandir la
  // mémoire, ce qui détache toute vue prise avant.
  const table = new DataView(engine.memory.buffer, slots, materials * 4);
  textures.forEach((texture, i) => table.setUint32(i * 4, texture, true));

  // Le maillage des caisses, chargé comme la carte : un bloc d'octets que le
  // moteur copie. Ses deux emplacements portent des noms, et l'hôte décide de
  // ce qu'il met dedans — ici un damier sur les côtés, rien sur le dessus, si
  // bien que les couleurs du fichier y décident.
  const meshFile = new Uint8Array(await (await fetch("caisse.mesh")).arrayBuffer());
  const meshBlock = engine.alloc(meshFile.length);
  engine.bytes().set(meshFile, meshBlock);
  if (e.scg_mesh_load(meshBlock, meshFile.length, out) !== scg.SCG_OK) {
    status(`maillage refusé : ${engine.lastError(0)}`);
    return;
  }
  const mesh = engine.readU32(out);
  engine.free(meshBlock, meshFile.length);

  const crateSlots = engine.alloc(8);
  const crateTexture = loadTexture(engine, makeChecker(TEXTURE_SIDE, 8));
  const crateTable = new DataView(engine.memory.buffer, crateSlots, 8);
  crateTable.setUint32(0, crateTexture, true);
  crateTable.setUint32(4, 0, true);

  const pixels = engine.alloc(WIDTH * HEIGHT * scg.BYTES_PER_PIXEL);
  const camera = engine.alloc(scg.CAMERA_SIZE);
  const model = engine.alloc(scg.MAT4_SIZE);
  const crateModelPtr = engine.alloc(scg.MAT4_SIZE);
  engine.writeIdentity(model);

  const canvas = document.getElementById("image");
  canvas.width = WIDTH;
  canvas.height = HEIGHT;
  const surface = canvas.getContext("2d", { alpha: false });

  const held = trackKeys();
  const position = [0, 0, 0];
  let angle = 0;
  let previous = performance.now();
  let frames = 0;
  let since = previous;

  /**
   * Une image : entrées, caméra, soumission, rendu.
   *
   * @param {number} now l'horodatage que le navigateur donne
   */
  function frame(now) {
    const dt = Math.min((now - previous) / 1000, 0.1);
    previous = now;

    if (held.has("ArrowLeft") || held.has("KeyA")) {
      angle += TURN * dt;
    }
    if (held.has("ArrowRight") || held.has("KeyD")) {
      angle -= TURN * dt;
    }
    const forward = (held.has("ArrowUp") || held.has("KeyW") ? 1 : 0)
      - (held.has("ArrowDown") || held.has("KeyS") ? 1 : 0);
    position[0] += Math.cos(angle) * forward * SPEED * dt;
    position[1] += Math.sin(angle) * forward * SPEED * dt;

    engine.writeCamera(camera, {
      position,
      orientation: yaw(angle),
      fovY: 1.2,
      nearPlane: 0.1,
    });
    if (e.scg_set_camera(ctx, camera) !== scg.SCG_OK) {
      status(engine.lastError(ctx));
      return;
    }
    if (e.scg_submit_world(ctx, model, world, slots, materials) !== scg.SCG_OK) {
      status(engine.lastError(ctx));
      return;
    }

    // Les caisses par-dessus, chacune avec sa matrice : la même ressource
    // dessinée trois fois, ce qu'un décor fait de ses accessoires.
    for (const placement of CRATES) {
      engine.writeMat4(crateModelPtr, crateModel(placement));
      if (e.scg_submit_mesh(ctx, crateModelPtr, mesh, crateSlots, 2) !== scg.SCG_OK) {
        status(engine.lastError(ctx));
        return;
      }
    }

    // Par tuiles, comme un hôte qui voudrait les répartir : le web n'a qu'un
    // thread, mais le découpage est celui que les autres emprunteront.
    if (e.scg_frame_begin(ctx, out) !== scg.SCG_OK) {
      status(engine.lastError(ctx));
      return;
    }
    const tiles = engine.readU32(out);
    for (let i = 0; i < tiles; i++) {
      if (e.scg_frame_tile(ctx, i, pixels, WIDTH) !== scg.SCG_OK) {
        status(engine.lastError(ctx));
        return;
      }
    }
    if (e.scg_frame_end(ctx, pixels, WIDTH) !== scg.SCG_OK) {
      status(engine.lastError(ctx));
      return;
    }

    // La vue se construit après l'appel, jamais avant : il a pu agrandir la
    // mémoire et détacher l'ancienne.
    const view = new Uint8ClampedArray(
      engine.memory.buffer,
      pixels,
      WIDTH * HEIGHT * scg.BYTES_PER_PIXEL,
    );
    surface.putImageData(new ImageData(view.slice(), WIDTH, HEIGHT), 0, 0);

    frames++;
    if (now - since >= 1000) {
      const rate = Math.round((frames * 1000) / (now - since));
      status(`${rate} images/s — flèches ou ZQSD pour avancer et tourner`);
      frames = 0;
      since = now;
    }
    requestAnimationFrame(frame);
  }

  status("flèches ou ZQSD pour avancer et tourner");
  requestAnimationFrame(frame);
}

main().catch((error) => status(String(error)));
