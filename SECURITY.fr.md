# Sécurité

English: [SECURITY.md](SECURITY.md)

## Versions suivies

La dernière version publiée. En `0.x`, il n'y a pas de branche de maintenance :
un correctif sort dans la version suivante.

## Signaler une faille

Par un avis de sécurité privé sur le dépôt GitHub, jamais par une issue
publique. Réponse sous quelques jours.

## Ce qui est dans le périmètre

La bibliothèque décode des octets que l'hôte lui remet et qu'il n'a pas écrits.
C'est la seule surface d'attaque réelle, et c'est celle qui compte — d'autant
qu'elle s'exécute dans le processus de l'hôte, pas dans le sien.

**Aujourd'hui, ce sont les blocs de texels d'une texture**, et eux seuls. Les
formats de carte et de maillage n'existent pas encore : ils sont l'étape 4 de la
feuille de route, et cette section les nommera quand ils arriveront plutôt que
de les promettre d'avance — un périmètre qui annonce plus que le code ne fait
gaspille le temps de celui qui rapporte.

- Une description de texture ou un bloc de texels qui fait planter le
  chargement, lire hors bornes, ou déborder un entier sur le chemin d'une taille
  d'allocation.
- Un dépassement de tampon, une lecture hors bornes ou un débordement d'entier
  atteignable depuis une entrée malformée.
- Un décalage entre ce que `docs/abi.md` garantit et ce que le code fait : un
  point d'entrée qui écrit au-delà du tampon annoncé, ou qui laisse échapper une
  panique vers l'appelant.
- Tout ce qui ferait exécuter du code depuis un contenu chargé — **rien de ce
  que la bibliothèque charge ne le permet, et c'est un invariant** : une texture
  est un bloc de texels, et les formats à venir ne contiendront que des
  identifiants, des positions et des dimensions. Aucun binaire, aucun script,
  aucun chemin de fichier.

## Ce qui n'y est pas

**Un hôte qui viole les préconditions de l'ABI.** Passer un pointeur invalide,
un `stride` plus petit que la largeur, un handle déjà détruit : ces conditions
sont documentées, et les respecter est la responsabilité de l'appelant. C'est la
nature d'une frontière C, pas un défaut de la bibliothèque.

Modifier ses propres fichiers pour obtenir un rendu différent. Rien ici n'est à
protéger contre son propriétaire.
