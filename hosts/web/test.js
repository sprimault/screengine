// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * @file Hôte wasm de Screengine, sans fenêtre, sous Node.
 *
 * Il charge le module comme le ferait un navigateur, sans aucun import, et
 * vérifie ce que les tests Rust ne voient pas : les exports réels du `.wasm`,
 * l'écriture des structures dans la mémoire linéaire, `scg_buffer_alloc`, les
 * pointeurs rendus signés et les vues détachées par la croissance de la
 * mémoire. Il écrit sur la sortie standard l'empreinte du triangle, que
 * `make test-wasm` compare à celle du chemin Rust ; tout le reste part sur la
 * sortie d'erreur.
 *
 * Ce que les hôtes C et C++ vérifient en plus n'a pas de sens ici : wasm n'a
 * pas de registre flottant, et une panique y est un trap, jamais un code.
 *
 * La scène est celle de `screengine-conformance --print arete` : 640×360,
 * tuiles de 64.
 *
 * Usage : node test.js <module.wasm> <screengine.h>
 */

import { readFile } from "node:fs/promises";
import process from "node:process";

import * as scg from "./screengine.js";

/** Largeur de la scène. */
const WIDTH = 640;

/** Hauteur de la scène. */
const HEIGHT = 360;

/** Côté de tuile de la scène. */
const TILE = 64;

/** Le stride de l'hôte, plus grand que la largeur pour qu'une fin de ligne existe. */
const STRIDE = WIDTH + 3;

/** Marge sentinelle avant et après le tampon, en octets. */
const GUARD = 64;

/** L'octet sentinelle, qui ne ressemble à aucune couleur du rendu. */
const SENTINEL = 0xa5;

/** Nombre de vérifications en échec. */
let failures = 0;

/**
 * Enregistre une vérification, et dit laquelle a échoué.
 *
 * @param {boolean} ok
 * @param {string} what
 */
function check(ok, what) {
  if (!ok) {
    process.stderr.write(`échec : ${what}\n`);
    failures++;
  }
}

/**
 * Une configuration valide.
 *
 * @returns {scg.Config}
 */
function sceneConfig() {
  return { maxWidth: WIDTH, maxHeight: HEIGHT, width: WIDTH, height: HEIGHT, tileSize: TILE };
}

/**
 * Le quadrilatère de la scène `arete`, en coordonnées de monde : X vers l'est,
 * Z en haut, la caméra par défaut le regardant depuis l'origine.
 *
 * Ce sont les valeurs de la scène de conformance, écrites ici en JavaScript et
 * poussées dans la mémoire linéaire octet par octet : c'est la comparaison des
 * deux empreintes qui dit que les décalages du header sont reproduits juste.
 */
const SCENE_VERTICES = [
  [2.0, 2.5, 1.6],
  [3.5, -2.5, 1.6],
  [3.5, -2.5, -1.6],
  [2.0, 2.5, -1.6],
];

/** Deux triangles qui partagent l'arête des sommets 0 et 2. */
const SCENE_TRIANGLES = [
  { indices: [0, 2, 1], color: [0xe0, 0xa0, 0x30, 0xff] },
  { indices: [0, 3, 2], color: [0xa0, 0xe0, 0x30, 0xff] },
];

/**
 * Soumet la scène au contexte, tampons alloués et libérés autour de l'appel.
 *
 * @param {scg.Screengine} engine
 * @param {number} ctx
 * @returns {boolean} vrai si la soumission a été acceptée
 */
function submitScene(engine, ctx) {
  const vertexBytes = SCENE_VERTICES.length * scg.VERTEX_SIZE;
  const triangleBytes = SCENE_TRIANGLES.length * scg.TRIANGLE_SIZE;
  const model = engine.alloc(scg.MAT4_SIZE);
  const vertices = engine.alloc(vertexBytes);
  const triangles = engine.alloc(triangleBytes);

  engine.writeIdentity(model);
  engine.writeVertices(vertices, SCENE_VERTICES);
  engine.writeTriangles(triangles, SCENE_TRIANGLES);
  const code = engine.exports.scg_submit(
    ctx,
    model,
    vertices,
    SCENE_VERTICES.length,
    triangles,
    SCENE_TRIANGLES.length,
  );

  engine.free(model, scg.MAT4_SIZE);
  engine.free(vertices, vertexBytes);
  engine.free(triangles, triangleBytes);
  return code === scg.SCG_OK;
}

