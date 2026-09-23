// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

package screengine.host;

import java.nio.ByteBuffer;
import java.util.Arrays;

/**
 * Hôte Android sans fenêtre : la frontière vue depuis l'ART, à travers JNI.
 *
 * <p>Lancé par {@code app_process} sur l'appareil ou l'émulateur, sans APK ni
 * activité. Il éprouve ce que l'exécutable C du même hôte ne voit pas : le
 * chargement par {@code System.load}, {@code JNI_OnLoad} et l'enregistrement des
 * méthodes, la version d'ABI reçue signée, et un tampon direct dont la base est
 * volontairement désalignée. Il écrit l'empreinte du triangle sur la sortie
 * standard, les échecs sur la sortie d'erreur.
 *
 * <p>La scène est celle de {@code screengine-conformance --print arete} :
 * 640×360, tuiles de 64.
 *
 * <p>Usage : {@code app_process <dir> screengine.host.Test <dir>}
 */
public final class Test {
    /** Largeur de la scène. */
    private static final int WIDTH = 640;

    /** Hauteur de la scène. */
    private static final int HEIGHT = 360;

    /** Côté de tuile. */
    private static final int TILE = 64;

    /** Le stride de l'hôte, plus grand que la largeur pour qu'une fin de ligne existe. */
    private static final int STRIDE = WIDTH + 3;

    /** Marge sentinelle avant et après la zone rendue, en octets. */
    private static final int GUARD = 64;

    /** Décalage ajouté à la marge : la base du rendu tombe sur une adresse impaire. */
    private static final int SKEW = 1;

    /** L'octet sentinelle, qui ne ressemble à aucune couleur du rendu. */
    private static final byte SENTINEL = (byte) 0xA5;

    /** Nombre de vérifications en échec. */
    private static int failures;

    /** Pas d'instance. */
    private Test() {}

    /**
     * Enregistre une vérification, et dit laquelle a échoué.
     *
     * @param ok le résultat
     * @param what ce qui est vérifié
     */
    private static void check(boolean ok, String what) {
        if (!ok) {
            System.err.println("échec : " + what);
            failures++;
        }
    }

    /**
     * Une configuration valide, champs réservés à zéro.
     *
     * @return les huit champs de {@code ScgContextConfig}
     */
    private static int[] sceneConfig() {
        return new int[] {WIDTH, HEIGHT, WIDTH, HEIGHT, TILE, 0, 0, 0};
    }

    /**
     * Soumet la scène {@code arete} : le quadrilatère de la conformance, en
     * coordonnées de monde, vu par la caméra par défaut depuis l'origine.
     *
     * <p>Les mêmes valeurs que la scène de référence, décrites ici en Java et
     * passées à travers JNI puis l'ABI : c'est la comparaison des empreintes
     * qui dit qu'elles arrivent intactes.
     *
     * @param ctx handle du contexte
     * @return le code de retour de la soumission
     */
    private static int submitScene(long ctx) {
        float[] model = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        float[] vertices = {
            2.0f, 2.5f, 1.6f,
            3.5f, -2.5f, 1.6f,
            3.5f, -2.5f, -1.6f,
            2.0f, 2.5f, -1.6f,
        };
        // L'arête des sommets 0 et 2, parcourue en sens opposés par les deux.
        int[] indices = {0, 2, 1, 0, 3, 2};
        byte[] colors = {
            (byte) 0xE0, (byte) 0xA0, 0x30, (byte) 0xFF,
            (byte) 0xA0, (byte) 0xE0, 0x30, (byte) 0xFF,
        };
        return Screengine.submit(ctx, model, vertices, indices, colors);
    }

    /** Côté de la texture du sol, en texels, et côté d'une de ses cases. */
    private static final int FLOOR_SIDE = 64;

    private static final int FLOOR_CELL = 8;

