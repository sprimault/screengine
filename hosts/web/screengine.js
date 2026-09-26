// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * @file Accès à Screengine depuis JavaScript, commun au test Node et à la page.
 *
 * Aucune API de Node ni du DOM : le module reçoit des octets ou un module
 * compilé, et rend des fonctions sur la mémoire linéaire. Il ne garde aucune
 * vue sur cette mémoire — un appel qui alloue peut l'agrandir, ce qui détache
 * l'`ArrayBuffer` existant, et une vue construite avant l'appel ne voit plus
 * rien après, sans lever d'erreur. Chaque accès relit donc `memory.buffer`.
 *
 * Les constantes sont recopiées du header, que JavaScript ne lit pas ; le test
 * les compare aux `#define` de `include/screengine.h`, pour qu'une recopie
 * périmée échoue au lieu de passer.
 */

/** Version d'ABI contre laquelle ce module est écrit. */
export const SCG_ABI_VERSION = 1;

/** Alignement garanti par `scg_buffer_alloc`. */
export const SCG_BUFFER_ALIGNMENT = 16;

/** Succès. */
export const SCG_OK = 0;

/** Argument pointeur nul. */
export const SCG_ERR_NULL = -1;

/** Argument hors de ce que le moteur accepte. */
export const SCG_ERR_INVALID_ARGUMENT = -2;

/** Allocation échouée. */
export const SCG_ERR_OUT_OF_MEMORY = -3;

/** Appel hors séquence. */
export const SCG_ERR_INVALID_STATE = -4;

/** Le moteur a paniqué. Sur wasm, ce code n'arrive jamais : la panique est un trap. */
export const SCG_ERR_PANIC = -5;

/** Objet défaillant après une panique antérieure. Inobservable sur wasm, pour la même raison. */
export const SCG_ERR_FAULTED = -6;

/** Ancien nom de `SCG_ERR_FAULTED`, gardé comme dans le header. */
export const SCG_ERR_POISONED = -6;

/** Recherche par identifiant stable qui ne trouve rien : la cellule de départ d'une traversée. */
export const SCG_ERR_UNKNOWN_RESOURCE = -100;

/**
 * Succès, et la traversée s'est arrêtée à une de ses bornes.
 *
 * **Un code positif est un succès.** Juger un appel par son signe, jamais par
 * « différent de `SCG_OK` » : un statut inconnu se traite comme un succès, parce
 * que l'ignorer est toujours correct.
 */
export const SCG_STATUS_INCOMPLETE = 1;

/** Succès, et aucune cellule n'a été donnée : rien n'a été soumis. */
export const SCG_STATUS_NO_CELL = 2;

/** Profondeur à laquelle la traversée suit une ligne de vue. */
export const SCG_TRAVERSAL_DEPTH = 64;

/** Nombre de cellules qu'une image peut retenir. */
export const SCG_TRAVERSAL_CELLS = 4096;

/** Le bloc n'est pas un fichier de données lisible : la faute est dans le contenu. */
export const SCG_ERR_INVALID_FORMAT = -101;

/** Version de format que cette bibliothèque ne lit pas. Le seul code qui dise quoi faire. */
export const SCG_ERR_UNSUPPORTED_FORMAT_VERSION = -102;

/**
 * Des pixels opaques : l'octet d'alpha est transporté, jamais lu.
 *
 * Un et non zéro : une description laissée à zéro est ainsi refusée plutôt
 * qu'interprétée, ce qui importe d'autant plus ici qu'une liaison JavaScript
 * écrit la structure octet par octet dans la mémoire linéaire.
 */
export const SCG_TEXTURE_FORMAT_RGBA8 = 1;

/**
 * Les mêmes octets, mais l'alpha décide : transparence binaire.
 *
 * Un texel transparent n'écrit ni couleur ni profondeur. La transparence est
 * une propriété de la texture et non du dessin, la chaîne de mipmaps se
 * construisant au chargement : toute soumission qui prend une texture en
 * hérite, sans point d'entrée ni réglage de plus.
 */
export const SCG_TEXTURE_FORMAT_RGBA8_MASKED = 2;