/** Côté de la texture du sol, en texels, et côté d'une de ses cases. */
const FLOOR_SIDE = 64;
const FLOOR_CELL = 8;

/**
 * Le sol de la scène `texture`, à 1,2 unité sous la caméra, habillé à huit
 * texels par unité de monde. Mêmes valeurs que la scène de conformance.
 */
const FLOOR_VERTICES = [
  [2.0, -24.0, -1.2, 2.0 * 8.0, -24.0 * 8.0],
  [60.0, -24.0, -1.2, 60.0 * 8.0, -24.0 * 8.0],
  [60.0, 24.0, -1.2, 60.0 * 8.0, 24.0 * 8.0],
  [2.0, 24.0, -1.2, 2.0 * 8.0, 24.0 * 8.0],
];

/** Ses deux triangles, blancs : c'est la texture qui porte la couleur. */
const FLOOR_TRIANGLES = [
  { indices: [0, 1, 2], color: [0xff, 0xff, 0xff, 0xff] },
  { indices: [0, 2, 3], color: [0xff, 0xff, 0xff, 0xff] },
];

/**
 * Le damier procédural, teinte pour teinte comme la suite de conformance
 * l'écrit : c'est lui qui décide de l'empreinte.
 *
 * @returns {Uint8Array} `FLOOR_SIDE` au carré texels de quatre octets
 */
function makeChecker() {
  const texels = new Uint8Array(FLOOR_SIDE * FLOOR_SIDE * 4);
  for (let v = 0; v < FLOOR_SIDE; v++) {
    for (let u = 0; u < FLOOR_SIDE; u++) {
      const base = (v * FLOOR_SIDE + u) * 4;
      const edge = u % FLOOR_CELL === 0 || v % FLOOR_CELL === 0;
      const dark = (Math.floor(u / FLOOR_CELL) + Math.floor(v / FLOOR_CELL)) % 2 === 0;
      const rgb = edge ? [0xf0, 0xe0, 0xa0] : dark ? [0x30, 0x38, 0x50] : [0x90, 0x70, 0x50];
      texels.set(rgb, base);
      texels[base + 3] = 0xff;
    }
  }
  return texels;
}

/**
 * Rend la scène texturée sous le filtrage demandé, ou `null` en cas d'échec.
 *
 * La texture est détruite avant le rendu, à dessein : le moteur en garde sa
 * propre référence jusqu'à la fin de l'image, et l'empreinte le prouve.
 *
 * La géométrie ne dépend pas du filtrage : un écart entre les deux empreintes
 * ne peut donc venir que de lui.
 *
 * @param {scg.Screengine} engine
 * @param {number} filter
 * @returns {string | null}
 */
