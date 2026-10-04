// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

// Les scènes qui passent par une texture : le damier qui fuit, sa variante
// bilinéaire, la courbe de sortie, le brouillard, les deux chemins éclairés et
// les lumières dynamiques.

package main

/*
#include "screengine.h"
*/
import "C"

import "unsafe"

// Le sol de la scène `texture` : un damier qui fuit vers l'horizon, à 1,2 unité
// sous la caméra.
const (
	floorSide = 64
	floorCell = 8
	lightSide = 16
)

// Les coordonnées de texture s'écrivent en clair, densité comprise : une
// constante de plus ne dirait rien que le littéral ne dise.
var floorVertices = [4]C.ScgVertexUv{
	{x: 2.0, y: -24.0, z: -1.2, u: 2.0 * 8.0, v: -24.0 * 8.0},
	{x: 60.0, y: -24.0, z: -1.2, u: 60.0 * 8.0, v: -24.0 * 8.0},
	{x: 60.0, y: 24.0, z: -1.2, u: 60.0 * 8.0, v: 24.0 * 8.0},
	{x: 2.0, y: 24.0, z: -1.2, u: 2.0 * 8.0, v: 24.0 * 8.0},
}

// floorTriangles porte le sol, blanc : sa texture et sa lightmap portent seules
// la teinte, et une couleur de triangle les multiplierait.
var floorTriangles = [2]C.ScgTriangle{
	{i0: 0, i1: 1, i2: 2, r: 0xFF, g: 0xFF, b: 0xFF, a: 0xFF},
	{i0: 0, i1: 2, i2: 3, r: 0xFF, g: 0xFF, b: 0xFF, a: 0xFF},
}

// Le sol de la scène `brouillard`, plus long que la rampe : sa moitié lointaine
// se confond avec le fond, sa moitié proche garde son damier.
var fogFloor = [4]C.ScgVertexUv{
	{x: 1.0, y: -20.0, z: -1.2, u: 1.0 * 8.0, v: -20.0 * 8.0},
	{x: 50.0, y: -20.0, z: -1.2, u: 50.0 * 8.0, v: -20.0 * 8.0},
	{x: 50.0, y: 20.0, z: -1.2, u: 50.0 * 8.0, v: 20.0 * 8.0},
	{x: 1.0, y: 20.0, z: -1.2, u: 1.0 * 8.0, v: 20.0 * 8.0},
}

// Les coordonnées de lightmap vont d'un demi-texel à un demi-texel du bord
// opposé : une lightmap ne se pave pas, et le bilinéaire irait chercher son
// voisin par le repli.
var litFloor = [4]C.ScgVertexUv2{
	{x: 2.0, y: -16.0, z: -1.2, u: 2.0 * 8.0, v: -16.0 * 8.0, u2: 0.5, v2: 0.5},
	{x: 40.0, y: -16.0, z: -1.2, u: 40.0 * 8.0, v: -16.0 * 8.0, u2: 15.5, v2: 0.5},
	{x: 40.0, y: 16.0, z: -1.2, u: 40.0 * 8.0, v: 16.0 * 8.0, u2: 15.5, v2: 15.5},
	{x: 2.0, y: 16.0, z: -1.2, u: 2.0 * 8.0, v: 16.0 * 8.0, u2: 0.5, v2: 15.5},
}

// Le mur, sans texture : sa densité est nulle, donc ses coordonnées de texture
// aussi, et c'est la couleur du triangle qui tient lieu de texel.
var litWall = [4]C.ScgVertexUv2{
	{x: 40.0, y: -16.0, z: -1.2, u: 0.0, v: 0.0, u2: 0.5, v2: 0.5},
	{x: 40.0, y: -16.0, z: 10.0, u: 0.0, v: 0.0, u2: 15.5, v2: 0.5},
	{x: 40.0, y: 16.0, z: 10.0, u: 0.0, v: 0.0, u2: 15.5, v2: 15.5},
	{x: 40.0, y: 16.0, z: -1.2, u: 0.0, v: 0.0, u2: 0.5, v2: 15.5},
}

