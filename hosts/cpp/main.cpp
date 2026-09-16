// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT

// Hôte C++ de Screengine, sans fenêtre, lié à la bibliothèque dynamique.
//
// C'est la forme sous laquelle un moteur ou un jeu écrit en C++ intégrera la
// bibliothèque. Il vérifie ce que l'hôte C, lié en statique, ne voit pas : le
// header compilé en C++ — gardes extern "C", assertions de disposition —, et un
// chargement dynamique réel, prouvé en demandant au système de quel fichier
// vient le symbole appelé. Le reste reprend les contrôles de l'hôte C, et son
// empreinte du triangle est comparée à celle du chemin Rust par `make test-cpp`.
//
// La scène est celle de `screengine-conformance --print triangle` : 640×360,
// tuiles de 64.

#include "screengine.h"

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
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
    HMODULE module = GetModuleHandleA("screengine_ffi.dll");
    if (module == nullptr || module == GetModuleHandleA(nullptr)) {
        return false;
    }
    const auto exported = reinterpret_cast<Version>(GetProcAddress(module, "scg_abi_version"));
#else
    const auto exported = reinterpret_cast<Version>(dlsym(RTLD_DEFAULT, "scg_abi_version"));
    Dl_info info{};
    if (exported == nullptr || dladdr(reinterpret_cast<const void *>(exported), &info) == 0 || info.dli_fname == nullptr
        || std::string(info.dli_fname).find("libscreengine_ffi.so") == std::string::npos) {
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

    if (failures > 0 || !ok) {
        std::fprintf(stderr, "%d vérification(s) en échec\n", failures);
        return 1;
    }
    std::printf("%016llx\n", static_cast<unsigned long long>(hash));
    return 0;
}