/**
 * Multiplier le tampon au lieu de l'écraser.
 *
 * Un et non zéro, et c'est la règle de **tout** mode passé à une soumission :
 * un mode est une description, comme le format de texture, et non un réglage de
 * contexte comme le filtrage. Zéro ne vaut défaut que pour les seconds.
 *
 * 255 est le neutre, comme pour une lightmap : une surface modulée assombrit ou
 * ne fait rien, et n'éclaircit jamais.
 */
export const SCG_BLEND_MODULATE = 1;

/**
 * Le tramage ordonné des coordonnées, filtrage par défaut.
 *
 * Zéro, à l'inverse du format de texture : un contexte qu'on ne configure pas
 * doit rendre ce que le moteur rend par défaut.
 */
export const SCG_FILTER_DITHER = 0;

/** Le bilinéaire, un niveau de qualité au-dessus, qui remplace le tramage. */
export const SCG_FILTER_BILINEAR = 1;

/** Les fonctions que le module doit exporter, en plus de `memory`. */
export const EXPORTS = [
  "scg_abi_version",
  "scg_create",
  "scg_destroy",
  "scg_set_camera",
  "scg_submit",
  "scg_frame_begin",
  "scg_frame_tile",
  "scg_frame_end",
  "scg_last_error",
  "scg_buffer_alloc",
  "scg_buffer_free",
  "scg_texture_load",
  "scg_texture_destroy",
  "scg_submit_textured",
  "scg_submit_blended",
  "scg_submit_lit",
  "scg_set_resolution",
  "scg_set_grade",
  "scg_clear_grade",
  "scg_set_filter",
  "scg_set_overbright",
  "scg_set_fog",
  "scg_clear_fog",
  "scg_set_lights",
  "scg_mesh_load",
  "scg_mesh_destroy",
  "scg_mesh_texture_count",
  "scg_mesh_texture_name",
  "scg_mesh_triangle_count",
  "scg_mesh_frame_count",
  "scg_submit_mesh",
  "scg_world_load",
  "scg_world_destroy",
  "scg_world_material_count",
  "scg_world_material_name",
  "scg_world_triangle_count",
  "scg_world_light_count",
  "scg_world_light",
  "scg_world_entity_count",
  "scg_world_entity_ids",
  "scg_world_entity_pose",
  "scg_world_entity_class",
  "scg_world_entity_data",
  "scg_submit_world",
  "scg_submit_world_visible",
  "scg_world_cell_count",
  "scg_world_cell_id",
  "scg_world_locate",
  "scg_world_track",
  "scg_lighting_create",
  "scg_lighting_destroy",
  "scg_lighting_build",
  "scg_lighting_state",
  "scg_lighting_save",
  "scg_lighting_restore",
];

/** Une cellule n'a pas encore de lightmap. */
export const SCG_LIGHTMAP_ABSENT = 0;

/** Sa lightmap est prête. */
export const SCG_LIGHTMAP_READY = 1;

/** Elle en a une, mais la cellule a changé depuis. */
export const SCG_LIGHTMAP_STALE = 2;

/** Taille de `ScgContextConfig`, celle qu'affirme le header. */
export const CONFIG_SIZE = 32;

/** Taille de `ScgVertex` : trois `float`. */
export const VERTEX_SIZE = 12;

/** Taille de `ScgVertexUv` : trois `float` de position, puis `u` et `v`. */
export const VERTEX_UV_SIZE = 20;

/** Taille de `ScgVertexUv2` : la précédente, plus `u2` et `v2`. */
export const VERTEX_UV2_SIZE = 28;

/** Taille de `ScgLight` : quatre `float`, puis trois canaux et un réservé. */
export const LIGHT_SIZE = 20;

/** Taille de `ScgTextureDesc` : six `uint32_t`. */
export const TEXTURE_DESC_SIZE = 24;

/**
 * Taille de `ScgGrade` : un gamma, trois gains, trois décalages, deux réservés.
 */
export const GRADE_SIZE = 36;

/**
 * Taille de `ScgCamera` : une position, un quaternion, un champ de vision, un
 * plan proche.
 */
export const CAMERA_SIZE = 36;

