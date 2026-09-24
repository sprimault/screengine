# Contribuer

English: [CONTRIBUTING.md](CONTRIBUTING.md)

## Comment ce projet est écrit

Le code est écrit en binôme avec un assistant, sous les règles de ce dépôt. Ce
n'est pas une note de bas de page : la méthode est le second objet du projet, et
la façon dont les règles sont posées en découle.

- **Les règles précèdent le code**, elles n'en sont pas déduites.
  [`docs/abi.md`](docs/abi.md) fait foi, et un désaccord entre le document et le
  code est un défaut du code.
- **Une décision porte ce qu'elle écarte.** Chaque arbitrage garde le motif des
  options rejetées, pour qu'on puisse le rouvrir sans le rejouer.
- **Ce qui se mesure se mesure.** Sur un rasteriseur, le raisonnement se trompe
  souvent et l'empreinte tranche.
- **Les messages de commit ne racontent pas la fabrication.** Ils disent ce qui
  change et pourquoi — le reste est dans le diff.

Rien de cela ne s'applique différemment à une contribution extérieure : mêmes
contrôles, mêmes règles de style. Seul le bilingue lui est épargné — voir
« Langue ».

## Avant d'écrire du code

Ouvrir une issue d'abord, pour tout ce qui dépasse une correction. Le contrat de
la frontière est écrit dans [`docs/abi.md`](docs/abi.md) ; le changer est une
discussion, pas un correctif.

## Ce qui se discute avant d'être écrit

Une pull request qui touche l'un de ces chemins sans discussion préalable sera
renvoyée à une issue, quelle que soit sa qualité — non par principe, mais parce
que ce sont les endroits où une modification en fait basculer d'autres.

- **[`docs/abi.md`](docs/abi.md)** fait foi. Le code s'y conforme, donc en
  changer une ligne change ce que le code doit faire.
- **`include/screengine.h`** est un contrat public : un hôte compilé contre lui
  il y a six mois doit continuer à se lier. On ajoute des fonctions, on ne
  modifie jamais une signature publiée. Le fichier est **généré** — une édition
  manuelle est un défaut, et la CI la refuse.
- **Le format de carte et de maillage** est un contrat public au même titre : un
  niveau produit aujourd'hui doit se charger demain. Un champ ajouté est
  optionnel, un champ retiré ou renommé casse tout ce qui circule.
- **`crates/screengine-conformance/references/`** décide de ce que le rendu doit
  produire. Une empreinte modifiée en même temps que le code qui la fait changer
  ne se relit pas : le lot et la mise à jour des références sont deux commits.
- **`.github/workflows/`** décide de ce qui est vérifié. La protection de branche
  exige les contrôles par leur nom, pas par leur contenu : un workflow modifié
  peut rendre vert un contrôle qui ne vérifie plus rien.

Le reste — code, tests, documentation d'accompagnement — se propose directement.

## Ce sur quoi une contribution est jugée

Les conventions de code et la doctrine de test sont dans
[`docs/rust.md`](docs/rust.md). Ce qui suit en est le résumé exigible.

- `make fmt && make lint && make test && make conform && make nostd && make header-verif && make deny && make audit` passent.
- Toute déclaration a sa documentation. Les commentaires disent *pourquoi*, ils
  ne paraphrasent jamais la ligne suivante.
- Pas de bannière, pas d'emoji décoratif, ni dans le code, ni dans les messages
  de commit.
- **Le noyau reste `no_std` et sans dépendance.** Un `use std::` ajouté dans
  `src/` est un défaut, même s'il compile sur le poste de son auteur.
- **Aucune allocation par image.** Toute allocation a lieu dans un appel nommé —
  création du contexte, chargement d'une ressource, calcul de lightmaps —, jamais
  pendant une image ; les tampons de travail se réutilisent par `clear`, jamais
  par réallocation.
- **Aucun appel à la libm.** Trigonométrie et racine inverse passent par les
  tables du noyau. Un `f32::sin` introduit fait diverger les empreintes entre
  plateformes, et l'écart n'apparaît que sur la cible qu'on ne construit pas
  soi-même.
- **`unsafe` uniquement dans `screengine-ffi` et les chemins SIMD**, avec un
  commentaire qui nomme l'invariant tenu par l'appelant.
- **Tout point d'entrée FFI est enveloppé de `catch_unwind`.** Une panique qui
  traverse la frontière est un comportement indéfini, pas un plantage propre.
- Rien dans le noyau n'importe `screengine-ffi`. Les runners sont sans
  écran ; un test qui exige une fenêtre n'a pas sa place dans la suite par
  défaut.
- Une dépendance qui voyage dans une archive publiée entre dans
  `THIRD-PARTY-NOTICES` — et dans le noyau, elle ne rentre pas. Le fichier
  couvre aussi la bibliothèque standard de Rust, liée dans chaque
  bibliothèque : à chaque relèvement de `rust-version`, relire les dépendances
  de `library/std/Cargo.toml` et y reporter ce qui change.

## Livraison

**Un lot, une branche, un commit.** La branche part de `master` à jour et se
nomme `<type>/<sujet>`, où le type est le préfixe conventionnel de son commit :
`feat/`, `fix/`, `docs/`, `chore/`, `test/`, `refactor/`. Ne pas enchaîner deux
lots sur la même branche — chacun doit rester relisible et annulable seul.

Elle retourne dans `master` **par une pull request**, jamais par une fusion
locale : c'est la PR qui laisse la trace de ce qui a été livré, et sa fusion qui
supprime la branche des deux côtés.

**Vérifier avant de pousser, pas après :**

