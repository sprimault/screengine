/*
 * Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
 * SPDX-License-Identifier: MIT OR Apache-2.0
 *
 * Hôte C de démonstration : une fenêtre, un clavier, une boucle.
 *
 * **Ce que `main.c` ne montre pas.** L'hôte de conformance rend trois scènes
 * sans fenêtre et compare un nombre : c'est ce qu'il faut pour éprouver la
 * frontière, et c'est inutilisable comme point de départ. Celui-ci est le
 * point de départ — il charge un décor, l'affiche et s'y déplace, et il tient
 * en un fichier qu'on lit d'un bout à l'autre.
 *
 * Il suit la page web pas à pas, parce qu'elle n'a aucune bibliothèque de
 * fenêtrage à démêler de ce qu'elle montre du moteur : charger, soumettre,
 * rendre par tuiles, recopier. Ce que SDL apporte ici — une fenêtre, une
 * texture, des événements — n'appartient pas au moteur, qui n'a ni l'un ni
 * l'autre.
 */

#include "screengine.h"

#include <SDL3/SDL.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Résolution interne, celle à laquelle le moteur rend. La fenêtre est plus
 * grande : c'est l'hôte qui met à l'échelle, et sur un téléphone le rendu ne se
 * fait jamais à la résolution de l'écran. */
enum { WIDTH = 640, HEIGHT = 360, TILE = 64 };

/* Facteur d'agrandissement de la fenêtre. */
enum { SCALE = 2 };

/* Côtés des damiers, en texels.
 *
 * Ils suivent la densité de plaquage de la carte — 256 texels par unité de
 * monde aux murs, 128 au sol : une case y fait alors un demi-mètre et un quart.
 * Un damier plus petit donnerait des cases de quelques centimètres, que le
 * mipmap ramènerait à un aplat dès le deuxième panneau. Les caisses gardent le
 * leur, leur maillage ayant son propre plaquage. */
enum { WALL_SIDE = 512, FLOOR_SIDE = 256, CRATE_SIDE = 64 };

/* Vitesse de déplacement, en unités de monde par seconde. */
static const float SPEED = 6.0f;

/* Vitesse de rotation, en radians par seconde. */
static const float TURN = 2.0f;

/* Écrit l'erreur du moteur, sans contexte quand il n'y en a pas encore. */
static void fail(const ScgContext *ctx, const char *what)
{
    fprintf(stderr, "%s : %s\n", what, scg_last_error(ctx));
}

/* Lit un fichier entier dans un tampon alloué, ou rend NULL.
 *
 * L'hôte lit les fichiers, jamais le moteur : c'est pour cela que le
 * chargement prend un bloc d'octets et non un chemin. */
static uint8_t *read_file(const char *path, size_t *len)
{
    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        return NULL;
    }
    if (fseek(file, 0, SEEK_END) != 0) {
        fclose(file);
        return NULL;
    }
    long size = ftell(file);
    if (size < 0 || fseek(file, 0, SEEK_SET) != 0) {
        fclose(file);
        return NULL;
    }
    uint8_t *bytes = malloc((size_t)size);
    if (bytes == NULL) {
        fclose(file);
        return NULL;
    }
    size_t read = fread(bytes, 1, (size_t)size, file);
    fclose(file);
    if (read != (size_t)size) {
        free(bytes);
        return NULL;
    }
    *len = read;
    return bytes;
}

/* Écrit un damier de `cell` texels de case, dans un bloc de TEXTURE_SIDE au
 * carré texels.
 *
 * Le même motif que la suite de conformance, teinte pour teinte : la
 * démonstration montre le décor de la scène de référence. */
