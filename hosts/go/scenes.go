// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

// Les scènes que l'hôte rend, transcrites de celles de la conformance.
//
// **Les valeurs s'écrivent en clair, littéral pour littéral.** Une constante
// nommée ne dirait rien de plus que le nombre, et cet hôte doit se lire comme ce
// qu'il est : la même scène décrite dans un autre langage, à travers l'ABI, pour
// retrouver la même empreinte. Les changer d'un seul côté fait diverger la
// comparaison, ce qui est exactement ce qu'on attend d'elle.

package main

/*
#include "screengine.h"
*/
import "C"

import "unsafe"

// Le quadrilatère de la scène `arete`, en coordonnées de monde : X vers l'est,
// Z en haut. La caméra par défaut le regarde depuis l'origine.
var sceneVertices = [4]C.ScgVertex{
	{x: 2.0, y: 2.5, z: 1.6},
	{x: 3.5, y: -2.5, z: 1.6},
	{x: 3.5, y: -2.5, z: -1.6},
	{x: 2.0, y: 2.5, z: -1.6},
}

// Deux triangles qui partagent l'arête des sommets 0 et 2, parcourue en sens
// opposés par chacun : le cas que la règle top-left doit trancher.
var sceneTriangles = [2]C.ScgTriangle{
	{i0: 0, i1: 2, i2: 1, r: 0xE0, g: 0xA0, b: 0x30, a: 0xFF},
	{i0: 0, i1: 3, i2: 2, r: 0xA0, g: 0xE0, b: 0x30, a: 0xFF},
}

// Soumet la scène au contexte, et rend vrai si elle a été acceptée.
func submitScene(ctx *C.ScgContext) bool {
	code := C.scg_submit(ctx, &identity, &sceneVertices[0], 4, &sceneTriangles[0], 2)
	return code == C.SCG_OK
}

// Le mur de la scène `trace`, plein cadre et à mi-distance : il coupe les
// brisures en deux, si bien qu'une moitié de chaque trait éprouve le mode de
// profondeur et l'autre la règle de couverture.
var traceWall = [4]C.ScgVertex{
	{x: 6.0, y: 4.0, z: -3.0},
	{x: 6.0, y: -4.0, z: -3.0},
	{x: 6.0, y: -4.0, z: 3.0},
	{x: 6.0, y: 4.0, z: 3.0},
}

// traceFaces est le mur sombre de la scène de tracé : il n'est là que pour
// occulter, et sa teinte est celle du fond pour que seules les lignes se lisent.
var traceFaces = [2]C.ScgTriangle{
	{i0: 0, i1: 1, i2: 2, r: 0x30, g: 0x38, b: 0x48, a: 0xFF},
	{i0: 0, i1: 2, i2: 3, r: 0x30, g: 0x38, b: 0x48, a: 0xFF},
}

// Les cinq sommets de la brisure, le dernier ramené sur le premier.
//
// **Ils sont partagés d'un segment au suivant**, et c'est tout l'objet : la
// règle de sortie du losange ne les peint qu'une fois. Les quatre segments
// prennent les quatre familles de pente qu'elle départage différemment.
var traceCorners = [5][3]float32{
	{8.0, -3.0, -2.0},
	{8.0, 1.0, -2.0},
	{8.0, 1.0, 2.0},
	{8.0, -3.0, 2.0},
	{8.0, -3.0, -2.0},
}

// Les quatre segments d'une brisure, décalée de dx en X et de shift en Y.
func tracePolyline(dx, shift float32, r, g, b byte) [4]C.ScgLine {
	var lines [4]C.ScgLine
	for i := 0; i < 4; i++ {
		lines[i] = C.ScgLine{
			ax: C.float(traceCorners[i][0] + dx),
			ay: C.float(traceCorners[i][1] + shift),
			az: C.float(traceCorners[i][2]),
			bx: C.float(traceCorners[i+1][0] + dx),
			by: C.float(traceCorners[i+1][1] + shift),
			bz: C.float(traceCorners[i+1][2]),
			r:  C.uint8_t(r), g: C.uint8_t(g), b: C.uint8_t(b), a: 0xFF,
		}
	}
	return lines
}