// wallTriangles porte le mur, dont la couleur tient lieu de texel : il n'a pas
// de texture, et c'est le cas qu'un lot éclairé doit savoir rendre.
var wallTriangles = [2]C.ScgTriangle{
	{i0: 0, i1: 1, i2: 2, r: 0xC0, g: 0xB0, b: 0x90, a: 0xFF},
	{i0: 0, i1: 2, i2: 3, r: 0xC0, g: 0xB0, b: 0x90, a: 0xFF},
}

// Les trois lumières de la scène `lumieres`, aux mêmes valeurs que la scène de
// conformance. Le champ réservé est nul, comme l'ABI l'exige.
var sceneLights = [3]C.ScgLight{
	{x: 8.0, y: -3.0, z: 1.5, radius: 12.0, r: 0xFF, g: 0x30, b: 0x20},
	{x: 16.0, y: 3.0, z: 1.5, radius: 12.0, r: 0x20, g: 0xFF, b: 0x40},
	{x: 24.0, y: -2.0, z: 2.5, radius: 14.0, r: 0x30, g: 0x50, b: 0xFF},
}

// Écrit le damier procédural, recopié de la suite de conformance teinte pour
// teinte : c'est lui qui décide de l'empreinte, et un liseré décalé d'un texel
// la ferait diverger.
func makeChecker(side, cell uint32) []byte {
	texels := make([]byte, int(side)*int(side)*4)
	for v := uint32(0); v < side; v++ {
		for u := uint32(0); u < side; u++ {
			texel := texels[(v*side+u)*4:]
			edge := u%cell == 0 || v%cell == 0
			dark := (u/cell+v/cell)%2 == 0
			switch {
			case edge:
				texel[0], texel[1], texel[2] = 0xF0, 0xE0, 0xA0
			case dark:
				texel[0], texel[1], texel[2] = 0x30, 0x38, 0x50
			default:
				texel[0], texel[1], texel[2] = 0x90, 0x70, 0x50
			}
			texel[3] = 0xFF
		}
	}
	return texels
}

// Écrit le dégradé de lightmap.
//
// Les deux axes n'y font pas la même chose, et c'est voulu : un dégradé
// symétrique laisserait passer un axe échangé entre les deux jeux de
// coordonnées.
func makeGradient() []byte {
	texels := make([]byte, lightSide*lightSide*4)
	for v := uint32(0); v < lightSide; v++ {
		for u := uint32(0); u < lightSide; u++ {
			texel := texels[(v*lightSide+u)*4:]
			texel[0] = byte(32 + u*223/(lightSide-1))
			texel[1] = byte(32 + ((u+v)/2)*223/(lightSide-1))
			texel[2] = byte(32 + v*223/(lightSide-1))
			texel[3] = 0xFF
		}
	}
	return texels
}

// Charge une texture depuis des texels Go, recopiés hors du tas le temps de
// l'appel.
func loadTexture(texels []byte, side uint32) *C.ScgTexture {
	var desc C.ScgTextureDesc
	desc.width = C.uint32_t(side)
	desc.height = C.uint32_t(side)
	desc.format = C.SCG_TEXTURE_FORMAT_RGBA8

	block := toNative(texels)
	defer release(block)

	var texture *C.ScgTexture
	if C.scg_texture_load(&desc, (*C.uint8_t)(block), C.size_t(len(texels)), &texture) < 0 {
		return nil
	}
	return texture
}

// Un tampon de pixels hors du tas, et la vue qui le lit.
//
// Toutes les scènes d'image en ouvrent un ; le rassembler ici évite de répéter
// l'allocation et sa libération dans chacune.
func newFrame() (unsafe.Pointer, []byte) {
	block := alloc(stride * height * 4)
	return block, view(block, stride*height*4)
}

