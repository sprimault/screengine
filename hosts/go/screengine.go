// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

// Hôte Go de Screengine, sans fenêtre.
//
// **Ce qu'il éprouve que les quatre autres n'éprouvent pas** : que l'ABI se
// consomme depuis un langage qui déplace ses objets en mémoire, et dont la pile
// n'est pas celle du C. Un pointeur Go passé au moteur et retenu par lui serait
// un défaut que ni C, ni C++, ni JavaScript, ni Java ne révèlent — les deux
// premiers ne déplaçant rien, les deux autres ne passant jamais de pointeur.
//
// Les chemins d'inclusion et de liaison viennent du `Makefile` : écrits ici, ils
// désigneraient un répertoire de construction que le poste voisin n'a pas.

package main

/*
#include "screengine.h"
#include <stdlib.h>
*/
import "C"

import (
	"fmt"
	"os"
	"unsafe"
)

// Largeur, hauteur et taille de tuile de la scène des hôtes.
//
// Celles de `screengine-conformance --print` : les changer d'un seul côté fait
// diverger les deux empreintes, ce qui est exactement ce qu'on veut qu'il
// arrive.
const (
	width  = 640
	height = 360
	tile   = 64
)

// Le pas de ligne de l'hôte, plus grand que la largeur.
//
// L'espace de fin de ligne existe ainsi, et les sentinelles vérifient que le
// moteur n'y écrit pas.
const stride = width + 3

// Marge sentinelle avant et après le tampon, en octets.
const guard = 64

// L'octet sentinelle, choisi pour ne ressembler à aucune couleur du rendu.
const sentinel = 0xA5

// Nombre de vérifications en échec.
var failures int

// Enregistre une vérification, et dit laquelle a échoué.
func check(ok bool, what string) {
	if !ok {
		fmt.Fprintf(os.Stderr, "échec : %s\n", what)
		failures++
	}
}

// FNV-1a 64 bits, dans la forme de screengine-conformance : largeur et hauteur
// en u32 poids faible en tête, puis la zone utile ligne par ligne.
func fingerprint(pixels []byte, w, h, s uint32) uint64 {
	hash := uint64(0xcbf29ce484222325)
	for _, dim := range [2]uint32{w, h} {
		for shift := 0; shift < 32; shift += 8 {
			hash = (hash ^ uint64((dim>>uint(shift))&0xff)) * 0x100000001b3
		}
	}
	for y := uint32(0); y < h; y++ {
		row := y * s * 4
		for i := uint32(0); i < w*4; i++ {
			hash = (hash ^ uint64(pixels[row+i])) * 0x100000001b3
		}
	}
	return hash
}

// FNV-1a 64 bits sur une suite d'octets quelconque.
//
// Le pendant de [fingerprint] pour la scène qui ne rend pas d'image.
func hashBytes(bytes []byte) uint64 {
	hash := uint64(0xcbf29ce484222325)
	for _, b := range bytes {
		hash = (hash ^ uint64(b)) * 0x100000001b3
	}
	return hash
}

// Une configuration valide, mise à zéro entière comme l'exige l'ABI.
//
// Le zéro vient de Go lui-même : une structure C déclarée ici naît nulle, là où
// le C réclame un `memset` explicite. C'est la seule facilité du langage dont
// cet hôte profite sans la nommer à chaque emploi.
func sceneConfig() C.ScgContextConfig {
	var config C.ScgContextConfig
	config.max_width = width
	config.width = width
	config.max_height = height
	config.height = height
	config.tile_size = tile
	return config
}

// L'identité, par colonnes.
var identity = C.ScgMat4{m: [16]C.float{
	1, 0, 0, 0,
	0, 1, 0, 0,
	0, 0, 1, 0,
	0, 0, 0, 1,
}}

// Alloue un bloc hors du tas de Go et rend son adresse.
//
// **Tout ce que le moteur lit passe par ici**, jamais par un pointeur vers une
// tranche Go. Le langage déplace ses objets, et une adresse prise sur l'un
// d'eux cesse d'être valide sans prévenir ; la règle du langage l'interdit
// d'ailleurs pour un pointeur conservé au-delà de l'appel. Le moteur ne conserve
// rien, mais s'en remettre à cette clause pour une écriture de tampon serait
// tenir l'invariant du moteur pour une garantie du langage.
func alloc(n int) unsafe.Pointer {
	if n == 0 {
		n = 1
	}
	return C.calloc(1, C.size_t(n))
}

// Rend un bloc alloué par [alloc].
func release(p unsafe.Pointer) {
	C.free(p)
}

// Une vue en tranche d'octets sur un bloc alloué hors du tas.
//
// Elle ne possède rien : le bloc reste à libérer par [release], et la tranche
// cesse d'être lisible dès que c'est fait.
func view(p unsafe.Pointer, n int) []byte {
	return unsafe.Slice((*byte)(p), n)
}

// Écrit trois flottants à une adresse hors du tas, et rend cette adresse.
func writeVec3(p unsafe.Pointer, x, y, z float32) *C.float {
	floats := unsafe.Slice((*C.float)(p), 3)
	floats[0] = C.float(x)
	floats[1] = C.float(y)
	floats[2] = C.float(z)
	return (*C.float)(p)
}

// Lit un fichier entier, ou dit pourquoi il n'a pas pu l'être.
//
// L'hôte lit les fichiers, jamais le moteur : c'est pour cela que le chargement
// prend un bloc d'octets et non un chemin.
func readFile(path string) []byte {
	bytes, err := os.ReadFile(path)
	if err != nil {
		check(false, fmt.Sprintf("lecture de %s : %v", path, err))
		return nil
	}
	return bytes
}

// Recopie une tranche Go dans un bloc hors du tas, et rend ce bloc.
//
// L'appelant le libère. La recopie n'est pas une précaution de style : c'est ce
// qui permet de passer les octets au moteur sans épingler de la mémoire que le
// langage se réserve de déplacer.
func toNative(bytes []byte) unsafe.Pointer {
	block := alloc(len(bytes))
	copy(view(block, len(bytes)), bytes)
	return block
}