    /**
     * Le damier procédural, teinte pour teinte comme la suite de conformance
     * l'écrit : c'est lui qui décide de l'empreinte, et un liseré décalé d'un
     * texel la ferait diverger.
     *
     * @return les texels, quatre octets chacun, lignes jointives
     */
    private static byte[] makeChecker() {
        byte[] texels = new byte[FLOOR_SIDE * FLOOR_SIDE * 4];
        for (int v = 0; v < FLOOR_SIDE; v++) {
            for (int u = 0; u < FLOOR_SIDE; u++) {
                int base = (v * FLOOR_SIDE + u) * 4;
                boolean edge = u % FLOOR_CELL == 0 || v % FLOOR_CELL == 0;
                boolean dark = (u / FLOOR_CELL + v / FLOOR_CELL) % 2 == 0;
                if (edge) {
                    texels[base] = (byte) 0xF0;
                    texels[base + 1] = (byte) 0xE0;
                    texels[base + 2] = (byte) 0xA0;
                } else if (dark) {
                    texels[base] = 0x30;
                    texels[base + 1] = 0x38;
                    texels[base + 2] = 0x50;
                } else {
                    texels[base] = (byte) 0x90;
                    texels[base + 1] = 0x70;
                    texels[base + 2] = 0x50;
                }
                texels[base + 3] = (byte) 0xFF;
            }
        }
        return texels;
    }

    /**
     * Rend la scène texturée sous le filtrage demandé, ou {@code null} si le
     * rendu a échoué.
     *
     * <p>La texture est détruite avant le rendu, à dessein : le moteur en garde
     * sa propre référence jusqu'à la fin de l'image, et l'empreinte le prouve.
     *
     * <p>La géométrie ne dépend pas du filtrage : un écart entre les deux
     * empreintes ne peut donc venir que de lui.
     *
     * @param filter {@code SCG_FILTER_DITHER} ou {@code SCG_FILTER_BILINEAR}
     * @return l'empreinte
     */
    private static String renderTextured(int filter) {
        long texture = Screengine.textureLoad(FLOOR_SIDE, FLOOR_SIDE, makeChecker());
        check(texture != 0, "la texture se charge sans contexte");

        long[] out = {0};
        if (texture == 0 || Screengine.create(sceneConfig(), out) != Screengine.OK) {
            check(false, "création du contexte texturé");
            return null;
        }

        check(Screengine.setFilter(out[0], filter) == Screengine.OK, "le filtrage se règle");

        float[] model = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        // Cinq flottants par sommet : la position, puis u et v en texels, à
        // huit texels par unité de monde comme la scène de référence.
        float[] vertices = {
            2.0f, -24.0f, -1.2f, 2.0f * 8.0f, -24.0f * 8.0f,
            60.0f, -24.0f, -1.2f, 60.0f * 8.0f, -24.0f * 8.0f,
            60.0f, 24.0f, -1.2f, 60.0f * 8.0f, 24.0f * 8.0f,
            2.0f, 24.0f, -1.2f, 2.0f * 8.0f, 24.0f * 8.0f,
        };
        int[] indices = {0, 1, 2, 0, 2, 3};
        byte[] colors = {
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
        };
        check(Screengine.submitTextured(out[0], model, vertices, indices, colors, texture)
                == Screengine.OK, "le lot texturé est accepté");
        Screengine.textureDestroy(texture);

        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        ByteBuffer block = ByteBuffer.allocateDirect(body);
        int code = Screengine.frameEnd(out[0], block, 0, STRIDE);
        check(code == Screengine.OK, "l'image texturée se rend");

        String hash = fingerprint(block, 0, STRIDE);
        Screengine.destroy(out[0]);
        return code == Screengine.OK ? hash : null;
    }

    /** Côté de la lightmap de la scène {@code lumiere}, en texels. */
    private static final int LIGHT_SIDE = 16;

