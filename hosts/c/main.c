// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

/*
 * Hôte C de Screengine, sans fenêtre.
 *
 * Il franchit réellement la frontière, lié à la bibliothèque statique, et
 * vérifie ce que des tests Rust appelant les fonctions exportées ne peuvent pas
 * voir : l'édition de liens, la disposition des structures vue par un
 * compilateur C, l'écriture hors du tampon, l'environnement flottant de l'hôte.
 * Il écrit sur la sortie standard l'empreinte du triangle, que `make test-abi`
 * compare à celle du chemin Rust ; tout le reste part sur la sortie d'erreur.
 *
 * La scène est celle de `screengine-conformance --print arete` : 640×360,
 * tuiles de 64. La changer d'un seul côté ferait diverger les deux empreintes.
 */

#include "screengine.h"

/* Sans C11, le header saute ses assertions de disposition sans rien dire, et
 * c'est précisément ce que cet hôte existe pour vérifier. */
#if !defined(__STDC_VERSION__) || __STDC_VERSION__ < 201112L
#error "C11 requis : sans lui, les assertions de disposition du header ne sont pas compilées"
#endif

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__x86_64__) || defined(_M_X64)
#include <xmmintrin.h>
#define HAS_FP_CONTROL 1

/* Le registre de contrôle flottant de l'hôte. */
typedef unsigned int fp_word;

/* Lit MXCSR. */
static fp_word fp_read(void)
{
    return _mm_getcsr();
}

/* Écrit MXCSR. */
static void fp_write(fp_word word)
{
    _mm_setcsr(word);
}

/* Exceptions démasquées, arrondi vers le haut, DAZ et FZ. */
static fp_word fp_hostile(fp_word host)
{
    return (host & ~0x7F80u) | 0x4000u | 0x8040u;
}
#elif defined(__aarch64__) || defined(__arm__)
#include <fenv.h>
#define HAS_FP_CONTROL 1

/* Le mot de contrôle : FPCR sur aarch64, FPSCR sur armv7. */
typedef uint32_t fp_word;

/* Lit le mot de contrôle par fenv.h. Ses quatre premiers octets sont ce mot
 * chez bionic comme chez la glibc, sur les deux architectures ; les champs, eux,
 * n'ont pas le même nom. */
static fp_word fp_read(void)
{
    fenv_t env;
    fp_word word;
    fegetenv(&env);
    memcpy(&word, &env, sizeof word);
    return word;
}

/* Écrit le mot de contrôle, sans toucher au reste de l'environnement. */
static void fp_write(fp_word word)
{
    fenv_t env;
    fegetenv(&env);
    memcpy(&env, &word, sizeof word);
    fesetenv(&env);
}

/* FZ, arrondi vers le haut et les six autorisations de piège. Beaucoup de cœurs
 * — et qemu — laissent ces dernières à zéro : c'est pourquoi le registre est
 * relu après l'écriture plutôt que comparé à cette valeur. */
static fp_word fp_hostile(fp_word host)
{
    return (host & ~0x00C00000u) | 0x01000000u | 0x00400000u | 0x9F00u;
}
#else
#define HAS_FP_CONTROL 0
#endif

/* Largeur, hauteur et tuile de la scène. */
enum { WIDTH = 640, HEIGHT = 360, TILE = 64 };

/* Le stride de l'hôte : plus grand que la largeur, pour que l'espace de fin de
 * ligne existe et qu'on vérifie que le moteur n'y écrit pas. */
enum { STRIDE = WIDTH + 3 };

/* Marge sentinelle avant et après le tampon, en octets. */
enum { GUARD = 64 };

/* L'octet sentinelle, choisi pour ne ressembler à aucune couleur du rendu. */
enum { SENTINEL = 0xA5 };

/* Nombre de vérifications en échec. */
static int failures;

/* Enregistre une vérification, et dit laquelle a échoué. */
static void check(int ok, const char *what)
{
    if (!ok) {
        fprintf(stderr, "échec : %s\n", what);
        failures++;
    }
}

/* FNV-1a 64 bits, dans la forme de screengine-conformance : largeur et hauteur
 * en u32 petit-boutiste, puis la zone utile ligne par ligne. */