```
make fmt && make lint && make test && make conform && make nostd && make header-verif && make deny && make audit
```

**La liste est fixe et se passe entière**, jamais réduite à ce qui touche au
changement qu'on vient d'écrire. Composer sa liste revient à ne vérifier que ce
qu'on a déjà en tête, et le défaut est ailleurs par construction : s'il avait été
là où l'on regardait, on l'aurait vu en écrivant. **Celui qui trouve est celui
qu'on n'avait pas de raison de lancer.**

`make nostd` et `make header-verif` sont les deux qu'on est tenté de sauter parce
qu'ils passent toujours. Ce sont aussi les deux dont l'échec se paie le plus
cher : découvert au portage, un `use std::` oublié a déjà trois semaines de code
construit dessus.

Un contrôle s'ajoute dès qu'un hôte est touché :

```
make hosts
```

`cargo audit` interroge sa base d'avis **en direct** : un job vert le matin peut
être rouge l'après-midi sur exactement le même code. Ne pas se reposer sur
l'intégration continue seule, qui valide une fois la branche déjà poussée.

**La section du `CHANGELOG` part avec le lot**, pas au moment du tag : elle est
relue en pull request, donc au moment où elle compte. La publication en tire le
nom et les notes de la version, et une section absente l'arrête.

**Le tag ne publie pas.** Il déclenche la construction des archives et crée la
release **en brouillon** ; la rendre visible demande un geste de plus, une fois
les notes relues telles qu'un intégrateur les lira. Tant qu'il n'est pas fait,
les archives existent sans que personne ne les trouve. Voir
[`docs/construction.md`](docs/construction.md), section « Publication ».

**La documentation part avec le changement.** Avant de commiter, vérifier ce que
le changement rend faux ailleurs : l'état annoncé dans le README, une clause de
[`docs/abi.md`](docs/abi.md), une étape de [`ROADMAP.md`](ROADMAP.md).

**Un message dit ce qui change et pourquoi**, en quelques lignes. Le défaut est
le titre seul : un corps n'existe que s'il porte quelque chose que le titre ne
dit pas et que le diff ne montre pas.

## Corriger une vulnérabilité sans en créer une autre

Ne pas adopter une version publiée **le jour même**, même corrective. Chercher la
plus ancienne qui suffit :

```
cargo search <crate> --limit 1
cargo tree -i <crate>
```

Une version parue dans l'heure est le profil type d'une compromission de compte
mainteneur.

Un épinglage s'explique : une dépendance figée plus bas que le dernier
disponible porte un commentaire de fin de ligne disant pourquoi, et **quand le
retirer**.

## Trois numéros, à ne pas confondre

| Numéro | Où | Ce qu'il suit |
|---|---|---|
| version du dépôt | tag git | la bibliothèque |
| `SCG_ABI_VERSION` | `include/screengine.h` et `scg_abi_version()` | la frontière C |
| `version_format` | chaque carte et chaque maillage | le format de fichier |

Les deux derniers ne suivent pas SemVer. Ce sont des entiers : ajouter une
fonction à l'ABI ou un champ optionnel à un format ne les incrémente pas, tout le
reste les incrémente, et un incrément de `version_format` oblige à écrire la
migration des fichiers existants.

`SCG_ABI_VERSION` existe pour qu'une liaison refuse proprement une bibliothèque
trop ancienne, plutôt que de se lier et de rendre n'importe quoi.

Le dépôt suit SemVer avec la clause du zéro, définie dans
[`CHANGELOG.md`](CHANGELOG.md) : **en `0.x`, rien n'est imposé.** Le mineur
marque une étape de [`ROADMAP.md`](ROADMAP.md), pas une rupture d'API ; tout le
reste s'accumule en correctif. Conséquence directe : **le numéro ne prévient de
rien**, et ce sont les notes de version qui doivent dire ce qu'un auteur de
liaison doit reprendre.

## Langue

**Les identifiants sont en anglais** — répertoires, fichiers, modules, types,
fonctions, champs. **La documentation est en français** : doc de module, doc
d'élément, commentaires, messages d'erreur. L'API se lit en anglais parce que
c'est du code ; le raisonnement se lit en français parce que c'est de la pensée.

**Une exception, et elle est structurante : ce qui franchit la frontière C.**
`cbindgen` recopie la documentation des éléments exportés dans
`include/screengine.h`, que lisent des auteurs de liaisons qui ne parlent pas
français ; le message rendu par `scg_last_error` est lu par les mêmes personnes.
Ces docstrings et ce message sont en anglais, et ce sont les seuls.

**Ce message n'est jamais localisé** — ni par `LC_MESSAGES`, ni par un paramètre
de langue qu'on ajouterait plus tard. Un texte qui change avec l'environnement
donne des journaux qu'on ne peut plus rapprocher d'un poste à l'autre.

Messages de commit en français d'abord, anglais ensuite, dans un seul texte
séparé par `***`. Jamais `---` : `git am` le traite comme un séparateur de patch
et tronque tout ce qui suit.

Les contributions en anglais sont bienvenues et ne sont pas soumises à la règle
bilingue.

## Liaisons

Les liaisons vers d'autres langages vivent dans des dépôts séparés, avec leur
propre rythme de publication, et **ne contiennent aucune logique** — uniquement
de la conversion de types.

Une liaison qui calcule quelque chose est une liaison qui divergera : le même
calcul existera bientôt en Go et en Python, avec deux comportements. Ce qui doit
être partagé remonte dans le noyau.

Une liaison est admise dans la liste officielle quand elle passe la suite de
conformance et que ses empreintes sont identiques à celles du chemin scalaire
natif.
