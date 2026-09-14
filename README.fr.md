# Screengine

English: [README.md](README.md)

Un moteur de rendu 3D logiciel, appelable depuis n'importe quel langage. Pas de
GPU, pas de fenêtre : on lui donne une scène et un tampon, il remplit le tampon.

MIT — voir [`LICENSE`](LICENSE).

Le rendu est celui des jeux de la fin des années 90, et c'est un choix : tampon
indexé 256 couleurs, atténuation par table, texture affine corrigée par
segments, aucun filtrage, résolution interne basse remontée en entier. Ces
défauts-là ne se rajoutent pas après coup, ils tiennent au pipeline.

## Ce qu'il ne fait pas

C'est ce qui le rend intégrable, donc ça vient avant le reste. Il n'ouvre aucune
fenêtre, ne lit aucun clavier, n'ouvre aucun fichier, ne joue aucun son et ne
connaît aucune notion de jeu — ni joueur, ni arme, ni score.

L'hôte fournit la fenêtre, les entrées et les octets. Le moteur transforme et
rend. Cette frontière est la seule raison pour laquelle le même code sert sous
Windows, dans un navigateur et sur un téléphone.

## État

**Rien n'est encore écrit.** Le projet démarre à l'étape 0 : afficher un
triangle depuis quatre hôtes — C, PHP, wasm, Android — avant toute ligne de
moteur. Tant que cette étape n'est pas franchie, la bibliothèque panique sur
`todo!`.

La feuille de route compte dix étapes, publiées à chacune.

- [`ROADMAP.md`](ROADMAP.md) — les étapes, celles qui sont franchies et ce qui
  est hors périmètre v1
- [`CHANGELOG.md`](CHANGELOG.md) — ce que chaque version a apporté, daté
- [`docs/abi.md`](docs/abi.md) — le contrat de la frontière C, qui fait foi
- [`docs/construction.md`](docs/construction.md) — cibles, matrice de
  compilation, génération du header

## Utilisation

Depuis Rust, l'API est idiomatique et le crate se lie directement. Depuis tout
le reste, la bibliothèque expose une ABI C stable, préfixée `scg_`, dont le
header est généré :

```c
ScgContext* ctx = scg_create(640, 360);
scg_camera_set(ctx, pos, yaw, pitch, fov);
scg_frame_begin(ctx);
scg_draw_mesh(ctx, mesh, matrix);
scg_frame_end(ctx, pixels, stride);
```

Dix-sept fonctions, des handles opaques, aucun callback, aucune allocation qui
traverse la frontière. Les liaisons vivent dans des dépôts séparés et ne
contiennent que de la conversion de types.

Sur le web, l'hôte ne peut pas fournir un pointeur arbitraire : il alloue son
tampon par `scg_buffer_alloc` et construit une vue dessus. C'est la seule
différence entre les plateformes.

## Construction

```
make build     # noyau et bibliothèque partagée
make header    # régénère include/screengine.h
make test
make conform   # rejoue les scènes de référence et compare les empreintes
make lint
make nostd     # preuve que le noyau compile sans std
```

Le noyau est `no_std` et n'a aucune dépendance. Un dépôt fraîchement cloné
compile sans rien installer d'autre qu'une chaîne Rust.

Les cibles Android et wasm exigent chacune leur outillage et passent par
l'intégration continue. iOS attend que le reste soit stable — voir la feuille de
route. [`docs/construction.md`](docs/construction.md) porte la
matrice complète.