    /**
     * Le dégradé de lightmap, recopié de la suite de conformance.
     *
     * <p>Les deux axes n'y font pas la même chose : un dégradé symétrique
     * laisserait passer un axe échangé entre les deux jeux de coordonnées.
     *
     * @return {@code LIGHT_SIDE} au carré texels de quatre octets
     */
    private static byte[] makeGradient() {
        byte[] luxels = new byte[LIGHT_SIDE * LIGHT_SIDE * 4];
        for (int v = 0; v < LIGHT_SIDE; v++) {
            for (int u = 0; u < LIGHT_SIDE; u++) {
                int base = (v * LIGHT_SIDE + u) * 4;
                luxels[base] = (byte) (32 + u * 223 / (LIGHT_SIDE - 1));
                luxels[base + 1] = (byte) (32 + (u + v) / 2 * 223 / (LIGHT_SIDE - 1));
                luxels[base + 2] = (byte) (32 + v * 223 / (LIGHT_SIDE - 1));
                luxels[base + 3] = (byte) 0xFF;
            }
        }
        return luxels;
    }

    /**
     * Rend la scène éclairée, ou {@code null} si le rendu a échoué.
     *
     * <p>Deux lots : le sol, texturé et éclairé, puis le mur, éclairé seul. Le
     * second passe une texture nulle, ce que ce point d'entrée accepte là où
     * {@code submitTextured} la refuse — l'asymétrie que le header signale, et
     * que cet hôte exerce pour de bon.
     *
     * @return l'empreinte
     */
    private static String renderLit() {
        long texture = Screengine.textureLoad(FLOOR_SIDE, FLOOR_SIDE, makeChecker());
        check(texture != 0, "la texture du sol se charge");
        long lightmap = Screengine.textureLoad(LIGHT_SIDE, LIGHT_SIDE, makeGradient());
        check(lightmap != 0, "la lightmap se charge par le meme chemin");

        long[] out = {0};
        if (texture == 0 || lightmap == 0
                || Screengine.create(sceneConfig(), out) != Screengine.OK) {
            check(false, "création du contexte éclairé");
            return null;
        }

        float[] model = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        // Sept flottants par sommet : la position, u et v en texels, puis u2
        // et v2 en texels de la lightmap. Celles-ci vont d'un demi-texel à un
        // demi-texel du bord opposé : une lightmap ne se pave pas, et le
        // bilinéaire irait autrement chercher son voisin par le repli.
        float[] floor = {
            2.0f, -16.0f, -1.2f, 2.0f * 8.0f, -16.0f * 8.0f, 0.5f, 0.5f,
            40.0f, -16.0f, -1.2f, 40.0f * 8.0f, -16.0f * 8.0f, 15.5f, 0.5f,
            40.0f, 16.0f, -1.2f, 40.0f * 8.0f, 16.0f * 8.0f, 15.5f, 15.5f,
            2.0f, 16.0f, -1.2f, 2.0f * 8.0f, 16.0f * 8.0f, 0.5f, 15.5f,
        };
        float[] wall = {
            40.0f, -16.0f, -1.2f, 0.0f, 0.0f, 0.5f, 0.5f,
            40.0f, -16.0f, 10.0f, 0.0f, 0.0f, 15.5f, 0.5f,
            40.0f, 16.0f, 10.0f, 0.0f, 0.0f, 15.5f, 15.5f,
            40.0f, 16.0f, -1.2f, 0.0f, 0.0f, 0.5f, 15.5f,
        };
        int[] indices = {0, 1, 2, 0, 2, 3};
        byte[] white = {
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
        };
        byte[] stone = {
            (byte) 0xC0, (byte) 0xB0, (byte) 0x90, (byte) 0xFF,
            (byte) 0xC0, (byte) 0xB0, (byte) 0x90, (byte) 0xFF,
        };

        // Une lightmap nulle est refusée, elle : sans elle, ce lot n'a rien à
        // faire sur ce chemin.
        check(Screengine.submitLit(out[0], model, floor, indices, white, texture, 0)
                == Screengine.ERR_NULL, "une lightmap nulle est refusée");
        check(Screengine.submitLit(out[0], model, floor, indices, white, texture, lightmap)
                == Screengine.OK, "le sol texturé et éclairé est accepté");
        check(Screengine.submitLit(out[0], model, wall, indices, stone, 0, lightmap)
                == Screengine.OK, "le mur uni et éclairé est accepté");
        Screengine.textureDestroy(texture);
        Screengine.textureDestroy(lightmap);

        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        ByteBuffer block = ByteBuffer.allocateDirect(body);
        int code = Screengine.frameEnd(out[0], block, 0, STRIDE);
        check(code == Screengine.OK, "l'image éclairée se rend");

        String hash = fingerprint(block, 0, STRIDE);
        Screengine.destroy(out[0]);
        return code == Screengine.OK ? hash : null;
    }