/** Taille de `ScgTriangle` : trois `uint32_t` puis quatre `uint8_t`. */
export const TRIANGLE_SIZE = 16;

/** Taille de `ScgMat4` : seize `float`, par colonnes. */
export const MAT4_SIZE = 64;

/** Octets par pixel du tampon de sortie, R, G, B, A. */
export const BYTES_PER_PIXEL = 4;

/** Base de FNV-1a 64 bits. */
const FNV_OFFSET = 0xcbf29ce484222325n;

/** Nombre premier de FNV-1a 64 bits. */
const FNV_PRIME = 0x100000001b3n;

/**
 * Les 256 valeurs d'octet en `BigInt`, construites une fois : l'empreinte
 * d'une image 640×360 en consomme près d'un million.
 */
const BYTE = Array.from({ length: 256 }, (_, i) => BigInt(i));

/**
 * Configuration d'un contexte, dans les unités de `ScgContextConfig`.
 *
 * @typedef {object} Config
 * @property {number} maxWidth largeur maximale, de 1 à 2048
 * @property {number} maxHeight hauteur maximale, de 1 à 2048
 * @property {number} width largeur initiale
 * @property {number} height hauteur initiale
 * @property {number} tileSize côté de tuile, 32 ou 64
 * @property {number} [maxTriangles] triangles préparés par image, 0 pour le défaut
 * @property {number[]} [reserved] les deux champs réservés, nuls par défaut
 */

/** Une instance du module et l'accès à sa mémoire. */
export class Screengine {
  /**
   * Instancie le module sans aucun import : la cible n'en réclame pas, et un
   * import qui apparaîtrait ferait échouer l'instanciation plutôt que de passer
   * inaperçu.
   *
   * @param {BufferSource | WebAssembly.Module} source octets du `.wasm`, ou module compilé
   * @returns {Promise<Screengine>}
   */
  static async instantiate(source) {
    const module = source instanceof WebAssembly.Module ? source : await WebAssembly.compile(source);
    return new Screengine(module, await WebAssembly.instantiate(module, {}));
  }

  /**
   * @param {WebAssembly.Module} module le module compilé
   * @param {WebAssembly.Instance} instance son instance
   */
  constructor(module, instance) {
    /** Le module compilé, pour en lire les imports et les exports. */
    this.module = module;
    /** Les fonctions exportées, appelées telles quelles. */
    this.exports = instance.exports;
    /** @type {WebAssembly.Memory} la mémoire linéaire du module */
    this.memory = instance.exports.memory;
    /**
     * L'adresse de l'emplacement d'erreur sans contexte, prise avant tout
     * appel risqué. Elle ne change pas pendant la vie de l'instance, et c'est
     * là que le crochet de panique écrit avant le trap : on la relit sans
     * rappeler le module, dont la pile est alors dans un état inconnu.
     */
    this.orphan = this.exports.scg_last_error(0) >>> 0;
  }

  /**
   * Une vue fraîche sur toute la mémoire linéaire.
   *
   * @returns {Uint8Array}
   */
  bytes() {
    return new Uint8Array(this.memory.buffer);
  }

  /**
   * La version d'ABI de la bibliothèque, lue non signée : un `i32` exporté
   * arrive signé en JavaScript.
   *
   * @returns {number}
   */
  abiVersion() {
    return this.exports.scg_abi_version() >>> 0;
  }

  /**
   * Alloue un tampon dans la mémoire linéaire.
   *
   * @param {number} len longueur en octets
   * @returns {number} l'adresse non signée, ou 0 ; tester par `=== 0`
   */
  alloc(len) {
    return this.exports.scg_buffer_alloc(len) >>> 0;
  }

  /**
   * Libère un tampon de `alloc`, avec exactement la même longueur.
   *
   * @param {number} ptr adresse rendue par `alloc`
   * @param {number} len longueur passée à `alloc`
   */
  free(ptr, len) {
    this.exports.scg_buffer_free(ptr, len);
  }

