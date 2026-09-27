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

**Ce sont les blocs de texels d'une texture, les fichiers de maillage et de
carte, et les blocs de cache de lightmaps.** Les trois derniers sont arrivés
avec les étapes 4 et 5, et cette section les nomme désormais : un périmètre qui
annonce moins que le code ne fait décourage un rapport utile, autant qu'un
périmètre trop large gaspille le temps de celui qui le rédige.

Tout ce qui se trouve à l'intérieur des octets remis est tenu pour hostile, sans
exception. Que le couple pointeur-longueur couvre bien les octets annoncés reste
en revanche une précondition de l'appelant : le moteur ne peut pas le vérifier.

- Une description de texture ou un bloc de texels qui fait planter le
  chargement, lire hors bornes, ou déborder un entier sur le chemin d'une taille
  d'allocation.
- Un fichier de maillage, de carte ou de cache de lightmaps qui obtient le même
  effet — un compte qui ne recoupe pas la longueur de sa section, un indice hors
  bornes, une table de sections qui se recouvre, une taille d'allocation tirée
  d'un nombre déclaré.
- Un dépassement de tampon, une lecture hors bornes ou un débordement d'entier
  atteignable depuis une entrée malformée.
- Un décalage entre ce que `docs/abi.md` garantit et ce que le code fait : un
  point d'entrée qui écrit au-delà du tampon annoncé, ou qui laisse échapper une
  panique vers l'appelant.
- Tout ce qui ferait exécuter du code depuis un contenu chargé — **rien de ce
  que la bibliothèque charge ne le permet, et c'est un invariant** : une texture
  est un bloc de texels, et les formats ne portent que des identifiants, des
  positions, des dimensions et des octets que le moteur copie sans jamais les
  lire. Aucun binaire, aucun script, aucun chemin de fichier.

## Ce qui n'y est pas

**Un hôte qui viole les préconditions de l'ABI.** Passer un pointeur invalide,
un `stride` plus petit que la largeur, un handle déjà détruit : ces conditions
sont documentées, et les respecter est la responsabilité de l'appelant. C'est la
nature d'une frontière C, pas un défaut de la bibliothèque.

Modifier ses propres fichiers pour obtenir un rendu différent. Rien ici n'est à
protéger contre son propriétaire.
