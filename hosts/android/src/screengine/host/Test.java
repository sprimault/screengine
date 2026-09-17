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
 * <p>La scène est celle de {@code screengine-conformance --print triangle} :
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

        if (failures > 0 || hash == null) {
            System.err.println(failures + " vérification(s) en échec");
            System.exit(1);
        }
        System.out.println(hash);
        System.exit(0);
    }
}
