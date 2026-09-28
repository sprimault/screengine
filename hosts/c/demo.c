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

/* Côté de la planche de la créature et de sa tache, en texels. */
enum { SPRITE_SIDE = 64 };

/* La créature : un disque et son pied, sur fond transparent.
 *
 * Écrite plutôt que chargée, comme les damiers : cette démonstration n'a aucun
 * décodeur d'image, et ce qu'elle montre du moteur est la primitive, pas
 * l'illustration. */
static void make_creature(uint8_t *pixels)
{
    const float cx = (float)SPRITE_SIDE / 2.0f;
    const float cy = (float)SPRITE_SIDE * 0.35f;
    const float radius = (float)SPRITE_SIDE * 0.28f;
    for (uint32_t v = 0; v < SPRITE_SIDE; v++) {
        for (uint32_t u = 0; u < SPRITE_SIDE; u++) {
            uint8_t *texel = pixels + ((size_t)v * SPRITE_SIDE + u) * 4;
            float fu = (float)u + 0.5f;
            float fv = (float)v + 0.5f;
            float dx = fu - cx;
            float dy = fv - cy;
            int disc = dx * dx + dy * dy <= radius * radius;
            int foot = fv > (float)SPRITE_SIDE * 0.6f && fu > (float)SPRITE_SIDE * 0.28f
                       && fu < (float)SPRITE_SIDE * 0.52f;
            if (disc || foot) {
                texel[0] = (uint8_t)(0x40 + (int)(fu * 160.0f / (float)SPRITE_SIDE));
                texel[1] = (uint8_t)(0xFF - (int)(fv * 140.0f / (float)SPRITE_SIDE));
                texel[2] = 0x60;
                texel[3] = 0xFF;
            } else {
                texel[0] = 0;
                texel[1] = 0;
                texel[2] = 0;
                texel[3] = 0;
            }
        }
    }
}

/* La tache d'ombre : sombre au centre, blanche au bord, 255 étant le neutre de
 * la modulation — un texel blanc laisse le sol intact. */
static void make_blot(uint8_t *pixels)
{
    const float half = (float)SPRITE_SIDE / 2.0f;
    for (uint32_t v = 0; v < SPRITE_SIDE; v++) {
        for (uint32_t u = 0; u < SPRITE_SIDE; u++) {
            uint8_t *texel = pixels + ((size_t)v * SPRITE_SIDE + u) * 4;
            float dx = (float)u + 0.5f - half;
            float dy = (float)v + 0.5f - half;
            float q = (dx * dx + dy * dy) / (half * half);
            if (q > 1.0f) {
                q = 1.0f;
            }
            uint8_t level = (uint8_t)(0x38 + (int)((float)(0xFF - 0x38) * q));
            texel[0] = level;
            texel[1] = level;
            texel[2] = level;
            texel[3] = 0xFF;
        }
    }
}

/* Charge une planche engendrée par `fill`, dans le format donné. */
static ScgTexture *load_sprite(void (*fill)(uint8_t *), uint32_t format)
{
    size_t bytes = (size_t)SPRITE_SIDE * SPRITE_SIDE * 4;
    uint8_t *texels = malloc(bytes);
    if (texels == NULL) {
        return NULL;
    }
    fill(texels);

    ScgTextureDesc desc;
    memset(&desc, 0, sizeof desc);
    desc.width = SPRITE_SIDE;
    desc.height = SPRITE_SIDE;
    desc.format = format;

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
    { 3.0f, 2.0f, 0.4f },
    { 10.0f, 2.0f, -0.7f },
    { 16.0f, 4.0f, 1.1f },
};

/* L'échelle des caisses.
 *
 * **Le maillage fait deux unités de côté**, et le couloir six de large : posée
 * telle quelle, une caisse en occupe le tiers. Le fichier ne se redimensionne
 * pas — c'est celui de la scène de conformance, et son empreinte est figée —,
 * donc l'échelle va dans la matrice de modèle, qui est faite pour ça. */
static const float CRATE_SCALE = 0.5f;

/* La cote du centre d'une caisse : sa demi-hauteur au-dessus du sol, qui est en
 * zéro dans ce décor. */
static const float CRATE_Z = 0.5f;

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