static uint64_t fingerprint(const uint8_t *pixels, uint32_t width, uint32_t height, uint32_t stride)
{
    uint64_t hash = 0xcbf29ce484222325u;
    uint32_t dims[2] = {width, height};

    for (int d = 0; d < 2; d++) {
        for (int shift = 0; shift < 32; shift += 8) {
            hash = (hash ^ ((dims[d] >> shift) & 0xff)) * 0x100000001b3u;
        }
    }
    for (uint32_t y = 0; y < height; y++) {
        const uint8_t *row = pixels + (size_t)y * stride * 4;
        for (uint32_t i = 0; i < width * 4; i++) {
            hash = (hash ^ row[i]) * 0x100000001b3u;
        }
    }
    return hash;
}

/* Une configuration valide, mise à zéro entière comme l'exige l'ABI. */
static ScgContextConfig scene_config(void)
{
    ScgContextConfig config;
    memset(&config, 0, sizeof config);
    config.max_width = config.width = WIDTH;
    config.max_height = config.height = HEIGHT;
    config.tile_size = TILE;
    return config;
}

/* Le quadrilatère de la scène `arete`, en coordonnées de monde : X vers l'est,
 * Z en haut. La caméra par défaut le regarde depuis l'origine.
 *
 * Ces valeurs sont celles de la scène de conformance, et c'est tout l'objet de
 * cet hôte : décrire la même scène dans un autre langage, à travers l'ABI, et
 * retrouver la même empreinte. Les changer ici sans la changer là-bas fait
 * diverger la comparaison, ce qui est exactement ce qu'on veut qu'il arrive. */
static const ScgVertex SCENE_VERTICES[4] = {
    { 2.0f, 2.5f, 1.6f },
    { 3.5f, -2.5f, 1.6f },
    { 3.5f, -2.5f, -1.6f },
    { 2.0f, 2.5f, -1.6f },
};

/* Deux triangles qui partagent l'arête des sommets 0 et 2, parcourue en sens
 * opposés par chacun : le cas que la règle top-left doit trancher. */
static const ScgTriangle SCENE_TRIANGLES[2] = {
    { 0, 2, 1, 0xE0, 0xA0, 0x30, 0xFF },
    { 0, 3, 2, 0xA0, 0xE0, 0x30, 0xFF },
};

/* L'identité, par colonnes. */
static const ScgMat4 IDENTITY = {
    { 1.0f, 0.0f, 0.0f, 0.0f,
      0.0f, 1.0f, 0.0f, 0.0f,
      0.0f, 0.0f, 1.0f, 0.0f,
      0.0f, 0.0f, 0.0f, 1.0f }
};

/* Soumet la scène au contexte, et rend vrai si elle a été acceptée. */
static int submit_scene(ScgContext *ctx)
{
    int32_t code = scg_submit(ctx, &IDENTITY, SCENE_VERTICES, 4, SCENE_TRIANGLES, 2);
    return code == SCG_OK;
}

/* Le sol de la scène `texture` : un damier qui fuit vers l'horizon, à 1,2 unité
 * sous la caméra. Mêmes valeurs que la scène de conformance, et pour la même
 * raison que le quadrilatère ci-dessus. */
enum { FLOOR_SIDE = 64, FLOOR_CELL = 8 };

/* Les coordonnées de texture s'écrivent en clair, densité comprise : une
 * constante de plus ne dirait rien que le littéral ne dise, et l'hôte doit se
 * lire comme ce qu'il est — la transcription d'une scène de référence. */
static const ScgVertexUv FLOOR_VERTICES[4] = {
    {  2.0f, -24.0f, -1.2f,   2.0f * 8.0f, -24.0f * 8.0f },
    { 60.0f, -24.0f, -1.2f,  60.0f * 8.0f, -24.0f * 8.0f },
    { 60.0f,  24.0f, -1.2f,  60.0f * 8.0f,  24.0f * 8.0f },
    {  2.0f,  24.0f, -1.2f,   2.0f * 8.0f,  24.0f * 8.0f },
};

static const ScgTriangle FLOOR_TRIANGLES[2] = {
    { 0, 1, 2, 0xFF, 0xFF, 0xFF, 0xFF },
    { 0, 2, 3, 0xFF, 0xFF, 0xFF, 0xFF },
};

