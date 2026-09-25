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
 * L'image entière dans un Bitmap : début, chaque tuile, fin, sous un seul
 * verrou.
 *
 * Le découpage appartient au moteur et l'appel de chaque tuile à l'hôte ; la
 * boucle est ici plutôt que côté Java parce qu'une image en compte une
 * cinquantaine, et qu'appeler chacune depuis Java verrouillerait et
 * déverrouillerait le bitmap autant de fois. Un hôte qui répartirait les tuiles
 * sur ses threads remplacerait cette boucle, et rien d'autre.
 *
 * Le verrou est pris avant scg_frame_begin : un échec entre les deux laisserait
 * l'image ouverte, et le contexte refuserait tout jusqu'à la fin des temps.
 */
static jint frame_bitmap(JNIEnv *env, jclass cls, jlong ctx, jobject bitmap)
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

    ScgContext *handle = (ScgContext *)(intptr_t)ctx;
    uint32_t stride = info.stride / 4;
    uint32_t tiles = 0;
    int32_t code = scg_frame_begin(handle, &tiles);
    for (uint32_t i = 0; code == SCG_OK && i < tiles; i++) {
        code = scg_frame_tile(handle, i, pixels, stride);
    }
    if (code == SCG_OK) {
        code = scg_frame_end(handle, pixels, stride);
    }
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
/*
 * scg_mesh_load, le bloc recopié depuis le tableau Java.
 *
 * Le fichier vient du système de fichiers de l'appareil, lu côté Java : le
 * moteur n'ouvre rien, et c'est ce que ce chemin éprouve à travers le pont.
 */
static jlong mesh_load(JNIEnv *env, jclass cls, jbyteArray bytes)
{
    (void)cls;
    if (bytes == NULL) {
        return 0;
    }

    jsize len = (*env)->GetArrayLength(env, bytes);
    uint8_t *block = calloc(len ? (size_t)len : 1, 1);
    ScgMesh *mesh = NULL;
    if (block != NULL) {
        (*env)->GetByteArrayRegion(env, bytes, 0, len, (jbyte *)block);
        if (scg_mesh_load(block, (size_t)len, &mesh) != SCG_OK) {
            mesh = NULL;
        }
    }
    free(block);
    return (jlong)(intptr_t)mesh;
}

/* scg_mesh_destroy. Un handle nul ne fait rien, comme `free`. */
static void mesh_destroy(JNIEnv *env, jclass cls, jlong mesh)
{
    (void)env;
    (void)cls;
    scg_mesh_destroy((ScgMesh *)(intptr_t)mesh);
}

/* scg_mesh_triangle_count, rendu en valeur plutôt que par paramètre de sortie :
 * Java n'a pas de pointeur, et une erreur y rend -1. */
static jint mesh_triangle_count(JNIEnv *env, jclass cls, jlong mesh)
{
    (void)env;
    (void)cls;
    uint32_t count = 0;
    if (scg_mesh_triangle_count((const ScgMesh *)(intptr_t)mesh, &count) != SCG_OK) {
        return -1;
    }
    return (jint)count;
}

/* scg_mesh_texture_count, même convention. */
static jint mesh_texture_count(JNIEnv *env, jclass cls, jlong mesh)
{
    (void)env;
    (void)cls;
    uint32_t count = 0;
    if (scg_mesh_texture_count((const ScgMesh *)(intptr_t)mesh, &count) != SCG_OK) {
        return -1;
    }
    return (jint)count;
}

/*
 * scg_mesh_texture_name, la lecture en deux temps faite ici.
 *
 * C'est la couche C qui mesure puis remplit, parce que c'est elle qui connaît
 * le protocole ; Java reçoit une chaîne, ou nul. Le tampon vit sur le tas le
 * temps de l'appel : un nom d'emplacement tient en quelques octets, et une
 * borne écrite ici serait un plafond de plus à tenir.
 */
static jstring mesh_texture_name(JNIEnv *env, jclass cls, jlong mesh, jint slot)
{
    (void)cls;
    const ScgMesh *handle = (const ScgMesh *)(intptr_t)mesh;
    size_t len = 0;
    if (scg_mesh_texture_name(handle, (uint32_t)slot, NULL, 0, &len) != SCG_OK) {
        return NULL;
    }
    char *buffer = calloc(len + 1, 1);
    if (buffer == NULL) {
        return NULL;
    }
    jstring name = NULL;
    if (scg_mesh_texture_name(handle, (uint32_t)slot, buffer, len + 1, &len) == SCG_OK) {
        name = (*env)->NewStringUTF(env, buffer);
    }
    free(buffer);
    return name;
}

/*
 * scg_submit_mesh, le tableau d'emplacements recomposé depuis des `long`.
 *
 * Un zéro y vaut « sans texture », comme un pointeur nul de l'autre côté : Java
 * n'a pas de pointeur nul typé, et un tableau de handles est ce qui traverse le
 * plus simplement.
 */
