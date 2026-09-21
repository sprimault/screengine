// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

// Hôte C++ de Screengine, sans fenêtre, lié à la bibliothèque dynamique.
//
// C'est la forme sous laquelle un moteur ou un jeu écrit en C++ intégrera la
// bibliothèque. Il vérifie ce que l'hôte C, lié en statique, ne voit pas : le
// header compilé en C++ — gardes extern "C", assertions de disposition —, et un
// chargement dynamique réel, prouvé en demandant au système de quel fichier
// vient le symbole appelé. Le reste reprend les contrôles de l'hôte C, et son
// empreinte du triangle est comparée à celle du chemin Rust par `make test-cpp`.
//
// La scène est celle de `screengine-conformance --print arete` : 640×360,
// tuiles de 64.

#include "screengine.h"

#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
#include <thread>
#include <vector>

#if defined(_WIN32)
#include <windows.h>
#else
#include <dlfcn.h>
#endif

#if defined(__x86_64__) || defined(_M_X64)
#include <xmmintrin.h>
#define HAS_MXCSR 1
#else
#define HAS_MXCSR 0
#endif

namespace {

/// Largeur, hauteur et tuile de la scène.
constexpr uint32_t WIDTH = 640, HEIGHT = 360, TILE = 64;

/// Le stride de l'hôte, plus grand que la largeur pour éprouver les fins de ligne.
constexpr uint32_t STRIDE = WIDTH + 3;

/// Marge sentinelle avant et après le tampon, en octets.
constexpr size_t GUARD = 64;

/// L'octet sentinelle.
constexpr uint8_t SENTINEL = 0xA5;

/// Nombre de vérifications en échec.
int failures = 0;

/// Enregistre une vérification, et dit laquelle a échoué.
void check(bool ok, const char *what)
{
    if (!ok) {
        std::fprintf(stderr, "échec : %s\n", what);
        ++failures;
    }
}

/// Un contexte détruit à la sortie de portée, quel que soit le chemin.
struct ContextDeleter {
    /// Rend le contexte au moteur.
    void operator()(ScgContext *ctx) const { scg_destroy(ctx); }
};

/// Le contexte possédé, comme un intégrateur C++ l'écrira.
using Context = std::unique_ptr<ScgContext, ContextDeleter>;

/// FNV-1a 64 bits, dans la forme de screengine-conformance : largeur et hauteur
/// en u32 petit-boutiste, puis la zone utile ligne par ligne.
uint64_t fingerprint(const uint8_t *pixels, uint32_t width, uint32_t height, uint32_t stride)
{
    uint64_t hash = 0xcbf29ce484222325u;
    auto mix = [&hash](uint8_t byte) { hash = (hash ^ byte) * 0x100000001b3u; };

    for (uint32_t dim : {width, height}) {
        for (int shift = 0; shift < 32; shift += 8) {
            mix(static_cast<uint8_t>(dim >> shift));
        }
    }
    for (uint32_t y = 0; y < height; ++y) {
        const uint8_t *row = pixels + size_t{y} * stride * 4;
        for (uint32_t i = 0; i < width * 4; ++i) {
            mix(row[i]);
        }
    }
    return hash;
}

/// Une configuration valide ; l'initialisation par valeur met tout à zéro,
/// champs réservés compris.
ScgContextConfig scene_config()
{
    ScgContextConfig config{};
    config.max_width = config.width = WIDTH;
    config.max_height = config.height = HEIGHT;
    config.tile_size = TILE;
    return config;
}

/// Le quadrilatère de la scène `arete`, en coordonnées de monde : X vers
/// l'est, Z en haut, la caméra par défaut le regardant depuis l'origine.
///
/// Ce sont les valeurs de la scène de conformance. Cet hôte les décrit en C++
/// et les fait traverser l'ABI : c'est la comparaison des deux empreintes qui
/// dit que sommets, indices, couleurs et matrice arrivent intacts.
constexpr ScgVertex SCENE_VERTICES[4] = {
    { 2.0f, 2.5f, 1.6f },
    { 3.5f, -2.5f, 1.6f },
    { 3.5f, -2.5f, -1.6f },
    { 2.0f, 2.5f, -1.6f },
};

/// Deux triangles qui partagent l'arête des sommets 0 et 2, parcourue en sens
/// opposés par chacun.
constexpr ScgTriangle SCENE_TRIANGLES[2] = {
    { 0, 2, 1, 0xE0, 0xA0, 0x30, 0xFF },
    { 0, 3, 2, 0xA0, 0xE0, 0x30, 0xFF },
};

/// L'identité, par colonnes.
constexpr ScgMat4 IDENTITY = {
    { 1.0f, 0.0f, 0.0f, 0.0f,
      0.0f, 1.0f, 0.0f, 0.0f,
      0.0f, 0.0f, 1.0f, 0.0f,
      0.0f, 0.0f, 0.0f, 1.0f }
};

/// Soumet la scène au contexte, et rend vrai si elle a été acceptée.
bool submit_scene(ScgContext *ctx)
{
    return scg_submit(ctx, &IDENTITY, SCENE_VERTICES, 4, SCENE_TRIANGLES, 2) == SCG_OK;
}

/// Le côté de la texture du sol, en texels, et celui d'une de ses cases.
constexpr uint32_t FLOOR_SIDE = 64;
constexpr uint32_t FLOOR_CELL = 8;

/// Le sol de la scène `texture`, à 1,2 unité sous la caméra, habillé à huit
/// texels par unité de monde. Mêmes valeurs que la scène de conformance.
constexpr ScgVertexUv FLOOR_VERTICES[4] = {
    {  2.0f, -24.0f, -1.2f,   2.0f * 8.0f, -24.0f * 8.0f },
    { 60.0f, -24.0f, -1.2f,  60.0f * 8.0f, -24.0f * 8.0f },
    { 60.0f,  24.0f, -1.2f,  60.0f * 8.0f,  24.0f * 8.0f },
    {  2.0f,  24.0f, -1.2f,   2.0f * 8.0f,  24.0f * 8.0f },
};

/// Ses deux triangles, blancs : c'est la texture qui porte la couleur.
constexpr ScgTriangle FLOOR_TRIANGLES[2] = {
    { 0, 1, 2, 0xFF, 0xFF, 0xFF, 0xFF },
    { 0, 2, 3, 0xFF, 0xFF, 0xFF, 0xFF },
};

/// Le damier procédural, teinte pour teinte comme la suite de conformance
/// l'écrit : c'est lui qui décide de l'empreinte, et un liseré décalé d'un
/// texel la ferait diverger.
std::vector<uint8_t> make_checker()
{
    std::vector<uint8_t> texels(static_cast<size_t>(FLOOR_SIDE) * FLOOR_SIDE * 4);
    for (uint32_t v = 0; v < FLOOR_SIDE; v++) {
        for (uint32_t u = 0; u < FLOOR_SIDE; u++) {
            uint8_t *texel = texels.data() + (static_cast<size_t>(v) * FLOOR_SIDE + u) * 4;
            const bool edge = (u % FLOOR_CELL) == 0 || (v % FLOOR_CELL) == 0;
            const bool dark = ((u / FLOOR_CELL) + (v / FLOOR_CELL)) % 2 == 0;
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

/// Rend la scène texturée sous le filtrage demandé et hache son image.
///
/// La texture est détruite avant le rendu, à dessein : le moteur en garde sa
/// propre référence jusqu'à la fin de l'image, et l'empreinte le prouve.
///
/// `filter` rejoue la même géométrie en bilinéaire : un écart entre les deux
/// empreintes ne peut alors venir que du filtrage.
uint64_t render_textured(bool &ok, uint32_t filter)
{
    ok = false;
    ScgContextConfig config = scene_config();
    ScgContext *ctx = nullptr;
    ScgTexture *texture = nullptr;
    const std::vector<uint8_t> texels = make_checker();

    ScgTextureDesc desc{};
    desc.width = FLOOR_SIDE;
    desc.height = FLOOR_SIDE;
    desc.format = SCG_TEXTURE_FORMAT_RGBA8;

    check(scg_texture_load(&desc, texels.data(), texels.size(), &texture) == SCG_OK,
          "la texture se charge sans contexte");
    check(scg_create(&config, &ctx) == SCG_OK, "création du contexte texturé");
    if (texture == nullptr || ctx == nullptr) {
        scg_texture_destroy(texture);
        scg_destroy(ctx);
        return 0;
    }

    check(scg_set_filter(ctx, filter) == SCG_OK, "le filtrage se règle");
    check(scg_submit_textured(ctx, &IDENTITY, FLOOR_VERTICES, 4, FLOOR_TRIANGLES, 2, texture)
              == SCG_OK,
          "le lot texturé est accepté");
    scg_texture_destroy(texture);

    std::vector<uint8_t> pixels(static_cast<size_t>(STRIDE) * HEIGHT * 4);
    const int32_t code = scg_frame_end(ctx, pixels.data(), STRIDE);
    check(code == SCG_OK, "l'image texturée se rend");
    ok = code == SCG_OK;

    const uint64_t hash = fingerprint(pixels.data(), WIDTH, HEIGHT, STRIDE);
    scg_destroy(ctx);
    return hash;
}

/// Vrai si `scg_abi_version` est exporté par la bibliothèque dynamique chargée
/// dans le processus, et rend la même version que l'appel direct. Sans ce
/// contrôle, une bibliothèque statique liée par erreur passerait tous les
/// autres, et le chargement dynamique ne serait prouvé nulle part.
///
/// L'adresse de la fonction ne suffit pas : sous Windows, `&scg_abi_version`
/// désigne le thunk d'import de l'exécutable. On interroge donc le module par
/// son nom, puis son export.
bool loaded_dynamically()
{
    using Version = uint32_t (*)();
#if defined(_WIN32)
    HMODULE module = GetModuleHandleA("screengine.dll");
    if (module == nullptr || module == GetModuleHandleA(nullptr)) {
        return false;
    }
    const auto exported = reinterpret_cast<Version>(GetProcAddress(module, "scg_abi_version"));
#else
    const auto exported = reinterpret_cast<Version>(dlsym(RTLD_DEFAULT, "scg_abi_version"));
    Dl_info info{};
    if (exported == nullptr || dladdr(reinterpret_cast<const void *>(exported), &info) == 0 || info.dli_fname == nullptr
        || std::string(info.dli_fname).find("libscreengine.so") == std::string::npos) {
        return false;
    }
#endif
    return exported != nullptr && exported() == scg_abi_version();
}

/// Vrai si le message est non nul, terminé, non vide si `expect_text`, et sans
/// octet de contrôle venu d'un tampon non initialisé.
bool message_ok(const char *message, bool expect_text)
{
    if (message == nullptr) {
        return false;
    }
    const std::string text(message);
    if (expect_text != !text.empty()) {
        return false;
    }
    for (unsigned char c : text) {
        if (c < 0x20) {
            return false;
        }
    }
    return true;
}

/// Les refus, vus depuis C++ : codes exacts et messages lisibles.
void check_refusals()
{
    ScgContext *raw = nullptr;
    ScgContextConfig config = scene_config();

    check(scg_create(nullptr, &raw) == SCG_ERR_NULL, "configuration nulle refusée par SCG_ERR_NULL");
    check(message_ok(scg_last_error(nullptr), true), "message sans contexte après une configuration nulle");

    config.tile_size = 48;
    check(scg_create(&config, &raw) == SCG_ERR_INVALID_ARGUMENT, "taille de tuile 48 refusée");
    check(raw == nullptr, "rien n'est écrit dans le paramètre de sortie après un refus");

    config = scene_config();
    config.reserved1 = 1;
    check(scg_create(&config, &raw) == SCG_ERR_INVALID_ARGUMENT, "champ réservé non nul refusé");

    config = scene_config();
    check(scg_create(&config, nullptr) == SCG_ERR_NULL, "paramètre de sortie nul refusé");

    check(scg_create(&config, &raw) == SCG_OK, "configuration valide acceptée");
    Context ctx(raw);
    if (!ctx) {
        return;
    }
    check(message_ok(scg_last_error(ctx.get()), false), "message vide après un succès");

    // Pas `small` : windows.h en fait une macro.
    uint8_t scratch[16] = {};
    check(scg_frame_end(ctx.get(), nullptr, WIDTH) == SCG_ERR_NULL, "tampon nul refusé");
    check(scg_frame_end(ctx.get(), scratch, WIDTH - 1) == SCG_ERR_INVALID_ARGUMENT, "stride inférieur à la largeur refusé");
    check(message_ok(scg_last_error(ctx.get()), true), "message du contexte après un stride refusé");
    check(scg_frame_end(nullptr, scratch, WIDTH) == SCG_ERR_NULL, "contexte nul refusé");
}

/// L'allocation pour le compte de l'hôte : alignement de l'ABI, longueur écrite
/// en entier, cas limites.
void check_buffers()
{
    const size_t len = size_t{WIDTH} * HEIGHT * 4;
    uint8_t *buffer = scg_buffer_alloc(len);

    check(buffer != nullptr, "scg_buffer_alloc rend un tampon");
    if (buffer != nullptr) {
        check(reinterpret_cast<uintptr_t>(buffer) % SCG_BUFFER_ALIGNMENT == 0, "tampon aligné sur SCG_BUFFER_ALIGNMENT");
        std::memset(buffer, 0, len);
        scg_buffer_free(buffer, len);
    }
    check(scg_buffer_alloc(0) == nullptr, "une allocation de zéro octet rend NULL");
    scg_buffer_free(nullptr, 0);
}

/// Rend le triangle entre deux marges sentinelles, vérifie que rien n'est écrit
/// hors de la zone utile, et rend l'empreinte. `ok` reçoit faux si le rendu
/// lui-même a échoué.
uint64_t render(bool &ok)
{
    ok = false;
    const size_t body = size_t{STRIDE} * HEIGHT * 4;
    std::vector<uint8_t> block(GUARD + body + GUARD, SENTINEL);

    ScgContextConfig config = scene_config();
    ScgContext *raw = nullptr;
    if (scg_create(&config, &raw) != SCG_OK) {
        check(false, "création du contexte de rendu");
        return 0;
    }
    Context ctx(raw);

    uint8_t *pixels = block.data() + GUARD;
    check(submit_scene(ctx.get()), "scène soumise");
    const int32_t code = scg_frame_end(ctx.get(), pixels, STRIDE);
    check(code == SCG_OK, "scg_frame_end aboutit");
    ok = code == SCG_OK;

    bool intact = true;
    for (size_t i = 0; i < GUARD; ++i) {
        intact = intact && block[i] == SENTINEL && pixels[body + i] == SENTINEL;
    }
    bool opaque = true;
    for (size_t y = 0; y < HEIGHT; ++y) {
        const uint8_t *row = pixels + y * STRIDE * 4;
        for (size_t i = size_t{WIDTH} * 4; i < size_t{STRIDE} * 4; ++i) {
            intact = intact && row[i] == SENTINEL;
        }
        for (size_t x = 0; x < WIDTH; ++x) {
            opaque = opaque && row[x * 4 + 3] == 255;
        }
    }
    check(intact, "rien n'est écrit hors de la zone utile, marges et fins de ligne comprises");
    check(opaque, "l'alpha est écrit à 255 sur chaque pixel");

    return fingerprint(pixels, WIDTH, HEIGHT, STRIDE);
}

/// Le rendu par tuiles depuis plusieurs threads, comme un moteur C++ le fera
/// avec son propre pool : chaque thread prend les tuiles d'un indice modulo le
/// nombre de threads, une sur quatre est laissée à la fin, et l'empreinte doit
/// être celle de la fin seule. Chaque thread vérifie aussi que son message se
/// lit dans son propre emplacement.
void check_tiles(uint64_t expected)
{
    ScgContextConfig config = scene_config();
    ScgContext *raw = nullptr;
    if (scg_create(&config, &raw) != SCG_OK) {
        check(false, "création du contexte des tuiles");
        return;
    }
    Context ctx(raw);
    std::vector<uint8_t> pixels(size_t{STRIDE} * HEIGHT * 4);

    check(submit_scene(ctx.get()), "scène soumise");
    uint32_t count = 0;
    check(scg_frame_begin(ctx.get(), &count) == SCG_OK, "scg_frame_begin aboutit");

    const unsigned workers = 4;
    std::atomic<int> refused{0};
    std::vector<std::thread> threads;
    for (unsigned w = 0; w < workers; ++w) {
        threads.emplace_back([&, w] {
            for (uint32_t i = 0; i < count; ++i) {
                if (i % workers != w || i % 4 == 0) {
                    continue;
                }
                if (scg_frame_tile(ctx.get(), i, pixels.data(), STRIDE) != SCG_OK) {
                    ++refused;
                }
            }
            if (scg_frame_tile(ctx.get(), count, pixels.data(), STRIDE) != SCG_ERR_INVALID_ARGUMENT
                || std::strlen(scg_last_error(nullptr)) == 0) {
                ++refused;
            }
        });
    }
    for (std::thread &thread : threads) {
        thread.join();
    }
    check(refused == 0, "chaque thread rend ses tuiles et lit son propre message");

    check(scg_frame_end(ctx.get(), pixels.data(), STRIDE) == SCG_OK, "la fin complète les tuiles manquantes");
    check(fingerprint(pixels.data(), WIDTH, HEIGHT, STRIDE) == expected, "les tuiles rendues sur plusieurs threads donnent l'image de la fin seule");
}

#if HAS_MXCSR
/// Un hôte hostile : exceptions démasquées, arrondi vers le haut, DAZ et FZ. Le
/// moteur doit rendre la même image, et rendre le registre intact. Comme pour
/// l'hôte C, seule la restauration du registre a aujourd'hui un effet
/// observable sur le triangle en dur.
void check_float_environment(uint64_t expected)
{
    const unsigned int host = _mm_getcsr();
    const unsigned int hostile = (host & ~0x7F80u) | 0x4000u | 0x8040u;

    _mm_setcsr(hostile);
    bool ok = false;
    const uint64_t hash = render(ok);
    const unsigned int after = _mm_getcsr();
    _mm_setcsr(host);

    check(ok, "le rendu aboutit sous un environnement flottant hostile");
    check(hash == expected, "l'environnement flottant de l'hôte ne change pas l'image");
    check(after == hostile, "le registre flottant de l'hôte est rendu à l'identique");
}
#endif

} // namespace

/// Toutes les vérifications, puis l'empreinte sur la sortie standard.
int main()
{
    check(scg_abi_version() == SCG_ABI_VERSION, "la bibliothèque chargée est celle du header");
    check(loaded_dynamically(), "scg_abi_version vient de la bibliothèque dynamique");

    check_refusals();
    check_buffers();

    bool ok = false;
    const uint64_t hash = render(ok);

#if HAS_MXCSR
    if (ok) {
        check_float_environment(hash);
    }
#endif
    if (ok) {
        check_tiles(hash);
    }

    bool textured_ok = false;
    const uint64_t textured = render_textured(textured_ok, SCG_FILTER_DITHER);

    bool bilinear_ok = false;
    const uint64_t bilinear = render_textured(bilinear_ok, SCG_FILTER_BILINEAR);

    if (failures > 0 || !ok || !textured_ok || !bilinear_ok) {
        std::fprintf(stderr, "%d vérification(s) en échec\n", failures);
        return 1;
    }
    std::printf("%016llx\n", static_cast<unsigned long long>(hash));
    std::printf("%016llx\n", static_cast<unsigned long long>(textured));
    std::printf("%016llx\n", static_cast<unsigned long long>(bilinear));
    return 0;
}
