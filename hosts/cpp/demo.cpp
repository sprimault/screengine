// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0
//
/// @file Hôte C++ de démonstration : une fenêtre, un clavier, une boucle.
///
/// **Le même programme que `hosts/c/demo.c`, dans l'autre langage.** Les deux
/// existent parce que la frontière est une ABI C : ce qui se lit d'un côté doit
/// se relire de l'autre, et un intégrateur C++ part d'ici sans traduire du C.
///
/// La différence tient à ce que le langage apporte — des destructeurs qui
/// rendent les handles, des conteneurs qui portent leur longueur — et à rien
/// d'autre : les appels sont les mêmes, dans le même ordre.

#include "screengine.h"

#include <SDL3/SDL.h>

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iterator>
#include <memory>
#include <string>
#include <vector>

namespace {

/// Résolution interne, celle à laquelle le moteur rend.
constexpr int WIDTH = 640;
/// Hauteur interne.
constexpr int HEIGHT = 360;
/// Côté de tuile.
constexpr uint32_t TILE = 64;
/// Facteur d'agrandissement de la fenêtre.
constexpr int SCALE = 2;
/// Côté du damier des murs, en texels.
///
/// Les côtés suivent la densité de plaquage de la carte — 256 texels par unité
/// aux murs, 128 au sol : une case y fait un demi-mètre et un quart. Un damier
/// plus petit donnerait des cases de quelques centimètres, que le mipmap
/// ramènerait à un aplat.
constexpr uint32_t WALL_SIDE = 512;
/// Celui du sol et du plafond.
constexpr uint32_t FLOOR_SIDE = 256;
/// Celui des caisses, dont le maillage a son propre plaquage.
constexpr uint32_t CRATE_SIDE = 64;
/// Vitesse de déplacement, en unités de monde par seconde.
constexpr float SPEED = 6.0f;
/// Vitesse de rotation, en radians par seconde.
constexpr float TURN = 2.0f;

/// Écrit l'erreur du moteur, sans contexte quand il n'y en a pas encore.
void fail(const ScgContext *ctx, const char *what)
{
    std::fprintf(stderr, "%s : %s\n", what, scg_last_error(ctx));
}

/// Lit un fichier entier, ou rend un vecteur vide.
///
/// L'hôte lit les fichiers, jamais le moteur : c'est pour cela que le
/// chargement prend un bloc d'octets et non un chemin.
std::vector<uint8_t> read_file(const char *path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return {};
    }
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(file),
                                std::istreambuf_iterator<char>());
}