/* Ouvre la fenêtre et rend jusqu'à ce qu'on la ferme.
 *
 * Les deux chemins de fichier sont des paramètres et non des constantes : le
 * moteur n'ouvre rien, et c'est l'hôte qui sait où vivent les données. */
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

    /* Les lightmaps, cuites une fois pour toutes les cellules avant la première
     * image. Une lightmap est un cache de la carte et non de la vue : la cuire en
     * chemin ferait allouer un atlas au milieu d'une image, ce que le moteur
     * promet de ne jamais faire. */
    ScgLighting *lighting = NULL;
    if (scg_lighting_create(world, &lighting) < 0) {
        fail(NULL, "porteur de lightmaps refusé");
        return 1;
    }
    uint32_t cells = 0;
    scg_world_cell_count(world, &cells);
    for (uint32_t i = 0; i < cells; i++) {
        uint32_t id = 0;
        if (scg_world_cell_id(world, i, &id) < 0 || scg_lighting_build(lighting, id) < 0) {
            fail(NULL, "cuisson refusée");
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

    /* Le sol de ce décor est en zéro : une caméra laissée à l'origine serait dans
     * le plancher, hors de toute cellule, et la traversée ne rendrait rien. */
    float position[3] = {2.0f, 2.0f, 2.0f};
    float angle = 0.0f;
    uint32_t cell = 0;
    if (scg_world_locate(world, position, &cell) < 0 || cell == 0) {
        fail(NULL, "la caméra ne part d'aucune cellule");
        return 1;
    }
    /* La créature et sa tache : le format masqué se déclare au chargement,
     * jamais au dessin, parce que c'est là que la chaîne de mipmaps se
     * construit. */
    ScgTexture *creature = load_sprite(make_creature, SCG_TEXTURE_FORMAT_RGBA8_MASKED);
    ScgTexture *blot = load_sprite(make_blot, SCG_TEXTURE_FORMAT_RGBA8);
    if (creature == NULL || blot == NULL) {
        fail(NULL, "la créature ou sa tache ne se chargent pas");
        return 1;
    }
    /* Son va-et-vient le long du couloir, en abscisses de monde. */
    float walker = 8.0f;
    float heading = 1.0f;

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
        float previous_position[3] = { position[0], position[1], position[2] };
        position[0] += SDL_cosf(angle) * forward * SPEED * dt;
        position[1] += SDL_sinf(angle) * forward * SPEED * dt;

        /* La cellule se suit par le déplacement, et c'est l'hôte qui la garde : le
         * moteur ne retient aucune caméra. Zéro veut dire « sorti du décor » et ne
         * s'écrit pas — garder la dernière cellule connue laisse voir le décor
         * depuis dehors, là où l'écraser éteindrait l'image. */
        uint32_t found = 0;
        if (scg_world_track(world, cell, previous_position, position, &found) >= 0
            && found != 0) {
            cell = found;
        }

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
        if (scg_submit_world_visible(ctx, &model, world, slots, materials, lighting, cell) < 0) {
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

        /* La créature arpente le couloir, sa tache la suit.
         *
         * **La tache d'abord, en surface modulée** : elle multiplie le sol au
         * lieu de l'écraser, et teste la profondeur sans l'écrire — c'est
         * l'ordre de soumission qui la fait gagner sur les dalles, sans biais
         * de profondeur. Un centimètre au-dessus du sol suffit ici, où on la
         * voit de près ; vue en rasant il en faudrait davantage. */
        /* Elle arpente la salle en L puis le couloir, dans la bande que les
         * deux partagent : **le décor n'est pas centré sur l'origine**, et
         * `y = 0` y est une paroi. */
        walker += heading * 1.1f * dt;
        if (walker > 14.0f) {
            walker = 14.0f;
            heading = -1.0f;
        } else if (walker < 6.0f) {
            walker = 6.0f;
            heading = 1.0f;
        }

        const float blot_radius = 0.55f;
        const float lane = 2.0f;
        /* Le sol de ce décor est à zéro : un centimètre au-dessus suffit. */
        const float floor_z = 0.01f;
        ScgVertexUv patch[4] = {
            { walker - blot_radius, lane - blot_radius, floor_z,  0.0f,  0.0f },
            { walker + blot_radius, lane - blot_radius, floor_z, 64.0f,  0.0f },
            { walker + blot_radius, lane + blot_radius, floor_z, 64.0f, 64.0f },
            { walker - blot_radius, lane + blot_radius, floor_z,  0.0f, 64.0f },
        };
        static const ScgTriangle PATCH_FACES[2] = {
            { 0, 1, 2, 0xFF, 0xFF, 0xFF, 0xFF },
            { 0, 2, 3, 0xFF, 0xFF, 0xFF, 0xFF },
        };
        if (scg_submit_blended(ctx, &model, patch, 4, PATCH_FACES, 2, blot, SCG_BLEND_MODULATE)
            < 0) {
            fail(ctx, "tache refusée");
            break;
        }

        /* **En mode axial**, le seul juste pour un personnage debout : plein
         * face, il se coucherait au sol dès qu'on le regarde d'en haut. */
        ScgSprite walker_quad;
        memset(&walker_quad, 0, sizeof walker_quad);
        walker_quad.x = walker;
        walker_quad.y = lane;
        walker_quad.half_width = 0.6f;
        walker_quad.half_height = 0.9f;
        /* Son centre à une demi-hauteur du sol : elle est posée dessus, pas
         * enterrée ni flottante. */
        walker_quad.z = walker_quad.half_height;
        walker_quad.u1 = (float)SPRITE_SIDE;
        walker_quad.v1 = (float)SPRITE_SIDE;
        walker_quad.r = 0xFF;
        walker_quad.g = 0xFF;
        walker_quad.b = 0xFF;
        walker_quad.a = 0xFF;
        if (scg_submit_sprites(ctx, &model, &walker_quad, 1, creature, SCG_SPRITE_AXIAL) < 0) {
            fail(ctx, "créature refusée");
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
    scg_texture_destroy(creature);
    scg_texture_destroy(blot);
    scg_mesh_destroy(crate);
    /* Le porteur avant la carte : il en garde une référence, et l'ordre inverse
     * marcherait aussi — le contrat le dit —, mais celui-ci se relit mieux. */
    scg_lighting_destroy(lighting);
    scg_world_destroy(world);
    scg_destroy(ctx);
    SDL_DestroyTexture(screen);
    SDL_DestroyRenderer(renderer);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}
