// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

// Les quatre scènes qui chargent un fichier : la caisse, la composite de
// l'étape 6, le décor à quatre cellules et les balayages.

package main

/*
#include "screengine.h"
*/
import "C"

import (
	"encoding/binary"
	"math"
	"unsafe"
)

// Le côté de l'emblème et de la tache de la scène composite.
const emblemSide = 64

// La matrice qui place la caisse, recopiée de la suite de conformance,
// coefficient pour coefficient.
//
// Elle y est écrite en littéraux pour cette raison précise : les tables
// trigonométriques du moteur ne traversent pas l'ABI, et un hôte qui
// recalculerait ces valeurs avec sa propre bibliothèque mathématique n'obtiendrait
// pas les mêmes bits, donc pas la même empreinte.
var crateModel = C.ScgMat4{m: [16]C.float{
	0.64, 0.48, 0.6, 0.0,
	-0.6, 0.8, 0.0, 0.0,
	-0.48, -0.36, 0.8, 0.0,
	5.0, 0.0, 0.0, 1.0,
}}

// Les deux lumières de la scène composite.
var compositeLights = [2]C.ScgLight{
	{x: 2.0, y: -3.0, z: 1.5, radius: 24.0, r: 0xFF, g: 0xC0, b: 0x60},
	{x: 2.0, y: 3.0, z: 3.0, radius: 24.0, r: 0x40, g: 0x80, b: 0xFF},
}

// Le sol de la composite, à huit texels par unité de monde.
var compositeFloor = [4]C.ScgVertexUv{
	{x: 4.0, y: -6.0, z: -2.6, u: 4.0 * 8.0, v: -6.0 * 8.0},
	{x: 14.0, y: -6.0, z: -2.6, u: 14.0 * 8.0, v: -6.0 * 8.0},
	{x: 14.0, y: 6.0, z: -2.6, u: 14.0 * 8.0, v: 6.0 * 8.0},
	{x: 4.0, y: 6.0, z: -2.6, u: 4.0 * 8.0, v: 6.0 * 8.0},
}

// La tache modulée, coplanaire au sol.
var compositeShadow = [4]C.ScgVertexUv{
	{x: 4.5, y: -5.5, z: -2.6, u: 0.0, v: 0.0},
	{x: 9.5, y: -5.5, z: -2.6, u: 64.0, v: 0.0},
	{x: 9.5, y: -0.5, z: -2.6, u: 64.0, v: 64.0},
	{x: 4.5, y: -0.5, z: -2.6, u: 0.0, v: 64.0},
}

// L'emblème masqué : un disque et son pied, sur fond transparent.
//
// Mêmes valeurs que la scène de conformance, écrites ici plutôt que chargées :
// ce que cet hôte doit reproduire est la disposition des structures, pas une
// texture qui viendrait d'ailleurs.
func makeEmblem() []byte {
	const cx = float32(emblemSide) / 2.0
	const cy = float32(emblemSide) * 0.35
	const radius = float32(emblemSide) * 0.28

	texels := make([]byte, emblemSide*emblemSide*4)
	for v := 0; v < emblemSide; v++ {
		for u := 0; u < emblemSide; u++ {
			texel := texels[(v*emblemSide+u)*4:]
			fu := float32(u) + 0.5
			fv := float32(v) + 0.5
			dx := fu - cx
			dy := fv - cy
			disc := dx*dx+dy*dy <= radius*radius
			foot := fv > float32(emblemSide)*0.6 && fu > float32(emblemSide)*0.28 &&
				fu < float32(emblemSide)*0.52
			if disc || foot {
				texel[0] = byte(0x40 + int(fu*160.0/float32(emblemSide)))
				texel[1] = byte(0xFF - int(fv*140.0/float32(emblemSide)))
				texel[2] = 0x60
				texel[3] = 0xFF
			}
		}
	}
	return texels
}

// La tache d'ombre : sombre au centre, blanche au bord, 255 étant le neutre de
// la modulation.
func makeShadow() []byte {
	const half = float32(emblemSide) / 2.0

	texels := make([]byte, emblemSide*emblemSide*4)
	for v := 0; v < emblemSide; v++ {
		for u := 0; u < emblemSide; u++ {
			texel := texels[(v*emblemSide+u)*4:]
			dx := float32(u) + 0.5 - half
			dy := float32(v) + 0.5 - half
			q := (dx*dx + dy*dy) / (half * half)
			if q > 1.0 {
				q = 1.0
			}
			level := byte(0x30 + int(float32(0xFF-0x30)*q))
			texel[0], texel[1], texel[2], texel[3] = level, level, level, 0xFF
		}
	}
	return texels
}