  /**
   * Écrit une configuration à `ptr`, octet par octet selon les décalages du
   * header, après avoir mis les 32 octets à zéro comme l'exige l'ABI.
   *
   * @param {number} ptr adresse d'au moins `CONFIG_SIZE` octets
   * @param {Config} config
   */
  writeConfig(ptr, config) {
    const view = new DataView(this.memory.buffer, ptr, CONFIG_SIZE);
    const reserved = config.reserved ?? [0, 0];
    new Uint8Array(this.memory.buffer, ptr, CONFIG_SIZE).fill(0);
    view.setUint32(0, config.maxWidth, true);
    view.setUint32(4, config.maxHeight, true);
    view.setUint32(8, config.width, true);
    view.setUint32(12, config.height, true);
    view.setUint32(16, config.tileSize, true);
    view.setUint32(20, config.maxTriangles ?? 0, true);
    view.setUint32(24, reserved[0], true);
    view.setUint32(28, reserved[1], true);
  }

  /**
   * Écrit un tableau de sommets, trois `float` chacun.
   *
   * @param {number} ptr adresse d'au moins `vertices.length * VERTEX_SIZE` octets
   * @param {number[][]} vertices triplets `[x, y, z]` en coordonnées de monde
   */
  writeVertices(ptr, vertices) {
    const view = new DataView(this.memory.buffer, ptr, vertices.length * VERTEX_SIZE);
    vertices.forEach(([x, y, z], i) => {
      view.setFloat32(i * VERTEX_SIZE, x, true);
      view.setFloat32(i * VERTEX_SIZE + 4, y, true);
      view.setFloat32(i * VERTEX_SIZE + 8, z, true);
    });
  }

  /**
   * Écrit un tableau de triangles : trois indices, puis quatre octets de
   * couleur dans l'ordre mémoire des pixels.
   *
   * @param {number} ptr adresse d'au moins `triangles.length * TRIANGLE_SIZE` octets
   * @param {{indices: number[], color: number[]}[]} triangles
   */
  writeTriangles(ptr, triangles) {
    const view = new DataView(this.memory.buffer, ptr, triangles.length * TRIANGLE_SIZE);
    triangles.forEach(({ indices, color }, i) => {
      const base = i * TRIANGLE_SIZE;
      indices.forEach((index, k) => view.setUint32(base + k * 4, index, true));
      color.forEach((channel, k) => view.setUint8(base + 12 + k, channel));
    });
  }

  /**
   * Écrit un tableau de sommets texturés : trois `float` de position, puis
   * `u` et `v` en texels.
   *
   * @param {number} ptr adresse d'au moins `vertices.length * VERTEX_UV_SIZE` octets
   * @param {number[][]} vertices quintuplets `[x, y, z, u, v]`
   */
  writeVerticesUv(ptr, vertices) {
    const view = new DataView(this.memory.buffer, ptr, vertices.length * VERTEX_UV_SIZE);
    vertices.forEach((vertex, i) => {
      vertex.forEach((value, k) => view.setFloat32(i * VERTEX_UV_SIZE + k * 4, value, true));
    });
  }

  /**
   * Écrit un tableau de sommets éclairés : les cinq `float` du sommet texturé,
   * puis `u2` et `v2` en texels de la lightmap.
   *
   * Les sept valeurs sont jointives, sans bourrage : c'est ce que le header
   * affirme, et ce que cette liaison écrit octet par octet.
   *
   * @param {number} ptr adresse d'au moins `vertices.length * VERTEX_UV2_SIZE` octets
   * @param {number[][]} vertices septuplets `[x, y, z, u, v, u2, v2]`
   */
  writeVerticesUv2(ptr, vertices) {
    const view = new DataView(this.memory.buffer, ptr, vertices.length * VERTEX_UV2_SIZE);
    vertices.forEach((vertex, i) => {
      vertex.forEach((value, k) => view.setFloat32(i * VERTEX_UV2_SIZE + k * 4, value, true));
    });
  }