// Rend la scène texturée sous le filtrage demandé et hache son image.
//
// Plus courte que [renderEdge] : les sentinelles, les fins de ligne et l'alpha
// sont déjà éprouvés par la première scène, qui passe par le même tampon et le
// même chemin de sortie. Ce que celle-ci ajoute est le chemin que l'autre
// n'emprunte pas — chargement d'une texture, soumission texturée,
// échantillonnage.
func renderTextured(filter uint32) (uint64, bool) {
	config := sceneConfig()
	frame, pixels := newFrame()
	defer release(frame)

	texture := loadTexture(makeChecker(floorSide, floorCell), floorSide)
	check(texture != nil, "la texture se charge sans contexte")

	var ctx *C.ScgContext
	check(C.scg_create(&config, &ctx) == C.SCG_OK, "création du contexte texturé")
	if ctx == nil || texture == nil {
		C.scg_texture_destroy(texture)
		return 0, false
	}
	defer C.scg_destroy(ctx)

	check(C.scg_set_filter(ctx, C.uint32_t(filter)) == C.SCG_OK, "le filtrage se règle")
	code := C.scg_submit_textured(ctx, &identity, &floorVertices[0], 4, &floorTriangles[0], 2, texture)
	check(code == C.SCG_OK, "le lot texturé est accepté")

	// Détruite avant le rendu, à dessein : le moteur en garde sa propre
	// référence jusqu'à la fin de l'image, et l'empreinte le prouve.
	C.scg_texture_destroy(texture)

	code = C.scg_frame_end(ctx, (*C.uint8_t)(frame), stride)
	check(code == C.SCG_OK, "l'image texturée se rend")
	return fingerprint(pixels, width, height, stride), code == C.SCG_OK
}

// Rend la même scène passée par la courbe de sortie.
//
// C'est le seul endroit où cet hôte écrit une `ScgGrade`, et ce qui vérifie sa
// disposition autrement que par une assertion statique : les décalages relèvent
// le noir du fond, et une structure mal remplie se verrait dans l'empreinte.
func renderGraded() (uint64, bool) {
	config := sceneConfig()
	frame, pixels := newFrame()
	defer release(frame)

	texture := loadTexture(makeChecker(floorSide, floorCell), floorSide)
	check(texture != nil, "la texture de la scène étalonnée se charge")

	var ctx *C.ScgContext
	check(C.scg_create(&config, &ctx) == C.SCG_OK, "création du contexte étalonné")
	if ctx == nil || texture == nil {
		C.scg_texture_destroy(texture)
		return 0, false
	}
	defer C.scg_destroy(ctx)

	var grade C.ScgGrade
	grade.gamma = 2.2
	grade.gain_r = 1.15
	grade.gain_g = 1.0
	grade.gain_b = 0.85
	grade.offset_r = 0.04
	grade.offset_g = -0.02
	grade.offset_b = 0.08

	// Un réservé non nul est refusé, et le contexte garde sa courbe : sans ce
	// refus, la promesse d'extension ne vaudrait rien.
	grade.reserved1 = 1
	check(C.scg_set_grade(ctx, &grade) == C.SCG_ERR_INVALID_ARGUMENT,
		"un champ réservé non nul est refusé")
	grade.reserved1 = 0

	// Éteindre une courbe qu'on n'a pas réglée n'est pas une erreur.
	check(C.scg_clear_grade(ctx) == C.SCG_OK, "l'extinction sans courbe passe")
	check(C.scg_set_grade(ctx, &grade) == C.SCG_OK, "la courbe se règle")

	code := C.scg_submit_textured(ctx, &identity, &floorVertices[0], 4, &floorTriangles[0], 2, texture)
	check(code == C.SCG_OK, "le lot de la scène étalonnée est accepté")
	C.scg_texture_destroy(texture)

	code = C.scg_frame_end(ctx, (*C.uint8_t)(frame), stride)
	check(code == C.SCG_OK, "l'image étalonnée se rend")
	return fingerprint(pixels, width, height, stride), code == C.SCG_OK
}

