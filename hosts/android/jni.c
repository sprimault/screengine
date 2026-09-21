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
