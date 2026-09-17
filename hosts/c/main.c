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
 * La scène est celle de `screengine-conformance --print triangle` : 640×360,
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
#define HAS_MXCSR 1
#else
#define HAS_MXCSR 0
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

#if HAS_MXCSR
/* Un hôte hostile : exceptions démasquées, arrondi vers le haut, DAZ et FZ. Le
 * moteur doit rendre la même image, et rendre le registre intact.
 *
 * Aujourd'hui, seuls les masques et la restauration du registre ont un effet
 * observable : le triangle en dur ne passe par aucun calcul que l'arrondi ou
 * DAZ changeraient. La comparaison d'empreinte prendra son sens avec la
 * transformation des sommets, et elle est en place pour ce jour-là. */
static void check_float_environment(uint64_t expected)
{
    unsigned int host = _mm_getcsr();
    unsigned int hostile = (host & ~0x7F80u) | 0x4000u | 0x8040u;

    _mm_setcsr(hostile);
    int ok = 0;
    uint64_t hash = render(&ok);
    unsigned int after = _mm_getcsr();
    _mm_setcsr(host);

    check(ok, "le rendu aboutit sous un environnement flottant hostile");
    check(hash == expected, "l'environnement flottant de l'hôte ne change pas l'image");
    check(after == hostile, "le registre flottant de l'hôte est rendu à l'identique");
}
#endif

/* Toutes les vérifications, puis l'empreinte sur la sortie standard. */
int main(void)
{
    check(scg_abi_version() == SCG_ABI_VERSION, "la bibliothèque liée est celle du header");

    check_refusals();
    check_buffers();

    int ok = 0;
    uint64_t hash = render(&ok);

#if HAS_MXCSR
    if (ok) {
        check_float_environment(hash);
    }
#endif

    if (failures > 0 || !ok) {
        fprintf(stderr, "%d vérification(s) en échec\n", failures);
        return 1;
    }
    printf("%016llx\n", (unsigned long long)hash);
    return 0;
}
