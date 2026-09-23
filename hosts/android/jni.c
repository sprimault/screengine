// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

/*
 * Couche JNI de l'hôte Android : conversion de types, rien d'autre.
 *
 * Bibliothèque séparée, liée dynamiquement à libscreengine.so : c'est la
 * bibliothèque publiée qu'on charge, pas une copie liée ici. Seul JNI_OnLoad est
 * exporté ; les méthodes s'enregistrent par RegisterNatives, pour qu'un nom ou
 * une signature fausse fasse échouer le chargement au lieu du premier appel.
 */

#include "screengine.h"

#include <android/bitmap.h>
#include <jni.h>
#include <stdlib.h>
#include <string.h>

/* La classe Java dont les méthodes `native` sont enregistrées ici. */
#define CLASS "screengine/host/Screengine"

/* scg_abi_version. */
static jint abi_version(JNIEnv *env, jclass cls)
{
    (void)env;
    (void)cls;
    return (jint)scg_abi_version();
}

/*
 * scg_create. `config` porte les huit champs de ScgContextConfig dans leur
 * ordre ; un tableau nul donne une configuration nulle, pour que l'hôte puisse
 * éprouver ce refus. Le handle est écrit dans `out[0]`, ou pas du tout.
 */
static jint create(JNIEnv *env, jclass cls, jintArray config, jlongArray out)
{
    (void)cls;
    ScgContextConfig fields;
    ScgContext *ctx = NULL;
    const ScgContextConfig *config_ptr = NULL;

    memset(&fields, 0, sizeof fields);
    if (config != NULL) {
        if ((*env)->GetArrayLength(env, config) != 8) {
            return SCG_ERR_INVALID_ARGUMENT;
        }
        jint values[8];
        (*env)->GetIntArrayRegion(env, config, 0, 8, values);
        fields.max_width = (uint32_t)values[0];
        fields.max_height = (uint32_t)values[1];
        fields.width = (uint32_t)values[2];
        fields.height = (uint32_t)values[3];
        fields.tile_size = (uint32_t)values[4];
        fields.max_triangles = (uint32_t)values[5];
        fields.reserved1 = (uint32_t)values[6];
        fields.reserved2 = (uint32_t)values[7];
        config_ptr = &fields;
    }

    int32_t code = scg_create(config_ptr, out != NULL ? &ctx : NULL);
    if (code == SCG_OK && out != NULL) {
        jlong handle = (jlong)(intptr_t)ctx;
        (*env)->SetLongArrayRegion(env, out, 0, 1, &handle);
    }
    return code;
}

/*
 * scg_submit, les structures recomposées depuis trois tableaux Java.
 *
 * Java ne sait pas décrire un tableau de structures : la couche JNI alloue les
 * siennes le temps de l'appel. C'est de la mémoire d'hôte, pas du moteur — la
 * règle « zéro allocation par image » porte sur ce que le noyau fait, pas sur
 * ce qu'un hôte se donne pour lui parler.
 */
