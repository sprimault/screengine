// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce qui peut échouer dans le noyau.

/// Une erreur du noyau.
///
/// **Aucune variante ne porte de chaîne**, et c'est ce qui la rend utilisable
/// sans `std` : construire un message demanderait d'allouer, donc d'échouer une
/// seconde fois là où la première échoue déjà. Le texte vient de
/// [`message`](Self::message), qui ne rend que des littéraux.
///
/// L'énumération n'est volontairement pas `non_exhaustive` : un traducteur
/// écrit dehors cesse alors de compiler le jour où une variante apparaît, au
/// lieu de la laisser tomber dans un bras générique et de rendre une erreur
/// pour une autre.
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
    /// Un identifiant stable qui ne désigne rien dans la ressource.
    ///
    /// Le fichier est bon et l'appel est bien formé : c'est la référence qui ne
    /// trouve pas sa cible, ce qui n'est ni une malformation ni une faute
    /// d'argument. Réservée dans l'ABI depuis l'étape 4 sans qu'aucun appel ne la
    /// rende, elle trouve son premier avec la cellule de départ d'une traversée.
    UnknownResource,
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
    /// Plus de primitives de tracé soumises que la capacité réservée.
    ///
    /// Distincte de [`Argument::TriangleCapacity`] : les deux budgets sont
    /// séparés, et un hôte qui ne saurait pas lequel il vient d'épuiser
    /// chercherait du mauvais côté.
    LineCapacity,
    /// Un indice de triangle au-delà du tableau de sommets du lot.
    VertexIndex,
    /// Un indice de trame au-delà de ce que le maillage porte.
    ///
    /// Le fichier est bon, c'est l'appel qui sort des bornes : même forme que
    /// tout indice d'accesseur, et rien n'est dessiné.
    FrameIndex,
    /// Un facteur d'interpolation non fini, ou hors de `[0, 1]`.
    ///
    /// **Refusé et jamais ramené dans l'intervalle.** Un bornage silencieux
    /// rendrait une pose extrapolée sans le dire, et extrapoler est une
    /// décision de jeu — le moteur n'en prend aucune.
    FrameFactor,
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

/// Ce qu'un refus de contenu désigne, quand il désigne quelque chose.
///
/// **Un type unique plutôt qu'un champ nommé par variante**, parce que la même
/// variante sert plusieurs familles : un identifiant nul est refusé sur une
/// cellule, une surface, un portail, une lumière, une entité, un matériau et un
/// groupe de maillage, et un champ `surface` n'aurait rien à dire des six autres.
///
/// L'identifiant porté est **celui de l'éditeur**, stable d'un chargement à
/// l'autre, partout où l'élément en a un ; c'est un **rang** là où le format n'en
/// attribue pas — un groupe de surface, un emplacement de texture, un sommet —,
/// et la variante le dit. Un générateur retrouve ainsi ce qu'il a écrit sans
/// réimplémenter le prédicat du chargeur.
///
/// Rien de tout cela ne traverse l'ABI C : le message y reste un littéral, et
/// `docs/abi.md` interdit à une liaison de parler le texte. C'est l'appelant
/// **Rust** qui lit la charge utile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    /// Le refus porte sur le conteneur, et rien d'identifiable ne le porte.
    ///
    /// C'est le cas du rangement d'un atlas, qui échoue sur le volume de toutes
    /// les surfaces d'une cellule et non sur l'une d'elles. Une variante plutôt
    /// qu'un `Option<Element>` : l'absence est un cas du domaine, pas une valeur
    /// manquante, et l'imbrication se lirait mal au point de filtrage.
    None,
    /// La cellule de cet identifiant.
    Cell(u32),
    /// La surface de cet identifiant.
    Surface(u32),
    /// Le portail de cet identifiant.
    Portal(u32),
    /// La lumière statique de cet identifiant.
    Light(u32),
    /// L'entité de cet identifiant.
    Entity(u32),
    /// Le matériau de cet identifiant.
    Material(u32),
    /// Le groupe de surface de cet identifiant, dans un maillage.
    Group(u32),
    /// L'emplacement de texture de ce **rang**.
    ///
    /// Un rang, parce que le format n'attribue pas d'identifiant aux
    /// emplacements : ils se désignent par leur place dans la section des noms,
    /// qui est aussi l'ordre où l'hôte passe ses handles à la soumission.
    Slot(u32),
    /// Le sommet de ce **rang**, pour la même raison que [`Element::Slot`].
    Vertex(u32),
}