/* La scène `lumiere` : un sol texturé qui fuit et un mur uni au fond, tous
 * deux éclairés par la même lightmap. Les deux chemins éclairés dans une seule
 * image — `texel x lightmap` au sol, `couleur x lightmap` au mur —, et c'est
 * le second que cet hôte serait seul à ne pas emprunter si on l'omettait. */
enum { LIGHT_SIDE = 16 };

/* Les coordonnées de lightmap vont d'un demi-texel à un demi-texel du bord
 * opposé : une lightmap ne se pave pas, et le bilinéaire irait chercher son
 * voisin par le repli. */
static const ScgVertexUv2 LIT_FLOOR[4] = {
    {  2.0f, -16.0f, -1.2f,   2.0f * 8.0f, -16.0f * 8.0f,  0.5f,  0.5f },
    { 40.0f, -16.0f, -1.2f,  40.0f * 8.0f, -16.0f * 8.0f, 15.5f,  0.5f },
    { 40.0f,  16.0f, -1.2f,  40.0f * 8.0f,  16.0f * 8.0f, 15.5f, 15.5f },
    {  2.0f,  16.0f, -1.2f,   2.0f * 8.0f,  16.0f * 8.0f,  0.5f, 15.5f },
};

/* Le mur, sans texture : sa densité est nulle, donc ses coordonnées de texture
 * aussi, et c'est la couleur du triangle qui tient lieu de texel. */
static const ScgVertexUv2 LIT_WALL[4] = {
    { 40.0f, -16.0f, -1.2f, 0.0f, 0.0f,  0.5f,  0.5f },
    { 40.0f, -16.0f, 10.0f, 0.0f, 0.0f, 15.5f,  0.5f },
    { 40.0f,  16.0f, 10.0f, 0.0f, 0.0f, 15.5f, 15.5f },
    { 40.0f,  16.0f, -1.2f, 0.0f, 0.0f,  0.5f, 15.5f },
};

static const ScgTriangle WALL_TRIANGLES[2] = {
    { 0, 1, 2, 0xC0, 0xB0, 0x90, 0xFF },
    { 0, 2, 3, 0xC0, 0xB0, 0x90, 0xFF },
};

/* Écrit le dégradé de lightmap dans `pixels`, qui couvre `LIGHT_SIDE` au carré
 * texels de quatre octets.
 *
 * Les deux axes n'y font pas la même chose, et c'est voulu : un dégradé
 * symétrique laisserait passer un axe échangé entre les deux jeux de
 * coordonnées. Recopié de la suite de conformance, teinte pour teinte. */
static void make_gradient(uint8_t *pixels)
{
    for (uint32_t v = 0; v < LIGHT_SIDE; v++) {
        for (uint32_t u = 0; u < LIGHT_SIDE; u++) {
            uint8_t *texel = pixels + ((size_t)v * LIGHT_SIDE + u) * 4;
            texel[0] = (uint8_t)(32 + u * 223 / (LIGHT_SIDE - 1));
            texel[1] = (uint8_t)(32 + ((u + v) / 2) * 223 / (LIGHT_SIDE - 1));
            texel[2] = (uint8_t)(32 + v * 223 / (LIGHT_SIDE - 1));
            texel[3] = 0xFF;
        }
    }
}

/* Écrit le damier procédural dans `pixels`, qui couvre `FLOOR_SIDE` au carré
 * texels de quatre octets.
 *
 * Le motif est recopié de la suite de conformance, teinte pour teinte : c'est
 * lui qui décide de l'empreinte, et un liseré décalé d'un texel la ferait
 * diverger — ce qui est exactement l'objet de cette comparaison. */
static void make_checker(uint8_t *pixels)
{
    for (uint32_t v = 0; v < FLOOR_SIDE; v++) {
        for (uint32_t u = 0; u < FLOOR_SIDE; u++) {
            uint8_t *texel = pixels + ((size_t)v * FLOOR_SIDE + u) * 4;
            int edge = (u % FLOOR_CELL) == 0 || (v % FLOOR_CELL) == 0;
            int dark = ((u / FLOOR_CELL) + (v / FLOOR_CELL)) % 2 == 0;
            if (edge) {
                texel[0] = 0xF0; texel[1] = 0xE0; texel[2] = 0xA0;
            } else if (dark) {
                texel[0] = 0x30; texel[1] = 0x38; texel[2] = 0x50;
            } else {
                texel[0] = 0x90; texel[1] = 0x70; texel[2] = 0x50;
            }
            texel[3] = 0xFF;
        }
    }
}