static jint submit(JNIEnv *env, jclass cls, jlong ctx, jfloatArray model,
                   jfloatArray vertices, jintArray indices, jbyteArray colors)
{
    (void)cls;
    if (model == NULL || vertices == NULL || indices == NULL || colors == NULL) {
        return SCG_ERR_NULL;
    }

    jsize floats = (*env)->GetArrayLength(env, vertices);
    jsize index_count = (*env)->GetArrayLength(env, indices);
    jsize channels = (*env)->GetArrayLength(env, colors);
    if ((*env)->GetArrayLength(env, model) != 16 || floats % 3 != 0 || index_count % 3 != 0
        || channels != index_count / 3 * 4) {
        return SCG_ERR_INVALID_ARGUMENT;
    }

    uint32_t vertex_count = (uint32_t)(floats / 3);
    uint32_t triangle_count = (uint32_t)(index_count / 3);
    ScgMat4 matrix;
    ScgVertex *points = calloc(vertex_count ? vertex_count : 1, sizeof *points);
    ScgTriangle *faces = calloc(triangle_count ? triangle_count : 1, sizeof *faces);
    jint *raw_indices = calloc(index_count ? (size_t)index_count : 1, sizeof *raw_indices);
    jbyte *raw_colors = calloc(channels ? (size_t)channels : 1, sizeof *raw_colors);
    int32_t code = SCG_ERR_OUT_OF_MEMORY;

    if (points != NULL && faces != NULL && raw_indices != NULL && raw_colors != NULL) {
        (*env)->GetFloatArrayRegion(env, model, 0, 16, matrix.m);
        /* Les sommets se remplissent d'un bloc : `ScgVertex` est exactement
         * trois `float` contigus, ce que les assertions du header vérifient sur
         * cette cible même. */
        (*env)->GetFloatArrayRegion(env, vertices, 0, floats, (jfloat *)points);
        (*env)->GetIntArrayRegion(env, indices, 0, index_count, raw_indices);
        (*env)->GetByteArrayRegion(env, colors, 0, channels, raw_colors);

        for (uint32_t i = 0; i < triangle_count; i++) {
            faces[i].i0 = (uint32_t)raw_indices[i * 3];
            faces[i].i1 = (uint32_t)raw_indices[i * 3 + 1];
            faces[i].i2 = (uint32_t)raw_indices[i * 3 + 2];
            faces[i].r = (uint8_t)raw_colors[i * 4];
            faces[i].g = (uint8_t)raw_colors[i * 4 + 1];
            faces[i].b = (uint8_t)raw_colors[i * 4 + 2];
            faces[i].a = (uint8_t)raw_colors[i * 4 + 3];
        }
        code = scg_submit((ScgContext *)(intptr_t)ctx, &matrix, points, vertex_count, faces,
                          triangle_count);
    }

    free(points);
    free(faces);
    free(raw_indices);
    free(raw_colors);
    return code;
}

/* scg_destroy. */
static void destroy(JNIEnv *env, jclass cls, jlong ctx)
{
    (void)env;
    (void)cls;
    scg_destroy((ScgContext *)(intptr_t)ctx);
}

/* scg_set_filter. Une valeur inconnue traverse et se fait refuser par la
 * bibliothèque : l'énumération n'est pas recopiée ici, elle vit dans le
 * header. */
static jint set_filter(JNIEnv *env, jclass cls, jlong ctx, jint filter)
{
    (void)env;
    (void)cls;
    return (jint)scg_set_filter((ScgContext *)(intptr_t)ctx, (uint32_t)filter);
}

/*
 * scg_frame_end dans un ByteBuffer direct, à `offset` octets de son début.
 * L'offset permet à l'hôte de désaligner volontairement la base : le moteur
 * écrit par accès non alignés, et armv7 est la cible qui le démentirait.
 */
static jint frame_end(JNIEnv *env, jclass cls, jlong ctx, jobject buffer, jint offset, jint stride)
{
    (void)cls;
    uint8_t *pixels = NULL;
    if (buffer != NULL) {
        uint8_t *base = (*env)->GetDirectBufferAddress(env, buffer);
        if (base == NULL || offset < 0 || offset > (*env)->GetDirectBufferCapacity(env, buffer)) {
            return SCG_ERR_INVALID_ARGUMENT;
        }
        pixels = base + offset;
    }
    return scg_frame_end((ScgContext *)(intptr_t)ctx, pixels, (uint32_t)stride);
}

/*
 * scg_frame_end dans la mémoire d'un Bitmap RGBA_8888, sans copie. Android
 * donne le stride en octets par ligne, l'ABI l'attend en pixels.
 */
