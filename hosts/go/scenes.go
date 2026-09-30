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