static jint submit_mesh(JNIEnv *env, jclass cls, jlong ctx, jfloatArray model, jlong mesh,
                        jlongArray textures)
{
    (void)cls;
    if (model == NULL || textures == NULL) {
        return SCG_ERR_NULL;
    }
    if ((*env)->GetArrayLength(env, model) != 16) {
        return SCG_ERR_INVALID_ARGUMENT;
    }

    jsize count = (*env)->GetArrayLength(env, textures);
    jlong *handles = calloc(count ? (size_t)count : 1, sizeof *handles);
    const ScgTexture **slots = calloc(count ? (size_t)count : 1, sizeof *slots);
    int32_t code = SCG_ERR_OUT_OF_MEMORY;

    if (handles != NULL && slots != NULL) {
        ScgMat4 matrix;
        (*env)->GetFloatArrayRegion(env, model, 0, 16, matrix.m);
        (*env)->GetLongArrayRegion(env, textures, 0, count, handles);
        for (jsize i = 0; i < count; i++) {
            slots[i] = (const ScgTexture *)(intptr_t)handles[i];
        }
        code = scg_submit_mesh((ScgContext *)(intptr_t)ctx, &matrix,
                               (const ScgMesh *)(intptr_t)mesh, slots, (uint32_t)count);
    }
    free(handles);
    free(slots);
    return code;
}

/* scg_world_load, le bloc recopié comme celui d'un maillage. */
static jlong world_load(JNIEnv *env, jclass cls, jbyteArray bytes)
{
    (void)cls;
    if (bytes == NULL) {
        return 0;
    }

    jsize len = (*env)->GetArrayLength(env, bytes);
    uint8_t *block = calloc(len ? (size_t)len : 1, 1);
    ScgWorld *world = NULL;
    if (block != NULL) {
        (*env)->GetByteArrayRegion(env, bytes, 0, len, (jbyte *)block);
        if (scg_world_load(block, (size_t)len, &world) != SCG_OK) {
            world = NULL;
        }
    }
    free(block);
    return (jlong)(intptr_t)world;
}

/* scg_world_destroy. */
static void world_destroy(JNIEnv *env, jclass cls, jlong world)
{
    (void)env;
    (void)cls;
    scg_world_destroy((ScgWorld *)(intptr_t)world);
}

/* scg_world_material_count, rendu en valeur, -1 sur refus. */
static jint world_material_count(JNIEnv *env, jclass cls, jlong world)
{
    (void)env;
    (void)cls;
    uint32_t count = 0;
    if (scg_world_material_count((const ScgWorld *)(intptr_t)world, &count) != SCG_OK) {
        return -1;
    }
    return (jint)count;
}

/*
 * scg_world_material_name, même lecture en deux temps que les emplacements d'un
 * maillage. Deuxième occurrence du protocole dans ce fichier : à la troisième,
 * elle s'extrait en un utilitaire qui prendrait la fonction de mesure.
 */
static jstring world_material_name(JNIEnv *env, jclass cls, jlong world, jint rank)
{
    (void)cls;
    const ScgWorld *handle = (const ScgWorld *)(intptr_t)world;
    size_t len = 0;
    if (scg_world_material_name(handle, (uint32_t)rank, NULL, 0, &len) != SCG_OK) {
        return NULL;
    }
    char *buffer = calloc(len + 1, 1);
    if (buffer == NULL) {
        return NULL;
    }
    jstring name = NULL;
    if (scg_world_material_name(handle, (uint32_t)rank, buffer, len + 1, &len) == SCG_OK) {
        name = (*env)->NewStringUTF(env, buffer);
    }
    free(buffer);
    return name;
}

/* scg_submit_world, les textures recomposées comme celles d'un maillage. */
static jint submit_world(JNIEnv *env, jclass cls, jlong ctx, jfloatArray model, jlong world,
                         jlongArray textures)
{
    (void)cls;
    if (model == NULL || textures == NULL) {
        return SCG_ERR_NULL;
    }
    if ((*env)->GetArrayLength(env, model) != 16) {
        return SCG_ERR_INVALID_ARGUMENT;
    }

    jsize count = (*env)->GetArrayLength(env, textures);
    jlong *handles = calloc(count ? (size_t)count : 1, sizeof *handles);
    const ScgTexture **slots = calloc(count ? (size_t)count : 1, sizeof *slots);
    int32_t code = SCG_ERR_OUT_OF_MEMORY;

    if (handles != NULL && slots != NULL) {
        ScgMat4 matrix;
        (*env)->GetFloatArrayRegion(env, model, 0, 16, matrix.m);
        (*env)->GetLongArrayRegion(env, textures, 0, count, handles);
        for (jsize i = 0; i < count; i++) {
            slots[i] = (const ScgTexture *)(intptr_t)handles[i];
        }
        code = scg_submit_world((ScgContext *)(intptr_t)ctx, &matrix,
                                (const ScgWorld *)(intptr_t)world, slots, (uint32_t)count);
    }
    free(handles);
    free(slots);
    return code;
}

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