/* Vrai si le message est non nul, terminé, non vide si `expect_text`, et fait
 * d'octets UTF-8 plausibles. Les messages du moteur sont ASCII aujourd'hui ;
 * ce contrôle refuse surtout un octet de contrôle venu d'un tampon non
 * initialisé. */
static int message_ok(const char *message, int expect_text)
{
    if (message == NULL) {
        return 0;
    }
    size_t len = strlen(message);
    if (expect_text != (len > 0)) {
        return 0;
    }
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)message[i];
        if (c < 0x20) {
            return 0;
        }
    }
    return 1;
}

/* Les refus, vus depuis C : codes exacts et messages lisibles. */
static void check_refusals(void)
{
    ScgContextConfig config = scene_config();
    ScgContext *ctx = NULL;

    check(scg_create(NULL, &ctx) == SCG_ERR_NULL, "configuration nulle refusée par SCG_ERR_NULL");
    check(message_ok(scg_last_error(NULL), 1), "message sans contexte après une configuration nulle");

    config.tile_size = 48;
    check(scg_create(&config, &ctx) == SCG_ERR_INVALID_ARGUMENT, "taille de tuile 48 refusée");
    check(ctx == NULL, "rien n'est écrit dans le paramètre de sortie après un refus");

    config = scene_config();
    config.reserved1 = 1;
    check(scg_create(&config, &ctx) == SCG_ERR_INVALID_ARGUMENT, "champ réservé non nul refusé");

    config = scene_config();
    check(scg_create(&config, NULL) == SCG_ERR_NULL, "paramètre de sortie nul refusé");

    config = scene_config();
    check(scg_create(&config, &ctx) == SCG_OK, "configuration valide acceptée");
    if (ctx == NULL) {
        return;
    }
    check(message_ok(scg_last_error(ctx), 0), "message vide après un succès");
    check(message_ok(scg_last_error(NULL), 0), "message sans contexte vidé par un appel réussi");

    uint8_t small[4 * 4];
    check(scg_frame_end(ctx, NULL, WIDTH) == SCG_ERR_NULL, "tampon nul refusé");
    check(scg_frame_end(ctx, small, WIDTH - 1) == SCG_ERR_INVALID_ARGUMENT, "stride inférieur à la largeur refusé");
    check(message_ok(scg_last_error(ctx), 1), "message du contexte après un stride refusé");
    check(scg_frame_end(NULL, small, WIDTH) == SCG_ERR_NULL, "contexte nul refusé");

    scg_destroy(ctx);
    scg_destroy(NULL);
}

/* L'allocation pour le compte de l'hôte : alignement de l'ABI, longueur écrite
 * en entier, cas limites. */
static void check_buffers(void)
{
    size_t len = (size_t)WIDTH * HEIGHT * 4;
    uint8_t *buffer = scg_buffer_alloc(len);

    check(buffer != NULL, "scg_buffer_alloc rend un tampon");
    if (buffer != NULL) {
        check(((uintptr_t)buffer % SCG_BUFFER_ALIGNMENT) == 0, "tampon aligné sur SCG_BUFFER_ALIGNMENT");
        memset(buffer, 0, len);
        scg_buffer_free(buffer, len);
    }
    check(scg_buffer_alloc(0) == NULL, "une allocation de zéro octet rend NULL");
    scg_buffer_free(NULL, 0);
}

/* Rend le triangle dans un tampon entouré de sentinelles, vérifie que rien
 * n'est écrit hors de la zone utile, et rend l'empreinte. `ok` reçoit 0 si le
 * rendu lui-même a échoué. */
