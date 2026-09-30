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
 * @param {number} [side] côté de la texture, en texels
 * @param {number} [cell] côté d'une case, en texels
 * @returns {Uint8Array} `side` au carré texels de quatre octets
 */
function makeChecker(side = FLOOR_SIDE, cell = FLOOR_CELL) {
  const texels = new Uint8Array(side * side * 4);
  for (let v = 0; v < side; v++) {
    for (let u = 0; u < side; u++) {
      const base = (v * side + u) * 4;
      const edge = u % cell === 0 || v % cell === 0;
      const dark = (Math.floor(u / cell) + Math.floor(v / cell)) % 2 === 0;
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
  if (loaded < 0 || e.scg_create(config, out) < 0) {
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

/**
 * Rend la scène `gamma`, ou `null` en cas d'échec.
 *
 * C'est le seul endroit où cet hôte écrit une `ScgGrade`, **octet par octet
 * dans la mémoire linéaire**, d'après les décalages du header. Une structure
 * dont la disposition aurait bougé ne se verrait nulle part ailleurs ici : les
 * assertions statiques du header ne sont compilées que par les hôtes C et C++.
 *
 * @param {scg.Screengine} engine
 * @returns {bigint | null}
 */
function renderGraded(engine) {
  const e = engine.exports;
  const texels = makeChecker();
  const desc = engine.alloc(scg.TEXTURE_DESC_SIZE);
  const block = engine.alloc(texels.length);
  const out = engine.alloc(4);
  const config = engine.alloc(scg.CONFIG_SIZE);

  engine.writeTextureDesc(desc, FLOOR_SIDE, FLOOR_SIDE);
  engine.bytes().set(texels, block);
  const loaded = e.scg_texture_load(desc, block, texels.length, out);
  check(loaded === scg.SCG_OK, "la texture de la scène étalonnée se charge");
  const texture = engine.readU32(out);

  engine.writeConfig(config, sceneConfig());
  if (loaded < 0 || e.scg_create(config, out) < 0) {
    check(false, "création du contexte étalonné");
    return null;
  }
  const ctx = engine.readU32(out);

  const grade = engine.alloc(scg.GRADE_SIZE);
  engine.writeGrade(grade, {
    gamma: 2.2,
    gains: [1.15, 1.0, 0.85],
    offsets: [0.04, -0.02, 0.08],
  });

  // Un réservé non nul est refusé : sans ce refus, la promesse d'extension ne
  // vaudrait rien. Le champ est remis à zéro ensuite, à la main, puisque la
  // mémoire linéaire garde ce qu'on y écrit.
  new DataView(engine.memory.buffer, grade, scg.GRADE_SIZE).setUint32(32, 1, true);
  const refuse = e.scg_set_grade(ctx, grade);
  check(refuse === scg.SCG_ERR_INVALID_ARGUMENT, "un champ réservé non nul est refusé");
  new DataView(engine.memory.buffer, grade, scg.GRADE_SIZE).setUint32(32, 0, true);

  check(e.scg_clear_grade(ctx) === scg.SCG_OK, "l'extinction sans courbe passe");
  check(e.scg_set_grade(ctx, grade) === scg.SCG_OK, "la courbe se règle");

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
  check(submitted === scg.SCG_OK, "le lot de la scène étalonnée est accepté");
  e.scg_texture_destroy(texture);

  const pixels = engine.alloc(STRIDE * HEIGHT * scg.BYTES_PER_PIXEL);
  const code = e.scg_frame_end(ctx, pixels, STRIDE);
  check(code === scg.SCG_OK, "l'image étalonnée se rend");
  const hash = code === scg.SCG_OK
    ? engine.fingerprint(pixels, WIDTH, HEIGHT, STRIDE)
    : null;

  e.scg_destroy(ctx);
  return hash;
}

/**
 * Les trois lumières de la scène `lumieres`, aux mêmes valeurs que la scène de
 * conformance.
 */
const SCENE_LIGHTS = [
  { position: [8.0, -3.0, 1.5], radius: 12.0, color: [0xff, 0x30, 0x20] },
  { position: [16.0, 3.0, 1.5], radius: 12.0, color: [0x20, 0xff, 0x40] },
  { position: [24.0, -2.0, 2.5], radius: 14.0, color: [0x30, 0x50, 0xff] },
];

/**
 * Rend la scène éclairée par des lumières, ou `null` en cas d'échec.
 *
 * Le sol et le mur sont découpés en panneaux : l'atténuation étant par sommet,
 * une surface d'un seul quadrilatère ne rendrait qu'un dégradé entre ses
 * quatre coins. Le sol va au-delà de la portée des trois lumières, si bien que
 * ses derniers panneaux s'éteignent — en continuité.
 *
 * @param {scg.Screengine} engine
 * @returns {string | null}
 */
function renderLights(engine) {
  const e = engine.exports;
  const out = engine.alloc(4);
  const config = engine.alloc(scg.CONFIG_SIZE);
  engine.writeConfig(config, sceneConfig());
  if (e.scg_create(config, out) < 0) {
    check(false, "création du contexte éclairé");
    return null;
  }
  const ctx = engine.readU32(out);

  // Un champ réservé non nul est refusé : c'est le mécanisme d'extension de
  // l'ABI, et il ne vaut que si personne n'y écrit.
  const dirty = engine.alloc(scg.LIGHT_SIZE);
  engine.writeLights(dirty, [SCENE_LIGHTS[0]]);
  new DataView(engine.memory.buffer).setUint8(dirty + 19, 1);
  check(
    e.scg_set_lights(ctx, dirty, 1) === scg.SCG_ERR_INVALID_ARGUMENT,
    "un champ réservé non nul est refusé",
  );

  const lights = engine.alloc(SCENE_LIGHTS.length * scg.LIGHT_SIZE);
  engine.writeLights(lights, SCENE_LIGHTS);
  check(
    e.scg_set_lights(ctx, lights, SCENE_LIGHTS.length) === scg.SCG_OK,
    "les lumières se règlent",
  );

  const panels = 16;
  const nearEdge = 2.0;
  const farEdge = 50.0;
  const step = (farEdge - nearEdge) / panels;
  const model = engine.alloc(scg.MAT4_SIZE);
  engine.writeIdentity(model);
  let submitted = scg.SCG_OK;
  for (let i = 0; i < panels && submitted === scg.SCG_OK; i++) {
    const a = nearEdge + i * step;
    const b = nearEdge + (i + 1) * step;
    const surfaces = [
      {
        corners: [
          [a, -7.0, -1.2],
          [b, -7.0, -1.2],
          [b, 7.0, -1.2],
          [a, 7.0, -1.2],
        ],
        color: [0xb0, 0xb0, 0xb0, 0xff],
      },
      {
        corners: [
          [a, -7.0, 4.0],
          [b, -7.0, 4.0],
          [b, -7.0, -1.2],
          [a, -7.0, -1.2],
        ],
        color: [0x90, 0x90, 0x98, 0xff],
      },
    ];
    for (const surface of surfaces) {
      if (submitted < 0) {
        break;
      }
      const vertices = engine.alloc(4 * scg.VERTEX_SIZE);
      const triangles = engine.alloc(2 * scg.TRIANGLE_SIZE);
      engine.writeVertices(vertices, surface.corners);
      engine.writeTriangles(triangles, [
        { indices: [0, 1, 2], color: surface.color },
        { indices: [0, 2, 3], color: surface.color },
      ]);
      submitted = e.scg_submit(ctx, model, vertices, 4, triangles, 2);
    }
  }
  check(submitted === scg.SCG_OK, "les panneaux éclairés sont acceptés");

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
 * Le sol de la scène `brouillard`, plus long que la rampe : sa moitié
 * lointaine se confond avec le fond, sa moitié proche garde son damier.
 */
const FOG_FLOOR = [
  [1.0, -20.0, -1.2, 1.0 * 8.0, -20.0 * 8.0],
  [50.0, -20.0, -1.2, 50.0 * 8.0, -20.0 * 8.0],
  [50.0, 20.0, -1.2, 50.0 * 8.0, 20.0 * 8.0],
  [1.0, 20.0, -1.2, 1.0 * 8.0, 20.0 * 8.0],
];

/**
 * Rend la scène embrumée, ou `null` en cas d'échec.
 *
 * Le fond n'est effacé de rien : c'est le moteur qui lui donne la couleur du
 * brouillard, parce qu'un pixel non peint est infiniment lointain. Un hôte qui
 * effacerait lui-même redessinerait la couture qu'on cherche à supprimer, et
 * l'empreinte le dirait.
 *
 * @param {scg.Screengine} engine
 * @returns {string | null}
 */
function renderFog(engine) {
  const e = engine.exports;
  const texels = makeChecker();
  const desc = engine.alloc(scg.TEXTURE_DESC_SIZE);
  const block = engine.alloc(texels.length);
  const out = engine.alloc(4);
  const config = engine.alloc(scg.CONFIG_SIZE);

  engine.writeTextureDesc(desc, FLOOR_SIDE, FLOOR_SIDE);
  engine.bytes().set(texels, block);
  const loaded = e.scg_texture_load(desc, block, texels.length, out);
  check(loaded === scg.SCG_OK, "la texture du sol embrumé se charge");
  const texture = engine.readU32(out);

  engine.writeConfig(config, sceneConfig());
  if (loaded < 0 || e.scg_create(config, out) < 0) {
    check(false, "création du contexte embrumé");
    return null;
  }
  const ctx = engine.readU32(out);

  // Une rampe vide est refusée : c'est une division par zéro, et l'appelant
  // voulait vraisemblablement éteindre le brouillard.
  check(
    e.scg_set_fog(ctx, 0x30, 0x38, 0x48, 10.0, 10.0) === scg.SCG_ERR_INVALID_ARGUMENT,
    "une rampe vide est refusée",
  );
  // Éteindre un brouillard qui n'existe pas n'est pas une erreur.
  check(e.scg_clear_fog(ctx) === scg.SCG_OK, "l'extinction sans brouillard passe");
  check(
    e.scg_set_fog(ctx, 0x30, 0x38, 0x48, 3.0, 14.0) === scg.SCG_OK,
    "le brouillard se règle",
  );

  const model = engine.alloc(scg.MAT4_SIZE);
  const vertices = engine.alloc(FOG_FLOOR.length * scg.VERTEX_UV_SIZE);
  const triangles = engine.alloc(FLOOR_TRIANGLES.length * scg.TRIANGLE_SIZE);
  engine.writeIdentity(model);
  engine.writeVerticesUv(vertices, FOG_FLOOR);
  engine.writeTriangles(triangles, FLOOR_TRIANGLES);

  const submitted = e.scg_submit_textured(
    ctx,
    model,
    vertices,
    FOG_FLOOR.length,
    triangles,
    FLOOR_TRIANGLES.length,
    texture,
  );
  check(submitted === scg.SCG_OK, "le sol embrumé est accepté");
  e.scg_texture_destroy(texture);

  const pixels = engine.alloc(STRIDE * HEIGHT * scg.BYTES_PER_PIXEL);
  const code = e.scg_frame_end(ctx, pixels, STRIDE);
  check(code === scg.SCG_OK, "l'image embrumée se rend");
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
function renderLit(engine, overbright) {
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
    texLoaded < 0 ||
    lightLoaded < 0 ||
    e.scg_create(config, out) < 0
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

  // Le sur-éclairement, seul réglage de l'étape 3 qu'aucun hôte n'empruntait :
  // trois valeurs permises, et toute autre refusée plutôt que rabattue.
  check(
    e.scg_set_overbright(ctx, 3) === scg.SCG_ERR_INVALID_ARGUMENT,
    "un sur-éclairement de trois est refusé",
  );
  check(
    e.scg_set_overbright(ctx, overbright) === scg.SCG_OK,
    "le sur-éclairement se règle",
  );

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
    ScgLight: scg.LIGHT_SIZE,
    ScgTextureDesc: scg.TEXTURE_DESC_SIZE,
    ScgTriangle: scg.TRIANGLE_SIZE,
    ScgMat4: scg.MAT4_SIZE,
    ScgGrade: scg.GRADE_SIZE,
    ScgCamera: scg.CAMERA_SIZE,
    ScgSprite: scg.SPRITE_SIZE,
    ScgSweepHit: scg.SWEEP_HIT_SIZE,
  };

  // **La table au-dessus se compare à ce que la liaison déclare**, et non
  // l'inverse : elle était écrite à la main, et une taille ajoutée à
  // `screengine.js` n'y entrait que si quelqu'un y pensait. `ScgSprite` et
  // `ScgCamera` y ont manqué — les deux que la liaison écrit champ par champ
  // dans la mémoire linéaire, donc les deux qu'un décalage atteindrait sans
  // qu'aucune autre vérification ne bronche. `SCG_` écarte les constantes du
  // contrat, qui finissent aussi par `_SIZE` sans nommer de structure.
  const declarees = Object.keys(scg).filter(
    (name) => name.endsWith("_SIZE") && !name.startsWith("SCG_"),
  );
  check(
    declarees.length === Object.keys(sizes).length,
    `les ${declarees.length} tailles de screengine.js sont toutes vérifiées ici, ${Object.keys(sizes).length} le sont`,
  );

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

    check(e.scg_set_resolution(ctx, WIDTH, HEIGHT) === scg.SCG_OK, "la résolution du maximum est acceptée");
    check(e.scg_set_resolution(ctx, WIDTH + 1, HEIGHT) === scg.SCG_ERR_INVALID_ARGUMENT, "largeur au-delà du maximum refusée");
    check(e.scg_set_resolution(ctx, WIDTH, HEIGHT + 1) === scg.SCG_ERR_INVALID_ARGUMENT, "hauteur au-delà du maximum refusée");
    check(e.scg_set_resolution(ctx, 0, HEIGHT) === scg.SCG_ERR_INVALID_ARGUMENT, "largeur nulle refusée");
    check(messageOk(() => engine.lastError(ctx), true), "message du contexte après une résolution refusée");
    check(e.scg_set_resolution(0, WIDTH, HEIGHT) === scg.SCG_ERR_NULL, "contexte nul refusé par scg_set_resolution");

    check(e.scg_frame_end(ctx, 0, WIDTH) === scg.SCG_ERR_NULL, "tampon nul refusé");
    check(e.scg_frame_end(ctx, small, WIDTH - 1) === scg.SCG_ERR_INVALID_ARGUMENT, "stride inférieur à la largeur refusé");
    check(messageOk(() => engine.lastError(ctx), true), "message du contexte après un stride refusé");
    check(e.scg_frame_end(0, small, WIDTH) === scg.SCG_ERR_NULL, "contexte nul refusé");

    // La caméra : aucune scène de conformance n'en règle, celle du chemin Rust
    // tournant son modèle et non son point de vue. Son contrat se vérifie donc
    // ici plutôt que par une image — et c'est le seul hôte qui écrit cette
    // structure octet par octet.
    const camera = engine.alloc(scg.CAMERA_SIZE);
    const sane = {
      position: [0, 0, 0],
      orientation: [0, 0, 0, 1],
      fovY: 1.2,
      nearPlane: 0.1,
    };
    engine.writeCamera(camera, sane);
    check(e.scg_set_camera(ctx, camera) === scg.SCG_OK, "la caméra se règle");
    check(e.scg_set_camera(ctx, 0) === scg.SCG_ERR_NULL, "caméra nulle refusée");
    check(
      e.scg_set_camera(0, camera) === scg.SCG_ERR_NULL,
      "contexte nul refusé par scg_set_camera",
    );

    engine.writeCamera(camera, { ...sane, fovY: 4.0 });
    check(
      e.scg_set_camera(ctx, camera) === scg.SCG_ERR_INVALID_ARGUMENT,
      "champ de vision au-delà de pi refusé",
    );
    engine.writeCamera(camera, { ...sane, nearPlane: 0.0 });
    check(
      e.scg_set_camera(ctx, camera) === scg.SCG_ERR_INVALID_ARGUMENT,
      "plan proche nul refusé",
    );

    // Et refusée dès qu'un triangle de l'image en cours est retenu : chaque
    // soumission projette immédiatement, si bien qu'une caméra changée au
    // milieu laisserait deux espaces écran dans la même image.
    engine.writeCamera(camera, sane);
    const model = engine.alloc(scg.MAT4_SIZE);
    const vertices = engine.alloc(SCENE_VERTICES.length * scg.VERTEX_SIZE);
    const triangles = engine.alloc(SCENE_TRIANGLES.length * scg.TRIANGLE_SIZE);
    engine.writeIdentity(model);
    engine.writeVertices(vertices, SCENE_VERTICES);
    engine.writeTriangles(triangles, SCENE_TRIANGLES);
    check(
      e.scg_submit(ctx, model, vertices, SCENE_VERTICES.length, triangles,
        SCENE_TRIANGLES.length) === scg.SCG_OK,
      "un triangle est retenu",
    );
    check(
      e.scg_set_camera(ctx, camera) === scg.SCG_ERR_INVALID_STATE,
      "la caméra est refusée après une soumission retenue",
    );

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
  if (block === 0 || e.scg_create(config, out) < 0) {
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
 * La résolution interne change sans recréer le contexte, et l'image ne dépend
 * pas de celle qu'il avait à l'ouverture.
 *
 * Le contexte s'ouvre en 1×1 sous un maximum de WIDTH×HEIGHT, puis passe à la
 * résolution de la scène : son empreinte doit être celle qu'un contexte ouvert
 * directement dessus a rendue. Ouvrir en 1×1 plutôt qu'à la résolution finale
 * est ce qui rend une projection laissée périmée visible ici.
 *
 * @param {scg.Screengine} engine
 * @param {bigint} expected
 */
function checkResize(engine, expected) {
  const e = engine.exports;
  const body = STRIDE * HEIGHT * scg.BYTES_PER_PIXEL;
  const pixels = engine.alloc(body);
  const config = engine.alloc(scg.CONFIG_SIZE);
  const out = engine.alloc(4);

  engine.writeConfig(config, { ...sceneConfig(), width: 1, height: 1 });
  if (pixels === 0 || e.scg_create(config, out) < 0) {
    check(false, "création du contexte redimensionnable");
    return;
  }
  const ctx = engine.readU32(out);

  check(e.scg_set_resolution(ctx, WIDTH, HEIGHT) === scg.SCG_OK, "la résolution passe de 1×1 au maximum");
  check(submitScene(engine, ctx), "scène soumise après redimensionnement");
  check(e.scg_frame_end(ctx, pixels, STRIDE) === scg.SCG_OK, "image rendue après redimensionnement");
  check(
    engine.fingerprint(pixels, WIDTH, HEIGHT, STRIDE) === expected,
    "un contexte redimensionné rend l'empreinte d'un contexte neuf",
  );

  // Après une soumission, la résolution est figée pour l'image en cours,
  // comme la caméra : la projection a déjà eu lieu.
  check(submitScene(engine, ctx), "seconde scène soumise");
  check(e.scg_set_resolution(ctx, 1, 1) === scg.SCG_ERR_INVALID_STATE, "résolution refusée après une soumission");

  e.scg_destroy(ctx);
  engine.free(out, 4);
  engine.free(config, scg.CONFIG_SIZE);
  engine.free(pixels, body);
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
  if (pixels === 0 || e.scg_create(config, out) < 0) {
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

/**
 * La matrice qui place la caisse, recopiée de la suite de conformance.
 *
 * Elle y est écrite en littéraux pour cette raison : les tables
 * trigonométriques du moteur ne traversent pas l'ABI, et un hôte qui
 * recalculerait ces coefficients n'obtiendrait pas les mêmes bits.
 */
const CRATE_MODEL = [
  0.64, 0.48, 0.6, 0,
  -0.6, 0.8, 0, 0,
  -0.48, -0.36, 0.8, 0,
  5, 0, 0, 1,
];

/**
 * Rend la caisse du fichier de maillage, ou `null` en cas d'échec.
 *
 * Le bloc d'octets passe par `scg_buffer_alloc` comme tout ce qui entre dans le
 * moteur sur wasm : l'hôte ne peut pas lui donner un pointeur arbitraire, et
 * c'est la précondition que cette scène éprouve en plus des autres.
 *
 * @param {scg.Screengine} engine
 * @param {Uint8Array} meshBytes le fichier versionné, lu par l'appelant
 * @returns {bigint | null}
 */
function renderMesh(engine, meshBytes) {
  const e = engine.exports;
  const out = engine.alloc(4);
  const block = engine.alloc(meshBytes.length);
  engine.bytes().set(meshBytes, block);

  const loaded = e.scg_mesh_load(block, meshBytes.length, out);
  check(loaded === scg.SCG_OK, "le fichier de maillage se charge");
  if (loaded < 0) {
    return null;
  }
  const mesh = engine.readU32(out);
  // Le bloc est libéré avant toute soumission : le moteur copie ce qu'il garde.
  engine.free(block, meshBytes.length);

  check(
    e.scg_mesh_triangle_count(mesh, out) === scg.SCG_OK && engine.readU32(out) === 12,
    "le maillage porte douze triangles",
  );
  check(
    e.scg_mesh_texture_count(mesh, out) === scg.SCG_OK && engine.readU32(out) === 2,
    "le maillage réclame deux emplacements",
  );

  // Le nom en deux temps, et la mesure est le seul chemin vers la longueur :
  // un tampon trop court est refusé sans rien écrire.
  const lenOut = engine.alloc(4);
  check(
    e.scg_mesh_texture_name(mesh, 0, 0, 0, lenOut) === scg.SCG_OK && engine.readU32(lenOut) === 4,
    "la mesure du nom rend sa longueur",
  );
  const nameLen = engine.readU32(lenOut);
  const nameBuf = engine.alloc(nameLen + 1);
  check(
    e.scg_mesh_texture_name(mesh, 0, nameBuf, 1, lenOut) === scg.SCG_ERR_INVALID_ARGUMENT,
    "un tampon trop court est refusé",
  );
  check(
    e.scg_mesh_texture_name(mesh, 0, nameBuf, nameLen + 1, lenOut) === scg.SCG_OK,
    "le nom se lit",
  );
  const name = new TextDecoder().decode(engine.bytes().subarray(nameBuf, nameBuf + nameLen));
  check(name === "cote", `le premier emplacement s'appelle cote, et non ${name}`);

  const texels = makeChecker();
  const desc = engine.alloc(scg.TEXTURE_DESC_SIZE);
  const texelBlock = engine.alloc(texels.length);
  engine.writeTextureDesc(desc, FLOOR_SIDE, FLOOR_SIDE);
  engine.bytes().set(texels, texelBlock);
  check(
    e.scg_texture_load(desc, texelBlock, texels.length, out) === scg.SCG_OK,
    "le damier des faces se charge",
  );
  const texture = engine.readU32(out);

  const config = engine.alloc(scg.CONFIG_SIZE);
  engine.writeConfig(config, sceneConfig());
  if (e.scg_create(config, out) < 0) {
    check(false, "création du contexte du maillage");
    return null;
  }
  const ctx = engine.readU32(out);

  // Le tableau d'emplacements : deux pointeurs, le second nul — « sans
  // texture », et les couleurs du fichier décident.
  const slots = engine.alloc(8);
  const view = new DataView(engine.memory.buffer, slots, 8);
  view.setUint32(0, texture, true);
  view.setUint32(4, 0, true);

  const model = engine.alloc(scg.MAT4_SIZE);
  engine.writeMat4(model, CRATE_MODEL);
  check(
    e.scg_submit_mesh(ctx, model, mesh, slots, 1) === scg.SCG_ERR_INVALID_ARGUMENT,
    "un compte d'emplacements faux est refusé",
  );
  check(e.scg_submit_mesh(ctx, model, mesh, slots, 2) === scg.SCG_OK, "la caisse est acceptée");

  e.scg_mesh_destroy(mesh);
  e.scg_texture_destroy(texture);

  const pixels = engine.alloc(STRIDE * HEIGHT * scg.BYTES_PER_PIXEL);
  const code = e.scg_frame_end(ctx, pixels, STRIDE);
  check(code === scg.SCG_OK, "l'image de la caisse se rend");
  const hash = code === scg.SCG_OK
    ? engine.fingerprint(pixels, WIDTH, HEIGHT, STRIDE)
    : null;

  e.scg_destroy(ctx);
  return hash;
}

/** Le côté de la planche d'emblèmes masqués, et celui de la tache. */
const EMBLEM_SIDE = 64;

/**
 * L'emblème masqué de la scène composite : un disque et son pied, sur fond
 * transparent.
 *
 * Mêmes valeurs que la scène de conformance, écrites ici plutôt que chargées :
 * ce que cet hôte doit reproduire est la **disposition** des structures, pas
 * une texture qui viendrait d'ailleurs.
 *
 * @returns {Uint8Array}
 */
function makeEmblem() {
  const texels = new Uint8Array(EMBLEM_SIDE * EMBLEM_SIDE * 4);
  const cx = EMBLEM_SIDE / 2;
  const cy = EMBLEM_SIDE * 0.35;
  const radius = EMBLEM_SIDE * 0.28;
  for (let v = 0; v < EMBLEM_SIDE; v++) {
    for (let u = 0; u < EMBLEM_SIDE; u++) {
      const base = (v * EMBLEM_SIDE + u) * 4;
      const fu = u + 0.5;
      const fv = v + 0.5;
      const dx = fu - cx;
      const dy = fv - cy;
      const disc = dx * dx + dy * dy <= radius * radius;
      const foot = fv > EMBLEM_SIDE * 0.6 && fu > EMBLEM_SIDE * 0.28 && fu < EMBLEM_SIDE * 0.52;
      if (disc || foot) {
        texels[base] = 0x40 + Math.floor((fu * 160.0) / EMBLEM_SIDE);
        texels[base + 1] = 0xff - Math.floor((fv * 140.0) / EMBLEM_SIDE);
        texels[base + 2] = 0x60;
        texels[base + 3] = 0xff;
      }
    }
  }
  return texels;
}

/**
 * La tache d'ombre : sombre au centre, **blanche au bord**, 255 étant le neutre
 * de la modulation.
 *
 * @returns {Uint8Array}
 */
function makeShadow() {
  const texels = new Uint8Array(EMBLEM_SIDE * EMBLEM_SIDE * 4);
  const half = EMBLEM_SIDE / 2;
  for (let v = 0; v < EMBLEM_SIDE; v++) {
    for (let u = 0; u < EMBLEM_SIDE; u++) {
      const base = (v * EMBLEM_SIDE + u) * 4;
      const dx = u + 0.5 - half;
      const dy = v + 0.5 - half;
      const q = Math.min(1.0, (dx * dx + dy * dy) / (half * half));
      const level = Math.floor(0x30 + (0xff - 0x30) * q);
      texels[base] = level;
      texels[base + 1] = level;
      texels[base + 2] = level;
      texels[base + 3] = 0xff;
    }
  }
  return texels;
}

/** Les deux lumières de la scène composite. */
const COMPOSITE_LIGHTS = [
  { position: [2.0, -3.0, 1.5], radius: 24.0, color: [0xff, 0xc0, 0x60] },
  { position: [2.0, 3.0, 3.0], radius: 24.0, color: [0x40, 0x80, 0xff] },
];

/**
 * La caméra en plongée de la scène composite, d'un seizième de tour.
 *
 * Le quaternion se range `x, y, z, w` : un demi-angle sur l'axe, le cosinus en
 * dernier. Les valeurs sont écrites plutôt que calculées par une bibliothèque
 * tierce, comme le reste de cet hôte.
 */
const COMPOSITE_CAMERA = {
  position: [0, 0, 3],
  orientation: [0, 0.19509032, 0, 0.98078528],
  fovY: 1.0471976,
  nearPlane: 0.1,
};

/** Le sol de la scène composite, à huit texels par unité de monde. */
const COMPOSITE_FLOOR = [
  [4.0, -6.0, -2.6, 4.0 * 8.0, -6.0 * 8.0],
  [14.0, -6.0, -2.6, 14.0 * 8.0, -6.0 * 8.0],
  [14.0, 6.0, -2.6, 14.0 * 8.0, 6.0 * 8.0],
  [4.0, 6.0, -2.6, 4.0 * 8.0, 6.0 * 8.0],
];

/** La tache modulée, coplanaire au sol. */
const COMPOSITE_SHADOW = [
  [4.5, -5.5, -2.6, 0.0, 0.0],
  [9.5, -5.5, -2.6, 64.0, 0.0],
  [9.5, -0.5, -2.6, 64.0, 64.0],
  [4.5, -0.5, -2.6, 0.0, 64.0],
];

/** Les deux triangles d'un quadrilatère, blancs. */
const QUAD_FACES = [
  { indices: [0, 1, 2], color: [0xff, 0xff, 0xff, 0xff] },
  { indices: [0, 2, 3], color: [0xff, 0xff, 0xff, 0xff] },
];

/**
 * Rend la scène composite de l'étape 6, ou `null` en cas d'échec.
 *
 * **La seule scène que cet hôte rende pour les cinq chemins de l'étape** :
 * maillage entre deux trames, texture masquée, les deux modes d'orientation de
 * sprite, le roulis, et la surface modulée. Une scène par chemin aurait été
 * plus lisible en cas de divergence ; c'est une seule, parce que chacune se
 * paie en cinq descriptions — une par langage — et que les scènes séparées
 * existent déjà côté Rust pour dire lequel a bougé.
 *
 * **Ce qu'elle ferme ici et nulle part ailleurs** : les quarante-quatre octets
 * de `ScgSprite`, que cette liaison écrit à la main. Les hôtes C et C++
 * compilent les assertions statiques du header, et le pont JNI est en C ; ce
 * fichier n'a rien de tel.
 *
 * @param {scg.Screengine} engine
 * @param {Uint8Array} meshBytes le fichier versionné, à deux trames
 * @returns {bigint | null}
 */
function renderComposite(engine, meshBytes) {
  const e = engine.exports;
  const out = engine.alloc(4);

  const block = engine.alloc(meshBytes.length);
  engine.bytes().set(meshBytes, block);
  if (e.scg_mesh_load(block, meshBytes.length, out) < 0) {
    check(false, "le maillage de la composite se charge");
    return null;
  }
  const mesh = engine.readU32(out);
  engine.free(block, meshBytes.length);

  // **Deux trames**, ce qu'un maillage statique ne pourrait pas éprouver.
  check(
    e.scg_mesh_frame_count(mesh, out) === scg.SCG_OK && engine.readU32(out) === 2,
    "le maillage versionné porte deux trames",
  );

  const desc = engine.alloc(scg.TEXTURE_DESC_SIZE);
  // **Le côté vient avec les texels**, il ne se suppose pas : le damier est
  // bâti sur `FLOOR_SIDE` et les deux autres sur `EMBLEM_SIDE`. Les deux valent
  // soixante-quatre aujourd'hui, si bien qu'une description écrite au jugé
  // passe — jusqu'au jour où l'une des deux change, et la texture est alors lue
  // à côté sans qu'aucune erreur soit rendue.
  const loadTexture = (texels, side, format) => {
    const bytes = engine.alloc(texels.length);
    engine.writeTextureDesc(desc, side, side, format);
    engine.bytes().set(texels, bytes);
    const code = e.scg_texture_load(desc, bytes, texels.length, out);
    check(code === scg.SCG_OK, "la texture de la composite se charge");
    return code === scg.SCG_OK ? engine.readU32(out) : 0;
  };
  const sides = loadTexture(makeChecker(), FLOOR_SIDE, scg.SCG_TEXTURE_FORMAT_RGBA8);
  const emblem = loadTexture(
    makeEmblem(),
    EMBLEM_SIDE,
    scg.SCG_TEXTURE_FORMAT_RGBA8_MASKED,
  );
  const shadow = loadTexture(makeShadow(), EMBLEM_SIDE, scg.SCG_TEXTURE_FORMAT_RGBA8);

  const config = engine.alloc(scg.CONFIG_SIZE);
  engine.writeConfig(config, sceneConfig());
  if (e.scg_create(config, out) < 0) {
    check(false, "création du contexte de la composite");
    return null;
  }
  const ctx = engine.readU32(out);

  const camera = engine.alloc(scg.CAMERA_SIZE);
  engine.writeCamera(camera, COMPOSITE_CAMERA);
  check(e.scg_set_camera(ctx, camera) === scg.SCG_OK, "la caméra plongeante se règle");

  const lights = engine.alloc(COMPOSITE_LIGHTS.length * scg.LIGHT_SIZE);
  engine.writeLights(lights, COMPOSITE_LIGHTS);
  check(
    e.scg_set_lights(ctx, lights, COMPOSITE_LIGHTS.length) === scg.SCG_OK,
    "les deux lumières se règlent",
  );

  const identity = engine.alloc(scg.MAT4_SIZE);
  engine.writeIdentity(identity);
  const quadVertices = engine.alloc(4 * scg.VERTEX_UV_SIZE);
  const quadFaces = engine.alloc(2 * scg.TRIANGLE_SIZE);
  engine.writeTriangles(quadFaces, QUAD_FACES);

  // Le sol d'abord : une surface modulée multiplie ce qui est déjà écrit, et
  // n'aurait rien à assombrir sans lui.
  engine.writeVerticesUv(quadVertices, COMPOSITE_FLOOR);
  check(
    e.scg_submit_textured(ctx, identity, quadVertices, 4, quadFaces, 2, sides) === scg.SCG_OK,
    "le sol de la composite est accepté",
  );

  const slots = engine.alloc(8);
  const slotView = new DataView(engine.memory.buffer, slots, 8);
  slotView.setUint32(0, sides, true);
  slotView.setUint32(4, 0, true);
  const model = engine.alloc(scg.MAT4_SIZE);
  engine.writeMat4(model, CRATE_MODEL);
  check(
    e.scg_submit_mesh_frame(ctx, model, mesh, slots, 2, 0, 1, 0.35) === scg.SCG_OK,
    "la caisse interpolée est acceptée",
  );

  // Les deux modes d'orientation, le second avec un roulis non nul : un lot ne
  // porte qu'une orientation, donc deux soumissions.
  const sprite = engine.alloc(scg.SPRITE_SIZE);
  engine.writeSprites(sprite, [
    {
      center: [9.0, -3.5, 0.0],
      half: [2.0, 2.4],
      uv: [0.0, 0.0, EMBLEM_SIDE, EMBLEM_SIDE],
      roll: 0,
      color: [0xff, 0xff, 0xff, 0xff],
    },
  ]);
  check(
    e.scg_submit_sprites(ctx, identity, sprite, 1, emblem, scg.SCG_SPRITE_AXIAL) === scg.SCG_OK,
    "le sprite axial est accepté",
  );
  check(
    e.scg_submit_sprites(ctx, identity, sprite, 1, emblem, 0) === scg.SCG_ERR_INVALID_ARGUMENT,
    "une orientation nulle est refusée, jamais rabattue sur un défaut",
  );
  engine.writeSprites(sprite, [
    {
      center: [9.0, 3.5, 0.0],
      half: [2.0, 2.4],
      uv: [0.0, 0.0, EMBLEM_SIDE, EMBLEM_SIDE],
      // Cinq huitièmes de tour, en angle binaire. C'est le seul champ entier au
      // milieu de flottants, donc celui qu'une liaison écrit mal sans qu'aucun
      // contrôle ne la reprenne.
      roll: 5 * 0x20000000,
      color: [0xff, 0xff, 0xff, 0xff],
    },
  ]);
  check(
    e.scg_submit_sprites(ctx, identity, sprite, 1, emblem, scg.SCG_SPRITE_FACING) === scg.SCG_OK,
    "le sprite plein face et son roulis sont acceptés",
  );

  // La tache modulée en dernier : elle multiplie ce que les lots précédents ont
  // écrit, donc l'ordre de soumission décide.
  engine.writeVerticesUv(quadVertices, COMPOSITE_SHADOW);
  check(
    e.scg_submit_blended(ctx, identity, quadVertices, 4, quadFaces, 2, shadow, 0) ===
      scg.SCG_ERR_INVALID_ARGUMENT,
    "un mode de mélange nul est refusé",
  );
  check(
    e.scg_submit_blended(
      ctx,
      identity,
      quadVertices,
      4,
      quadFaces,
      2,
      shadow,
      scg.SCG_BLEND_MODULATE,
    ) === scg.SCG_OK,
    "la tache modulée est acceptée",
  );

  e.scg_mesh_destroy(mesh);
  e.scg_texture_destroy(sides);
  e.scg_texture_destroy(emblem);
  e.scg_texture_destroy(shadow);

  const pixels = engine.alloc(STRIDE * HEIGHT * scg.BYTES_PER_PIXEL);
  const code = e.scg_frame_end(ctx, pixels, STRIDE);
  check(code === scg.SCG_OK, "l'image composite se rend");
  const compositeHash = code === scg.SCG_OK
    ? engine.fingerprint(pixels, WIDTH, HEIGHT, STRIDE)
    : null;

  e.scg_destroy(ctx);
  return compositeHash;
}

/**
 * La scène `salles` : un décor chargé d'un fichier, cuit cellule par cellule,
 * parcouru par sa traversée.
 *
 * **Treize points d'entrée que les dix scènes précédentes n'atteignent pas**,
 * et dont deux ont manqué au code pendant une version entière sans que rien ne
 * le dise : un symbole que personne n'appelle s'exporte aussi bien qu'il
 * manque.
 *
 * La vue est la première de la scène de conformance : dans la salle en L, face
 * à l'ouverture du couloir.
 *
 * @param {scg.Screengine} engine le module chargé
 * @param {Uint8Array} worldBytes le contenu de `salles.world`
 * @returns {string|null} l'empreinte, ou `null` en cas d'échec
 */
function renderRooms(engine, worldBytes) {
  // Les deux damiers de la scène de référence : le mur est plus fin que le
  // sol, et c'est le nom du matériau qui décide lequel va où.
  const WALL_SIDE = 512;
  const WALL_CELL = 128;
  const ROOM_FLOOR_SIDE = 256;
  const ROOM_FLOOR_CELL = 32;

  const e = engine.exports;
  const out = engine.alloc(4);

  const block = engine.alloc(worldBytes.length);
  engine.bytes().set(worldBytes, block);
  if (e.scg_world_load(block, worldBytes.length, out) < 0) {
    check(false, "le fichier de carte se charge");
    return null;
  }
  const world = engine.readU32(out);
  engine.free(block, worldBytes.length);

  check(
    e.scg_world_material_count(world, out) === scg.SCG_OK && engine.readU32(out) === 2,
    "la carte déclare deux matériaux",
  );
  const materials = engine.readU32(out);

  // Le nom en deux temps : mesure, puis remplissage. Un hôte qui devinerait la
  // longueur se tromperait le jour où elle change.
  const textures = [];
  for (let i = 0; i < materials; i++) {
    const len = engine.alloc(4);
    e.scg_world_material_name(world, i, 0, 0, len);
    const size = engine.readU32(len);
    const name = engine.alloc(size + 1);
    check(
      e.scg_world_material_name(world, i, name, size + 1, len) === scg.SCG_OK,
      "le nom du matériau se lit en deux temps",
    );
    const text = new TextDecoder().decode(engine.bytes().subarray(name, name + size));
    engine.free(name, size + 1);
    engine.free(len, 4);

    const wall = text === "mur";
    const side = wall ? WALL_SIDE : ROOM_FLOOR_SIDE;
    const texels = makeChecker(side, wall ? WALL_CELL : ROOM_FLOOR_CELL);
    const desc = engine.alloc(scg.TEXTURE_DESC_SIZE);
    const bytes = engine.alloc(texels.length);
    engine.writeTextureDesc(desc, side, side);
    engine.bytes().set(texels, bytes);
    const code = e.scg_texture_load(desc, bytes, texels.length, out);
    check(code === scg.SCG_OK, "le damier du matériau se charge");
    textures.push(code === scg.SCG_OK ? engine.readU32(out) : 0);
  }

  check(e.scg_lighting_create(world, out) === scg.SCG_OK, "le porteur de lightmaps se crée");
  const lighting = engine.readU32(out);

  // **Toutes les cellules, pas seulement celles que la vue montre** : une
  // lightmap est un cache de la carte et non du point de vue.
  check(
    e.scg_world_cell_count(world, out) === scg.SCG_OK && engine.readU32(out) === 4,
    "la carte porte quatre cellules",
  );
  const cells = engine.readU32(out);
  for (let i = 0; i < cells; i++) {
    e.scg_world_cell_id(world, i, out);
    const id = engine.readU32(out);
    // Ce qu'un hôte lit avant de cuire, pour pondérer sa progression : le
    // compte de cellules ne dit rien du coût de chacune.
    check(
      e.scg_world_cell_luxel_count(world, id, out) === scg.SCG_OK && engine.readU32(out) > 0,
      "la cellule annonce ses luxels",
    );
    check(e.scg_lighting_build(lighting, id) === scg.SCG_OK, "la cellule se cuit");
  }

  const config = engine.alloc(scg.CONFIG_SIZE);
  engine.writeConfig(config, sceneConfig());
  check(e.scg_create(config, out) === scg.SCG_OK, "création du contexte du décor");
  const ctx = engine.readU32(out);

  const position = [2.0, 2.0, 2.0];
  const camera = engine.alloc(scg.CAMERA_SIZE);
  engine.writeCamera(camera, {
    position,
    // Sans rotation : la vue regarde le +X du monde, et le quaternion identité
    // range sa partie réelle en dernier.
    orientation: [0, 0, 0, 1],
    fovY: 1.0471976,
    nearPlane: 0.1,
  });
  check(e.scg_set_camera(ctx, camera) === scg.SCG_OK, "la caméra du décor se règle");

  // La cellule se trouve, elle ne se devine pas : zéro veut dire « nulle
  // part », ce qui est une clause et non une erreur.
  const point = engine.alloc(12);
  engine.writePoint(point, position);
  check(
    e.scg_world_locate(world, point, out) === scg.SCG_OK && engine.readU32(out) !== 0,
    "la caméra est dans une cellule",
  );
  const cell = engine.readU32(out);

  // La vue se construit après les chargements : chacun a pu agrandir la
  // mémoire, ce qui détache toute vue prise avant.
  const slots = engine.alloc(materials * 4);
  const table = new DataView(engine.memory.buffer, slots, materials * 4);
  textures.forEach((texture, i) => table.setUint32(i * 4, texture, true));

  const model = engine.alloc(scg.MAT4_SIZE);
  engine.writeIdentity(model);
  check(
    e.scg_submit_world_visible(ctx, model, world, slots, materials, lighting, cell) === scg.SCG_OK,
    "la traversée accepte le décor",
  );

  const pixels = engine.alloc(STRIDE * HEIGHT * scg.BYTES_PER_PIXEL);
  const code = e.scg_frame_end(ctx, pixels, STRIDE);
  check(code === scg.SCG_OK, "l'image du décor se rend");
  const hash = code === scg.SCG_OK
    ? engine.fingerprint(pixels, WIDTH, HEIGHT, STRIDE)
    : null;

  textures.forEach((texture) => e.scg_texture_destroy(texture));
  // La carte part avant le porteur, qui en garde une référence : l'ordre est
  // libre, et c'est ce que cette destruction éprouve.
  e.scg_world_destroy(world);
  e.scg_lighting_destroy(lighting);
  e.scg_destroy(ctx);
  return hash;
}

/**
 * Rejoue les balayages du fichier versionné et hache leurs résultats.
 *
 * **La seule scène sans image, et la seule sans contexte.** Elle éprouve les
 * deux points d'entrée du balayage, qu'aucun rendu n'emprunte, et la liste vient
 * du dépôt : la reconstruire ici ferait mesurer à l'empreinte un report de règle
 * plutôt que le moteur.
 *
 * Sur wasm, l'hôte ne peut pas donner un pointeur arbitraire : les trois
 * vecteurs et la structure de sortie passent par `scg_buffer_alloc`, ce que les
 * autres scènes font déjà pour leurs sommets.
 *
 * @param {scg.Screengine} engine
 * @param {Uint8Array} worldBytes le décor de collision
 * @param {Uint8Array} list la liste de balayages, magie comprise
 * @returns {string | null} l'empreinte, ou null si une vérification a échoué
 */
function renderSweeps(engine, worldBytes, list) {
  const e = engine.exports;
  const out = engine.alloc(4);

  const block = engine.alloc(worldBytes.length);
  engine.bytes().set(worldBytes, block);
  if (e.scg_world_load(block, worldBytes.length, out) < 0) {
    check(false, "le décor de collision se charge");
    return null;
  }
  const world = engine.readU32(out);
  engine.free(block, worldBytes.length);

  // La magie avant toute lecture : un mauvais chemin doit échouer ici plutôt
  // que produire une empreinte de bruit.
  const magic = new TextDecoder().decode(list.subarray(0, 8));
  if (list.length < 12 || magic !== "SCGSWEEP") {
    check(false, "la liste de balayages porte sa magie");
    return null;
  }
  const source = new DataView(list.buffer, list.byteOffset, list.byteLength);
  const count = source.getUint32(8, true);
  if (list.length !== 12 + count * SWEEP_RECORD) {
    check(false, "la liste annonce le nombre de balayages qu'elle porte");
    return null;
  }

  // Trois vecteurs contigus, écrits une fois par balayage : un allocateur
  // sollicité huit cents fois dirait surtout le coût de l'allocateur.
  const vectors = engine.alloc(9 * 4);
  const hit = engine.alloc(scg.SWEEP_HIT_SIZE);
  const digest = new Uint8Array(count * 37);
  let written = 0;

  for (let i = 0; i < count; i++) {
    const base = 12 + i * SWEEP_RECORD;
    const view = new DataView(engine.memory.buffer);
    for (let rank = 0; rank < 9; rank++) {
      view.setFloat32(vectors + rank * 4, source.getFloat32(base + rank * 4, true), true);
    }
    const half = vectors;
    const from = vectors + 12;
    const to = vectors + 24;

    // Zéro veut dire « nulle part », et se passe tel quel : c'est le balayage
    // qui rend le déplacement libre, pas l'hôte qui le fabrique.
    if (e.scg_world_locate(world, from, out) < 0) {
      check(false, "la cellule de départ se cherche");
      return null;
    }
    const status = e.scg_world_sweep(world, engine.readU32(out), half, from, to, hit);
    if (status < 0) {
      check(false, "le balayage est accepté");
      return null;
    }

    // Les trente-six premiers octets de `ScgSweepHit` sont exactement ceux que
    // l'empreinte veut, dans l'ordre : le header le garantit par ses assertions
    // de décalage, et les deux champs réservés viennent après.
    digest.set(engine.bytes().subarray(hit, hit + 36), written);
    digest[written + 36] = status;
    written += 37;

    // Le contrepoids de `surface_id` : sans cet appel, le champ serait un
    // identifiant qu'aucune fonction ne traduit.
    const surface = engine.readU32(hit + 28);
    if (surface !== 0) {
      check(
        e.scg_world_surface_material(world, surface, out) === scg.SCG_OK,
        "la surface touchée nomme son matériau",
      );
    }
  }

  engine.free(vectors, 9 * 4);
  engine.free(hit, scg.SWEEP_HIT_SIZE);
  engine.free(out, 4);
  e.scg_world_destroy(world);
  return engine.hashBytes(digest);
}

/** La taille d'un enregistrement de la liste : neuf flottants. */
const SWEEP_RECORD = 36;

/** Toutes les vérifications, puis l'empreinte sur la sortie standard. */
async function main() {
  const [wasmPath, headerPath, meshPath, worldPath, collisionPath, sweepsPath] =
    process.argv.slice(2);
  if (!wasmPath || !headerPath || !meshPath || !worldPath || !collisionPath || !sweepsPath) {
    process.stderr.write(
      "usage : node test.js <module.wasm> <screengine.h> <caisse.mesh> <salles.world> " +
        "<collision.world> <collision.sweeps>\n",
    );
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
    checkResize(engine, hash);
  }

  if (failures > 0 || hash === null) {
    process.stderr.write(`${failures} vérification(s) en échec\n`);
    return 1;
  }
  const textured = renderTextured(engine, scg.SCG_FILTER_DITHER);
  const bilinear = renderTextured(engine, scg.SCG_FILTER_BILINEAR);
  const graded = renderGraded(engine);
  const lit = renderLit(engine, 0);
  // La même scène au sur-éclairement maximal : seul le réglage du contexte les
  // sépare, donc une divergence ne peut venir que de lui.
  const overbright = renderLit(engine, 2);
  const fog = renderFog(engine);
  const lights = renderLights(engine);
  const meshBytes = new Uint8Array(await readFile(meshPath));
  const mesh = renderMesh(engine, meshBytes);
  const composite = renderComposite(engine, meshBytes);
  const rooms = renderRooms(engine, new Uint8Array(await readFile(worldPath)));
  const sweeps = renderSweeps(
    engine,
    new Uint8Array(await readFile(collisionPath)),
    new Uint8Array(await readFile(sweepsPath)),
  );
  if (
    failures > 0 ||
    sweeps === null ||
    rooms === null ||
    composite === null ||
    textured === null ||
    bilinear === null ||
    graded === null ||
    lit === null ||
    overbright === null ||
    fog === null ||
    lights === null ||
    mesh === null
  ) {
    process.stderr.write(`${failures} vérification(s) en échec\n`);
    return 1;
  }

  process.stdout.write(
    `${hash}\n${textured}\n${bilinear}\n${graded}\n${lit}\n${overbright}\n${fog}\n${lights}\n` +
      `${mesh}\n${composite}\n${rooms}\n${sweeps}\n`,
  );
  return 0;
}

process.exitCode = await main();