static jint frame_end_bitmap(JNIEnv *env, jclass cls, jlong ctx, jobject bitmap)
{
    (void)cls;
    AndroidBitmapInfo info;
    void *pixels = NULL;

    if (AndroidBitmap_getInfo(env, bitmap, &info) != ANDROID_BITMAP_RESULT_SUCCESS
        || info.format != ANDROID_BITMAP_FORMAT_RGBA_8888 || info.stride % 4 != 0) {
        return SCG_ERR_INVALID_ARGUMENT;
    }
    if (AndroidBitmap_lockPixels(env, bitmap, &pixels) != ANDROID_BITMAP_RESULT_SUCCESS) {
        return SCG_ERR_INVALID_STATE;
    }
    int32_t code = scg_frame_end((ScgContext *)(intptr_t)ctx, pixels, info.stride / 4);
    AndroidBitmap_unlockPixels(env, bitmap);
    return code;
}

/*
 * scg_last_error, copié aussitôt dans une chaîne Java. NewStringUTF lit de
 * l'UTF-8 modifié, identique à l'UTF-8 pour les messages ASCII du moteur ; un
 * message qui en sortirait devrait passer par un tableau d'octets.
 */
static jstring last_error(JNIEnv *env, jclass cls, jlong ctx)
{
    (void)cls;
    return (*env)->NewStringUTF(env, scg_last_error((const ScgContext *)(intptr_t)ctx));
}

/* scg_buffer_alloc, adresse rendue en entier pour que l'hôte en vérifie l'alignement. */
static jlong buffer_alloc(JNIEnv *env, jclass cls, jlong len)
{
    (void)env;
    (void)cls;
    return (jlong)(intptr_t)scg_buffer_alloc((size_t)len);
}

/* scg_buffer_free. */
static void buffer_free(JNIEnv *env, jclass cls, jlong ptr, jlong len)
{
    (void)env;
    (void)cls;
    scg_buffer_free((uint8_t *)(intptr_t)ptr, (size_t)len);
}

/*
 * scg_texture_load, la description recomposée ici.
 *
 * Java passe les deux côtés et le bloc de texels ; le format et les champs
 * réservés sont écrits par cette couche, qui est la seule à connaître le
 * header. La description est mise à zéro d'abord, comme l'ABI l'exige.
 */
static jlong texture_load(JNIEnv *env, jclass cls, jint width, jint height, jbyteArray texels)
{
    (void)cls;
    if (texels == NULL) {
        return 0;
    }

    jsize len = (*env)->GetArrayLength(env, texels);
    uint8_t *block = calloc(len ? (size_t)len : 1, 1);
    ScgTexture *texture = NULL;
    if (block != NULL) {
        (*env)->GetByteArrayRegion(env, texels, 0, len, (jbyte *)block);

        ScgTextureDesc desc;
        memset(&desc, 0, sizeof desc);
        desc.width = (uint32_t)width;
        desc.height = (uint32_t)height;
        desc.format = SCG_TEXTURE_FORMAT_RGBA8;
        if (scg_texture_load(&desc, block, (size_t)len, &texture) != SCG_OK) {
            texture = NULL;
        }
    }
    free(block);
    return (jlong)(intptr_t)texture;
}

/* scg_texture_destroy. */
static void texture_destroy(JNIEnv *env, jclass cls, jlong texture)
{
    (void)env;
    (void)cls;
    scg_texture_destroy((ScgTexture *)(intptr_t)texture);
}

/*
 * scg_submit_textured : mêmes tableaux que `submit`, les sommets portant cinq
 * `float` au lieu de trois.
 */