static uint64_t render(int *ok)
{
    ScgContextConfig config = scene_config();
    ScgContext *ctx = NULL;
    size_t body = (size_t)STRIDE * HEIGHT * 4;
    uint8_t *block = malloc(GUARD + body + GUARD);
    uint64_t hash = 0;

    *ok = 0;
    if (block == NULL || scg_create(&config, &ctx) != SCG_OK) {
        check(0, "création du contexte de rendu");
        free(block);
        return 0;
    }

    uint8_t *pixels = block + GUARD;
    memset(block, SENTINEL, GUARD + body + GUARD);

    check(submit_scene(ctx), "scène soumise");
    int32_t code = scg_frame_end(ctx, pixels, STRIDE);
    check(code == SCG_OK, "scg_frame_end aboutit");
    *ok = code == SCG_OK;

    int intact = 1;
    for (size_t i = 0; i < GUARD; i++) {
        intact &= block[i] == SENTINEL && pixels[body + i] == SENTINEL;
    }
    for (size_t y = 0; y < HEIGHT; y++) {
        const uint8_t *tail = pixels + y * STRIDE * 4 + (size_t)WIDTH * 4;
        for (size_t i = 0; i < (size_t)(STRIDE - WIDTH) * 4; i++) {
            intact &= tail[i] == SENTINEL;
        }
    }
    check(intact, "rien n'est écrit hors de la zone utile, marges et fins de ligne comprises");

    int opaque = 1;
    for (size_t y = 0; y < HEIGHT; y++) {
        const uint8_t *row = pixels + y * STRIDE * 4;
        for (size_t x = 0; x < WIDTH; x++) {
            opaque &= row[x * 4 + 3] == 255;
        }
    }
    check(opaque, "l'alpha est écrit à 255 sur chaque pixel");

    hash = fingerprint(pixels, WIDTH, HEIGHT, STRIDE);
    scg_destroy(ctx);
    free(block);
    return hash;
}

/* Le rendu par tuiles, vu depuis C : la séquence et ses refus, puis une image
 * dont une partie des tuiles est rendue dans l'ordre inverse et le reste laissé
 * à la fin. Son empreinte doit être celle de la fin seule. */
static void check_tiles(uint64_t expected)
{
    ScgContextConfig config = scene_config();
    ScgContext *ctx = NULL;
    uint8_t *pixels = malloc((size_t)STRIDE * HEIGHT * 4);
    uint32_t count = 0;

    if (pixels == NULL || scg_create(&config, &ctx) != SCG_OK) {
        check(0, "création du contexte des tuiles");
        free(pixels);
        return;
    }

    check(scg_frame_tile(ctx, 0, pixels, STRIDE) == SCG_ERR_INVALID_STATE, "tuile avant le début refusée");
    check(message_ok(scg_last_error(NULL), 1), "message de la tuile dans l'emplacement du thread");
    check(message_ok(scg_last_error(ctx), 0), "message du contexte intact après une tuile refusée");

    check(submit_scene(ctx), "scène soumise");
    check(scg_frame_begin(ctx, NULL) == SCG_ERR_NULL, "compteur de tuiles nul refusé");
    check(scg_frame_begin(ctx, &count) == SCG_OK, "scg_frame_begin aboutit");
    check(count == (WIDTH + TILE - 1) / TILE * ((HEIGHT + TILE - 1) / TILE), "nombre de tuiles de l'image");
    check(scg_frame_begin(ctx, &count) == SCG_ERR_INVALID_STATE, "second début refusé");
    check(scg_frame_tile(ctx, count, pixels, STRIDE) == SCG_ERR_INVALID_ARGUMENT, "index hors de l'image refusé");

    for (uint32_t i = count; i-- > 0;) {
        if (i % 3 != 0) {
            check(scg_frame_tile(ctx, i, pixels, STRIDE) == SCG_OK, "tuile rendue");
        }
    }
    check(scg_frame_tile(ctx, 1, pixels, STRIDE) == SCG_ERR_INVALID_STATE, "tuile rendue deux fois refusée");

    check(scg_frame_end(ctx, pixels, STRIDE) == SCG_OK, "la fin complète les tuiles manquantes");
    check(fingerprint(pixels, WIDTH, HEIGHT, STRIDE) == expected, "les tuiles rendent l'image de la fin seule");

    scg_destroy(ctx);
    free(pixels);
}