function renderTextured(engine, filter) {
  const e = engine.exports;
  const texels = makeChecker();
  const desc = engine.alloc(scg.TEXTURE_DESC_SIZE);
  const block = engine.alloc(texels.length);
  const out = engine.alloc(4);
  const config = engine.alloc(scg.CONFIG_SIZE);

  engine.writeTextureDesc(desc, FLOOR_SIDE, FLOOR_SIDE);
  engine.bytes().set(texels, block);
  const loaded = e.scg_texture_load(desc, block, texels.length, out);
  check(loaded === scg.SCG_OK, "la texture se charge sans contexte");
  const texture = engine.readU32(out);

  engine.writeConfig(config, sceneConfig());
  if (loaded !== scg.SCG_OK || e.scg_create(config, out) !== scg.SCG_OK) {
    check(false, "création du contexte texturé");
    return null;
  }
  const ctx = engine.readU32(out);
  check(e.scg_set_filter(ctx, filter) === scg.SCG_OK, "le filtrage se règle");

  const vertexBytes = FLOOR_VERTICES.length * scg.VERTEX_UV_SIZE;
  const triangleBytes = FLOOR_TRIANGLES.length * scg.TRIANGLE_SIZE;
  const model = engine.alloc(scg.MAT4_SIZE);
  const vertices = engine.alloc(vertexBytes);
  const triangles = engine.alloc(triangleBytes);
  engine.writeIdentity(model);
  engine.writeVerticesUv(vertices, FLOOR_VERTICES);
  engine.writeTriangles(triangles, FLOOR_TRIANGLES);

  const submitted = e.scg_submit_textured(
    ctx,
    model,
    vertices,
    FLOOR_VERTICES.length,
    triangles,
    FLOOR_TRIANGLES.length,
    texture,
  );
  check(submitted === scg.SCG_OK, "le lot texturé est accepté");
  e.scg_texture_destroy(texture);

  const pixels = engine.alloc(STRIDE * HEIGHT * scg.BYTES_PER_PIXEL);
  const code = e.scg_frame_end(ctx, pixels, STRIDE);
  check(code === scg.SCG_OK, "l'image texturée se rend");
  const hash = code === scg.SCG_OK
    ? engine.fingerprint(pixels, WIDTH, HEIGHT, STRIDE)
    : null;

  e.scg_destroy(ctx);
  return hash;
}

/** Côté de la lightmap de la scène `lumiere`, en texels. */
const LIGHT_SIDE = 16;

/**
 * Le sol de `lumiere`, texturé comme le précédent et éclairé par-dessus.
 *
 * Les coordonnées de lightmap vont d'un demi-texel à un demi-texel du bord
 * opposé : une lightmap ne se pave pas, et le bilinéaire irait autrement
 * chercher son voisin par le repli.
 */
const LIT_FLOOR = [
  [2.0, -16.0, -1.2, 2.0 * 8.0, -16.0 * 8.0, 0.5, 0.5],
  [40.0, -16.0, -1.2, 40.0 * 8.0, -16.0 * 8.0, 15.5, 0.5],
  [40.0, 16.0, -1.2, 40.0 * 8.0, 16.0 * 8.0, 15.5, 15.5],
  [2.0, 16.0, -1.2, 2.0 * 8.0, 16.0 * 8.0, 0.5, 15.5],
];

/**
 * Le mur du fond, sans texture : c'est la couleur du triangle qui tient lieu
 * de texel, et ses coordonnées de texture sont donc nulles.
 */
const LIT_WALL = [
  [40.0, -16.0, -1.2, 0.0, 0.0, 0.5, 0.5],
  [40.0, -16.0, 10.0, 0.0, 0.0, 15.5, 0.5],
  [40.0, 16.0, 10.0, 0.0, 0.0, 15.5, 15.5],
  [40.0, 16.0, -1.2, 0.0, 0.0, 0.5, 15.5],
];

/** Ses deux triangles, dont la couleur est celle du mur. */
const WALL_TRIANGLES = [
  { indices: [0, 1, 2], color: [0xc0, 0xb0, 0x90, 0xff] },
  { indices: [0, 2, 3], color: [0xc0, 0xb0, 0x90, 0xff] },
];

/**
 * Le dégradé de lightmap, recopié de la suite de conformance.
 *
 * Les deux axes n'y font pas la même chose : un dégradé symétrique laisserait
 * passer un axe échangé entre les deux jeux de coordonnées.
 *
 * @returns {Uint8Array} `LIGHT_SIDE` au carré texels de quatre octets
 */
function makeGradient() {
  const luxels = new Uint8Array(LIGHT_SIDE * LIGHT_SIDE * 4);
  const scale = (c) => 32 + Math.floor((c * 223) / (LIGHT_SIDE - 1));
  for (let v = 0; v < LIGHT_SIDE; v++) {
    for (let u = 0; u < LIGHT_SIDE; u++) {
      const base = (v * LIGHT_SIDE + u) * 4;
      luxels.set([scale(u), scale(Math.floor((u + v) / 2)), scale(v)], base);
      luxels[base + 3] = 0xff;
    }
  }
  return luxels;
}