static jint submit_textured(JNIEnv *env, jclass cls, jlong ctx, jfloatArray model,
                            jfloatArray vertices, jintArray indices, jbyteArray colors,
                            jlong texture)
{
    (void)cls;
    if (model == NULL || vertices == NULL || indices == NULL || colors == NULL) {
        return SCG_ERR_NULL;
    }

    jsize floats = (*env)->GetArrayLength(env, vertices);
    jsize index_count = (*env)->GetArrayLength(env, indices);
    jsize channels = (*env)->GetArrayLength(env, colors);
    if ((*env)->GetArrayLength(env, model) != 16 || floats % 5 != 0 || index_count % 3 != 0
        || channels != index_count / 3 * 4) {
        return SCG_ERR_INVALID_ARGUMENT;
    }

    uint32_t vertex_count = (uint32_t)(floats / 5);
    uint32_t triangle_count = (uint32_t)(index_count / 3);
    ScgMat4 matrix;
    ScgVertexUv *points = calloc(vertex_count ? vertex_count : 1, sizeof *points);
    ScgTriangle *faces = calloc(triangle_count ? triangle_count : 1, sizeof *faces);
    jint *raw_indices = calloc(index_count ? (size_t)index_count : 1, sizeof *raw_indices);
    jbyte *raw_colors = calloc(channels ? (size_t)channels : 1, sizeof *raw_colors);
    int32_t code = SCG_ERR_OUT_OF_MEMORY;

    if (points != NULL && faces != NULL && raw_indices != NULL && raw_colors != NULL) {
        (*env)->GetFloatArrayRegion(env, model, 0, 16, matrix.m);
        /* Même remplissage d'un bloc que pour `submit` : `ScgVertexUv` est
         * exactement cinq `float` contigus, ce que les assertions du header
         * vérifient sur cette cible même. */
        (*env)->GetFloatArrayRegion(env, vertices, 0, floats, (jfloat *)points);
        (*env)->GetIntArrayRegion(env, indices, 0, index_count, raw_indices);
        (*env)->GetByteArrayRegion(env, colors, 0, channels, raw_colors);

        for (uint32_t i = 0; i < triangle_count; i++) {
            faces[i].i0 = (uint32_t)raw_indices[i * 3];
            faces[i].i1 = (uint32_t)raw_indices[i * 3 + 1];
            faces[i].i2 = (uint32_t)raw_indices[i * 3 + 2];
            faces[i].r = (uint8_t)raw_colors[i * 4];
            faces[i].g = (uint8_t)raw_colors[i * 4 + 1];
            faces[i].b = (uint8_t)raw_colors[i * 4 + 2];
            faces[i].a = (uint8_t)raw_colors[i * 4 + 3];
        }
        code = scg_submit_textured((ScgContext *)(intptr_t)ctx, &matrix, points, vertex_count,
                                   faces, triangle_count, (ScgTexture *)(intptr_t)texture);
    }

    free(points);
    free(faces);
    free(raw_indices);
    free(raw_colors);
    return code;
}

/*
 * scg_submit_lit : mêmes tableaux que `submitTextured`, les sommets portant
 * sept `float` au lieu de cinq, et deux handles d'image au lieu d'un.
 *
 * `texture` vaut zéro pour un lot uni, ce que le moteur accepte ici ; une
 * `lightmap` nulle, elle, est refusée, et ce n'est pas ce pont qui le décide.
 */
