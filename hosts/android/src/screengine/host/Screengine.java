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

    /** Appel hors séquence : pendant une image, ou après une soumission. */
    public static final int ERR_INVALID_STATE = -4;

    /** Le moteur a paniqué ; l'objet est défaillant. */
    public static final int ERR_PANIC = -5;

    /** Octets par pixel du tampon de sortie, R, G, B, A. */
    public static final int BYTES_PER_PIXEL = 4;

    /** Tramage ordonné des coordonnées, le filtrage par défaut du moteur. */
    public static final int FILTER_DITHER = 0;

    /** Bilinéaire, qui remplace le tramage au lieu de s'y ajouter. */
    public static final int FILTER_BILINEAR = 1;

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

    /**
     * {@code scg_set_filter}.
     *
     * Les valeurs sont celles du header, {@code SCG_FILTER_DITHER} et
     * {@code SCG_FILTER_BILINEAR} : elles ne sont pas recopiées ici, et une
     * valeur inconnue traverse la couche JNI pour se faire refuser par la
     * bibliothèque, qui seule fait foi.
     *
     * @param ctx handle, ou 0
     * @param filter le niveau de filtrage
     * @return le code de retour
     */
    static native int setFilter(long ctx, int filter);

    /**
     * {@code scg_set_resolution}.
     *
     * Les deux côtés doivent tenir sous le maximum passé à la création : au
     * delà, la bibliothèque refuse et le contexte garde sa résolution. Le
     * tampon de l'hôte ne suit pas tout seul — après une hausse, il faut le
     * réallouer et passer le nouveau {@code stride}.
     *
     * @param ctx handle, ou 0
     * @param width la largeur interne, en pixels
     * @param height la hauteur interne, en pixels
     * @return le code de retour
     */
    static native int setResolution(long ctx, int width, int height);

    /**
     * {@code scg_submit_lit}.
     *
     * Mêmes tableaux que {@link #submitTextured}, les sommets portant sept
     * flottants au lieu de cinq : les cinq précédents, puis {@code u2} et
     * {@code v2} en texels de la lightmap.
     *
     * {@code texture} vaut 0 pour un lot uni, que ce chemin accepte — c'est
     * ainsi qu'un mur sans texture rend {@code couleur x lightmap}. Une
     * {@code lightmap} à 0, en revanche, est refusée : un lot qui n'en a pas
     * n'a rien à faire ici.
     *
     * @param ctx handle, ou 0
     * @param model seize coefficients, par colonnes
     * @param vertices sept flottants par sommet
     * @param indices trois indices par triangle
     * @param colors quatre octets R, G, B, A par triangle
     * @param texture handle de la texture du lot, ou 0
     * @param lightmap handle de sa lightmap
     * @return le code de retour
     */
    static native int submitLit(long ctx, float[] model, float[] vertices, int[] indices,
            byte[] colors, long texture, long lightmap);

    /**
     * {@code scg_set_overbright}.
     *
     * Le décalage vaut 0, 1 ou 2 ; une autre valeur traverse la couche JNI
     * pour se faire refuser par la bibliothèque, qui seule fait foi.
     *
     * @param ctx handle, ou 0
     * @param shift le décalage de sur-éclairement
     * @return le code de retour
     */
    static native int setOverbright(long ctx, int shift);

    /**
     * {@code scg_set_fog}.
     *
     * <p>Les trois canaux sont des entiers et non des octets : Java n'a pas
     * d'octet non signé, et une couleur passée en {@code byte} obligerait
     * chaque appelant à masquer.
     *
     * <p>Le fond de l'image prend la couleur du brouillard de lui-même : il
     * n'y a rien à effacer avec elle.
     *
     * @param ctx handle, ou 0
     * @param r canal rouge, de 0 à 255
     * @param g canal vert
     * @param b canal bleu
     * @param start distance où le brouillard commence
     * @param end distance où il est plein
     * @return le code de retour
     */
    static native int setFog(long ctx, int r, int g, int b, float start, float end);

    /**
     * {@code scg_clear_fog}.
     *
     * <p>Éteindre un brouillard qui n'existe pas n'est pas une erreur.
     *
     * @param ctx handle, ou 0
     * @return le code de retour
     */
    static native int clearFog(long ctx);

    /**
     * {@code scg_set_grade}.
     *
     * <p>Les sept valeurs sont le gamma, les trois gains puis les trois
     * décalages, dans cet ordre : c'est la couche JNI qui remplit la structure,
     * au seul endroit qui voit le header, plutôt que de faire reproduire sa
     * disposition ici.
     *
     * <p>{@code reserved} traverse quand même, pour que le refus d'un champ
     * réservé non nul puisse être éprouvé depuis Java. Un hôte réel y passe
     * zéro.
     *
     * @param ctx handle, ou 0
     * @param values gamma, trois gains, trois décalages
     * @param reserved la valeur du second champ réservé, nulle sauf à l'éprouver
     * @return le code de retour
     */
    static native int setGrade(long ctx, float[] values, int reserved);

    /**
     * {@code scg_clear_grade}.
     *
     * <p>Éteindre une courbe qui n'existe pas n'est pas une erreur.
     *
     * @param ctx handle, ou 0
     * @return le code de retour
     */
    static native int clearGrade(long ctx);

    /**
     * {@code scg_set_lights}.
     *
     * <p>Les lumières arrivent en <b>deux tableaux parallèles</b> : quatre
     * flottants de pose par lumière — position puis rayon — et trois octets de
     * couleur. Java n'a pas de structure à disposition mémoire garantie, et le
     * pont reconstitue les {@code ScgLight}, champ réservé compris.
     *
     * <p>Huit lumières au plus. Au-delà, l'appel entier est refusé plutôt que
     * tronqué : une scène à demi éclairée ne se distingue pas d'une scène dont
     * les rayons sont mal réglés.
     *
     * <p>L'atténuation se calcule <b>par sommet, à la soumission</b> : des
     * lumières réglées après un lot ne l'éclairent pas.
     *
     * @param ctx handle, ou 0
     * @param poses quatre flottants par lumière : x, y, z, rayon
     * @param colors trois octets par lumière : R, G, B
     * @return le code de retour
     */
    static native int setLights(long ctx, float[] poses, byte[] colors);
}