  /**
   * Écrit un tableau de lumières : quatre `float`, puis trois canaux de
   * couleur et l'octet réservé, mis à zéro.
   *
   * L'octet réservé est écrit explicitement plutôt que laissé tel quel : la
   * mémoire linéaire n'est pas remise à zéro entre deux allocations, et un
   * reste d'écriture précédente ferait refuser le lot.
   *
   * @param {number} ptr adresse d'au moins `lights.length * LIGHT_SIZE` octets
   * @param {{position: number[], radius: number, color: number[]}[]} lights
   */
  writeLights(ptr, lights) {
    const view = new DataView(this.memory.buffer, ptr, lights.length * LIGHT_SIZE);
    lights.forEach((light, i) => {
      const base = i * LIGHT_SIZE;
      light.position.forEach((value, k) => view.setFloat32(base + k * 4, value, true));
      view.setFloat32(base + 12, light.radius, true);
      light.color.forEach((channel, k) => view.setUint8(base + 16 + k, channel));
      view.setUint8(base + 19, 0);
    });
  }

  /**
   * Écrit une courbe de sortie, champs réservés compris.
   *
   * La structure est mise à zéro d'abord : la mémoire linéaire ne l'est pas
   * entre deux allocations, et un reste d'écriture précédente dans un champ
   * réservé ferait refuser le réglage.
   *
   * @param {number} ptr adresse d'au moins `GRADE_SIZE` octets
   * @param {{gamma: number, gains: number[], offsets: number[]}} grade
   */
  writeGrade(ptr, grade) {
    new Uint8Array(this.memory.buffer, ptr, GRADE_SIZE).fill(0);
    const view = new DataView(this.memory.buffer, ptr, GRADE_SIZE);
    view.setFloat32(0, grade.gamma, true);
    grade.gains.forEach((value, k) => view.setFloat32(4 + k * 4, value, true));
    grade.offsets.forEach((value, k) => view.setFloat32(16 + k * 4, value, true));
  }

  /**
   * Écrit une caméra.
   *
   * Le quaternion se range `x, y, z, w`, la partie réelle en dernier :
   * l'identité est `{0, 0, 0, 1}`, et c'est la convention inverse de la plus
   * répandue — une liaison qui écrit ces octets à la main s'y trompe une fois.
   * Il n'est pas exigé unitaire, le moteur le normalise.
   *
   * @param {number} ptr adresse d'au moins `CAMERA_SIZE` octets
   * @param {{position: number[], orientation: number[], fovY: number,
   *   nearPlane: number}} camera la caméra à écrire
   */
  /**
   * Écrit trois flottants à `ptr` : une position, telle que l'ABI l'attend.
   *
   * La localisation et le suivi de cellule prennent des points par pointeur, et
   * c'est le seul endroit où l'hôte en passe un sans structure autour. Sans cette
   * aide, chaque appel recopierait son propre `DataView` — et c'est ainsi qu'un
   * boutisme finit par diverger d'un appel à l'autre.
   *
   * @param {number} ptr adresse d'au moins douze octets
   * @param {number[]} point les trois coordonnées
   */
  writePoint(ptr, point) {
    const view = new DataView(this.memory.buffer, ptr, 12);
    point.forEach((value, k) => view.setFloat32(k * 4, value, true));
  }

  writeCamera(ptr, camera) {
    new Uint8Array(this.memory.buffer, ptr, CAMERA_SIZE).fill(0);
    const view = new DataView(this.memory.buffer, ptr, CAMERA_SIZE);
    camera.position.forEach((value, k) => view.setFloat32(k * 4, value, true));
    camera.orientation.forEach((value, k) =>
      view.setFloat32(12 + k * 4, value, true),
    );
    view.setFloat32(28, camera.fovY, true);
    view.setFloat32(32, camera.nearPlane, true);
  }

  /**
   * Écrit une description de texture, champs réservés compris.
   *
   * La structure est mise à zéro d'abord : ses champs réservés doivent l'être,
   * et c'est ce qui permettra d'en employer un sans casser cette liaison.
   *
   * @param {number} ptr adresse d'au moins `TEXTURE_DESC_SIZE` octets
   * @param {number} width largeur en texels, puissance de deux
   * @param {number} height hauteur en texels, puissance de deux
   */
  writeTextureDesc(ptr, width, height) {
    new Uint8Array(this.memory.buffer, ptr, TEXTURE_DESC_SIZE).fill(0);
    const view = new DataView(this.memory.buffer, ptr, TEXTURE_DESC_SIZE);
    view.setUint32(0, width, true);
    view.setUint32(4, height, true);
    view.setUint32(8, SCG_TEXTURE_FORMAT_RGBA8, true);
  }