/*
 * scg_set_grade : la courbe arrive en sept `float` et deux réservés, plutôt
 * qu'en tableau, pour que Java n'ait pas à reproduire la disposition de la
 * structure — c'est ici qu'elle se remplit, au seul endroit qui voit le header.
 *
 * Les réservés traversent quand même : sans eux, cet hôte ne pourrait pas
 * vérifier qu'un réservé non nul est refusé, et la promesse d'extension ne
 * serait éprouvée nulle part côté Java.
 */
static jint set_grade(JNIEnv *env, jclass cls, jlong ctx, jfloatArray values, jint reserved)
{
    (void)cls;
    jfloat raw[7];
    if ((*env)->GetArrayLength(env, values) != 7) {
        return SCG_ERR_INVALID_ARGUMENT;
    }
    (*env)->GetFloatArrayRegion(env, values, 0, 7, raw);

    ScgGrade grade;
    memset(&grade, 0, sizeof grade);
    grade.gamma = raw[0];
    grade.gain_r = raw[1];
    grade.gain_g = raw[2];
    grade.gain_b = raw[3];
    grade.offset_r = raw[4];
    grade.offset_g = raw[5];
    grade.offset_b = raw[6];
    grade.reserved1 = (uint32_t)reserved;

    return scg_set_grade((ScgContext *)(intptr_t)ctx, &grade);
}

/* scg_set_camera : neuf `float` — trois de position, quatre d'orientation, le
 * champ de vision et le plan proche —, pour la raison qui vaut pour la courbe.
 * C'est ici que la structure se remplit, et Java n'a pas à connaître l'ordre
 * `x, y, z, w` du quaternion, qui est l'inverse de la convention la plus
 * répandue.
 *
 * Un tableau nul vaut une caméra nulle : c'est ainsi que l'hôte éprouve le
 * refus par SCG_ERR_NULL, qu'aucun appel Java ne produirait autrement. */
static jint set_camera(JNIEnv *env, jclass cls, jlong ctx, jfloatArray values)
{
    (void)cls;
    if (values == NULL) {
        return scg_set_camera((ScgContext *)(intptr_t)ctx, NULL);
    }

    jfloat raw[9];
    if ((*env)->GetArrayLength(env, values) != 9) {
        return SCG_ERR_INVALID_ARGUMENT;
    }
    (*env)->GetFloatArrayRegion(env, values, 0, 9, raw);

    ScgCamera camera;
    memset(&camera, 0, sizeof camera);
    camera.position[0] = raw[0];
    camera.position[1] = raw[1];
    camera.position[2] = raw[2];
    camera.orientation[0] = raw[3];
    camera.orientation[1] = raw[4];
    camera.orientation[2] = raw[5];
    camera.orientation[3] = raw[6];
    camera.fov_y = raw[7];
    camera.near_plane = raw[8];

    return scg_set_camera((ScgContext *)(intptr_t)ctx, &camera);
}

/* scg_clear_grade. */
static jint clear_grade(JNIEnv *env, jclass cls, jlong ctx)
{
    (void)env;
    (void)cls;
    return scg_clear_grade((ScgContext *)(intptr_t)ctx);
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
    {"frameBitmap", "(JLandroid/graphics/Bitmap;)I", (void *)frame_bitmap},
    {"lastError", "(J)Ljava/lang/String;", (void *)last_error},
    {"bufferAlloc", "(J)J", (void *)buffer_alloc},
    {"bufferFree", "(JJ)V", (void *)buffer_free},
    {"textureLoad", "(II[B)J", (void *)texture_load},
    {"textureDestroy", "(J)V", (void *)texture_destroy},
    {"meshLoad", "([B)J", (void *)mesh_load},
    {"meshDestroy", "(J)V", (void *)mesh_destroy},
    {"meshTriangleCount", "(J)I", (void *)mesh_triangle_count},
    {"meshTextureCount", "(J)I", (void *)mesh_texture_count},
    {"meshTextureName", "(JI)Ljava/lang/String;", (void *)mesh_texture_name},
    {"submitMesh", "(J[FJ[J)I", (void *)submit_mesh},
    {"worldLoad", "([B)J", (void *)world_load},
    {"worldDestroy", "(J)V", (void *)world_destroy},
    {"worldMaterialCount", "(J)I", (void *)world_material_count},
    {"worldMaterialName", "(JI)Ljava/lang/String;", (void *)world_material_name},
    {"submitWorld", "(J[FJ[J)I", (void *)submit_world},
    {"submitTextured", "(J[F[F[I[BJ)I", (void *)submit_textured},
    {"submitLit", "(J[F[F[I[BJJ)I", (void *)submit_lit},
    {"setResolution", "(JII)I", (void *)set_resolution},
    {"setFilter", "(JI)I", (void *)set_filter},
    {"setOverbright", "(JI)I", (void *)set_overbright},
    {"setFog", "(JIIIFF)I", (void *)set_fog},
    {"clearFog", "(J)I", (void *)clear_fog},
    {"setGrade", "(J[FI)I", (void *)set_grade},
    {"setCamera", "(J[F)I", (void *)set_camera},
    {"clearGrade", "(J)I", (void *)clear_grade},
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