/**
 * Rend la scène éclairée, ou `null` en cas d'échec.
 *
 * Deux lots : le sol, texturé et éclairé, puis le mur, éclairé seul. Le second
 * passe une texture nulle, ce que ce point d'entrée accepte là où
 * `scg_submit_textured` la refuse — l'asymétrie que le header signale, et que
 * cet hôte exerce pour de bon.
 *
 * @param {scg.Screengine} engine
 * @returns {string | null}
 */
function renderLit(engine) {
  const e = engine.exports;
  const out = engine.alloc(4);
  const desc = engine.alloc(scg.TEXTURE_DESC_SIZE);

  const texels = makeChecker();
  const texelBlock = engine.alloc(texels.length);
  engine.writeTextureDesc(desc, FLOOR_SIDE, FLOOR_SIDE);
  engine.bytes().set(texels, texelBlock);
  const texLoaded = e.scg_texture_load(desc, texelBlock, texels.length, out);
  check(texLoaded === scg.SCG_OK, "la texture du sol se charge");
  const texture = engine.readU32(out);

  const luxels = makeGradient();
  const luxelBlock = engine.alloc(luxels.length);
  engine.writeTextureDesc(desc, LIGHT_SIDE, LIGHT_SIDE);
  engine.bytes().set(luxels, luxelBlock);
  const lightLoaded = e.scg_texture_load(desc, luxelBlock, luxels.length, out);
  check(lightLoaded === scg.SCG_OK, "la lightmap se charge par le meme chemin");
  const lightmap = engine.readU32(out);

  const config = engine.alloc(scg.CONFIG_SIZE);
  engine.writeConfig(config, sceneConfig());
  if (
    texLoaded !== scg.SCG_OK ||
    lightLoaded !== scg.SCG_OK ||
    e.scg_create(config, out) !== scg.SCG_OK
  ) {
    check(false, "création du contexte éclairé");
    return null;
  }
  const ctx = engine.readU32(out);

  const model = engine.alloc(scg.MAT4_SIZE);
  engine.writeIdentity(model);
  const submit = (corners, batch, tex) => {
    const vertices = engine.alloc(corners.length * scg.VERTEX_UV2_SIZE);
    const triangles = engine.alloc(batch.length * scg.TRIANGLE_SIZE);
    engine.writeVerticesUv2(vertices, corners);
    engine.writeTriangles(triangles, batch);
    return e.scg_submit_lit(
      ctx,
      model,
      vertices,
      corners.length,
      triangles,
      batch.length,
      tex,
      lightmap,
    );
  };

  // Une lightmap nulle est refusée, elle : sans elle, ce lot n'a rien à faire
  // sur ce chemin.
  const vertices = engine.alloc(LIT_FLOOR.length * scg.VERTEX_UV2_SIZE);
  const triangles = engine.alloc(FLOOR_TRIANGLES.length * scg.TRIANGLE_SIZE);
  engine.writeVerticesUv2(vertices, LIT_FLOOR);
  engine.writeTriangles(triangles, FLOOR_TRIANGLES);
  const refused = e.scg_submit_lit(
    ctx,
    model,
    vertices,
    LIT_FLOOR.length,
    triangles,
    FLOOR_TRIANGLES.length,
    texture,
    0,
  );
  check(refused === scg.SCG_ERR_NULL, "une lightmap nulle est refusée");

  check(submit(LIT_FLOOR, FLOOR_TRIANGLES, texture) === scg.SCG_OK,
    "le sol texturé et éclairé est accepté");
  check(submit(LIT_WALL, WALL_TRIANGLES, 0) === scg.SCG_OK,
    "le mur uni et éclairé est accepté");

  e.scg_texture_destroy(texture);
  e.scg_texture_destroy(lightmap);

  const pixels = engine.alloc(STRIDE * HEIGHT * scg.BYTES_PER_PIXEL);
  const code = e.scg_frame_end(ctx, pixels, STRIDE);
  check(code === scg.SCG_OK, "l'image éclairée se rend");
  const hash = code === scg.SCG_OK
    ? engine.fingerprint(pixels, WIDTH, HEIGHT, STRIDE)
    : null;

  e.scg_destroy(ctx);
  return hash;
}

