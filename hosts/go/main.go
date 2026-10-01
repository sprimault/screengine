// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

// Le point d'entrée : toutes les vérifications, puis les empreintes sur la
// sortie standard, une par ligne et dans l'ordre que le Makefile attend.

package main

/*
#include "screengine.h"
*/
import "C"

import (
	"fmt"
	"os"
)

// Vérifie que la bibliothèque liée est celle du header, joue les scènes, et
// rend 1 si une vérification a échoué.
//
// Les chemins des fichiers de données viennent de la ligne de commande : l'hôte
// s'exécute depuis un répertoire de sortie qui n'est pas celui du dépôt, et les
// y écrire en dur les rendrait introuvables.
func main() {
	if len(os.Args) != 6 {
		fmt.Fprintf(os.Stderr,
			"usage : %s <fichier de maillage> <fichier de carte> "+
				"<decor de collision> <liste de balayages> <liste de rayons>\n", os.Args[0])
		os.Exit(2)
	}

	check(uint32(C.scg_abi_version()) == C.SCG_ABI_VERSION,
		"la bibliothèque liée est celle du header")

	edge, edgeOK := renderEdge()
	textured, texturedOK := renderTextured(C.SCG_FILTER_DITHER)
	bilinear, bilinearOK := renderTextured(C.SCG_FILTER_BILINEAR)
	graded, gradedOK := renderGraded()
	lit, litOK := renderLit(0)
	// La même scène au sur-éclairement maximal : seul le réglage du contexte
	// les sépare, donc une divergence ne peut venir que de lui.
	overbright, overbrightOK := renderLit(2)
	fog, fogOK := renderFog()
	lights, lightsOK := renderLights()
	mesh, meshOK := renderMesh(os.Args[1])
	composite, compositeOK := renderComposite(os.Args[1])
	rooms, roomsOK := renderRooms(os.Args[2])
	trace, traceOK := renderTrace()
	sweeps, sweepsOK := renderSweeps(os.Args[3], os.Args[4])
	picks, picksOK := renderPicks(os.Args[3], os.Args[5])

	rendered := []struct {
		hash uint64
		ok   bool
	}{
		{edge, edgeOK}, {textured, texturedOK}, {bilinear, bilinearOK}, {graded, gradedOK},
		{lit, litOK}, {overbright, overbrightOK}, {fog, fogOK}, {lights, lightsOK},
		{mesh, meshOK}, {composite, compositeOK}, {rooms, roomsOK}, {trace, traceOK},
		{sweeps, sweepsOK}, {picks, picksOK},
	}

	for _, scene := range rendered {
		if !scene.ok {
			failures++
		}
	}
	if failures > 0 {
		fmt.Fprintf(os.Stderr, "%d vérification(s) en échec\n", failures)
		os.Exit(1)
	}
	for _, scene := range rendered {
		fmt.Printf("%016x\n", scene.hash)
	}
}