static jint submit_lit(JNIEnv *env, jclass cls, jlong ctx, jfloatArray model,
                       jfloatArray vertices, jintArray indices, jbyteArray colors,
                       jlong texture, jlong lightmap)
{
    (void)cls;
    if (model == NULL || vertices == NULL || indices == NULL || colors == NULL) {
        return SCG_ERR_NULL;
    }

    jsize floats = (*env)->GetArrayLength(env, vertices);
    jsize index_count = (*env)->GetArrayLength(env, indices);
    jsize channels = (*env)->GetArrayLength(env, colors);
    if ((*env)->GetArrayLength(env, model) != 16 || floats % 7 != 0 || index_count % 3 != 0
        || channels != index_count / 3 * 4) {
        return SCG_ERR_INVALID_ARGUMENT;
    }

    uint32_t vertex_count = (uint32_t)(floats / 7);
    uint32_t triangle_count = (uint32_t)(index_count / 3);
    ScgMat4 matrix;
    ScgVertexUv2 *points = calloc(vertex_count ? vertex_count : 1, sizeof *points);
    ScgTriangle *faces = calloc(triangle_count ? triangle_count : 1, sizeof *faces);
    jint *raw_indices = calloc(index_count ? (size_t)index_count : 1, sizeof *raw_indices);
    jbyte *raw_colors = calloc(channels ? (size_t)channels : 1, sizeof *raw_colors);
    int32_t code = SCG_ERR_OUT_OF_MEMORY;

    if (points != NULL && faces != NULL && raw_indices != NULL && raw_colors != NULL) {
        (*env)->GetFloatArrayRegion(env, model, 0, 16, matrix.m);
        /* `ScgVertexUv2` est exactement sept `float` contigus, ce que les
         * assertions du header vérifient sur cette cible même. */
        (*env)->GetFloatArrayRegion(env, vertices, 0, floats, (jfloat *)points);
        (*env)->GetIntArrayRegion(env, indices, 0, index_count, raw_indices);
        (*env)->GetByteArrayRegion(env, colors, 0, channels, raw_colors);

        for (uint32_t i = 0; i < triangle_count; i++) {
            faces[i].i0 = (uint32_t)raw_indices[i * 3];
            faces[i].i1 = (uint32_t)raw_indices[i * 3 + 1];
            faces[i].i2 = (uint32_t)raw_indices[i * 3 + 2];
            faces[i].r = (uint8_t)raw_colors[i * 4];
            faces[i].g = (uint8_t)raw_colors[i * 4 + 1];
            faces[i].b = (uint8_t)raw_colors[i * 4 + 2];
            faces[i].a = (uint8_t)raw_colors[i * 4 + 3];
        }
        code = scg_submit_lit((ScgContext *)(intptr_t)ctx, &matrix, points, vertex_count,
                              faces, triangle_count, (ScgTexture *)(intptr_t)texture,
                              (ScgTexture *)(intptr_t)lightmap);
    }

    free(points);
    free(faces);
    free(raw_indices);
    free(raw_colors);
    return code;
}

/* scg_set_resolution. */
static jint set_resolution(JNIEnv *env, jclass cls, jlong ctx, jint width, jint height)
{
    (void)env;
    (void)cls;
    return scg_set_resolution((ScgContext *)(intptr_t)ctx, (uint32_t)width, (uint32_t)height);
}

/* scg_set_overbright. */
static jint set_overbright(JNIEnv *env, jclass cls, jlong ctx, jint shift)
{
    (void)env;
    (void)cls;
    return scg_set_overbright((ScgContext *)(intptr_t)ctx, (uint32_t)shift);
}

/*
 * scg_set_fog : les trois canaux arrivent en `jint` parce que Java n'a pas
 * d'octet non signé, et une couleur passée en `jbyte` obligerait chaque
 * appelant à masquer.
 */
static jint set_fog(JNIEnv *env, jclass cls, jlong ctx, jint r, jint g, jint b,
                    jfloat start, jfloat end)
{
    (void)env;
    (void)cls;
    return scg_set_fog((ScgContext *)(intptr_t)ctx, (uint8_t)r, (uint8_t)g, (uint8_t)b,
                       start, end);
}

/* scg_clear_fog. */
static jint clear_fog(JNIEnv *env, jclass cls, jlong ctx)
{
    (void)env;
    (void)cls;
    return scg_clear_fog((ScgContext *)(intptr_t)ctx);
}

/*
 * scg_set_lights : les lumières arrivent en deux tableaux parallèles — quatre
 * flottants de pose par lumière, trois octets de couleur —, parce que Java n'a
 * pas de structure à disposition mémoire garantie. Le pont reconstitue les
 * `ScgLight`, champ réservé compris.
 */