// Charge une texture dans un format donné.
func loadTextureFormat(texels []byte, side uint32, format C.uint32_t) *C.ScgTexture {
	var desc C.ScgTextureDesc
	desc.width = C.uint32_t(side)
	desc.height = C.uint32_t(side)
	desc.format = format

	block := toNative(texels)
	defer release(block)

	var texture *C.ScgTexture
	if C.scg_texture_load(&desc, (*C.uint8_t)(block), C.size_t(len(texels)), &texture) < 0 {
		return nil
	}
	return texture
}

// Charge un maillage depuis des octets Go, recopiés hors du tas le temps de
// l'appel.
//
// Le bloc est rendu avant toute soumission : le moteur copie ce qu'il garde, et
// un hôte qui devrait le conserver l'apprendrait ici.
func loadMesh(bytes []byte) *C.ScgMesh {
	block := toNative(bytes)
	defer release(block)

	var mesh *C.ScgMesh
	if C.scg_mesh_load((*C.uint8_t)(block), C.size_t(len(bytes)), &mesh) != C.SCG_OK {
		return nil
	}
	return mesh
}

// Lit un nom en deux temps — mesure, puis remplissage — comme l'ABI l'impose.
//
// Un hôte qui devinerait la longueur se tromperait le jour où elle change.
func readName(read func(buf *C.char, cap C.size_t, need *C.size_t) C.int32_t) (string, bool) {
	var needed C.size_t
	if read(nil, 0, &needed) != C.SCG_OK {
		return "", false
	}
	buf := alloc(int(needed) + 1)
	defer release(buf)
	if read((*C.char)(buf), C.size_t(needed)+1, &needed) != C.SCG_OK {
		return "", false
	}
	return string(view(buf, int(needed))), true
}

// Rend la caisse du fichier de maillage et hache son image.
func renderMesh(path string) (uint64, bool) {
	bytes := readFile(path)
	if bytes == nil {
		return 0, false
	}
	config := sceneConfig()
	frame, pixels := newFrame()
	defer release(frame)

	mesh := loadMesh(bytes)
	check(mesh != nil, "le fichier de maillage se charge")
	sides := loadTexture(makeChecker(floorSide, floorCell), floorSide)
	check(sides != nil, "le damier des faces se charge")

	var ctx *C.ScgContext
	check(C.scg_create(&config, &ctx) == C.SCG_OK, "création du contexte du maillage")
	if ctx == nil || mesh == nil || sides == nil {
		C.scg_mesh_destroy(mesh)
		C.scg_texture_destroy(sides)
		return 0, false
	}
	defer C.scg_destroy(ctx)

	var triangles, slots C.uint32_t
	check(C.scg_mesh_triangle_count(mesh, &triangles) == C.SCG_OK && triangles == 12,
		"le maillage porte douze triangles")
	check(C.scg_mesh_texture_count(mesh, &slots) == C.SCG_OK && slots == 2,
		"le maillage réclame deux emplacements")

	name, named := readName(func(buf *C.char, size C.size_t, need *C.size_t) C.int32_t {
		return C.scg_mesh_texture_name(mesh, 0, buf, size, need)
	})
	check(named && name == "cote", "le premier emplacement s'appelle cote")

	var needed C.size_t
	check(C.scg_mesh_texture_name(mesh, slots, nil, 0, &needed) == C.SCG_ERR_INVALID_ARGUMENT,
		"un emplacement au-delà du dernier est refusé")

	bound := [2]*C.ScgTexture{sides, nil}
	check(C.scg_submit_mesh(ctx, &crateModel, mesh, &bound[0], 1) == C.SCG_ERR_INVALID_ARGUMENT,
		"un compte d'emplacements faux est refusé")

	code := C.scg_submit_mesh(ctx, &crateModel, mesh, &bound[0], 2)
	check(code == C.SCG_OK, "la caisse est acceptée")

	// Détruit avant le rendu : plus rien ne lit un maillage une fois la
	// soumission revenue — ce qui n'est pas la raison qui protège une texture,
	// et le header le dit.
	C.scg_mesh_destroy(mesh)
	C.scg_texture_destroy(sides)

	code = C.scg_frame_end(ctx, (*C.uint8_t)(frame), stride)
	check(code == C.SCG_OK, "l'image de la caisse se rend")
	return fingerprint(pixels, width, height, stride), code == C.SCG_OK
}