/// Ce qui a fait refuser un bloc par [`Error::InvalidFormat`].
///
/// Un seul code d'ABI pour tout le contenu d'un fichier, parce qu'un hôte n'a
/// qu'une chose à en faire ; la variante ne se lit que dans le message, et
/// c'est ce qui reste à l'auteur d'un exportateur pour trouver son défaut.
///
/// **Celles qui désignent un élément le nomment**, par [`Element`] : la règle est
/// dans `docs/rust.md`, avec la raison de chaque variante qui reste muette.
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
    Index {
        /// Ce dans quoi l'indice fautif pointait.
        element: Element,
    },
    /// Un identifiant nul, que l'éditeur réserve à « aucun », ou deux fois le
    /// même dans sa famille.
    ///
    /// L'unicité se lit sur une table triée, en une passe adjacente. Le tri se
    /// vérifie, il ne se suppose pas : une recherche dichotomique sur une table
    /// non triée ne plante pas, elle rend la mauvaise surface — un défaut
    /// « image fausse, aucune erreur ».
    Identifier {
        /// La famille où l'identifiant est refusé, et sa valeur.
        ///
        /// Sur un doublon, cette valeur est l'identifiant partagé, et c'est elle
        /// qui mène aux deux porteurs. Sur un identifiant nul, elle vaut zéro —
        /// ce qui est l'information même, le format réservant zéro à « aucun » ;
        /// la famille suffit alors à savoir quelle table relire.
        element: Element,
    },
    /// Les groupes de surface ne pavent pas les triangles : un trou, un
    /// recouvrement, ou un total qui n'est pas leur nombre.
    ///
    /// Une soumission vaut pour un groupe. Un groupe dispersé imposerait de
    /// rassembler ses triangles à chaque image, donc un tampon, donc une
    /// allocation par image.
    GroupBounds {
        /// Le groupe où le pavage se rompt, par son rang.
        element: Element,
    },
    /// Un compte qui ne recoupe pas la longueur qui le borne, ou un total qui
    /// déborde ce qu'un entier peut porter.
    Count,
    /// Un bit de drapeau qu'aucune version n'a défini.
    ///
    /// Nuls obligatoires, même règle que les champs réservés de l'ABI : c'est
    /// ce qui permettra d'en employer un sans casser les cartes déjà écrites.
    Flags {
        /// L'élément qui porte le bit non défini.
        element: Element,
    },
    /// Un polygone que le chargement ne peut pas prendre : moins de trois
    /// sommets, plus que le plafond, plat, qui se recoupe — ou, pour un
    /// portail, qui n'est pas convexe.
    ///
    /// La convexité est exigée du portail et non de la surface parce que le
    /// portail décide de ce qu'on voit : une projection concave n'a pas
    /// d'intersection exprimable comme réduction de fenêtre, et l'erreur se
    /// paierait en trou définitif.
    ///
    /// La charge utile dit **laquelle des deux familles** : une surface ou un
    /// portail, qui n'ont ni la même exigence — la convexité ne vaut que pour le
    /// second — ni le même espace d'identifiants.
    Polygon {
        /// La surface ou le portail refusé.
        element: Element,
    },
    /// Un repère de plaquage inutilisable : un axe dégénéré, une origine hors
    /// de sa grille, des axes non orthogonaux ou hors du plan de leur surface,
    /// une étendue démesurée, ou des coordonnées dérivées qui ne sont pas
    /// finies.
    ///
    /// **Nomme la surface en cause**, et [`Element::None`] quand aucune ne l'est
    /// seule : le rangement de l'atlas d'une cellule échoue sur le volume de
    /// toutes ses surfaces, pas sur l'une d'elles.
    ///
    /// Sans cet identifiant, un générateur de cartes ne retrouvait la surface
    /// fautive qu'en réimplémentant le prédicat du chargeur — ce qu'un
    /// intégrateur a fait, somme de Newell comprise. C'était le bénéfice que le
    /// refus au chargement revendiquait sans le rendre.
    ///
    /// La charge utile était un `surface: u32` jusqu'à ce que les huit autres
    /// variantes reçoivent la leur : un entier nu n'aurait rien dit de la famille
    /// là où `Identifier` en sert sept.
    Mapping {
        /// La surface refusée, ou [`Element::None`] pour la cellule entière.
        element: Element,
    },
    /// Une lumière statique inutilisable : un rayon nul, négatif ou non fini.
    ///
    /// Un rayon nul n'éclaire rien et ferait diviser par zéro le calcul
    /// d'atténuation.
    Light {
        /// La lumière refusée.
        element: Element,
    },
    /// Une orientation qui n'a pas de direction à porter.
    ///
    /// Un quaternion nul se normaliserait en l'identité sans rien signaler, et
    /// une entité posée de travers se retrouverait droite.
    Pose {
        /// L'entité dont l'orientation est refusée.
        element: Element,
    },
    /// Trois portails partagent les mêmes sommets.
    ///
    /// Deux s'apparient ; à trois, il n'y a pas de réponse à « lequel des
    /// deux ». Un portail seul, lui, est un mur et non une erreur : une carte
    /// en cours d'édition en a toujours.
    ///
    /// La charge utile nomme **le troisième rencontré**, celui dont la clé était
    /// déjà prise deux fois. Nommer les trois demanderait un tableau dans une
    /// énumération que tout le reste garde `Copy`, et le premier suffit à
    /// retrouver la clé commune — c'est elle qui situe le défaut, pas le rang de
    /// l'exemplaire.
    Portal {
        /// Le portail dont la clé était déjà partagée par deux autres.
        element: Element,
    },
}