#if HAS_FP_CONTROL
/* Un hôte hostile : exceptions démasquées, arrondi vers le haut, zéro forcé. Le
 * moteur doit rendre la même image, et rendre le registre intact.
 *
 * La comparaison d'empreinte porte désormais : la scène est soumise en
 * coordonnées de monde et passe par la transformation des sommets, la
 * projection et la division, tout ce que l'arrondi et DAZ changeraient si la
 * frontière ne fixait pas le registre à l'entrée. */
static void check_float_environment(uint64_t expected)
{
    fp_word host = fp_read();

    fp_write(fp_hostile(host));
    fp_word hostile = fp_read();
    check(hostile != host, "le registre flottant accepte un environnement hostile");
    int ok = 0;
    uint64_t hash = render(&ok);
    fp_word after = fp_read();
    fp_write(host);

    check(ok, "le rendu aboutit sous un environnement flottant hostile");
    check(hash == expected, "l'environnement flottant de l'hôte ne change pas l'image");
    check(after == hostile, "le registre flottant de l'hôte est rendu à l'identique");
}
#endif

/* Rend la scène texturée sous le filtrage demandé et hache son image.
 *
 * Plus courte que `render` : les sentinelles, l'alpha et les tuiles sont déjà
 * éprouvés par la première scène, qui passe par le même tampon et le même
 * chemin de sortie. Ce que celle-ci ajoute est le seul chemin que l'autre
 * n'emprunte pas — chargement d'une texture, soumission texturée,
 * échantillonnage — et son empreinte le compare au chemin Rust.
 *
 * `filter` la rejoue telle quelle en bilinéaire : la géométrie ne bouge pas,
 * si bien qu'un écart entre les deux empreintes ne peut venir que de là.
 */
static uint64_t render_textured(int *ok, uint32_t filter)
{
    ScgContextConfig config = scene_config();
    ScgContext *ctx = NULL;
    ScgTexture *texture = NULL;
    uint8_t *pixels = malloc((size_t)STRIDE * HEIGHT * 4);
    uint8_t *texels = malloc((size_t)FLOOR_SIDE * FLOOR_SIDE * 4);
    uint64_t hash = 0;

    *ok = 0;
    if (pixels == NULL || texels == NULL) {
        check(0, "allocation des tampons de la scène texturée");
        free(pixels);
        free(texels);
        return 0;
    }
    make_checker(texels);

    ScgTextureDesc desc;
    memset(&desc, 0, sizeof desc);
    desc.width = FLOOR_SIDE;
    desc.height = FLOOR_SIDE;
    desc.format = SCG_TEXTURE_FORMAT_RGBA8;

    int loaded = scg_texture_load(&desc, texels, (size_t)FLOOR_SIDE * FLOOR_SIDE * 4, &texture);
    check(loaded == SCG_OK, "la texture se charge sans contexte");
    check(scg_create(&config, &ctx) == SCG_OK, "création du contexte texturé");
    if (ctx != NULL) {
        check(scg_set_filter(ctx, filter) == SCG_OK, "le filtrage se règle");
    }

    if (loaded == SCG_OK && ctx != NULL) {
        int32_t code = scg_submit_textured(ctx, &IDENTITY, FLOOR_VERTICES, 4,
                                           FLOOR_TRIANGLES, 2, texture);
        check(code == SCG_OK, "le lot texturé est accepté");
        /* Détruite avant le rendu, à dessein : le moteur en garde sa propre
         * référence jusqu'à la fin de l'image, et l'empreinte le prouve. */
        scg_texture_destroy(texture);
        texture = NULL;

        code = scg_frame_end(ctx, pixels, STRIDE);
        check(code == SCG_OK, "l'image texturée se rend");
        *ok = code == SCG_OK;
        hash = fingerprint(pixels, WIDTH, HEIGHT, STRIDE);
    }

    scg_texture_destroy(texture);
    scg_destroy(ctx);
    free(pixels);
    free(texels);
    return hash;
}

/* Le sol de la scène `brouillard`, plus long que la rampe : sa moitié
 * lointaine se confond avec le fond, sa moitié proche garde son damier. */
static const ScgVertexUv FOG_FLOOR[4] = {
    {  1.0f, -20.0f, -1.2f,   1.0f * 8.0f, -20.0f * 8.0f },
    { 50.0f, -20.0f, -1.2f,  50.0f * 8.0f, -20.0f * 8.0f },
    { 50.0f,  20.0f, -1.2f,  50.0f * 8.0f,  20.0f * 8.0f },
    {  1.0f,  20.0f, -1.2f,   1.0f * 8.0f,  20.0f * 8.0f },
};