// Rend la scène composite de l'étape 6.
//
// Les cinq chemins de l'étape dans une seule image : maillage entre deux trames,
// texture masquée, les deux modes d'orientation de sprite, le roulis, et la
// surface modulée.
func renderComposite(path string) (uint64, bool) {
	bytes := readFile(path)
	if bytes == nil {
		return 0, false
	}
	config := sceneConfig()
	frame, pixels := newFrame()
	defer release(frame)

	mesh := loadMesh(bytes)
	check(mesh != nil, "le maillage de la composite se charge")
	sides := loadTexture(makeChecker(floorSide, floorCell), floorSide)
	check(sides != nil, "le damier de la composite se charge")
	// Le format masqué se déclare au chargement, jamais au dessin : c'est là que
	// la chaîne de mipmaps se construit.
	emblem := loadTextureFormat(makeEmblem(), emblemSide, C.SCG_TEXTURE_FORMAT_RGBA8_MASKED)
	check(emblem != nil, "l'emblème masqué se charge")
	shadow := loadTextureFormat(makeShadow(), emblemSide, C.SCG_TEXTURE_FORMAT_RGBA8)
	check(shadow != nil, "la tache se charge")

	var ctx *C.ScgContext
	check(C.scg_create(&config, &ctx) == C.SCG_OK, "création du contexte de la composite")
	if ctx == nil || mesh == nil || sides == nil || emblem == nil || shadow == nil {
		C.scg_mesh_destroy(mesh)
		C.scg_texture_destroy(sides)
		C.scg_texture_destroy(emblem)
		C.scg_texture_destroy(shadow)
		return 0, false
	}
	defer C.scg_destroy(ctx)

	var frames C.uint32_t
	check(C.scg_mesh_frame_count(mesh, &frames) == C.SCG_OK && frames == 2,
		"le maillage versionné porte deux trames")

	// La caméra plonge d'un seizième de tour : le quaternion se range x, y, z, w,
	// un demi-angle sur l'axe et le cosinus en dernier.
	var camera C.ScgCamera
	camera.orientation[1] = 0.19509032
	camera.orientation[3] = 0.98078528
	camera.position[2] = 3.0
	camera.fov_y = 1.0471976
	camera.near_plane = 0.1
	check(C.scg_set_camera(ctx, &camera) == C.SCG_OK, "la caméra plongeante se règle")
	check(C.scg_set_lights(ctx, &compositeLights[0], 2) == C.SCG_OK, "les deux lumières se règlent")

	// Le sol d'abord : une surface modulée multiplie ce qui est déjà écrit, et
	// n'aurait rien à assombrir sans lui.
	check(C.scg_submit_textured(ctx, &identity, &compositeFloor[0], 4, &floorTriangles[0], 2, sides) == C.SCG_OK,
		"le sol de la composite est accepté")

	bound := [2]*C.ScgTexture{sides, nil}
	check(C.scg_submit_mesh_frame(ctx, &crateModel, mesh, &bound[0], 2, 0, 1, 0.35) == C.SCG_OK,
		"la caisse interpolée est acceptée")

	// Les deux modes d'orientation, le second avec un roulis non nul : un lot ne
	// porte qu'une orientation, donc deux soumissions.
	var sprite C.ScgSprite
	sprite.x = 9.0
	sprite.y = -3.5
	sprite.z = 0.0
	sprite.half_width = 2.0
	sprite.half_height = 2.4
	sprite.u1 = emblemSide
	sprite.v1 = emblemSide
	sprite.r = 0xFF
	sprite.g = 0xFF
	sprite.b = 0xFF
	sprite.a = 0xFF
	check(C.scg_submit_sprites(ctx, &identity, &sprite, 1, emblem, C.SCG_SPRITE_AXIAL) == C.SCG_OK,
		"le sprite axial est accepté")
	check(C.scg_submit_sprites(ctx, &identity, &sprite, 1, emblem, 0) == C.SCG_ERR_INVALID_ARGUMENT,
		"une orientation nulle est refusée, jamais rabattue sur un défaut")

	sprite.y = 3.5
	// Cinq huitièmes de tour, en angle binaire.
	sprite.roll = 5 * 0x20000000
	check(C.scg_submit_sprites(ctx, &identity, &sprite, 1, emblem, C.SCG_SPRITE_FACING) == C.SCG_OK,
		"le sprite plein face et son roulis sont acceptés")

	// La tache en dernier : elle multiplie ce que les lots précédents ont écrit,
	// donc l'ordre de soumission décide.
	check(C.scg_submit_blended(ctx, &identity, &compositeShadow[0], 4, &floorTriangles[0], 2, shadow, 0) == C.SCG_ERR_INVALID_ARGUMENT,
		"un mode de mélange nul est refusé")
	check(C.scg_submit_blended(ctx, &identity, &compositeShadow[0], 4, &floorTriangles[0], 2, shadow, C.SCG_BLEND_MODULATE) == C.SCG_OK,
		"la tache modulée est acceptée")

	C.scg_mesh_destroy(mesh)
	C.scg_texture_destroy(sides)
	C.scg_texture_destroy(emblem)
	C.scg_texture_destroy(shadow)

	code := C.scg_frame_end(ctx, (*C.uint8_t)(frame), stride)
	check(code == C.SCG_OK, "l'image composite se rend")
	return fingerprint(pixels, width, height, stride), code == C.SCG_OK
}