impl Error {
    /// Le texte de cette erreur, toujours un littéral.
    ///
    /// En anglais, jamais localisé, et c'est `docs/abi.md` qui l'arrête :
    /// `scg_last_error` rend ce texte à travers la frontière, et un message qui
    /// changerait avec l'environnement donnerait des journaux qu'on ne peut plus
    /// rapprocher d'un poste à l'autre.
    ///
    /// Une table plate, indexée par le couple complet, plutôt qu'un préfixe
    /// composé avec le texte de l'[`Argument`] ou de la [`Malformation`] :
    /// composer demanderait un tampon, et c'est ce `&'static str` qui permet à la
    /// frontière C d'en rendre un pointeur sans rien allouer.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidArgument(Argument::Resolution) => {
                "invalid resolution: each side must be between 1 and 2048, and the current resolution within the maximum"
            }
            Self::InvalidArgument(Argument::TileSize) => "invalid tile size: must be 32 or 64",
            Self::InvalidArgument(Argument::Stride) => {
                "invalid stride: must be at least the internal width"
            }
            Self::InvalidArgument(Argument::BufferLength) => {
                "pixel buffer too short: needs stride x height x 4 bytes"
            }
            Self::InvalidArgument(Argument::TileIndex) => {
                "invalid tile index: must be less than the tile count of the frame"
            }
            Self::InvalidArgument(Argument::Region) => "invalid region: must lie within the image",
            Self::InvalidArgument(Argument::ScratchLength) => {
                "scratch buffer too short: needs one word per pixel of the region"
            }
            Self::InvalidArgument(Argument::TriangleCapacity) => {
                "too many triangles submitted for the capacity reserved at creation"
            }
            Self::InvalidArgument(Argument::LineCapacity) => {
                "too many drawing primitives submitted for max_lines reserved at creation"
            }
            Self::InvalidArgument(Argument::VertexIndex) => {
                "invalid triangle index: must be less than the vertex count of the batch"
            }
            Self::InvalidArgument(Argument::Projection) => {
                "invalid projection: the vertical field of view must be within ]0, pi[ radians, and the near plane positive and finite"
            }
            Self::InvalidArgument(Argument::VertexCoordinate) => {
                "vertex coordinate is not finite: NaN and infinities are rejected, and the whole batch with them"
            }
            Self::InvalidArgument(Argument::TextureCapacity) => {
                "too many distinct textures in one frame for the capacity reserved at creation"
            }
            Self::InvalidArgument(Argument::FrameIndex) => {
                "frame index beyond what the mesh carries: see scg_mesh_frame_count"
            }
            Self::InvalidArgument(Argument::FrameFactor) => {
                "interpolation factor must be finite and within [0, 1]: it is never clamped, since extrapolating is the game's decision"
            }
            Self::InvalidArgument(Argument::TextureCoordinate) => {
                "texture coordinate is not finite, or beyond 16384 texels"
            }
            Self::InvalidArgument(Argument::TextureSize) => {
                "invalid texture size: each side must be a power of two between 1 and 2048"
            }
            Self::InvalidArgument(Argument::Overbright) => {
                "invalid overbright shift: must be 0, 1 or 2"
            }
            Self::InvalidArgument(Argument::Fog) => {
                "invalid fog range: start must be finite and not negative, end finite and beyond start"
            }
            Self::InvalidArgument(Argument::LightCapacity) => {
                "too many dynamic lights for one frame"
            }
            Self::InvalidArgument(Argument::Light) => {
                "invalid light: position must be finite, and radius finite and positive"
            }
            Self::InvalidArgument(Argument::Grade) => {
                "invalid output curve: gamma must be within ]0, 8], each channel gain within [0, 4], \
                 each channel offset within [-1, 1]"
            }
            Self::InvalidArgument(Argument::TextureLength) => {
                "pixel block of the wrong length: needs width x height x 4 bytes"
            }
            Self::OutOfMemory => "out of memory",
            // Un seul message pour toute la variante, et il ne nomme aucun cas :
            // le noyau ne distingue pas une tuile déjà rendue d'une soumission
            // pendant le rendu, et un texte qui parlerait de tuiles enverrait sur
            // une fausse piste l'hôte qui a simplement soumis trop tard.
            Self::InvalidState => {
                "call out of sequence: check the frame state and whether this tile was already rendered"
            }
            Self::Faulted => "a tile did not return from rendering; this frame is incomplete",
            Self::InvalidFormat(Malformation::Truncated) => {
                "malformed data file: a field, the section table or a section runs past the end of the block"
            }
            Self::InvalidFormat(Malformation::Signature) => {
                "not a Screengine data file: the four signature bytes do not match"
            }
            Self::InvalidFormat(Malformation::Kind) => {
                "wrong kind of data file: a mesh was given where a world was expected, or the reverse"
            }
            Self::InvalidFormat(Malformation::Length) => {
                "malformed data file: the declared total length is not the length of the block received"
            }
            Self::InvalidFormat(Malformation::SectionKind) => {
                "malformed data file: a section of a kind this version does not know"
            }
            Self::InvalidFormat(Malformation::SectionOrder) => {
                "malformed data file: sections must be in increasing kind order, at most one of each"
            }
            Self::InvalidFormat(Malformation::SectionBounds) => {
                "malformed data file: sections must pave the file, with no gap and no overlap"
            }
            Self::InvalidFormat(Malformation::NonFinite) => {
                "malformed data file: a floating-point value is not finite"
            }
            Self::InvalidFormat(Malformation::NonUtf8) => {
                "malformed data file: a name is not valid UTF-8"
            }
            // **Les neuf variantes qui portent un élément l'ignorent ici**, et
            // c'est la clause qui les gouverne toutes : le message est un
            // littéral, parce que `scg_last_error` le rend à travers la frontière
            // et que le noyau n'alloue pas. Le noyau nomme donc la catégorie, la
            // frontière formaterait le texte d'un hôte si le besoin s'en
            // présentait, et c'est l'appelant Rust qui lit la charge utile.
            Self::InvalidFormat(Malformation::Index { .. }) => {
                "malformed data file: an index is beyond what it points into"
            }
            Self::InvalidFormat(Malformation::Identifier { .. }) => {
                "malformed data file: an identifier is zero, or used twice in its family"
            }
            Self::InvalidFormat(Malformation::GroupBounds { .. }) => {
                "malformed data file: surface groups must pave the triangles in order, with no gap and no overlap"
            }
            Self::InvalidFormat(Malformation::Count) => {
                "malformed data file: a count does not match the length that bounds it"
            }
            Self::InvalidFormat(Malformation::Flags { .. }) => {
                "malformed data file: undefined flag bits must be zero"
            }
            Self::InvalidFormat(Malformation::Polygon { .. }) => {
                "malformed data file: a polygon is degenerate, too large, or not convex where convexity is required"
            }
            Self::InvalidFormat(Malformation::Mapping { .. }) => {
                "malformed data file: a mapping frame is unusable, or the coordinates it derives are not finite"
            }
            Self::InvalidFormat(Malformation::Light { .. }) => {
                "malformed data file: a static light has a radius that is not finite and positive"
            }
            Self::InvalidFormat(Malformation::Pose { .. }) => {
                "malformed data file: an orientation is a zero quaternion, which carries no direction"
            }
            Self::InvalidFormat(Malformation::Portal { .. }) => {
                "malformed data file: three portals share the same vertices"
            }
            Self::UnsupportedFormatVersion => {
                "unsupported data format version: take a newer library, or export the data again"
            }
            Self::UnknownResource => "unknown identifier: the resource holds no such cell",
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.message())
    }
}

/// Le trait d'erreur de `core`, pour qu'un appelant Rust propage par `?` vers un
/// `Box<dyn Error>` sans avoir à envelopper.
///
/// `core::error::Error` et non celui de `std` : le même trait, réexporté là-bas,
/// et c'est ce qui permet au noyau de l'implémenter sans rien savoir du système.
/// Aucune `source` — les variantes ne contiennent rien d'autre qu'elles-mêmes.
impl core::error::Error for Error {}

/// Le résultat d'un appel du noyau.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests;