static jint set_lights(JNIEnv *env, jclass cls, jlong ctx, jfloatArray poses,
                       jbyteArray colors)
{
    (void)cls;
    if (poses == NULL || colors == NULL) {
        return SCG_ERR_NULL;
    }

    jsize floats = (*env)->GetArrayLength(env, poses);
    jsize channels = (*env)->GetArrayLength(env, colors);
    if (floats % 4 != 0 || channels != floats / 4 * 3) {
        return SCG_ERR_INVALID_ARGUMENT;
    }

    uint32_t count = (uint32_t)(floats / 4);
    ScgLight *lights = calloc(count ? count : 1, sizeof *lights);
    jfloat *raw_poses = calloc(floats ? (size_t)floats : 1, sizeof *raw_poses);
    jbyte *raw_colors = calloc(channels ? (size_t)channels : 1, sizeof *raw_colors);
    int32_t code = SCG_ERR_OUT_OF_MEMORY;

    if (lights != NULL && raw_poses != NULL && raw_colors != NULL) {
        (*env)->GetFloatArrayRegion(env, poses, 0, floats, raw_poses);
        (*env)->GetByteArrayRegion(env, colors, 0, channels, raw_colors);
        for (uint32_t i = 0; i < count; i++) {
            lights[i].x = raw_poses[i * 4];
            lights[i].y = raw_poses[i * 4 + 1];
            lights[i].z = raw_poses[i * 4 + 2];
            lights[i].radius = raw_poses[i * 4 + 3];
            lights[i].r = (uint8_t)raw_colors[i * 3];
            lights[i].g = (uint8_t)raw_colors[i * 3 + 1];
            lights[i].b = (uint8_t)raw_colors[i * 3 + 2];
            /* `calloc` l'a déjà mis à zéro ; écrit quand même, pour que le
             * jour où ce tableau viendrait d'ailleurs, la clause tienne. */
            lights[i]._reserved = 0;
        }
        code = scg_set_lights((ScgContext *)(intptr_t)ctx, lights, count);
    }

    free(lights);
    free(raw_poses);
    free(raw_colors);
    return code;
}

/* Les méthodes `native` de la classe, avec leur signature JNI. */
static const JNINativeMethod METHODS[] = {
    {"abiVersion", "()I", (void *)abi_version},
    {"create", "([I[J)I", (void *)create},
    {"destroy", "(J)V", (void *)destroy},
    {"submit", "(J[F[F[I[B)I", (void *)submit},
    {"frameEnd", "(JLjava/nio/ByteBuffer;II)I", (void *)frame_end},
    {"frameEndBitmap", "(JLandroid/graphics/Bitmap;)I", (void *)frame_end_bitmap},
    {"lastError", "(J)Ljava/lang/String;", (void *)last_error},
    {"bufferAlloc", "(J)J", (void *)buffer_alloc},
    {"bufferFree", "(JJ)V", (void *)buffer_free},
    {"textureLoad", "(II[B)J", (void *)texture_load},
    {"textureDestroy", "(J)V", (void *)texture_destroy},
    {"submitTextured", "(J[F[F[I[BJ)I", (void *)submit_textured},
    {"submitLit", "(J[F[F[I[BJJ)I", (void *)submit_lit},
    {"setResolution", "(JII)I", (void *)set_resolution},
    {"setFilter", "(JI)I", (void *)set_filter},
    {"setOverbright", "(JI)I", (void *)set_overbright},
    {"setFog", "(JIIIFF)I", (void *)set_fog},
    {"clearFog", "(J)I", (void *)clear_fog},
    {"setLights", "(J[F[B)I", (void *)set_lights},
};

/* Enregistre les méthodes ; un échec empêche le chargement de la bibliothèque. */
JNIEXPORT jint JNI_OnLoad(JavaVM *vm, void *reserved)
{
    (void)reserved;
    JNIEnv *env;
    if ((*vm)->GetEnv(vm, (void **)&env, JNI_VERSION_1_6) != JNI_OK) {
        return JNI_ERR;
    }
    jclass cls = (*env)->FindClass(env, CLASS);
    if (cls == NULL) {
        return JNI_ERR;
    }
    if ((*env)->RegisterNatives(env, cls, METHODS, sizeof METHODS / sizeof METHODS[0]) != JNI_OK) {
        return JNI_ERR;
    }
    return JNI_VERSION_1_6;
}