// Rend le décor chargé d'un fichier, cuit cellule par cellule et parcouru par sa
// traversée.
//
// C'est le seul endroit où cet hôte lit un second fichier, et où il rend une
// image dont la géométrie ne vient pas de lui.
func renderRooms(path string) (uint64, bool) {
	// Les deux damiers de la scène de référence : le mur est plus fin que le
	// sol, et c'est le nom du matériau qui décide lequel va où.
	const wallSide, wallCell = 512, 128
	const roomFloorSide, roomFloorCell = 256, 32

	bytes := readFile(path)
	if bytes == nil {
		return 0, false
	}
	config := sceneConfig()
	frame, pixels := newFrame()
	defer release(frame)

	block := toNative(bytes)
	var world *C.ScgWorld
	loaded := C.scg_world_load((*C.uint8_t)(block), C.size_t(len(bytes)), &world) == C.SCG_OK
	// Rendu tout de suite : le moteur copie ce qu'il garde, et un hôte qui
	// devrait conserver le bloc l'apprendrait ici.
	release(block)
	check(loaded, "le fichier de carte se charge")
	if !loaded {
		return 0, false
	}
	defer C.scg_world_destroy(world)

	var materials C.uint32_t
	check(C.scg_world_material_count(world, &materials) == C.SCG_OK && materials == 2,
		"la carte déclare deux matériaux")

	slots := make([]*C.ScgTexture, materials)
	for i := C.uint32_t(0); i < materials; i++ {
		rank := i
		name, named := readName(func(buf *C.char, size C.size_t, need *C.size_t) C.int32_t {
			return C.scg_world_material_name(world, rank, buf, size, need)
		})
		check(named, "le nom du matériau se lit en deux temps")
		if name == "mur" {
			slots[i] = loadTexture(makeChecker(wallSide, wallCell), wallSide)
		} else {
			slots[i] = loadTexture(makeChecker(roomFloorSide, roomFloorCell), roomFloorSide)
		}
		check(slots[i] != nil, "le damier du matériau se charge")
	}
	defer func() {
		for _, texture := range slots {
			C.scg_texture_destroy(texture)
		}
	}()

	var lighting *C.ScgLighting
	check(C.scg_lighting_create(world, &lighting) == C.SCG_OK, "le porteur de lightmaps se crée")
	if lighting == nil {
		return 0, false
	}
	defer C.scg_lighting_destroy(lighting)

	// **Toutes les cellules, pas seulement celles que la vue montre** : une
	// lightmap est un cache de la carte et non du point de vue.
	var cells C.uint32_t
	check(C.scg_world_cell_count(world, &cells) == C.SCG_OK && cells == 4,
		"la carte porte quatre cellules")
	for i := C.uint32_t(0); i < cells; i++ {
		var id, luxels C.uint32_t
		check(C.scg_world_cell_id(world, i, &id) == C.SCG_OK, "le rang rend un identifiant")
		// Ce qu'un hôte lit avant de cuire, pour pondérer sa progression : le
		// compte de cellules ne dit rien du coût de chacune.
		check(C.scg_world_cell_luxel_count(world, id, &luxels) == C.SCG_OK && luxels > 0,
			"la cellule annonce ses luxels")
		check(C.scg_lighting_build(lighting, id) == C.SCG_OK, "la cellule se cuit")
	}

	var ctx *C.ScgContext
	check(C.scg_create(&config, &ctx) == C.SCG_OK, "création du contexte du décor")
	if ctx == nil {
		return 0, false
	}
	defer C.scg_destroy(ctx)

	position := alloc(3 * 4)
	defer release(position)
	point := writeVec3(position, 2.0, 2.0, 2.0)

	var camera C.ScgCamera
	camera.position[0] = 2.0
	camera.position[1] = 2.0
	camera.position[2] = 2.0
	// Sans rotation : la vue regarde le +X du monde, et le quaternion identité
	// range sa partie réelle en dernier.
	camera.orientation[3] = 1.0
	camera.fov_y = 1.0471976
	camera.near_plane = 0.1
	check(C.scg_set_camera(ctx, &camera) == C.SCG_OK, "la caméra du décor se règle")

	// La cellule se trouve, elle ne se devine pas : zéro veut dire « nulle
	// part », ce qui est une clause et non une erreur.
	var cell C.uint32_t
	check(C.scg_world_locate(world, point, &cell) == C.SCG_OK && cell != 0,
		"la caméra est dans une cellule")

	code := C.scg_submit_world_visible(ctx, &identity, world, &slots[0], materials, lighting, cell)
	check(code == C.SCG_OK, "la traversée accepte le décor")

	code = C.scg_frame_end(ctx, (*C.uint8_t)(frame), stride)
	check(code == C.SCG_OK, "l'image du décor se rend")
	return fingerprint(pixels, width, height, stride), code == C.SCG_OK
}