/**
 * Vrai si le message se décode en UTF-8 strict, est non vide si `expectText`,
 * et ne contient aucun caractère de contrôle venu d'un tampon non initialisé.
 *
 * @param {() => string} read lecture du message
 * @param {boolean} expectText
 * @returns {boolean}
 */
function messageOk(read, expectText) {
  let text;
  try {
    text = read();
  } catch {
    return false;
  }
  return expectText === text.length > 0 && !/[\u0000-\u001f]/.test(text);
}

/**
 * Les exports réels du `.wasm` : aucun import, les sept fonctions et la
 * mémoire. C'est l'équivalent du contrôle de chargement dynamique de l'hôte
 * C++ — un module qui réclamerait un import `env` ou une colle générée
 * passerait tous les autres contrôles depuis Node, et échouerait chez un
 * intégrateur.
 *
 * @param {scg.Screengine} engine
 */
function checkModule(engine) {
  check(WebAssembly.Module.imports(engine.module).length === 0, "le module n'a aucun import");
  const names = new Set(WebAssembly.Module.exports(engine.module).map((e) => e.name));
  for (const name of [...scg.EXPORTS, "memory"]) {
    check(names.has(name), `le module exporte ${name}`);
  }
}

/**
 * Les constantes recopiées dans `screengine.js` sont celles du header.
 *
 * @param {string} header texte de `include/screengine.h`
 */