/* Rend la scène embrumée et en donne l'empreinte.
 *
 * Le fond n'est effacé de rien : c'est le moteur qui lui donne la couleur du
 * brouillard, parce qu'un pixel non peint est infiniment lointain. Un hôte qui
 * effacerait lui-même redessinerait la couture qu'on cherche à supprimer, et
 * l'empreinte le dirait. */
static uint64_t render_fog(int *ok)
{
    ScgContextConfig config = scene_config();
    ScgContext *ctx = NULL;
    ScgTexture *texture = NULL;
    uint8_t *pixels = malloc((size_t)STRIDE * HEIGHT * 4);
    uint8_t *texels = malloc((size_t)FLOOR_SIDE * FLOOR_SIDE * 4);
    uint64_t hash = 0;

    *ok = 0;
    if (pixels == NULL || texels == NULL) {
        check(0, "allocation des tampons de la scène embrumée");
        free(pixels);
        free(texels);
        return 0;
    }
    make_checker(texels);

    ScgTextureDesc desc;
    memset(&desc, 0, sizeof desc);
    desc.width = FLOOR_SIDE;
    desc.height = FLOOR_SIDE;
    desc.format = SCG_TEXTURE_FORMAT_RGBA8;
    int loaded = scg_texture_load(&desc, texels, (size_t)FLOOR_SIDE * FLOOR_SIDE * 4, &texture);
    check(loaded == SCG_OK, "la texture du sol embrumé se charge");
    check(scg_create(&config, &ctx) == SCG_OK, "création du contexte embrumé");

    if (loaded == SCG_OK && ctx != NULL) {
        /* Une rampe vide est refusée : c'est une division par zéro, et
         * l'appelant voulait vraisemblablement éteindre le brouillard. */
        check(scg_set_fog(ctx, 0x30, 0x38, 0x48, 10.0f, 10.0f) == SCG_ERR_INVALID_ARGUMENT,
              "une rampe vide est refusée");
        /* Éteindre un brouillard qui n'existe pas n'est pas une erreur. */
        check(scg_clear_fog(ctx) == SCG_OK, "l'extinction sans brouillard passe");

        check(scg_set_fog(ctx, 0x30, 0x38, 0x48, 3.0f, 14.0f) == SCG_OK,
              "le brouillard se règle");

        int32_t code = scg_submit_textured(ctx, &IDENTITY, FOG_FLOOR, 4,
                                           FLOOR_TRIANGLES, 2, texture);
        check(code == SCG_OK, "le sol embrumé est accepté");
        scg_texture_destroy(texture);
        texture = NULL;

        code = scg_frame_end(ctx, pixels, STRIDE);
        check(code == SCG_OK, "l'image embrumée se rend");
        *ok = code == SCG_OK;
        hash = fingerprint(pixels, WIDTH, HEIGHT, STRIDE);
    }

    scg_texture_destroy(texture);
    scg_destroy(ctx);
    free(pixels);
    free(texels);
    return hash;
}

/* Rend la scène éclairée et en donne l'empreinte.
 *
 * Deux lots : le sol, texturé et éclairé, puis le mur, éclairé seul. Le second
 * passe une texture nulle, ce que ce point d'entrée accepte là où
 * `scg_submit_textured` le refuse — c'est l'asymétrie que le header signale,
 * et cet hôte l'exerce pour de bon. */