    /**
     * Vrai si le message est non vide quand on l'attend, et sans caractère de
     * contrôle venu d'un tampon non initialisé.
     *
     * @param message le message lu
     * @param expectText s'il doit être non vide
     * @return le résultat
     */
    private static boolean messageOk(String message, boolean expectText) {
        if (message == null || expectText == message.isEmpty()) {
            return false;
        }
        for (int i = 0; i < message.length(); i++) {
            if (message.charAt(i) < 0x20) {
                return false;
            }
        }
        return true;
    }

    /** Les refus, vus depuis Java : codes exacts et messages lisibles. */
    private static void checkRefusals() {
        long[] out = {0x5A5A5A5AL};

        check(Screengine.create(null, out) == Screengine.ERR_NULL, "configuration nulle refusée par SCG_ERR_NULL");
        check(messageOk(Screengine.lastError(0), true), "message sans contexte après une configuration nulle");

        int[] config = sceneConfig();
        config[4] = 48;
        check(Screengine.create(config, out) == Screengine.ERR_INVALID_ARGUMENT, "taille de tuile 48 refusée");
        check(out[0] == 0x5A5A5A5AL, "rien n'est écrit dans le paramètre de sortie après un refus");

        config = sceneConfig();
        config[6] = 1;
        check(Screengine.create(config, out) == Screengine.ERR_INVALID_ARGUMENT, "champ réservé non nul refusé");

        check(Screengine.create(sceneConfig(), null) == Screengine.ERR_NULL, "paramètre de sortie nul refusé");

        check(Screengine.create(sceneConfig(), out) == Screengine.OK, "configuration valide acceptée");
        long ctx = out[0];
        check(messageOk(Screengine.lastError(ctx), false), "message vide après un succès");
        check(messageOk(Screengine.lastError(0), false), "message sans contexte vidé par un appel réussi");

        ByteBuffer small = ByteBuffer.allocateDirect(4 * 4);
        check(Screengine.frameEnd(ctx, null, 0, WIDTH) == Screengine.ERR_NULL, "tampon nul refusé");
        check(Screengine.frameEnd(ctx, small, 0, WIDTH - 1) == Screengine.ERR_INVALID_ARGUMENT, "stride inférieur à la largeur refusé");
        check(messageOk(Screengine.lastError(ctx), true), "message du contexte après un stride refusé");
        check(Screengine.frameEnd(0, small, 0, WIDTH) == Screengine.ERR_NULL, "contexte nul refusé");

        Screengine.destroy(ctx);
        Screengine.destroy(0);
    }

    /** L'allocation pour le compte de l'hôte : alignement de l'ABI, cas limites. */
    private static void checkBuffers() {
        long len = (long) WIDTH * HEIGHT * Screengine.BYTES_PER_PIXEL;
        long buffer = Screengine.bufferAlloc(len);
        check(buffer != 0, "scg_buffer_alloc rend un tampon");
        if (buffer != 0) {
            check((buffer & (Screengine.BUFFER_ALIGNMENT - 1)) == 0, "tampon aligné sur SCG_BUFFER_ALIGNMENT");
            Screengine.bufferFree(buffer, len);
        }
        check(Screengine.bufferAlloc(0) == 0, "une allocation de zéro octet rend 0");
        Screengine.bufferFree(0, 0);
    }

