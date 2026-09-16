# Journal des versions

Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/).

SemVer avec la clause du zéro : **en `0.x`, rien n'est imposé**. Le mineur
marque une étape de la feuille de route, pas une rupture d'API — tout le reste
s'accumule en correctif, correctifs, fonctionnalités et ruptures confondus.

Trois numéros à ne pas confondre :

| Numéro | Où | Ce qu'il suit |
|---|---|---|
| version du dépôt | tag git | la bibliothèque |
| `SCG_ABI_VERSION` | `include/screengine.h` et `scg_abi_version()` | la frontière C |
| `version_format` | chaque carte et chaque maillage | le format de fichier |

Les deux derniers ne suivent pas SemVer. Ce sont des entiers : ajouter une
fonction à l'ABI ou un champ optionnel à un format ne les incrémente pas, tout
le reste les incrémente, et un incrément de `version_format` oblige à écrire la
migration des fichiers existants. Une version peut sortir sans qu'ils bougent ;
ils ne bougent jamais sans version.

Un titre de section s'écrit `## [version] — date — titre`. La publication en
tire le nom et les notes de la version : ce qui est relu ici est ce qui sera lu
sur la page des versions, et il n'y a rien à recopier ensuite. Le titre est
facultatif ; sans lui, la version se nomme par son tag.

**Une section absente arrête la publication.** Une version sans notes ne dit ni
ce qui change, ni ce qu'un auteur de liaison doit reprendre.

**Les notes d'une version qui ne rend encore rien doivent dire ce qu'elle ne
fait pas.** Quelqu'un compile une bibliothèque qui panique sur `todo!`, et sans
cette phrase il croit à un défaut.

La section en cours s'écrit `## [Non publié]`, sans date ni titre, et chaque lot
y ajoute ce qu'il change : écrite au fil de l'eau, elle est relue en même temps
que le code qu'elle décrit, alors qu'une section rédigée le jour du tag résume
de mémoire un mois de travail. **Elle prend son numéro et sa date au moment du
tag**, faute de quoi la publication cherche une section qui n'existe pas et
s'arrête.

Chaque section est **bilingue, français d'abord, séparé par `***`** — les notes
de version sont ce que lit un auteur de liaison étranger avant de savoir s'il
doit reprendre son travail. Ce préambule reste en français : il n'est jamais
publié, et explique les conventions du dépôt à qui y contribue.

## [Non publié]

Aucun rendu : la frontière C existe, mais la fin d'image panique — le
remplissage n'est pas écrit. Un hôte reçoit `SCG_ERR_PANIC`, puis
`SCG_ERR_POISONED` sur le contexte devenu inutilisable.

### Ajouté
- Sept points d'entrée : version d'ABI, création et destruction du contexte,
  fin d'image, dernier message d'erreur, allocation et libération d'un tampon.
  Chacun passe par une enveloppe unique qui fixe l'environnement flottant,
  rattrape les paniques et traduit l'erreur en code de retour.
- Contrôle des décalages de `ScgContextConfig` des deux côtés de la frontière :
  un test Rust, et des assertions statiques que le compilateur de l'hôte vérifie
  sur sa propre cible.
- Espace de travail en quatre crates : noyau sans std, frontière C, hôte de
  développement, suite de conformance.
- Contrat d'ABI, conventions Rust et documentation de construction.
- Intégration continue et publication sur tag.

### Modifié
- Licence MIT seule.
- Cible de rendu : la classe des moteurs logiciels de 1996 à 1998 plutôt que
  palette et texture affine. Couleurs directes, mipmaps, lightmaps, cellules 3D,
  rendu par tuiles, virgule fixe après la projection.
- Le noyau est le paquet racine du dépôt ; frontière C, hôte et conformance
  restent dans `crates/`.
- Frontière C arrêtée : codes de retour entiers à plages réservées par étape,
  message d'erreur en anglais, objet empoisonné après une panique, structures
  figées et complétées par des champs réservés, tampon de sortie en RGBA. Le
  header et les hôtes s'écrivent contre elle.

***

No rendering yet: the C boundary exists, but ending a frame panics — the fill
is not written. A host gets `SCG_ERR_PANIC`, then `SCG_ERR_POISONED` on the
context, now unusable.

### Added
- Seven entry points: ABI version, context creation and destruction, frame end,
  last error message, buffer allocation and release. Each goes through a single
  wrapper that pins the floating-point environment, catches panics and turns
  errors into return codes.
- `ScgContextConfig` offsets checked on both sides of the boundary: a Rust test,
  and static assertions the host's compiler verifies on its own target.
- Four-crate workspace: std-free core, C boundary, development host,
  conformance suite.
- ABI contract, Rust conventions and build documentation.
- Continuous integration and tag-triggered release.

### Changed
- MIT licence only.
- Rendering target: the 1996–1998 software renderer class instead of palette
  and affine texturing. True colour, mipmaps, lightmaps, 3D cells, tile-based
  rendering, fixed-point after projection.
- The core is the repository's root package; C boundary, host and conformance
  stay in `crates/`.
- C boundary settled: integer return codes in per-stage reserved ranges, English
  error message, object poisoned after a panic, frozen structures extended
  through reserved fields, RGBA output buffer. The header and the hosts are
  written against it.