static uint64_t render_lit(int *ok)
{
    ScgContextConfig config = scene_config();
    ScgContext *ctx = NULL;
    ScgTexture *texture = NULL;
    ScgTexture *lightmap = NULL;
    uint8_t *pixels = malloc((size_t)STRIDE * HEIGHT * 4);
    uint8_t *texels = malloc((size_t)FLOOR_SIDE * FLOOR_SIDE * 4);
    uint8_t *luxels = malloc((size_t)LIGHT_SIDE * LIGHT_SIDE * 4);
    uint64_t hash = 0;

    *ok = 0;
    if (pixels == NULL || texels == NULL || luxels == NULL) {
        check(0, "allocation des tampons de la scène éclairée");
        free(pixels);
        free(texels);
        free(luxels);
        return 0;
    }
    make_checker(texels);
    make_gradient(luxels);

    ScgTextureDesc desc;
    memset(&desc, 0, sizeof desc);
    desc.width = FLOOR_SIDE;
    desc.height = FLOOR_SIDE;
    desc.format = SCG_TEXTURE_FORMAT_RGBA8;
    int loaded = scg_texture_load(&desc, texels, (size_t)FLOOR_SIDE * FLOOR_SIDE * 4, &texture);
    check(loaded == SCG_OK, "la texture du sol se charge");

    desc.width = LIGHT_SIDE;
    desc.height = LIGHT_SIDE;
    int lit = scg_texture_load(&desc, luxels, (size_t)LIGHT_SIDE * LIGHT_SIDE * 4, &lightmap);
    check(lit == SCG_OK, "la lightmap se charge par le meme chemin");

    check(scg_create(&config, &ctx) == SCG_OK, "création du contexte éclairé");

    if (loaded == SCG_OK && lit == SCG_OK && ctx != NULL) {
        /* Une lightmap nulle est refusée, elle : sans elle, ce lot n'a rien à
         * faire sur ce chemin. */
        int32_t refused = scg_submit_lit(ctx, &IDENTITY, LIT_FLOOR, 4,
                                         FLOOR_TRIANGLES, 2, texture, NULL);
        check(refused == SCG_ERR_NULL, "une lightmap nulle est refusée");

        int32_t code = scg_submit_lit(ctx, &IDENTITY, LIT_FLOOR, 4,
                                      FLOOR_TRIANGLES, 2, texture, lightmap);
        check(code == SCG_OK, "le sol texturé et éclairé est accepté");

        code = scg_submit_lit(ctx, &IDENTITY, LIT_WALL, 4,
                              WALL_TRIANGLES, 2, NULL, lightmap);
        check(code == SCG_OK, "le mur uni et éclairé est accepté");

        /* Détruites avant le rendu, comme la texture de la scène précédente :
         * le moteur garde ses propres références jusqu'à la fin de l'image. */
        scg_texture_destroy(texture);
        scg_texture_destroy(lightmap);
        texture = NULL;
        lightmap = NULL;

        code = scg_frame_end(ctx, pixels, STRIDE);
        check(code == SCG_OK, "l'image éclairée se rend");
        *ok = code == SCG_OK;
        hash = fingerprint(pixels, WIDTH, HEIGHT, STRIDE);
    }

    scg_texture_destroy(texture);
    scg_texture_destroy(lightmap);
    scg_destroy(ctx);
    free(pixels);
    free(texels);
    free(luxels);
    return hash;
}

/* Toutes les vérifications, puis les empreintes sur la sortie standard, une
 * par ligne et dans l'ordre que le Makefile attend. */
int main(void)
{
    check(scg_abi_version() == SCG_ABI_VERSION, "la bibliothèque liée est celle du header");

    check_refusals();
    check_buffers();

    int ok = 0;
    uint64_t hash = render(&ok);

#if HAS_FP_CONTROL
    if (ok) {
        check_float_environment(hash);
    }
#endif
    if (ok) {
        check_tiles(hash);
    }

    int textured_ok = 0;
    uint64_t textured = render_textured(&textured_ok, SCG_FILTER_DITHER);

    int bilinear_ok = 0;
    uint64_t bilinear = render_textured(&bilinear_ok, SCG_FILTER_BILINEAR);

    int lit_ok = 0;
    uint64_t lit = render_lit(&lit_ok);

    int fog_ok = 0;
    uint64_t fog = render_fog(&fog_ok);

    if (failures > 0 || !ok || !textured_ok || !bilinear_ok || !lit_ok || !fog_ok) {
        fprintf(stderr, "%d vérification(s) en échec\n", failures);
        return 1;
    }
    printf("%016llx\n", (unsigned long long)hash);
    printf("%016llx\n", (unsigned long long)textured);
    printf("%016llx\n", (unsigned long long)bilinear);
    printf("%016llx\n", (unsigned long long)lit);
    printf("%016llx\n", (unsigned long long)fog);
    return 0;
}
