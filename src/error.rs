// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce qui peut échouer dans le noyau.

/// Une erreur du noyau.
///
/// Sans chaîne de caractères : le message se formate dans `screengine-ffi`, qui
/// traduit aussi chaque variante en code d'ABI. L'énumération n'est
/// volontairement pas `non_exhaustive` — c'est ce qui fait échouer la
/// compilation de la couche FFI le jour où une variante y est ajoutée sans
/// traduction, au lieu de la laisser tomber dans un bras générique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Une valeur reçue est hors de ce que le moteur accepte, et [`Argument`]
    /// dit laquelle.
    ///
    /// Une variante qui porte l'argument plutôt qu'une variante par argument :
    /// la catégorie reste une, comme le code d'ABI qui la traduit, et c'est le
    /// message seul qui se précise.
    InvalidArgument(Argument),
    /// Un tampon du contexte n'a pas pu être alloué.
    OutOfMemory,
    /// Un appel hors séquence : une tuile déjà rendue dans cette image.
    InvalidState,
    /// Une tuile n'est pas sortie de son rendu, et l'image ne vaut plus rien.
    ///
    /// Le noyau ne connaît pas les paniques — il n'a pas `std` et n'en rattrape
    /// aucune —, mais il voit qu'une tuile est entrée sans ressortir : son
    /// garde de décompte le note avant de rendre la main. C'est ce qui permet à
    /// la fin d'image de refuser de conclure, au lieu d'annoncer une image
    /// complète dont un rectangle n'a jamais été dessiné.
    ///
    /// Sans quoi une course s'ouvre, et elle est réelle : le décompte retombe
    /// pendant le dépliage, donc **avant** que la couche qui rattrape la panique
    /// n'ait pu marquer quoi que ce soit. Entre les deux, une fin d'image voit
    /// zéro tuile en vol et un contexte sain.
    Faulted,
}

/// L'argument qu'une [`Error::InvalidArgument`] refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Argument {
    /// Une dimension nulle, un maximum au-delà de [`MAX_RESOLUTION`], ou une
    /// résolution courante au-delà du maximum reçu à la création.
    ///
    /// [`MAX_RESOLUTION`]: crate::MAX_RESOLUTION
    Resolution,
    /// Une taille de tuile autre que 32 ou 64.
    TileSize,
    /// Un `stride` inférieur à la largeur courante, ou si grand que la taille
    /// du tampon qu'il décrit ne tient pas dans un `usize`.
    Stride,
    /// Un tampon de pixels plus court que `stride × hauteur × 4` octets.
    ///
    /// Seul un appelant Rust le rencontre : la frontière C ne reçoit pas la
    /// longueur du tampon et en fait une précondition.
    BufferLength,
    /// Un index de tuile au-delà du nombre de tuiles de l'image.
    TileIndex,
    /// Une région qui déborde de l'image.
    Region,
    /// Un tampon de travail plus court que la région qu'il doit porter.
    ScratchLength,
    /// Plus de triangles soumis que la capacité réservée à la création.
    TriangleCapacity,
    /// Un indice de triangle au-delà du tableau de sommets du lot.
    VertexIndex,
    /// Une coordonnée de sommet non finie dans un lot soumis.
    ///
    /// Distincte d'un sommet simplement démesuré, qui fait disparaître son
    /// triangle sans erreur : celui-là dépend de la caméra et de la matrice
    /// modèle, donc l'hôte ne peut pas savoir d'avance s'il se projettera, et
    /// refuser le lot rendrait une scène valide irrendable selon l'endroit où
    /// la caméra se place. Un `NaN` ou un infini soumis, lui, ne dépend de
    /// rien : c'est une donnée fausse, du même statut qu'un indice hors
    /// tableau.
    VertexCoordinate,
    /// Un champ de vision hors de `]0, π[`, ou un plan proche nul, négatif ou
    /// démesuré.
    Projection,
    /// Une coordonnée de texture non finie, ou au-delà de
    /// [`MAX_TEXEL_COORD`] texels.
    ///
    /// Une erreur et non une disparition, contrairement à une position que la
    /// vue ne peut pas porter : un `uv` ne dépend ni de la caméra, ni de la
    /// matrice modèle, ni de la texture avec laquelle le lot sera dessiné.
    /// Hors borne, c'est une donnée fausse.
    ///
    /// [`MAX_TEXEL_COORD`]: crate::MAX_TEXEL_COORD
    TextureCoordinate,
    /// Plus de textures distinctes dans une image que la table du contexte
    /// n'en porte.
    ///
    /// Le cas limite, et il faut le chercher : la table est dimensionnée sur
    /// la capacité de triangles, or une texture vaut pour un lot et un lot
    /// porte au moins un triangle. Seul un hôte qui soumettrait plus de 65535
    /// lots texturés distincts dans une image l'atteint.
    TextureCapacity,
    /// Un côté de texture nul, au-delà de [`MAX_TEXTURE_SIZE`], ou qui n'est
    /// pas une puissance de deux.
    ///
    /// [`MAX_TEXTURE_SIZE`]: crate::MAX_TEXTURE_SIZE
    TextureSize,
    /// Un bloc de pixels dont la longueur n'est pas exactement
    /// `largeur × hauteur × 4` octets.
    TextureLength,
    /// Un décalage de sur-éclairement au-delà de [`MAX_OVERBRIGHT`].
    ///
    /// [`MAX_OVERBRIGHT`]: crate::MAX_OVERBRIGHT
    Overbright,
    /// Une rampe de brouillard dont une distance n'est pas finie, dont le début
    /// est négatif, ou dont la fin n'est pas strictement au-delà du début.
    ///
    /// Une rampe vide — début et fin confondus — n'est pas un brouillard qui
    /// commence partout : c'est une division par zéro, et l'appelant voulait
    /// vraisemblablement l'éteindre.
    Fog,
    /// Plus de lumières dynamiques que l'image n'en porte.
    ///
    /// Le lot est refusé en entier plutôt que tronqué : une scène à demi
    /// éclairée ne se distingue pas d'une scène dont les rayons sont mal
    /// réglés, et l'hôte chercherait longtemps.
    LightCapacity,
    /// Une lumière de position non finie, ou de rayon nul, négatif ou non
    /// fini.
    ///
    /// Un rayon nul n'éclaire rien et ferait diviser par zéro ; une position
    /// non finie empoisonnerait chaque sommet du lot.
    Light,
}

/// Le résultat d'un appel du noyau.
pub type Result<T> = core::result::Result<T, Error>;