function checkConstants(header) {
  const defines = [...header.matchAll(/^#define (SCG_\w+) (-?\d+)$/gm)];
  check(defines.length > 0, "le header définit des constantes SCG_");
  for (const [, name, value] of defines) {
    check(scg[name] === Number(value), `${name} vaut ${value} comme dans le header`);
  }
}

/**
 * La liste des exports attendus est exactement celle des fonctions du header.
 *
 * Elle était recopiée à la main, alors que les constantes, elles, sont
 * dérivées : une fonction ajoutée à l'ABI n'entrait dans cette liste que si
 * quelqu'un y pensait, et son absence ne se voyait nulle part — le contrôle des
 * exports du module ne vérifie que ce que la liste nomme.
 *
 * @param {string} header texte de `include/screengine.h`
 */
function checkExportList(header) {
  const declared = new Set(
    [...header.matchAll(/^[a-z].*?\b(scg_\w+)\(/gm)].map(([, name]) => name),
  );
  check(declared.size > 10, `le header déclare ${declared.size} fonctions scg_`);

  for (const name of declared) {
    check(scg.EXPORTS.includes(name), `${name} est dans la liste des exports`);
  }
  for (const name of scg.EXPORTS) {
    check(declared.has(name), `${name} est déclarée par le header`);
  }
}

/**
 * Les tailles de structures écrites dans `screengine.js` sont celles que le
 * header affirme.
 *
 * Les `#define` ne suffisent pas : cette liaison écrit les structures octet par
 * octet dans la mémoire linéaire, et c'est une taille fausse, jamais une
 * constante fausse, qui lui ferait poser un champ à côté. Le header porte déjà
 * ces tailles, en assertions que les hôtes C et C++ compilent ; ici on les lit.
 *
 * @param {string} header texte de `include/screengine.h`
 */
function checkLayout(header) {
  const sizes = {
    ScgContextConfig: scg.CONFIG_SIZE,
    ScgVertex: scg.VERTEX_SIZE,
    ScgVertexUv: scg.VERTEX_UV_SIZE,
    ScgVertexUv2: scg.VERTEX_UV2_SIZE,
    ScgTextureDesc: scg.TEXTURE_DESC_SIZE,
    ScgTriangle: scg.TRIANGLE_SIZE,
    ScgMat4: scg.MAT4_SIZE,
  };
  const asserts = [
    ...header.matchAll(/LAYOUT_ASSERT\(sizeof\((\w+)\) == (\d+)/g),
  ];
  check(asserts.length > 0, "le header affirme des tailles de structures");

  let vues = 0;
  for (const [, name, value] of asserts) {
    if (name in sizes) {
      check(sizes[name] === Number(value), `sizeof(${name}) vaut ${value} comme dans le header`);
      vues += 1;
    }
  }
  check(
    vues === Object.keys(sizes).length,
    `les ${Object.keys(sizes).length} tailles de la liaison sont dans le header, ${vues} vues`,
  );
}

/**
 * Les refus, vus depuis JavaScript : codes exacts et messages lisibles. La
 * configuration et le paramètre de sortie vivent eux-mêmes dans la mémoire
 * linéaire, seule mémoire que le module adresse.
 *
 * @param {scg.Screengine} engine
 */
function checkRefusals(engine) {
  const e = engine.exports;
  const config = engine.alloc(scg.CONFIG_SIZE);
  const out = engine.alloc(4);
  const small = engine.alloc(4 * 4);
  const untouched = 0xa5a5a5a5;

  engine.writeU32(out, untouched);
  check(e.scg_create(0, out) === scg.SCG_ERR_NULL, "configuration nulle refusée par SCG_ERR_NULL");
  check(messageOk(() => engine.lastError(0), true), "message sans contexte après une configuration nulle");

  engine.writeConfig(config, { ...sceneConfig(), tileSize: 48 });
  check(e.scg_create(config, out) === scg.SCG_ERR_INVALID_ARGUMENT, "taille de tuile 48 refusée");
  check(engine.readU32(out) === untouched, "rien n'est écrit dans le paramètre de sortie après un refus");

  engine.writeConfig(config, { ...sceneConfig(), reserved: [0, 1, 0] });
  check(e.scg_create(config, out) === scg.SCG_ERR_INVALID_ARGUMENT, "champ réservé non nul refusé");

  engine.writeConfig(config, sceneConfig());
  check(e.scg_create(config, 0) === scg.SCG_ERR_NULL, "paramètre de sortie nul refusé");

  check(e.scg_create(config, out) === scg.SCG_OK, "configuration valide acceptée");
  const ctx = engine.readU32(out);
  if (ctx !== 0 && ctx !== untouched) {
    check(messageOk(() => engine.lastError(ctx), false), "message vide après un succès");
    check(messageOk(() => engine.lastError(0), false), "message sans contexte vidé par un appel réussi");

    check(e.scg_frame_end(ctx, 0, WIDTH) === scg.SCG_ERR_NULL, "tampon nul refusé");
    check(e.scg_frame_end(ctx, small, WIDTH - 1) === scg.SCG_ERR_INVALID_ARGUMENT, "stride inférieur à la largeur refusé");
    check(messageOk(() => engine.lastError(ctx), true), "message du contexte après un stride refusé");
    check(e.scg_frame_end(0, small, WIDTH) === scg.SCG_ERR_NULL, "contexte nul refusé");

    e.scg_destroy(ctx);
  } else {
    check(false, "un handle est écrit après une création réussie");
  }
  e.scg_destroy(0);

  engine.free(small, 4 * 4);
  engine.free(out, 4);
  engine.free(config, scg.CONFIG_SIZE);
}

/**
 * L'allocation pour le compte de l'hôte, et les deux pièges propres à
 * JavaScript : le pointeur signé, et la vue qu'une croissance de la mémoire
 * détache sans prévenir.
 *
 * @param {scg.Screengine} engine
 */
function checkBuffers(engine) {
  const len = WIDTH * HEIGHT * scg.BYTES_PER_PIXEL;
  const buffer = engine.alloc(len);
  check(buffer !== 0, "scg_buffer_alloc rend un tampon");
  if (buffer !== 0) {
    check(buffer % scg.SCG_BUFFER_ALIGNMENT === 0, "tampon aligné sur SCG_BUFFER_ALIGNMENT");
    engine.bytes().fill(0, buffer, buffer + len);
    engine.free(buffer, len);
  }
  check(engine.alloc(0) === 0, "une allocation de zéro octet rend 0");
  engine.free(0, 0);

  // Une allocation plus grande que toute la mémoire actuelle l'oblige à
  // grandir : la vue prise avant doit être détachée, et une vue recréée doit
  // voir ce qu'on écrit.
  const before = engine.bytes();
  const big = before.byteLength + 1024 * 1024;
  const grown = engine.alloc(big);
  check(grown !== 0, "une allocation qui fait grandir la mémoire aboutit");
  if (grown !== 0) {
    check(before.byteLength === 0, "la vue prise avant la croissance est détachée");
    engine.bytes()[grown + big - 1] = SENTINEL;
    check(engine.bytes()[grown + big - 1] === SENTINEL, "une vue recréée voit la mémoire agrandie");
    engine.free(grown, big);
  }

  check(engine.exports.scg_last_error(0) >>> 0 === engine.orphan, "l'emplacement sans contexte garde son adresse");
}

/**
 * Rend le triangle dans un tampon entouré de sentinelles, vérifie que rien
 * n'est écrit hors de la zone utile, et rend l'empreinte, ou `null` si le
 * rendu lui-même a échoué.
 *
 * @param {scg.Screengine} engine
 * @returns {string | null}
 */
function render(engine) {
  const e = engine.exports;
  const body = STRIDE * HEIGHT * scg.BYTES_PER_PIXEL;
  const total = GUARD + body + GUARD;
  const block = engine.alloc(total);
  const config = engine.alloc(scg.CONFIG_SIZE);
  const out = engine.alloc(4);

  engine.writeConfig(config, sceneConfig());
  if (block === 0 || e.scg_create(config, out) !== scg.SCG_OK) {
    check(false, "création du contexte de rendu");
    return null;
  }
  const ctx = engine.readU32(out);
  const pixels = block + GUARD;
  engine.bytes().fill(SENTINEL, block, block + total);

  check(submitScene(engine, ctx), "scène soumise");
  const code = e.scg_frame_end(ctx, pixels, STRIDE);
  check(code === scg.SCG_OK, "scg_frame_end aboutit");

  const bytes = engine.bytes();
  let intact = true;
  for (let i = 0; i < GUARD; i++) {
    intact &&= bytes[block + i] === SENTINEL && bytes[pixels + body + i] === SENTINEL;
  }
  for (let y = 0; y < HEIGHT; y++) {
    const tail = pixels + y * STRIDE * scg.BYTES_PER_PIXEL + WIDTH * scg.BYTES_PER_PIXEL;
    for (let i = 0; i < (STRIDE - WIDTH) * scg.BYTES_PER_PIXEL; i++) {
      intact &&= bytes[tail + i] === SENTINEL;
    }
  }
  check(intact, "rien n'est écrit hors de la zone utile, marges et fins de ligne comprises");

  let opaque = true;
  for (let y = 0; y < HEIGHT; y++) {
    const row = pixels + y * STRIDE * scg.BYTES_PER_PIXEL;
    for (let x = 0; x < WIDTH; x++) {
      opaque &&= bytes[row + x * scg.BYTES_PER_PIXEL + 3] === 255;
    }
  }
  check(opaque, "l'alpha est écrit à 255 sur chaque pixel");

  const hash = engine.fingerprint(pixels, WIDTH, HEIGHT, STRIDE);
  e.scg_destroy(ctx);
  engine.free(out, 4);
  engine.free(config, scg.CONFIG_SIZE);
  engine.free(block, total);
  return code === scg.SCG_OK ? hash : null;
}

/**
 * Le rendu par tuiles sans threads, comme une page le fera : la séquence et ses
 * refus, puis une partie des tuiles dans l'ordre inverse et le reste laissé à
 * la fin, dont l'empreinte doit être celle de la fin seule.
 *
 * @param {scg.Screengine} engine
 * @param {string} expected
 */
function checkTiles(engine, expected) {
  const e = engine.exports;
  const len = STRIDE * HEIGHT * scg.BYTES_PER_PIXEL;
  const pixels = engine.alloc(len);
  const config = engine.alloc(scg.CONFIG_SIZE);
  const out = engine.alloc(4);

  engine.writeConfig(config, sceneConfig());
  if (pixels === 0 || e.scg_create(config, out) !== scg.SCG_OK) {
    check(false, "création du contexte des tuiles");
    return;
  }
  const ctx = engine.readU32(out);

  check(e.scg_frame_tile(ctx, 0, pixels, STRIDE) === scg.SCG_ERR_INVALID_STATE, "tuile avant le début refusée");
  check(engine.lastError(0) !== "", "message de la tuile dans l'emplacement sans contexte");
  check(engine.lastError(ctx) === "", "message du contexte intact après une tuile refusée");

  check(submitScene(engine, ctx), "scène soumise");
  check(e.scg_frame_begin(ctx, out) === scg.SCG_OK, "scg_frame_begin aboutit");
  const count = engine.readU32(out);
  check(count === Math.ceil(WIDTH / TILE) * Math.ceil(HEIGHT / TILE), "nombre de tuiles de l'image");
  check(e.scg_frame_begin(ctx, out) === scg.SCG_ERR_INVALID_STATE, "second début refusé");
  check(e.scg_frame_tile(ctx, count, pixels, STRIDE) === scg.SCG_ERR_INVALID_ARGUMENT, "index hors de l'image refusé");

  let rendered = true;
  for (let i = count - 1; i >= 0; i--) {
    if (i % 3 !== 0) {
      rendered &&= e.scg_frame_tile(ctx, i, pixels, STRIDE) === scg.SCG_OK;
    }
  }
  check(rendered, "les tuiles se rendent");
  check(e.scg_frame_tile(ctx, 1, pixels, STRIDE) === scg.SCG_ERR_INVALID_STATE, "tuile rendue deux fois refusée");

  check(e.scg_frame_end(ctx, pixels, STRIDE) === scg.SCG_OK, "la fin complète les tuiles manquantes");
  check(engine.fingerprint(pixels, WIDTH, HEIGHT, STRIDE) === expected, "les tuiles rendent l'image de la fin seule");

  e.scg_destroy(ctx);
  engine.free(out, 4);
  engine.free(config, scg.CONFIG_SIZE);
  engine.free(pixels, len);
}

/** Toutes les vérifications, puis l'empreinte sur la sortie standard. */
async function main() {
  const [wasmPath, headerPath] = process.argv.slice(2);
  if (!wasmPath || !headerPath) {
    process.stderr.write("usage : node test.js <module.wasm> <screengine.h>\n");
    return 2;
  }

  const engine = await scg.Screengine.instantiate(await readFile(wasmPath));
  check(engine.abiVersion() === scg.SCG_ABI_VERSION, "la bibliothèque chargée est celle du header");
  checkModule(engine);
  const header = await readFile(headerPath, "utf8");
  checkConstants(header);
  checkLayout(header);
  checkExportList(header);
  checkRefusals(engine);
  checkBuffers(engine);
  const hash = render(engine);
  if (hash !== null) {
    checkTiles(engine, hash);
  }

  if (failures > 0 || hash === null) {
    process.stderr.write(`${failures} vérification(s) en échec\n`);
    return 1;
  }
  const textured = renderTextured(engine, scg.SCG_FILTER_DITHER);
  const bilinear = renderTextured(engine, scg.SCG_FILTER_BILINEAR);
  const lit = renderLit(engine);
  if (failures > 0 || textured === null || bilinear === null || lit === null) {
    process.stderr.write(`${failures} vérification(s) en échec\n`);
    return 1;
  }

  process.stdout.write(`${hash}\n${textured}\n${bilinear}\n${lit}\n`);
  return 0;
}

process.exitCode = await main();
