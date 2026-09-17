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

/** Objet empoisonné par une panique antérieure. Inobservable sur wasm, pour la même raison. */
export const SCG_ERR_POISONED = -6;

/** Les fonctions que le module doit exporter, en plus de `memory`. */
export const EXPORTS = [
  "scg_abi_version",
  "scg_create",
  "scg_destroy",
  "scg_frame_end",
  "scg_last_error",
  "scg_buffer_alloc",
  "scg_buffer_free",
];

/** Taille de `ScgContextConfig`, celle qu'affirme le header. */
export const CONFIG_SIZE = 32;

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
 * @property {number[]} [reserved] les trois champs réservés, nuls par défaut
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
    const reserved = config.reserved ?? [0, 0, 0];
    new Uint8Array(this.memory.buffer, ptr, CONFIG_SIZE).fill(0);
    view.setUint32(0, config.maxWidth, true);
    view.setUint32(4, config.maxHeight, true);
    view.setUint32(8, config.width, true);
    view.setUint32(12, config.height, true);
    view.setUint32(16, config.tileSize, true);
    view.setUint32(20, reserved[0], true);
    view.setUint32(24, reserved[1], true);
    view.setUint32(28, reserved[2], true);
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