static void make_checker(uint8_t *pixels, uint32_t side, uint32_t cell)
{
    for (uint32_t v = 0; v < side; v++) {
        for (uint32_t u = 0; u < side; u++) {
            uint8_t *texel = pixels + ((size_t)v * side + u) * 4;
            int edge = (u % cell) == 0 || (v % cell) == 0;
            int dark = ((u / cell) + (v / cell)) % 2 == 0;
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

/* Charge une texture de damier, ou rend NULL. */
static ScgTexture *load_checker(uint32_t side, uint32_t cell)
{
    size_t bytes = (size_t)side * side * 4;
    uint8_t *texels = malloc(bytes);
    if (texels == NULL) {
        return NULL;
    }
    make_checker(texels, side, cell);

    ScgTextureDesc desc;
    memset(&desc, 0, sizeof desc);
    desc.width = side;
    desc.height = side;
    desc.format = SCG_TEXTURE_FORMAT_RGBA8;

    ScgTexture *texture = NULL;
    if (scg_texture_load(&desc, texels, bytes, &texture) < 0) {
        texture = NULL;
    }
    free(texels);
    return texture;
}

/* Le quaternion d'une rotation autour de l'axe vertical, rangé `x, y, z, w`.
 *
 * Écrit ici et non demandé au moteur : ses tables trigonométriques ne
 * traversent pas l'ABI, et où regarde la caméra est une décision de l'hôte. */
static void yaw(float angle, float *out)
{
    out[0] = 0.0f;
    out[1] = 0.0f;
    out[2] = SDL_sinf(angle / 2.0f);
    out[3] = SDL_cosf(angle / 2.0f);
}

/* Où sont posées les caisses : abscisse, ordonnée, et l'angle qui les tourne.
 *
 * Le décor et les accessoires sont deux ressources différentes, chargées
 * séparément et soumises séparément : la carte porte les murs, le maillage ce
 * qu'on y pose. C'est ce que l'étape des données a construit, et une
 * démonstration qui ne montrerait qu'un couloir vide n'en dirait que la
 * moitié. */
static const float CRATES[][3] = {
    { 6.0f, -1.2f, 0.4f },
    { 11.0f, 1.4f, -0.7f },
    { 17.0f, -0.6f, 1.1f },
};

/* L'échelle des caisses.
 *
 * **Le maillage fait deux unités de côté**, et le couloir six de large : posée
 * telle quelle, une caisse en occupe le tiers. Le fichier ne se redimensionne
 * pas — c'est celui de la scène de conformance, et son empreinte est figée —,
 * donc l'échelle va dans la matrice de modèle, qui est faite pour ça. */
static const float CRATE_SCALE = 0.5f;

/* La cote du centre d'une caisse : sa demi-hauteur au-dessus du sol du
 * couloir, qui est à -1,5. */
static const float CRATE_Z = -1.0f;

/* Écrit la matrice d'une caisse : une rotation autour de la verticale mise à
 * l'échelle, puis une translation. Par colonnes, comme l'ABI l'attend.
 *
 * Les coefficients viennent de la bibliothèque mathématique de l'hôte, et c'est
 * permis ici : une démonstration n'est comparée à aucune empreinte, là où une
 * scène de conformance exige les mêmes bits sur les quatre cibles. */
static void crate_model(const float *placement, ScgMat4 *out)
{
    float c = SDL_cosf(placement[2]) * CRATE_SCALE;
    float s = SDL_sinf(placement[2]) * CRATE_SCALE;
    memset(out, 0, sizeof *out);
    out->m[0] = c;
    out->m[1] = s;
    out->m[4] = -s;
    out->m[5] = c;
    out->m[10] = CRATE_SCALE;
    out->m[12] = placement[0];
    out->m[13] = placement[1];
    out->m[14] = CRATE_Z;
    out->m[15] = 1.0f;
}

int main(int argc, char **argv)
{
    if (argc != 3) {
        fprintf(stderr, "usage : %s <fichier de carte> <fichier de maillage>\n", argv[0]);
        return 2;
    }

    size_t len = 0;
    uint8_t *bytes = read_file(argv[1], &len);
    if (bytes == NULL) {
        fprintf(stderr, "%s : lecture impossible\n", argv[1]);
        return 1;
    }

    ScgWorld *world = NULL;
    int32_t code = scg_world_load(bytes, len, &world);
    /* Le bloc est libéré tout de suite : le moteur copie ce qu'il garde. */
    free(bytes);
    if (code < 0) {
        fail(NULL, "la carte est refusée");
        return 1;
    }

    /* Une texture par matériau, dans l'ordre que la carte déclare. L'hôte lit
     * les noms, décide de ce qu'il charge, et passe les handles dans cet
     * ordre — le moteur ne connaît que des emplacements à remplir. */
    uint32_t materials = 0;
    scg_world_material_count(world, &materials);
    const ScgTexture **slots = calloc(materials ? materials : 1, sizeof *slots);
    if (slots == NULL) {
        return 1;
    }
    for (uint32_t i = 0; i < materials; i++) {
        char name[64];
        size_t needed = 0;
        if (scg_world_material_name(world, i, NULL, 0, &needed) < 0
            || needed + 1 > sizeof name
            || scg_world_material_name(world, i, name, sizeof name, &needed) < 0) {
            fail(NULL, "nom de matériau illisible");
            return 1;
        }
        int wall = strcmp(name, "mur") == 0;
        slots[i] = load_checker(wall ? WALL_SIDE : FLOOR_SIDE, wall ? 128 : 32);
        if (slots[i] == NULL) {
            fail(NULL, "texture refusée");
            return 1;
        }
    }

    /* Le maillage des caisses, chargé comme la carte : un bloc d'octets, que le
     * moteur copie. Ses deux emplacements portent des noms, et l'hôte décide de
     * ce qu'il met dedans — ici le même damier sur les deux. */
    bytes = read_file(argv[2], &len);
    if (bytes == NULL) {
        fprintf(stderr, "%s : lecture impossible\n", argv[2]);
        return 1;
    }
    ScgMesh *crate = NULL;
    code = scg_mesh_load(bytes, len, &crate);
    free(bytes);
    if (code < 0) {
        fail(NULL, "le maillage est refusé");
        return 1;
    }
    const ScgTexture *side = load_checker(CRATE_SIDE, 8);
    const ScgTexture *crate_slots[2] = { side, side };
    if (crate_slots[0] == NULL) {
        fail(NULL, "texture de caisse refusée");
        return 1;
    }

    ScgContextConfig config;
    memset(&config, 0, sizeof config);
    config.max_width = WIDTH;
    config.max_height = HEIGHT;
    config.width = WIDTH;
    config.height = HEIGHT;
    config.tile_size = TILE;

    ScgContext *ctx = NULL;
    if (scg_create(&config, &ctx) < 0) {
        fail(NULL, "création du contexte");
        return 1;
    }

    if (!SDL_Init(SDL_INIT_VIDEO)) {
        fprintf(stderr, "SDL_Init : %s\n", SDL_GetError());
        return 1;
    }
    SDL_Window *window = SDL_CreateWindow("Screengine", WIDTH * SCALE, HEIGHT * SCALE, 0);
    SDL_Renderer *renderer = window ? SDL_CreateRenderer(window, NULL) : NULL;
    /* RGBA32 est l'ordre mémoire que le moteur écrit : rouge, vert, bleu,
     * alpha, quelle que soit la boutianité de la machine. */
    SDL_Texture *screen = renderer
        ? SDL_CreateTexture(renderer, SDL_PIXELFORMAT_RGBA32, SDL_TEXTUREACCESS_STREAMING,
                            WIDTH, HEIGHT)
        : NULL;
    if (screen == NULL) {
        fprintf(stderr, "SDL : %s\n", SDL_GetError());
        return 1;
    }
    /* Au plus proche : agrandir une image de rendu logiciel en la lissant
     * effacerait précisément ce qu'elle a de net. */
    SDL_SetTextureScaleMode(screen, SDL_SCALEMODE_NEAREST);

    uint8_t *pixels = malloc((size_t)WIDTH * HEIGHT * 4);
    if (pixels == NULL) {
        return 1;
    }

    float position[3] = {0.0f, 0.0f, 0.0f};
    float angle = 0.0f;
    Uint64 previous = SDL_GetTicks();
    Uint64 since = previous;
    unsigned frames = 0;
    int running = 1;

    while (running) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            if (event.type == SDL_EVENT_QUIT) {
                running = 0;
            }
            if (event.type == SDL_EVENT_KEY_DOWN && event.key.scancode == SDL_SCANCODE_ESCAPE) {
                running = 0;
            }
        }

        Uint64 now = SDL_GetTicks();
        float dt = (float)(now - previous) / 1000.0f;
        previous = now;
        if (dt > 0.1f) {
            dt = 0.1f;
        }

        const bool *keys = SDL_GetKeyboardState(NULL);
        if (keys[SDL_SCANCODE_LEFT] || keys[SDL_SCANCODE_A]) {
            angle += TURN * dt;
        }
        if (keys[SDL_SCANCODE_RIGHT] || keys[SDL_SCANCODE_D]) {
            angle -= TURN * dt;
        }
        float forward = (float)(keys[SDL_SCANCODE_UP] || keys[SDL_SCANCODE_W])
            - (float)(keys[SDL_SCANCODE_DOWN] || keys[SDL_SCANCODE_S]);
        position[0] += SDL_cosf(angle) * forward * SPEED * dt;
        position[1] += SDL_sinf(angle) * forward * SPEED * dt;

        ScgCamera camera;
        memset(&camera, 0, sizeof camera);
        memcpy(camera.position, position, sizeof camera.position);
        yaw(angle, camera.orientation);
        camera.fov_y = 1.2f;
        camera.near_plane = 0.1f;
        if (scg_set_camera(ctx, &camera) < 0) {
            fail(ctx, "caméra refusée");
            break;
        }

        ScgMat4 model;
        memset(&model, 0, sizeof model);
        model.m[0] = model.m[5] = model.m[10] = model.m[15] = 1.0f;
        if (scg_submit_world(ctx, &model, world, slots, materials) < 0) {
            fail(ctx, "carte refusée");
            break;
        }

        /* Les caisses par-dessus, chacune avec sa matrice : la même ressource
         * dessinée trois fois, ce qu'un décor fait de ses accessoires. */
        for (size_t i = 0; i < sizeof CRATES / sizeof CRATES[0]; i++) {
            ScgMat4 placement;
            crate_model(CRATES[i], &placement);
            if (scg_submit_mesh(ctx, &placement, crate, crate_slots, 2) < 0) {
                fail(ctx, "maillage refusé");
                running = 0;
                break;
            }
        }
        if (!running) {
            break;
        }

        /* Par tuiles : un hôte qui voudrait les répartir sur ses threads le
         * ferait ici, et rien d'autre ne changerait. */
        uint32_t tiles = 0;
        if (scg_frame_begin(ctx, &tiles) < 0) {
            fail(ctx, "début d'image");
            break;
        }
        for (uint32_t i = 0; i < tiles; i++) {
            if (scg_frame_tile(ctx, i, pixels, WIDTH) < 0) {
                fail(ctx, "tuile");
                running = 0;
                break;
            }
        }
        if (scg_frame_end(ctx, pixels, WIDTH) < 0) {
            fail(ctx, "fin d'image");
            break;
        }

        SDL_UpdateTexture(screen, NULL, pixels, WIDTH * 4);
        SDL_RenderClear(renderer);
        SDL_RenderTexture(renderer, screen, NULL, NULL);
        SDL_RenderPresent(renderer);

        /* La cadence dans le titre : une démonstration de rendu logiciel se
         * juge autant à sa fluidité qu'à son image, et il n'y a pas d'endroit
         * pour l'écrire dans la fenêtre. */
        frames++;
        if (now - since >= 1000) {
            char title[64];
            snprintf(title, sizeof title, "Screengine — %u images/s",
                     (unsigned)(frames * 1000 / (now - since)));
            SDL_SetWindowTitle(window, title);
            frames = 0;
            since = now;
        }
    }

    free(pixels);
    for (uint32_t i = 0; i < materials; i++) {
        scg_texture_destroy((ScgTexture *)slots[i]);
    }
    free(slots);
    scg_texture_destroy((ScgTexture *)crate_slots[0]);
    scg_mesh_destroy(crate);
    scg_world_destroy(world);
    scg_destroy(ctx);
    SDL_DestroyTexture(screen);
    SDL_DestroyRenderer(renderer);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}