// Rend la scène `trace` : les deux familles de primitives que le remplissage
// n'emprunte pas.
//
// Trois brisures sur la même géométrie, devant et derrière le mur, dans les deux
// modes de profondeur ; puis quatre points, qui sont le seul chemin de
// `scg_submit_points`. La brisure occultée derrière le mur ne doit **rien**
// peindre, et c'est le témoin devant le mur qui distingue cette absence d'un
// défaut rendant le mode occulté toujours invisible.
//
// L'ordre des soumissions est celui de la scène de référence, et il compte : le
// tracé n'écrit jamais la profondeur, donc deux traits qui se croisent se
// départagent par leur rang.
func renderTrace() (uint64, bool) {
	config := sceneConfig()
	var ctx *C.ScgContext
	body := stride * height * 4

	block := alloc(body)
	defer release(block)
	pixels := view(block, body)

	if C.scg_create(&config, &ctx) < 0 {
		check(false, "création du contexte de tracé")
		return 0, false
	}
	defer C.scg_destroy(ctx)

	occulted := tracePolyline(0.0, 2.5, 0xE0, 0x40, 0x30)
	through := tracePolyline(0.0, -2.5, 0x40, 0xE0, 0x80)
	witness := tracePolyline(-2.25, 2.5, 0x80, 0xC0, 0xFF)

	var dots [4]C.ScgPoint
	for i := range dots {
		dots[i] = C.ScgPoint{
			x: C.float(traceCorners[i][0] - 4.0),
			y: C.float(traceCorners[i][1]),
			z: C.float(traceCorners[i][2]),
			r: 0xFF, g: 0xE0, b: 0x40, a: 0xFF,
		}
	}

	check(C.scg_submit(ctx, &identity, &traceWall[0], 4, &traceFaces[0], 2) == C.SCG_OK,
		"le mur de la scène de tracé est accepté")
	check(C.scg_submit_lines(ctx, &identity, &occulted[0], 4, C.SCG_DEPTH_TESTED) == C.SCG_OK,
		"la brisure occultée est acceptée")
	check(C.scg_submit_lines(ctx, &identity, &through[0], 4, C.SCG_DEPTH_ALWAYS) == C.SCG_OK,
		"la brisure à travers est acceptée")
	check(C.scg_submit_lines(ctx, &identity, &witness[0], 4, C.SCG_DEPTH_TESTED) == C.SCG_OK,
		"le témoin devant le mur est accepté")
	check(C.scg_submit_points(ctx, &identity, &dots[0], 4, C.SCG_DEPTH_TESTED) == C.SCG_OK,
		"les points sont acceptés")

	code := C.scg_frame_end(ctx, (*C.uint8_t)(block), stride)
	check(code == C.SCG_OK, "l'image de tracé se rend")
	return fingerprint(pixels, width, height, stride), code == C.SCG_OK
}

// Rend le quadrilatère dans un tampon entouré de sentinelles, vérifie que rien
// n'est écrit hors de la zone utile, et rend l'empreinte.
//
// Le tampon vient de `calloc` et non d'une tranche Go : le moteur y écrit
// pendant l'appel, et une adresse prise sur le tas de Go n'est valable que tant
// que l'objet n'a pas bougé.
func renderEdge() (uint64, bool) {
	config := sceneConfig()
	var ctx *C.ScgContext
	body := stride * height * 4

	block := alloc(guard + body + guard)
	defer release(block)
	bytes := view(block, guard+body+guard)
	for i := range bytes {
		bytes[i] = sentinel
	}

	if C.scg_create(&config, &ctx) < 0 {
		check(false, "création du contexte de rendu")
		return 0, false
	}
	defer C.scg_destroy(ctx)

	pixels := unsafe.Pointer(uintptr(block) + guard)
	check(submitScene(ctx), "scène soumise")
	code := C.scg_frame_end(ctx, (*C.uint8_t)(pixels), stride)
	check(code == C.SCG_OK, "scg_frame_end aboutit")

	// Les deux marges se lisent dans le bloc entier et non dans la vue de
	// l'image : celle de fin commence là où l'image s'arrête.
	intact := true
	image := bytes[guard : guard+body]
	for i := 0; i < guard; i++ {
		intact = intact && bytes[i] == sentinel && bytes[guard+body+i] == sentinel
	}
	for y := 0; y < height; y++ {
		tail := y*stride*4 + width*4
		for i := 0; i < (stride-width)*4; i++ {
			intact = intact && image[tail+i] == sentinel
		}
	}
	check(intact, "rien n'est écrit hors de la zone utile, marges et fins de ligne comprises")

	opaque := true
	for y := 0; y < height; y++ {
		row := y * stride * 4
		for x := 0; x < width; x++ {
			opaque = opaque && image[row+x*4+3] == 255
		}
	}
	check(opaque, "l'alpha est écrit à 255 sur chaque pixel")

	return fingerprint(image, width, height, stride), code == C.SCG_OK
}