    /**
     * FNV-1a 64 bits dans la forme de la conformance. Le produit de deux
     * {@code long} se replie modulo 2⁶⁴, ce que FNV exige.
     *
     * @param pixels tampon
     * @param base position de la zone rendue
     * @param stride pas de ligne en pixels
     * @return seize chiffres hexadécimaux minuscules
     */
    private static String fingerprint(ByteBuffer pixels, int base, int stride) {
        long hash = 0xcbf29ce484222325L;
        for (int dim : new int[] {WIDTH, HEIGHT}) {
            for (int shift = 0; shift < 32; shift += 8) {
                hash = (hash ^ ((dim >>> shift) & 0xFF)) * 0x100000001b3L;
            }
        }
        for (int y = 0; y < HEIGHT; y++) {
            int row = base + y * stride * Screengine.BYTES_PER_PIXEL;
            for (int i = 0; i < WIDTH * Screengine.BYTES_PER_PIXEL; i++) {
                hash = (hash ^ (pixels.get(row + i) & 0xFF)) * 0x100000001b3L;
            }
        }
        return String.format("%016x", hash);
    }

    /**
     * Rend le triangle dans un tampon direct entouré de sentinelles, à une base
     * désalignée, vérifie que rien n'est écrit hors de la zone utile, et rend
     * l'empreinte, ou {@code null} si le rendu a échoué.
     *
     * @return l'empreinte
     */
    private static String render() {
        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        int base = SKEW + GUARD;
        ByteBuffer block = ByteBuffer.allocateDirect(base + body + GUARD);
        byte[] fill = new byte[block.capacity()];
        Arrays.fill(fill, SENTINEL);
        block.put(fill).clear();

        long[] out = {0};
        if (Screengine.create(sceneConfig(), out) != Screengine.OK) {
            check(false, "création du contexte de rendu");
            return null;
        }
        check(submitScene(out[0]) == Screengine.OK, "scène soumise");
        int code = Screengine.frameEnd(out[0], block, base, STRIDE);
        check(code == Screengine.OK, "scg_frame_end aboutit sur une base désalignée");

        boolean intact = true;
        for (int i = 0; i < base; i++) {
            intact &= block.get(i) == SENTINEL;
        }
        for (int i = 0; i < GUARD; i++) {
            intact &= block.get(base + body + i) == SENTINEL;
        }
        for (int y = 0; y < HEIGHT; y++) {
            int tail = base + y * STRIDE * Screengine.BYTES_PER_PIXEL + WIDTH * Screengine.BYTES_PER_PIXEL;
            for (int i = 0; i < (STRIDE - WIDTH) * Screengine.BYTES_PER_PIXEL; i++) {
                intact &= block.get(tail + i) == SENTINEL;
            }
        }
        check(intact, "rien n'est écrit hors de la zone utile, marges et fins de ligne comprises");

        boolean opaque = true;
        for (int y = 0; y < HEIGHT; y++) {
            int row = base + y * STRIDE * Screengine.BYTES_PER_PIXEL;
            for (int x = 0; x < WIDTH; x++) {
                opaque &= block.get(row + x * Screengine.BYTES_PER_PIXEL + 3) == (byte) 0xFF;
            }
        }
        check(opaque, "l'alpha est écrit à 255 sur chaque pixel");

        String hash = fingerprint(block, base, STRIDE);
        Screengine.destroy(out[0]);
        return code == Screengine.OK ? hash : null;
    }

    /**
     * Toutes les vérifications, puis l'empreinte sur la sortie standard.
     *
     * @param args le répertoire des bibliothèques
     */
    public static void main(String[] args) {
        if (args.length != 1) {
            System.err.println("usage : app_process <dir> screengine.host.Test <dir>");
            System.exit(2);
        }
        Screengine.load(args[0]);
        check(Screengine.abiVersionUnsigned() == Screengine.ABI_VERSION, "la bibliothèque chargée est celle du header");

        checkRefusals();
        checkBuffers();
        String hash = render();
        String textured = renderTextured(Screengine.FILTER_DITHER);
        String bilinear = renderTextured(Screengine.FILTER_BILINEAR);
        String lit = renderLit();

        if (failures > 0 || hash == null || textured == null || bilinear == null || lit == null) {
            System.err.println(failures + " vérification(s) en échec");
            System.exit(1);
        }
        System.out.println(hash);
        System.out.println(textured);
        System.out.println(bilinear);
        System.out.println(lit);
        System.exit(0);
    }
}