// La taille d'un enregistrement de la liste de balayages : neuf flottants.
const sweepRecord = 36

// Rejoue les balayages du fichier versionné et hache leurs résultats.
//
// **La seule scène sans image, et la seule sans contexte** : le module de
// collision n'en demande pas, ce qui est exactement ce qu'un serveur de jeu en
// attend. La liste vient du dépôt et ne se reconstruit pas ici.
func renderSweeps(worldPath, sweepsPath string) (uint64, bool) {
	worldBytes := readFile(worldPath)
	list := readFile(sweepsPath)
	if worldBytes == nil || list == nil {
		return 0, false
	}

	block := toNative(worldBytes)
	var world *C.ScgWorld
	loaded := C.scg_world_load((*C.uint8_t)(block), C.size_t(len(worldBytes)), &world) == C.SCG_OK
	release(block)
	check(loaded, "le décor de collision se charge")
	if !loaded {
		return 0, false
	}
	defer C.scg_world_destroy(world)

	// La magie avant toute lecture : un mauvais chemin doit échouer ici plutôt
	// que produire une empreinte de bruit.
	if len(list) < 12 || string(list[:8]) != "SCGSWEEP" {
		check(false, "la liste de balayages porte sa magie")
		return 0, false
	}
	count := int(binary.LittleEndian.Uint32(list[8:12]))
	if len(list) != 12+count*sweepRecord {
		check(false, "la liste annonce le nombre de balayages qu'elle porte")
		return 0, false
	}

	vectors := alloc(9 * 4)
	defer release(vectors)
	hit := alloc(44)
	defer release(hit)
	hitView := view(hit, 44)

	digest := make([]byte, 0, count*37)
	for i := 0; i < count; i++ {
		record := list[12+i*sweepRecord:]
		floats := unsafe.Slice((*C.float)(vectors), 9)
		for rank := 0; rank < 9; rank++ {
			bits := binary.LittleEndian.Uint32(record[rank*4:])
			floats[rank] = C.float(math.Float32frombits(bits))
		}
		half := (*C.float)(vectors)
		from := (*C.float)(unsafe.Pointer(uintptr(vectors) + 12))
		to := (*C.float)(unsafe.Pointer(uintptr(vectors) + 24))

		// Zéro veut dire « nulle part », et se passe tel quel : c'est le
		// balayage qui rend le déplacement libre, pas l'hôte qui le fabrique.
		var cell C.uint32_t
		if C.scg_world_locate(world, from, &cell) != C.SCG_OK {
			check(false, "la cellule de départ se cherche")
			return 0, false
		}
		status := C.scg_world_sweep(world, cell, half, from, to, (*C.ScgSweepHit)(hit))
		if status < 0 {
			check(false, "le balayage est accepté")
			return 0, false
		}

		// Les trente-six premiers octets de `ScgSweepHit` sont exactement ceux
		// que l'empreinte veut, dans l'ordre : le header le garantit par ses
		// assertions de décalage, et les deux champs réservés viennent après.
		digest = append(digest, hitView[:36]...)
		digest = append(digest, byte(status))

		// Le contrepoids de l'identifiant de surface, qu'aucune autre fonction
		// ne traduit.
		surface := binary.LittleEndian.Uint32(hitView[28:32])
		if surface != 0 {
			var material C.uint32_t
			check(C.scg_world_surface_material(world, C.uint32_t(surface), &material) == C.SCG_OK,
				"la surface touchée nomme son matériau")
		}
	}

	return hashBytes(digest), true
}
