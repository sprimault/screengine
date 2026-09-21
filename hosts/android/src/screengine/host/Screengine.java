// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

package screengine.host;

import android.graphics.Bitmap;
import java.nio.ByteBuffer;

/**
 * Les points d'entrée de Screengine vus depuis Java, enregistrés par la couche
 * JNI de {@code jni.c}.
 *
 * <p>Les constantes sont recopiées du header, que Java ne lit pas. Les handles
 * et les adresses sont des {@code long}, les valeurs {@code uint32_t} des
 * {@code int} à relire non signés.
 */
public final class Screengine {
    /** Version d'ABI contre laquelle cette classe est écrite. */
    public static final long ABI_VERSION = 1;

    /** Alignement garanti par {@code scg_buffer_alloc}. */
    public static final long BUFFER_ALIGNMENT = 16;

    /** Succès. */
    public static final int OK = 0;

    /** Argument pointeur nul. */
    public static final int ERR_NULL = -1;

    /** Argument hors de ce que le moteur accepte. */
    public static final int ERR_INVALID_ARGUMENT = -2;

    /** Le moteur a paniqué ; l'objet est défaillant. */
    public static final int ERR_PANIC = -5;

    /** Octets par pixel du tampon de sortie, R, G, B, A. */
    public static final int BYTES_PER_PIXEL = 4;

    /** Pas d'instance : des fonctions, comme l'ABI. */
    private Screengine() {}

    /**
     * Charge la bibliothèque publiée puis la couche JNI, par chemins absolus.
     * La première avant la seconde : la couche JNI la réclame à son chargement.
     *
     * @param dir répertoire des deux bibliothèques
     */
    public static void load(String dir) {
        System.load(dir + "/libscreengine.so");
        System.load(dir + "/libscreengine_jni.so");
    }

    /**
     * La version d'ABI de la bibliothèque chargée, relue non signée : JNI la rend
     * en {@code jint}.
     *
     * @return la version
     */
    public static long abiVersionUnsigned() {
        // Un masque et non Integer.toUnsignedLong, absent avant l'API 26.
        return abiVersion() & 0xFFFFFFFFL;
    }

    /**
     * {@code scg_abi_version}, signé tel que JNI le rend.
     *
     * @return la version, à relire non signée
     */
    static native int abiVersion();

    /**
     * {@code scg_create}.
     *
     * @param config les huit champs de {@code ScgContextConfig} dans leur ordre, ou
     *     {@code null} pour une configuration nulle
     * @param out reçoit le handle en {@code out[0]} en cas de succès, ou {@code null}
     * @return le code de retour
     */
    static native int create(int[] config, long[] out);

    /**
     * {@code scg_destroy}.
     *
     * @param ctx handle, ou 0
     */
    static native void destroy(long ctx);

    /**
     * {@code scg_submit}, les trois tableaux repris tels quels.
     *
     * <p>Trois tableaux plutôt qu'un tableau de structures, que Java ne sait pas
     * décrire : la couche JNI recompose les {@code ScgVertex} et les
     * {@code ScgTriangle}. Les couleurs voyagent en octets séparés et jamais
     * empaquetées dans un {@code int} — un entier réintroduirait la question de
     * l'ordre que les quatre champs nommés de l'ABI ferment.
     *
     * @param ctx handle, ou 0
     * @param model seize coefficients, par colonnes
     * @param vertices trois coordonnées par sommet
     * @param indices trois indices par triangle
     * @param colors quatre octets R, G, B, A par triangle
     * @return le code de retour
     */
    static native int submit(long ctx, float[] model, float[] vertices, int[] indices, byte[] colors);

    /**
     * {@code scg_frame_end} dans un tampon direct.
     *
     * @param ctx handle, ou 0
     * @param pixels tampon direct, ou {@code null}
     * @param offset décalage de la base dans le tampon, en octets
     * @param stride pas de ligne en pixels
     * @return le code de retour
     */
    static native int frameEnd(long ctx, ByteBuffer pixels, int offset, int stride);

    /**
     * {@code scg_frame_end} dans la mémoire d'un bitmap RGBA_8888, sans copie.
     *
     * @param ctx handle
     * @param bitmap bitmap en {@code ARGB_8888}, dont la mémoire est en R, G, B, A
     * @return le code de retour
     */
    static native int frameEndBitmap(long ctx, Bitmap bitmap);

    /**
     * {@code scg_last_error}, copié.
     *
     * @param ctx handle, ou 0 pour l'emplacement sans contexte
     * @return le message, vide sans erreur
     */
    static native String lastError(long ctx);

    /**
     * {@code scg_buffer_alloc}.
     *
     * @param len longueur en octets
     * @return l'adresse, ou 0
     */
    static native long bufferAlloc(long len);

    /**
     * {@code scg_buffer_free}.
     *
     * @param ptr adresse rendue par {@link #bufferAlloc}, ou 0
     * @param len la même longueur
     */
    static native void bufferFree(long ptr, long len);

    /**
     * {@code scg_texture_load}, la description écrite par la couche JNI.
     *
     * Elle seule connaît le header : Java ne passe que les deux côtés et les
     * texels, le format et les champs réservés étant remplis de l'autre côté.
     *
     * @param width largeur en texels, puissance de deux de 1 à 2048
     * @param height hauteur en texels, même contrainte
     * @param texels quatre octets R, G, B, A par texel, lignes jointives
     * @return le handle, ou 0 en cas d'échec
     */
    static native long textureLoad(int width, int height, byte[] texels);

    /**
     * {@code scg_texture_destroy}.
     *
     * @param texture handle rendu par {@link #textureLoad}, ou 0
     */
    static native void textureDestroy(long texture);

    /**
     * {@code scg_submit_textured}.
     *
     * Mêmes tableaux que {@link #submit}, les sommets portant cinq flottants
     * au lieu de trois : trois de position, puis {@code u} et {@code v} en
     * texels.
     *
     * @param ctx handle, ou 0
     * @param model seize coefficients, par colonnes
     * @param vertices cinq flottants par sommet
     * @param indices trois indices par triangle
     * @param colors quatre octets R, G, B, A par triangle
     * @param texture handle de la texture du lot
     * @return le code de retour
     */
    static native int submitTextured(long ctx, float[] model, float[] vertices, int[] indices,
            byte[] colors, long texture);
}
