# Sécurité

English: [SECURITY.md](SECURITY.md)

## Versions suivies

La dernière version publiée. En `0.x`, il n'y a pas de branche de maintenance :
un correctif sort dans la version suivante.

## Signaler une faille

Par un avis de sécurité privé sur le dépôt GitHub, jamais par une issue
publique. Réponse sous quelques jours.

## Ce qui est dans le périmètre

La bibliothèque décode des octets que l'hôte lui remet et qu'il n'a pas écrits :
cartes, maillages, textures. C'est la seule surface d'attaque réelle, et c'est
celle qui compte — d'autant qu'elle s'exécute dans le processus de l'hôte, pas
dans le sien.

- Une carte ou un maillage qui fait planter le décodeur, boucler indéfiniment ou
  consommer toute la mémoire au chargement.
- Un dépassement de tampon, une lecture hors bornes ou un débordement d'entier
  atteignable depuis un fichier malformé.
- Un décalage entre ce que `docs/abi.md` garantit et ce que le code fait : un
  point d'entrée qui écrit au-delà du tampon annoncé, ou qui laisse échapper une
  panique vers l'appelant.
- Tout ce qui ferait exécuter du code depuis un contenu chargé — **rien dans les
  formats ne le permet, et c'est un invariant** : une carte ne contient que des
  identifiants, des positions et des dimensions, aucun binaire, aucun script,
  aucun chemin de fichier.

## Ce qui n'y est pas

**Un hôte qui viole les préconditions de l'ABI.** Passer un pointeur invalide,
un `stride` plus petit que la largeur, un handle déjà détruit : ces conditions
sont documentées, et les respecter est la responsabilité de l'appelant. C'est la
nature d'une frontière C, pas un défaut de la bibliothèque.

Modifier ses propres fichiers pour obtenir un rendu différent. Rien ici n'est à
protéger contre son propriétaire.