// Rend la scène embrumée.
//
// Le fond n'est effacé de rien : c'est le moteur qui lui donne la couleur du
// brouillard, parce qu'un pixel non peint est infiniment lointain.
func renderFog() (uint64, bool) {
	config := sceneConfig()
	frame, pixels := newFrame()
	defer release(frame)

	texture := loadTexture(makeChecker(floorSide, floorCell), floorSide)
	check(texture != nil, "la texture du sol embrumé se charge")

	var ctx *C.ScgContext
	check(C.scg_create(&config, &ctx) == C.SCG_OK, "création du contexte embrumé")
	if ctx == nil || texture == nil {
		C.scg_texture_destroy(texture)
		return 0, false
	}
	defer C.scg_destroy(ctx)

	// Une rampe vide est refusée : c'est une division par zéro, et l'appelant
	// voulait vraisemblablement éteindre le brouillard.
	check(C.scg_set_fog(ctx, 0x30, 0x38, 0x48, 10.0, 10.0) == C.SCG_ERR_INVALID_ARGUMENT,
		"une rampe vide est refusée")
	check(C.scg_clear_fog(ctx) == C.SCG_OK, "l'extinction sans brouillard passe")
	check(C.scg_set_fog(ctx, 0x30, 0x38, 0x48, 3.0, 14.0) == C.SCG_OK, "le brouillard se règle")

	code := C.scg_submit_textured(ctx, &identity, &fogFloor[0], 4, &floorTriangles[0], 2, texture)
	check(code == C.SCG_OK, "le sol embrumé est accepté")
	C.scg_texture_destroy(texture)

	code = C.scg_frame_end(ctx, (*C.uint8_t)(frame), stride)
	check(code == C.SCG_OK, "l'image embrumée se rend")
	return fingerprint(pixels, width, height, stride), code == C.SCG_OK
}

// Rend la scène éclairée par une lightmap.
//
// Deux lots : le sol, texturé et éclairé, puis le mur, éclairé seul. Le second
// passe une texture nulle, ce que ce point d'entrée accepte là où
// `scg_submit_textured` le refuse — l'asymétrie que le header signale.
func renderLit(overbright uint32) (uint64, bool) {
	config := sceneConfig()
	frame, pixels := newFrame()
	defer release(frame)

	texture := loadTexture(makeChecker(floorSide, floorCell), floorSide)
	check(texture != nil, "la texture du sol se charge")
	lightmap := loadTexture(makeGradient(), lightSide)
	check(lightmap != nil, "la lightmap se charge par le même chemin")

	var ctx *C.ScgContext
	check(C.scg_create(&config, &ctx) == C.SCG_OK, "création du contexte éclairé")
	if ctx == nil || texture == nil || lightmap == nil {
		C.scg_texture_destroy(texture)
		C.scg_texture_destroy(lightmap)
		return 0, false
	}
	defer C.scg_destroy(ctx)

	// Trois valeurs permises, et toute autre refusée : un décalage rabattu en
	// silence rendrait une image plus sombre que demandée, sans rien pour
	// l'annoncer.
	check(C.scg_set_overbright(ctx, 3) == C.SCG_ERR_INVALID_ARGUMENT,
		"un sur-éclairement de trois est refusé")
	check(C.scg_set_overbright(ctx, C.uint32_t(overbright)) == C.SCG_OK,
		"le sur-éclairement se règle")

	// Une lightmap nulle est refusée, elle : sans elle, ce lot n'a rien à faire
	// sur ce chemin.
	refused := C.scg_submit_lit(ctx, &identity, &litFloor[0], 4, &floorTriangles[0], 2, texture, nil)
	check(refused == C.SCG_ERR_NULL, "une lightmap nulle est refusée")

	code := C.scg_submit_lit(ctx, &identity, &litFloor[0], 4, &floorTriangles[0], 2, texture, lightmap)
	check(code == C.SCG_OK, "le sol texturé et éclairé est accepté")

	code = C.scg_submit_lit(ctx, &identity, &litWall[0], 4, &wallTriangles[0], 2, nil, lightmap)
	check(code == C.SCG_OK, "le mur uni et éclairé est accepté")

	C.scg_texture_destroy(texture)
	C.scg_texture_destroy(lightmap)

	code = C.scg_frame_end(ctx, (*C.uint8_t)(frame), stride)
	check(code == C.SCG_OK, "l'image éclairée se rend")
	return fingerprint(pixels, width, height, stride), code == C.SCG_OK
}

