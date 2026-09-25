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
    if (scg_texture_load(&desc, texels.data(), texels.size(), &texture) != SCG_OK) {
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
    { 6.0f, -1.2f, 0.4f },
    { 11.0f, 1.4f, -0.7f },
    { 17.0f, -0.6f, 1.1f },
};

/// L'échelle des caisses.
///
/// **Le maillage fait deux unités de côté**, et le couloir six de large : posée
/// telle quelle, une caisse en occupe le tiers. Le fichier ne se redimensionne
/// pas — c'est celui de la scène de conformance, et son empreinte est figée —,
/// donc l'échelle va dans la matrice de modèle, qui est faite pour ça.
constexpr float CRATE_SCALE = 0.5f;

/// La cote du centre d'une caisse : sa demi-hauteur au-dessus du sol du
/// couloir, qui est à -1,5.
constexpr float CRATE_Z = -1.0f;

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
    if (scg_world_load(bytes.data(), bytes.size(), &raw_world) != SCG_OK) {
        fail(nullptr, "la carte est refusée");
        return 1;
    }
    // Le bloc reste dans `bytes` jusqu'au retour, mais le moteur n'en dépend
    // plus : il copie ce qu'il garde.
    const std::unique_ptr<ScgWorld, decltype(&scg_world_destroy)> world(raw_world,
                                                                       &scg_world_destroy);

    // Une texture par matériau, dans l'ordre que la carte déclare. L'hôte lit
    // les noms, décide de ce qu'il charge, et passe les handles dans cet ordre.
    uint32_t materials = 0;
    scg_world_material_count(world.get(), &materials);
    std::vector<const ScgTexture *> slots(materials, nullptr);
    for (uint32_t i = 0; i < materials; i++) {
        size_t needed = 0;
        if (scg_world_material_name(world.get(), i, nullptr, 0, &needed) != SCG_OK) {
            fail(nullptr, "nom de matériau illisible");
            return 1;
        }
        std::string name(needed + 1, '\0');
        if (scg_world_material_name(world.get(), i, name.data(), name.size(), &needed)
            != SCG_OK) {
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
    if (scg_mesh_load(mesh_bytes.data(), mesh_bytes.size(), &raw_crate) != SCG_OK) {
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
    if (scg_create(&config, &raw_ctx) != SCG_OK) {
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
    float position[3] = {0.0f, 0.0f, 0.0f};
    float angle = 0.0f;
    Uint64 previous = SDL_GetTicks();
    Uint64 since = previous;
    unsigned frames = 0;
    bool running = true;

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
        position[0] += SDL_cosf(angle) * forward * SPEED * dt;
        position[1] += SDL_sinf(angle) * forward * SPEED * dt;

        ScgCamera camera{};
        std::memcpy(camera.position, position, sizeof camera.position);
        yaw(angle, camera.orientation);
        camera.fov_y = 1.2f;
        camera.near_plane = 0.1f;
        if (scg_set_camera(ctx.get(), &camera) != SCG_OK) {
            fail(ctx.get(), "caméra refusée");
            break;
        }

        ScgMat4 model{};
        model.m[0] = model.m[5] = model.m[10] = model.m[15] = 1.0f;
        if (scg_submit_world(ctx.get(), &model, world.get(), slots.data(), materials) != SCG_OK) {
            fail(ctx.get(), "carte refusée");
            break;
        }

        // Les caisses par-dessus, chacune avec sa matrice : la même ressource
        // dessinée trois fois, ce qu'un décor fait de ses accessoires.
        for (const float(&placement)[3] : CRATES) {
            const ScgMat4 transform = crate_model(placement);
            if (scg_submit_mesh(ctx.get(), &transform, crate.get(), crate_slots, 2) != SCG_OK) {
                fail(ctx.get(), "maillage refusé");
                running = false;
                break;
            }
        }
        if (!running) {
            break;
        }

        // Par tuiles : un hôte qui voudrait les répartir sur ses threads le
        // ferait ici, et rien d'autre ne changerait.
        uint32_t tiles = 0;
        if (scg_frame_begin(ctx.get(), &tiles) != SCG_OK) {
            fail(ctx.get(), "début d'image");
            break;
        }
        for (uint32_t i = 0; i < tiles && running; i++) {
            if (scg_frame_tile(ctx.get(), i, pixels.data(), WIDTH) != SCG_OK) {
                fail(ctx.get(), "tuile");
                running = false;
            }
        }
        if (scg_frame_end(ctx.get(), pixels.data(), WIDTH) != SCG_OK) {
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
    SDL_DestroyTexture(screen);
    SDL_DestroyRenderer(renderer);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}