  /**
   * Écrit la matrice identité, par colonnes.
   *
   * @param {number} ptr adresse d'au moins `MAT4_SIZE` octets
   */
  writeIdentity(ptr) {
    const view = new DataView(this.memory.buffer, ptr, MAT4_SIZE);
    new Uint8Array(this.memory.buffer, ptr, MAT4_SIZE).fill(0);
    for (let i = 0; i < 4; i++) {
      view.setFloat32(i * 20, 1, true);
    }
  }

  /**
   * Écrit une `ScgMat4` quelconque, seize flottants par colonnes.
   *
   * Les valeurs sont écrites telles quelles : une matrice dont les
   * coefficients seraient recalculés ici ne rendrait pas les mêmes bits que le
   * chemin Rust, donc pas la même empreinte.
   *
   * @param {number} ptr adresse d'au moins `MAT4_SIZE` octets
   * @param {number[]} m les seize coefficients, par colonnes
   */
  writeMat4(ptr, m) {
    const view = new DataView(this.memory.buffer, ptr, MAT4_SIZE);
    for (let i = 0; i < 16; i++) {
      view.setFloat32(i * 4, m[i], true);
    }
  }

  /**
   * Lit un `uint32_t` petit-boutiste.
   *
   * @param {number} ptr
   * @returns {number}
   */
  readU32(ptr) {
    return new DataView(this.memory.buffer).getUint32(ptr, true);
  }

  /**
   * Écrit un `uint32_t` petit-boutiste.
   *
   * @param {number} ptr
   * @param {number} value
   */
  writeU32(ptr, value) {
    new DataView(this.memory.buffer).setUint32(ptr, value, true);
  }

  /**
   * Lit une chaîne terminée par un octet nul. Le décodage est strict : un
   * UTF-8 invalide lève plutôt que de rendre des caractères de remplacement.
   *
   * @param {number} ptr adresse non signée, non nulle
   * @returns {string}
   */
  readCString(ptr) {
    const bytes = this.bytes();
    let end = ptr;
    while (bytes[end] !== 0) {
      end++;
    }
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(ptr, end));
  }

  /**
   * Le dernier message d'erreur, copié aussitôt.
   *
   * @param {number} ctx handle de contexte, ou 0 pour l'emplacement sans contexte
   * @returns {string}
   */
  lastError(ctx) {
    return this.readCString(this.exports.scg_last_error(ctx) >>> 0);
  }

  /**
   * Le message qu'une panique a laissé avant le trap, lu sans appeler le
   * module. Après un trap, l'instance ne sert plus qu'à cela : on en crée une
   * autre.
   *
   * @returns {string}
   */
  trapMessage() {
    return this.readCString(this.orphan);
  }

  /**
   * FNV-1a 64 bits dans la forme de la conformance : largeur et hauteur en
   * `u32` petit-boutiste, puis la zone utile ligne par ligne, alpha compris.
   *
   * @param {number} ptr début du tampon de pixels
   * @param {number} width largeur en pixels
   * @param {number} height hauteur en pixels
   * @param {number} stride pas de ligne en pixels
   * @returns {string} seize chiffres hexadécimaux minuscules
   */
  fingerprint(ptr, width, height, stride) {
    const bytes = this.bytes();
    let hash = FNV_OFFSET;
    const mix = (byte) => {
      hash = BigInt.asUintN(64, (hash ^ BYTE[byte]) * FNV_PRIME);
    };
    for (const dim of [width, height]) {
      for (let shift = 0; shift < 32; shift += 8) {
        mix((dim >>> shift) & 0xff);
      }
    }
    for (let y = 0; y < height; y++) {
      const row = ptr + y * stride * BYTES_PER_PIXEL;
      for (let i = 0; i < width * BYTES_PER_PIXEL; i++) {
        mix(bytes[row + i]);
      }
    }
    return hash.toString(16).padStart(16, "0");
  }
}
