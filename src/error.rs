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
    /// Un bloc de données n'est pas un fichier que le moteur peut lire, et
    /// [`Malformation`] dit ce qui l'a fait refuser.
    ///
    /// Une donnée fausse, jamais un défaut du moteur : le contenu d'un bloc est
    /// hostile par hypothèse, et ce refus ne passe donc pas par une panique,
    /// même en débogage.
    InvalidFormat(Malformation),
    /// La signature et le genre sont justes, mais cette construction ne lit pas
    /// la version de format annoncée.
    ///
    /// Séparée de [`Error::InvalidFormat`] parce que c'est le seul refus de
    /// chargement qui dise à l'hôte quoi faire — prendre une bibliothèque plus
    /// récente, ou réexporter la donnée. Confondue avec l'autre, elle enverrait
    /// chercher une corruption qui n'existe pas.
    UnsupportedFormatVersion,
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
    /// Un gamma hors de `]0, 8]`, ou un gain de canal hors de `[0, 4]`.
    ///
    /// Les deux bornes sont larges — un réglage d'écran vit entre 1,8 et 2,6,
    /// un gain au-delà de deux diaphragmes ne laisse qu'un aplat — et
    /// n'existent que pour refuser l'absurde plutôt que de le rendre.
    Grade,
}

/// Ce qui a fait refuser un bloc par [`Error::InvalidFormat`].
///
/// Un seul code d'ABI pour tout le contenu d'un fichier, parce qu'un hôte n'a
/// qu'une chose à en faire ; la variante ne se lit que dans le message, et
/// c'est ce qui reste à l'auteur d'un exportateur pour trouver son défaut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformation {
    /// Une lecture au-delà de la fin du bloc, de la table de sections ou d'une
    /// section.
    Truncated,
    /// Les quatre premiers octets ne sont pas la signature du projet.
    Signature,
    /// Le genre annoncé n'est pas celui qu'attendait l'appel : un maillage là
    /// où une carte était attendue.
    Kind,
    /// La longueur annoncée en tête n'est pas celle du bloc reçu.
    ///
    /// Une troncature et un en-tête qui mentirait sur sa longueur arrivent ici
    /// ensemble : le moteur ne peut pas les distinguer, et aucun hôte n'agirait
    /// différemment sur les deux.
    Length,
    /// Une section d'un genre que ce format ne connaît pas.
    ///
    /// Refusée et non ignorée, et c'est la décision la moins intuitive du
    /// format : presque tout ce qui s'ajoutera à un format de rendu change ce
    /// qui est rendu, si bien qu'une construction qui sauterait en silence une
    /// section employée par une version plus récente rendrait une autre image
    /// sur le même fichier, sans erreur. La conformance ne le verrait pas,
    /// chaque construction étant cohérente avec elle-même.
    SectionKind,
    /// Deux sections du même genre, ou des genres qui ne croissent pas.
    ///
    /// Sans cette canonicité, le même contenu aurait plusieurs écritures
    /// légitimes : un choix dont l'écrivain n'a pas besoin, une recherche dont
    /// le lecteur n'a pas besoin non plus.
    SectionOrder,
    /// Les sections ne pavent pas le fichier : un décalage qui n'est pas la fin
    /// de la section précédente, ou des octets laissés au bout.
    SectionBounds,
    /// Un flottant du fichier n'est pas fini.
    ///
    /// Refusé à la lecture, avant toute arithmétique : la charge utile d'un
    /// `NaN` signalant peut être normalisée par un passage en registre, et la
    /// divergence entre deux cibles serait silencieuse.
    NonFinite,
    /// Une chaîne du fichier n'est pas de l'UTF-8 valide.
    NonUtf8,
    /// Un indice au-delà de ce qu'il désigne : un sommet, un emplacement de
    /// texture.
    ///
    /// Vérifié une fois au chargement, jamais ensuite. Un tableau reçu de
    /// l'hôte n'est pas validé et doit l'être ; une ressource chargée porte
    /// l'invariant dans son type, et le revérifier par image coûterait un
    /// parcours complet sans faire rougir aucun test.
    Index,
    /// Un identifiant nul, que l'éditeur réserve à « aucun », ou deux fois le
    /// même dans sa famille.
    ///
    /// L'unicité se lit sur une table triée, en une passe adjacente. Le tri se
    /// vérifie, il ne se suppose pas : une recherche dichotomique sur une table
    /// non triée ne plante pas, elle rend la mauvaise surface — un défaut
    /// « image fausse, aucune erreur ».
    Identifier,
    /// Les groupes de surface ne pavent pas les triangles : un trou, un
    /// recouvrement, ou un total qui n'est pas leur nombre.
    ///
    /// Une soumission vaut pour un groupe. Un groupe dispersé imposerait de
    /// rassembler ses triangles à chaque image, donc un tampon, donc une
    /// allocation par image.
    GroupBounds,
    /// Un compte qui ne recoupe pas la longueur qui le borne, ou un total qui
    /// déborde ce qu'un entier peut porter.
    Count,
    /// Un bit de drapeau qu'aucune version n'a défini.
    ///
    /// Nuls obligatoires, même règle que les champs réservés de l'ABI : c'est
    /// ce qui permettra d'en employer un sans casser les cartes déjà écrites.
    Flags,
    /// Un polygone que le chargement ne peut pas prendre : moins de trois
    /// sommets, plus que le plafond, plat, qui se recoupe — ou, pour un
    /// portail, qui n'est pas convexe.
    ///
    /// La convexité est exigée du portail et non de la surface parce que le
    /// portail décide de ce qu'on voit : une projection concave n'a pas
    /// d'intersection exprimable comme réduction de fenêtre, et l'erreur se
    /// paierait en trou définitif.
    Polygon,
    /// Un repère de plaquage inutilisable : des axes de lightmap dont la
    /// longueur n'est pas une puissance de deux, ou des coordonnées dérivées
    /// qui ne sont pas finies.
    Mapping,
    /// Trois portails partagent les mêmes sommets.
    ///
    /// Deux s'apparient ; à trois, il n'y a pas de réponse à « lequel des
    /// deux ». Un portail seul, lui, est un mur et non une erreur : une carte
    /// en cours d'édition en a toujours.
    Portal,
}

/// Le résultat d'un appel du noyau.
pub type Result<T> = core::result::Result<T, Error>;