// Rend la scène éclairée par des lumières dynamiques.
//
// Le sol et le mur sont découpés en panneaux : l'atténuation étant par sommet,
// une surface d'un seul quadrilatère ne rendrait qu'un dégradé entre ses quatre
// coins. Le sol va au-delà de la portée des trois lumières, si bien que ses
// derniers panneaux s'éteignent — et doivent le faire en continuité.
func renderLights() (uint64, bool) {
	const panels = 16
	const nearEdge, farEdge = 2.0, 50.0
	const step = (farEdge - nearEdge) / float32(panels)

	config := sceneConfig()
	frame, pixels := newFrame()
	defer release(frame)

	var ctx *C.ScgContext
	check(C.scg_create(&config, &ctx) == C.SCG_OK, "création du contexte éclairé")
	if ctx == nil {
		return 0, false
	}
	defer C.scg_destroy(ctx)

	// Un champ réservé non nul est refusé : c'est le mécanisme d'extension de
	// l'ABI, et il ne vaut que si personne n'y écrit.
	dirty := sceneLights[0]
	dirty._reserved = 1
	check(C.scg_set_lights(ctx, &dirty, 1) == C.SCG_ERR_INVALID_ARGUMENT,
		"un champ réservé non nul est refusé")

	check(C.scg_set_lights(ctx, &sceneLights[0], 3) == C.SCG_OK, "les lumières se règlent")

	code := C.int32_t(C.SCG_OK)
	for i := 0; i < panels && code == C.SCG_OK; i++ {
		a := C.float(nearEdge + float32(i)*step)
		b := C.float(nearEdge + float32(i+1)*step)
		floorV := [4]C.ScgVertex{
			{x: a, y: -7.0, z: -1.2}, {x: b, y: -7.0, z: -1.2},
			{x: b, y: 7.0, z: -1.2}, {x: a, y: 7.0, z: -1.2},
		}
		floorT := [2]C.ScgTriangle{
			{i0: 0, i1: 1, i2: 2, r: 0xB0, g: 0xB0, b: 0xB0, a: 0xFF},
			{i0: 0, i1: 2, i2: 3, r: 0xB0, g: 0xB0, b: 0xB0, a: 0xFF},
		}
		code = C.scg_submit(ctx, &identity, &floorV[0], 4, &floorT[0], 2)

		wallV := [4]C.ScgVertex{
			{x: a, y: -7.0, z: 4.0}, {x: b, y: -7.0, z: 4.0},
			{x: b, y: -7.0, z: -1.2}, {x: a, y: -7.0, z: -1.2},
		}
		wallT := [2]C.ScgTriangle{
			{i0: 0, i1: 1, i2: 2, r: 0x90, g: 0x90, b: 0x98, a: 0xFF},
			{i0: 0, i1: 2, i2: 3, r: 0x90, g: 0x90, b: 0x98, a: 0xFF},
		}
		if code == C.SCG_OK {
			code = C.scg_submit(ctx, &identity, &wallV[0], 4, &wallT[0], 2)
		}
	}
	check(code == C.SCG_OK, "les panneaux éclairés sont acceptés")

	code = C.scg_frame_end(ctx, (*C.uint8_t)(frame), stride)
	check(code == C.SCG_OK, "l'image éclairée se rend")
	return fingerprint(pixels, width, height, stride), code == C.SCG_OK
}
