// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

/*
 * Couche JNI de l'hôte Android : conversion de types, rien d'autre.
 *
 * Bibliothèque séparée, liée dynamiquement à libscreengine_ffi.so : c'est la
 * bibliothèque publiée qu'on charge, pas une copie liée ici. Seul JNI_OnLoad est
 * exporté ; les méthodes s'enregistrent par RegisterNatives, pour qu'un nom ou
 * une signature fausse fasse échouer le chargement au lieu du premier appel.
 */

#include "screengine.h"

#include <android/bitmap.h>
#include <jni.h>
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
        fields.reserved0 = (uint32_t)values[5];
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