/// Un damier de `cell` texels de case, le même que la suite de conformance.
std::vector<uint8_t> make_checker(uint32_t side, uint32_t cell)
{
    std::vector<uint8_t> texels(static_cast<size_t>(side) * side * 4);
    for (uint32_t v = 0; v < side; v++) {
        for (uint32_t u = 0; u < side; u++) {
            uint8_t *texel = texels.data() + (static_cast<size_t>(v) * side + u) * 4;
            const bool edge = u % cell == 0 || v % cell == 0;
            const bool dark = ((u / cell) + (v / cell)) % 2 == 0;
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
    return texels;
}

/// Charge une texture de damier, ou rend nul.
ScgTexture *load_checker(uint32_t side, uint32_t cell)
{
    const std::vector<uint8_t> texels = make_checker(side, cell);
    ScgTextureDesc desc{};
    desc.width = side;
    desc.height = side;
    desc.format = SCG_TEXTURE_FORMAT_RGBA8;

    ScgTexture *texture = nullptr;
    if (scg_texture_load(&desc, texels.data(), texels.size(), &texture) < 0) {
        return nullptr;
    }
    return texture;
}

/// Côté de la planche de la créature et de sa tache, en texels.
constexpr uint32_t SPRITE_SIDE = 64;

/// La créature : un disque et son pied, sur fond transparent.
///
/// Écrite plutôt que chargée, comme les damiers : cette démonstration n'a aucun
/// décodeur d'image, et ce qu'elle montre du moteur est la primitive, pas
/// l'illustration.
std::vector<uint8_t> make_creature()
{
    std::vector<uint8_t> texels(static_cast<size_t>(SPRITE_SIDE) * SPRITE_SIDE * 4, 0);
    const float cx = static_cast<float>(SPRITE_SIDE) / 2.0f;
    const float cy = static_cast<float>(SPRITE_SIDE) * 0.35f;
    const float radius = static_cast<float>(SPRITE_SIDE) * 0.28f;
    for (uint32_t v = 0; v < SPRITE_SIDE; v++) {
        for (uint32_t u = 0; u < SPRITE_SIDE; u++) {
            uint8_t *texel = texels.data() + (static_cast<size_t>(v) * SPRITE_SIDE + u) * 4;
            const float fu = static_cast<float>(u) + 0.5f;
            const float fv = static_cast<float>(v) + 0.5f;
            const float dx = fu - cx;
            const float dy = fv - cy;
            const bool disc = dx * dx + dy * dy <= radius * radius;
            const bool foot = fv > static_cast<float>(SPRITE_SIDE) * 0.6f
                              && fu > static_cast<float>(SPRITE_SIDE) * 0.28f
                              && fu < static_cast<float>(SPRITE_SIDE) * 0.52f;
            if (disc || foot) {
                texel[0] = static_cast<uint8_t>(
                    0x40 + static_cast<int>(fu * 160.0f / static_cast<float>(SPRITE_SIDE)));
                texel[1] = static_cast<uint8_t>(
                    0xFF - static_cast<int>(fv * 140.0f / static_cast<float>(SPRITE_SIDE)));
                texel[2] = 0x60;
                texel[3] = 0xFF;
            }
        }
    }
    return texels;
}

/// La tache d'ombre : sombre au centre, blanche au bord, 255 étant le neutre de
/// la modulation — un texel blanc laisse le sol intact.
std::vector<uint8_t> make_blot()
{
    std::vector<uint8_t> texels(static_cast<size_t>(SPRITE_SIDE) * SPRITE_SIDE * 4, 0);
    const float half = static_cast<float>(SPRITE_SIDE) / 2.0f;
    for (uint32_t v = 0; v < SPRITE_SIDE; v++) {
        for (uint32_t u = 0; u < SPRITE_SIDE; u++) {
            uint8_t *texel = texels.data() + (static_cast<size_t>(v) * SPRITE_SIDE + u) * 4;
            const float dx = static_cast<float>(u) + 0.5f - half;
            const float dy = static_cast<float>(v) + 0.5f - half;
            float q = (dx * dx + dy * dy) / (half * half);
            if (q > 1.0f) {
                q = 1.0f;
            }
            const auto level =
                static_cast<uint8_t>(0x38 + static_cast<int>(static_cast<float>(0xFF - 0x38) * q));
            texel[0] = level;
            texel[1] = level;
            texel[2] = level;
            texel[3] = 0xFF;
        }
    }
    return texels;
}

/// Charge une planche engendrée, dans le format donné.
ScgTexture *load_sprite(const std::vector<uint8_t> &texels, uint32_t format)
{
    ScgTextureDesc desc{};
    desc.width = SPRITE_SIDE;
    desc.height = SPRITE_SIDE;
    desc.format = format;

    ScgTexture *texture = nullptr;
    if (scg_texture_load(&desc, texels.data(), texels.size(), &texture) < 0) {
        return nullptr;
    }
    return texture;
}

/// Le quaternion d'une rotation autour de l'axe vertical, rangé `x, y, z, w`.
///
/// Écrit ici et non demandé au moteur : ses tables trigonométriques ne
/// traversent pas l'ABI, et où regarde la caméra est une décision de l'hôte.
void yaw(float angle, float *out)
{
    out[0] = 0.0f;
    out[1] = 0.0f;
    out[2] = SDL_sinf(angle / 2.0f);
    out[3] = SDL_cosf(angle / 2.0f);
}

/// Où sont posées les caisses : abscisse, ordonnée, et l'angle qui les tourne.
///
/// Le décor et les accessoires sont deux ressources différentes, chargées
/// séparément et soumises séparément : la carte porte les murs, le maillage ce
/// qu'on y pose.
constexpr float CRATES[][3] = {
    { 3.0f, 2.0f, 0.4f },
    { 10.0f, 2.0f, -0.7f },
    { 16.0f, 4.0f, 1.1f },
};

/// L'échelle des caisses.
///
/// **Le maillage fait deux unités de côté**, et les salles quatre de haut : posée
/// telle quelle, une caisse en occupe la moitié. Le fichier ne se redimensionne
/// pas — c'est celui de la scène de conformance, et son empreinte est figée —,
/// donc l'échelle va dans la matrice de modèle, qui est faite pour ça.
constexpr float CRATE_SCALE = 0.5f;

/// La cote du centre d'une caisse : sa demi-hauteur au-dessus du sol, qui est en
/// zéro dans ce décor.
constexpr float CRATE_Z = 0.5f;

/// La matrice d'une caisse : une rotation autour de la verticale mise à
/// l'échelle, puis une translation. Par colonnes, comme l'ABI l'attend.
///
/// Les coefficients viennent de la bibliothèque mathématique de l'hôte, et c'est
/// permis ici : une démonstration n'est comparée à aucune empreinte, là où une
/// scène de conformance exige les mêmes bits sur les quatre cibles.
ScgMat4 crate_model(const float *placement)
{
    const float c = SDL_cosf(placement[2]) * CRATE_SCALE;
    const float s = SDL_sinf(placement[2]) * CRATE_SCALE;
    ScgMat4 out{};
    out.m[0] = c;
    out.m[1] = s;
    out.m[4] = -s;
    out.m[5] = c;
    out.m[10] = CRATE_SCALE;
    out.m[12] = placement[0];
    out.m[13] = placement[1];
    out.m[14] = CRATE_Z;
    out.m[15] = 1.0f;
    return out;
}

}  // namespace

// Ouvre la fenêtre et rend jusqu'à ce qu'on la ferme, les tuiles réparties sur
// plusieurs threads.
//
// Les deux chemins de fichier sont des paramètres et non des constantes : le
// moteur n'ouvre rien, et c'est l'hôte qui sait où vivent les données.
int main(int argc, char **argv)
{
    if (argc != 3) {
        std::fprintf(stderr, "usage : %s <fichier de carte> <fichier de maillage>\n", argv[0]);
        return 2;
    }

    const std::vector<uint8_t> bytes = read_file(argv[1]);
    if (bytes.empty()) {
        std::fprintf(stderr, "%s : lecture impossible\n", argv[1]);
        return 1;
    }

    ScgWorld *raw_world = nullptr;
    if (scg_world_load(bytes.data(), bytes.size(), &raw_world) < 0) {
        fail(nullptr, "la carte est refusée");
        return 1;
    }
    // Le bloc reste dans `bytes` jusqu'au retour, mais le moteur n'en dépend
    // plus : il copie ce qu'il garde.
    const std::unique_ptr<ScgWorld, decltype(&scg_world_destroy)> world(raw_world,
                                                                       &scg_world_destroy);

    // Les lightmaps, cuites une fois pour toutes les cellules avant la première
    // image : une lightmap est un cache de la carte et non de la vue, et la cuire
    // en chemin ferait allouer un atlas au milieu d'une image.
    ScgLighting *raw_lighting = nullptr;
    if (scg_lighting_create(world.get(), &raw_lighting) < 0) {
        fail(nullptr, "porteur de lightmaps refusé");
        return 1;
    }
    const std::unique_ptr<ScgLighting, decltype(&scg_lighting_destroy)> lighting(
        raw_lighting, &scg_lighting_destroy);
    uint32_t cells = 0;
    scg_world_cell_count(world.get(), &cells);
    for (uint32_t i = 0; i < cells; i++) {
        uint32_t id = 0;
        if (scg_world_cell_id(world.get(), i, &id) < 0
            || scg_lighting_build(lighting.get(), id) < 0) {
            fail(nullptr, "cuisson refusée");
            return 1;
        }
    }

    // Une texture par matériau, dans l'ordre que la carte déclare. L'hôte lit
    // les noms, décide de ce qu'il charge, et passe les handles dans cet ordre.
    uint32_t materials = 0;
    scg_world_material_count(world.get(), &materials);
    std::vector<const ScgTexture *> slots(materials, nullptr);
    for (uint32_t i = 0; i < materials; i++) {
        size_t needed = 0;
        if (scg_world_material_name(world.get(), i, nullptr, 0, &needed) < 0) {
            fail(nullptr, "nom de matériau illisible");
            return 1;
        }
        std::string name(needed + 1, '\0');
        if (scg_world_material_name(world.get(), i, name.data(), name.size(), &needed)
            < 0) {
            fail(nullptr, "nom de matériau illisible");
            return 1;
        }
        const bool wall = std::strcmp(name.c_str(), "mur") == 0;
        slots[i] = load_checker(wall ? WALL_SIDE : FLOOR_SIDE, wall ? 128 : 32);
        if (slots[i] == nullptr) {
            fail(nullptr, "texture refusée");
            return 1;
        }
    }

    // Le maillage des caisses, chargé comme la carte : un bloc d'octets, que le
    // moteur copie. Ses deux emplacements portent des noms, et l'hôte décide de
    // ce qu'il met dedans — ici le même damier sur les deux.
    const std::vector<uint8_t> mesh_bytes = read_file(argv[2]);
    if (mesh_bytes.empty()) {
        std::fprintf(stderr, "%s : lecture impossible\n", argv[2]);
        return 1;
    }
    ScgMesh *raw_crate = nullptr;
    if (scg_mesh_load(mesh_bytes.data(), mesh_bytes.size(), &raw_crate) < 0) {
        fail(nullptr, "le maillage est refusé");
        return 1;
    }
    const std::unique_ptr<ScgMesh, decltype(&scg_mesh_destroy)> crate(raw_crate,
                                                                     &scg_mesh_destroy);
    const ScgTexture *crate_side = load_checker(CRATE_SIDE, 8);
    const ScgTexture *crate_slots[2] = { crate_side, crate_side };
    if (crate_slots[0] == nullptr) {
        fail(nullptr, "texture de caisse refusée");
        return 1;
    }

    ScgContextConfig config{};
    config.max_width = WIDTH;
    config.max_height = HEIGHT;
    config.width = WIDTH;
    config.height = HEIGHT;
    config.tile_size = TILE;

    ScgContext *raw_ctx = nullptr;
    if (scg_create(&config, &raw_ctx) < 0) {
        fail(nullptr, "création du contexte");
        return 1;
    }
    const std::unique_ptr<ScgContext, decltype(&scg_destroy)> ctx(raw_ctx, &scg_destroy);

    if (!SDL_Init(SDL_INIT_VIDEO)) {
        std::fprintf(stderr, "SDL_Init : %s\n", SDL_GetError());
        return 1;
    }
    SDL_Window *window = SDL_CreateWindow("Screengine", WIDTH * SCALE, HEIGHT * SCALE, 0);
    SDL_Renderer *renderer = window ? SDL_CreateRenderer(window, nullptr) : nullptr;
    // RGBA32 est l'ordre mémoire que le moteur écrit : rouge, vert, bleu,
    // alpha, quelle que soit la boutianité de la machine.
    SDL_Texture *screen = renderer
        ? SDL_CreateTexture(renderer, SDL_PIXELFORMAT_RGBA32, SDL_TEXTUREACCESS_STREAMING,
                            WIDTH, HEIGHT)
        : nullptr;
    if (screen == nullptr) {
        std::fprintf(stderr, "SDL : %s\n", SDL_GetError());
        return 1;
    }
    // Au plus proche : lisser une image de rendu logiciel en l'agrandissant
    // effacerait précisément ce qu'elle a de net.
    SDL_SetTextureScaleMode(screen, SDL_SCALEMODE_NEAREST);

    std::vector<uint8_t> pixels(static_cast<size_t>(WIDTH) * HEIGHT * 4);
    // Le sol de ce décor est en zéro : une caméra laissée à l'origine serait dans
    // le plancher, hors de toute cellule, et la traversée ne rendrait rien.
    float position[3] = {2.0f, 2.0f, 2.0f};
    float angle = 0.0f;
    uint32_t cell = 0;
    if (scg_world_locate(world.get(), position, &cell) < 0 || cell == 0) {
        fail(nullptr, "la caméra ne part d'aucune cellule");
        return 1;
    }
    Uint64 previous = SDL_GetTicks();
    Uint64 since = previous;
    unsigned frames = 0;
    bool running = true;

    // La créature et sa tache : le format masqué se déclare au chargement,
    // jamais au dessin, parce que c'est là que la chaîne de mipmaps se
    // construit.
    ScgTexture *creature = load_sprite(make_creature(), SCG_TEXTURE_FORMAT_RGBA8_MASKED);
    ScgTexture *blot = load_sprite(make_blot(), SCG_TEXTURE_FORMAT_RGBA8);
    if (creature == nullptr || blot == nullptr) {
        fail(nullptr, "la créature ou sa tache ne se chargent pas");
        return 1;
    }
    // Son va-et-vient, et la bande que la salle en L et le couloir partagent :
    // **ce décor n'est pas centré sur l'origine**, et `y = 0` y est une paroi.
    float walker = 8.0f;
    float heading = 1.0f;
    constexpr float LANE = 2.0f;

    while (running) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            if (event.type == SDL_EVENT_QUIT) {
                running = false;
            }
            if (event.type == SDL_EVENT_KEY_DOWN && event.key.scancode == SDL_SCANCODE_ESCAPE) {
                running = false;
            }
        }

        const Uint64 now = SDL_GetTicks();
        float dt = static_cast<float>(now - previous) / 1000.0f;
        previous = now;
        if (dt > 0.1f) {
            dt = 0.1f;
        }

        const bool *keys = SDL_GetKeyboardState(nullptr);
        if (keys[SDL_SCANCODE_LEFT] || keys[SDL_SCANCODE_A]) {
            angle += TURN * dt;
        }
        if (keys[SDL_SCANCODE_RIGHT] || keys[SDL_SCANCODE_D]) {
            angle -= TURN * dt;
        }
        const float forward = static_cast<float>(keys[SDL_SCANCODE_UP] || keys[SDL_SCANCODE_W])
            - static_cast<float>(keys[SDL_SCANCODE_DOWN] || keys[SDL_SCANCODE_S]);
        const float previous_position[3] = { position[0], position[1], position[2] };
        position[0] += SDL_cosf(angle) * forward * SPEED * dt;
        position[1] += SDL_sinf(angle) * forward * SPEED * dt;

        // La cellule se suit par le déplacement, et c'est l'hôte qui la garde : le
        // moteur ne retient aucune caméra. Zéro veut dire « sorti du décor » et ne
        // s'écrit pas — garder la dernière cellule connue laisse voir le décor
        // depuis dehors, là où l'écraser éteindrait l'image.
        uint32_t found = 0;
        if (scg_world_track(world.get(), cell, previous_position, position, &found) >= 0
            && found != 0) {
            cell = found;
        }

        ScgCamera camera{};
        std::memcpy(camera.position, position, sizeof camera.position);
        yaw(angle, camera.orientation);
        camera.fov_y = 1.2f;
        camera.near_plane = 0.1f;
        if (scg_set_camera(ctx.get(), &camera) < 0) {
            fail(ctx.get(), "caméra refusée");
            break;
        }

        ScgMat4 model{};
        model.m[0] = model.m[5] = model.m[10] = model.m[15] = 1.0f;
        if (scg_submit_world_visible(ctx.get(), &model, world.get(), slots.data(), materials,
                                     lighting.get(), cell)
            < 0) {
            fail(ctx.get(), "carte refusée");
            break;
        }

        // Les caisses par-dessus, chacune avec sa matrice : la même ressource
        // dessinée trois fois, ce qu'un décor fait de ses accessoires.
        for (const float(&placement)[3] : CRATES) {
            const ScgMat4 transform = crate_model(placement);
            if (scg_submit_mesh(ctx.get(), &transform, crate.get(), crate_slots, 2) < 0) {
                fail(ctx.get(), "maillage refusé");
                running = false;
                break;
            }
        }
        if (!running) {
            break;
        }

        // La créature arpente le décor, sa tache la suit.
        //
        // **La tache d'abord, en surface modulée** : elle multiplie le sol au
        // lieu de l'écraser, et teste la profondeur sans l'écrire — c'est
        // l'ordre de soumission qui la fait gagner sur les dalles, sans biais
        // de profondeur.
        walker += heading * 1.1f * dt;
        if (walker > 14.0f) {
            walker = 14.0f;
            heading = -1.0f;
        } else if (walker < 6.0f) {
            walker = 6.0f;
            heading = 1.0f;
        }

        constexpr float BLOT = 0.55f;
        constexpr float FLOOR_Z = 0.01f;
        const ScgVertexUv patch[4] = {
            { walker - BLOT, LANE - BLOT, FLOOR_Z, 0.0f, 0.0f },
            { walker + BLOT, LANE - BLOT, FLOOR_Z, 64.0f, 0.0f },
            { walker + BLOT, LANE + BLOT, FLOOR_Z, 64.0f, 64.0f },
            { walker - BLOT, LANE + BLOT, FLOOR_Z, 0.0f, 64.0f },
        };
        static constexpr ScgTriangle PATCH_FACES[2] = {
            { 0, 1, 2, 0xFF, 0xFF, 0xFF, 0xFF },
            { 0, 2, 3, 0xFF, 0xFF, 0xFF, 0xFF },
        };
        if (scg_submit_blended(ctx.get(), &model, patch, 4, PATCH_FACES, 2, blot,
                               SCG_BLEND_MODULATE)
            < 0) {
            fail(ctx.get(), "tache refusée");
            break;
        }

        // **En mode axial**, le seul juste pour un personnage debout : plein
        // face, il se coucherait au sol dès qu'on le regarde d'en haut.
        ScgSprite quad{};
        quad.x = walker;
        quad.y = LANE;
        quad.half_width = 0.6f;
        quad.half_height = 0.9f;
        quad.z = quad.half_height;
        quad.u1 = static_cast<float>(SPRITE_SIDE);
        quad.v1 = static_cast<float>(SPRITE_SIDE);
        quad.r = 0xFF;
        quad.g = 0xFF;
        quad.b = 0xFF;
        quad.a = 0xFF;
        if (scg_submit_sprites(ctx.get(), &model, &quad, 1, creature, SCG_SPRITE_AXIAL) < 0) {
            fail(ctx.get(), "créature refusée");
            break;
        }

        // Par tuiles : un hôte qui voudrait les répartir sur ses threads le
        // ferait ici, et rien d'autre ne changerait.
        uint32_t tiles = 0;
        if (scg_frame_begin(ctx.get(), &tiles) < 0) {
            fail(ctx.get(), "début d'image");
            break;
        }
        for (uint32_t i = 0; i < tiles && running; i++) {
            if (scg_frame_tile(ctx.get(), i, pixels.data(), WIDTH) < 0) {
                fail(ctx.get(), "tuile");
                running = false;
            }
        }
        if (scg_frame_end(ctx.get(), pixels.data(), WIDTH) < 0) {
            fail(ctx.get(), "fin d'image");
            break;
        }

        SDL_UpdateTexture(screen, nullptr, pixels.data(), WIDTH * 4);
        SDL_RenderClear(renderer);
        SDL_RenderTexture(renderer, screen, nullptr, nullptr);
        SDL_RenderPresent(renderer);

        // La cadence dans le titre : une démonstration de rendu logiciel se
        // juge autant à sa fluidité qu'à son image, et il n'y a pas d'endroit
        // pour l'écrire dans la fenêtre.
        frames++;
        if (now - since >= 1000) {
            const std::string title = "Screengine — "
                + std::to_string(frames * 1000 / (now - since)) + " images/s";
            SDL_SetWindowTitle(window, title.c_str());
            frames = 0;
            since = now;
        }
    }

    for (const ScgTexture *texture : slots) {
        scg_texture_destroy(const_cast<ScgTexture *>(texture));
    }
    scg_texture_destroy(const_cast<ScgTexture *>(crate_slots[0]));
    scg_texture_destroy(creature);
    scg_texture_destroy(blot);
    SDL_DestroyTexture(screen);
    SDL_DestroyRenderer(renderer);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}
