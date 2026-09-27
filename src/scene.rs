// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ce qu'une image reçoit : une caméra, des sommets, des triangles colorés.
//!
//! Rien du jeu n'entre ici. Ce sont les seules données que le moteur connaisse
//! d'une scène, et chacune a son équivalent exact dans l'ABI C : ce que l'API
//! Rust permet de décrire, une liaison le décrit aussi.

use crate::math::{Affine3, Angle, Quat, Vec3};

/// L'orientation des axes de vue que donne le quaternion identité.
///
/// Le monde est en main droite, Z en haut ; le repère de vue est X à droite, Y
/// vers le bas, Z vers l'avant. Les deux ne coïncident donc jamais, et il faut
/// dire lequel des deux l'identité aligne : ici, ses colonnes sont les images
/// des axes de vue dans le monde.
///
/// ```text
/// X_vue (droite) ↦ −Y monde
/// Y_vue (bas)    ↦ −Z monde
/// Z_vue (avant)  ↦ +X monde
/// ```
///
/// Une caméra d'orientation neutre regarde donc le +X du monde, le haut de
/// l'écran vers le zénith. Écarté : aligner naïvement les axes de vue sur ceux
/// du monde, qui ferait regarder le zénith par défaut et poserait le nord au
/// bas de l'écran. Le déterminant vaut un, donc la composition reste une
/// transformation rigide et [`Affine3::inverse_rigid`] garde sa précondition.
///
/// Une phrase — « la caméra regarde le +X » — ne suffirait pas : elle laisse le
/// roulis indéterminé. Les trois colonnes le fixent.
const VIEW_BASIS: Affine3 = Affine3 {
    m: [
        0.0, -1.0, 0.0, // X de vue
        0.0, 0.0, -1.0, // Y de vue
        1.0, 0.0, 0.0, // Z de vue
        0.0, 0.0, 0.0,
    ],
};

/// D'où l'on regarde, et avec quelle ouverture.
///
/// Position et orientation plutôt qu'une matrice de vue : une matrice laisserait
/// l'hôte composer lui-même l'inverse de la pose, donc normaliser un quaternion,
/// donc appeler sa libm — et les empreintes cesseraient d'être comparables d'une
/// liaison à l'autre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Sa position dans le monde.
    pub position: Vec3,
    /// Son orientation, normalisée à la réception. Voir [`Camera::view`] pour
    /// ce que l'identité désigne.
    pub orientation: Quat,
    /// Le champ de vision vertical, en radians, dans `]0, π[`.
    pub fov_y: f32,
    /// Le plan proche, strictement positif.
    pub near: f32,
}

impl Camera {
    /// Le champ de vision vertical par défaut, soixante degrés.
    pub const DEFAULT_FOV_Y: f32 = core::f32::consts::FRAC_PI_3;

    /// Le plan proche par défaut, en unités de monde.
    ///
    /// Assez près pour qu'un mur frôlé ne disparaisse pas, assez loin pour que
    /// la profondeur `near/w` garde de la résolution au fond d'un couloir.
    pub const DEFAULT_NEAR: f32 = 0.1;

    /// À l'origine, regardant le +X du monde.
    pub const DEFAULT: Self = Self {
        position: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        fov_y: Self::DEFAULT_FOV_Y,
        near: Self::DEFAULT_NEAR,
    };

    /// La transformation du monde vers l'espace de vue.
    ///
    /// La pose de la caméra dans le monde, composée avec [`VIEW_BASIS`], puis
    /// inversée. La composition est exacte au bit près — chaque coefficient du
    /// résultat est un coefficient de la rotation, au signe près, les autres
    /// termes étant des produits par zéro —, si bien qu'une caméra neutre rend
    /// une permutation d'axes et rien de plus.
    pub fn view(&self) -> Affine3 {
        let pose = Affine3::from_rotation_translation(self.orientation.normalize(), self.position);
        pose.product(VIEW_BASIS).inverse_rigid()
    }
}

/// Une couleur, quatre octets dans l'ordre où l'ABI les écrit en mémoire.
///
/// Quatre champs nommés et non un `u32` : l'entier que le rasteriseur manipule
/// s'écrit `0xAABBGGRR`, parce que c'est ce que sa conversion en octets
/// petit-boutistes impose. Un littéral pris pour du `0xAARRGGBB` échange le
/// rouge et le bleu sans que rien ne le signale — le défaut que la disposition
/// des pixels d'Android produit déjà chez les hôtes qui s'y trompent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    /// Rouge.
    pub r: u8,
    /// Vert.
    pub g: u8,
    /// Bleu.
    pub b: u8,
    /// Alpha. **Il ne ressort pas** : la sortie force l'opacité, et ce champ
    /// n'existe que parce qu'une couleur en compte quatre.
    pub a: u8,
}

impl Color {
    /// Une couleur de composantes données.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// La forme qu'en garde le rasteriseur.
    pub(crate) const fn packed(self) -> u32 {
        u32::from_le_bytes([self.r, self.g, self.b, self.a])
    }
}

/// Un sommet qui porte, en plus de sa position, un point de la texture.
///
/// **Les coordonnées vivent dans le sommet et non aux coins du triangle.**
/// Dupliquer un sommet là où l'habillage se coupe est ce que fait tout maillage
/// texturé ; porter six coordonnées par triangle coûterait davantage dès qu'un
/// sommet est partagé, ce qui est le cas courant sur une surface continue.
///
/// **En texels, jamais normalisées.** C'est le seul choix qui rende le bornage
/// vérifiable sur le tableau de sommets seul : normalisées, la borne dépendrait
/// de la texture avec laquelle le lot est finalement dessiné, donc d'un
/// paramètre que la validation du tableau n'a pas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexUv {
    /// Sa position, dans le repère de l'objet.
    pub position: Vec3,
    /// L'abscisse dans la texture, en texels.
    pub u: f32,
    /// L'ordonnée dans la texture, en texels.
    pub v: f32,
}

impl VertexUv {
    /// Le sommet d'une soumission sans texture, dont les coordonnées sont
    /// nulles.
    ///
    /// C'est ainsi que le chemin non texturé rejoint le chemin général : un
    /// seul corps de soumission, et pas deux à tenir accordés.
    pub const fn untextured(position: Vec3) -> Self {
        Self {
            position,
            u: 0.0,
            v: 0.0,
        }
    }
}

/// Un sommet qui porte un second jeu de coordonnées, celui de la lightmap.
///
/// **Un type à part plutôt que deux champs de plus sur [`VertexUv`]** : celui-ci
/// traverse déjà l'ABI sous sa forme publiée, qu'on n'élargit pas. Un hôte qui
/// n'éclaire rien continue d'écrire des sommets de quatre flottants au lieu de
/// six, et les surfaces éclairées se soumettent par leur propre chemin.
///
/// Les deux jeux sont indépendants : une même surface s'habille d'une texture
/// répétée plusieurs fois et d'une lightmap étirée une seule fois sur toute son
/// étendue, ce qui est précisément la raison d'être du second jeu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexUv2 {
    /// Sa position, dans le repère de l'objet.
    pub position: Vec3,
    /// L'abscisse dans la texture, en texels.
    pub u: f32,
    /// L'ordonnée dans la texture, en texels.
    pub v: f32,
    /// L'abscisse dans la lightmap, en texels de lightmap.
    pub u2: f32,
    /// L'ordonnée dans la lightmap, en texels de lightmap.
    pub v2: f32,
    /// Sa normale, ou le vecteur nul quand le lot n'en porte pas.
    ///
    /// **Le nul plutôt qu'un `Option`** : celui-ci ferait seize octets là où
    /// douze suffisent, sur une structure qui vit trois par trois sur la pile
    /// de la soumission, et le test se fait une fois par sommet et par lumière
    /// de toute façon. Une normale absente et une normale dégénérée reçoivent
    /// alors le même traitement, ce qui est juste : ni l'une ni l'autre ne dit
    /// vers où la surface regarde.
    pub normal: Vec3,
}

impl VertexUv2 {
    /// Le sommet d'une soumission sans éclairage, dont le second jeu est nul.
    ///
    /// C'est par là que le chemin ordinaire rejoint le chemin général : la
    /// soumission travaille sur un seul type de sommet, et les plans du second
    /// jeu ne se construisent que lorsque le lot en porte.
    pub const fn unlit(v: VertexUv) -> Self {
        Self {
            position: v.position,
            u: v.u,
            v: v.v,
            u2: 0.0,
            v2: 0.0,
            normal: Vec3::ZERO,
        }
    }

    /// Le même, avec la normale que le lot porte pour ce sommet.
    pub const fn shaded(v: VertexUv, normal: Vec3) -> Self {
        Self {
            normal,
            ..Self::unlit(v)
        }
    }
}

/// Un triangle soumis : trois indices dans le tableau de sommets, et sa couleur.
///
/// La couleur est portée par le triangle et non par le lot : une surface entière
/// se soumet alors en un seul franchissement de la frontière, ce qui est tout
/// l'objet de la soumission par lot. Les indices ne sont pas facultatifs — un
/// hôte sans maillage indexé écrit `0, 1, 2` puis `3, 4, 5`, et le moteur n'a
/// pas deux chemins à tenir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triangle {
    /// Les trois sommets, en sens antihoraire vus de la face avant.
    pub indices: [u32; 3],
    /// Sa couleur, uniforme sur toute sa surface. **Ignorée dès que le lot
    /// porte une texture** : c'est alors le texel qui décide, et la couleur ne
    /// le teinte pas.
    pub color: Color,
}

/// Une lumière ponctuelle, qui s'ajoute à l'éclairage d'une surface.
///
/// **Son atténuation se calcule par sommet, pas par pixel.** Il n'existe aucune
/// distance du côté entier du pipeline — la virgule fixe commence à la
/// projection, et la racine inverse du noyau vit avant elle. Un mur de deux
/// triangles rend donc un dégradé entre ses coins et non un halo : une source
/// qui doit s'y déplacer demande un mur découpé en panneaux.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// Sa position dans le monde.
    pub position: Vec3,
    /// Son rayon, au-delà duquel elle n'éclaire plus rien.
    ///
    /// Strictement positif. L'atténuation s'y annule **avec une dérivée
    /// nulle**, si bien qu'aucun anneau ne marque le bord — ce que ferait une
    /// atténuation linéaire en `d²`.
    pub radius: f32,
    /// Sa couleur, à pleine intensité au centre.
    ///
    /// L'alpha est ignoré : une lumière s'ajoute, elle ne se mélange pas.
    pub color: Color,
}

/// Comment le moteur oriente un quadrilatère.
///
/// **Les deux servent, et le choix n'est pas affaire de goût.** Un personnage
/// debout regardé d'en haut se couche avec [`Facing`], ce qui est une image
/// fausse, et s'aplatit avec [`Axial`], ce qui n'est qu'une silhouette perdue.
/// Une lueur ou une étincelle veut au contraire [`Facing`], n'ayant pas de
/// haut.
///
/// [`Facing`]: SpriteOrientation::Facing
/// [`Axial`]: SpriteOrientation::Axial
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteOrientation {
    /// Le quadrilatère tourne autour du **Z du monde** seulement : il reste
    /// debout.
    ///
    /// Dégénère quand la caméra regarde à la verticale exacte — le
    /// quadrilatère disparaît alors, sans erreur, comme un triangle qui ne se
    /// projette pas. C'est une donnée, pas un cas d'erreur.
    Axial,
    /// Le quadrilatère se met plein face à la caméra.
    Facing,
}

/// Un quadrilatère que le moteur oriente sur la caméra.
///
/// **C'est une description, pas une forme de sommet.** En aval un sprite ne
/// produit que des [`VertexUv`] : le rasteriseur en a exactement trois formes et
/// n'en gagne pas une quatrième.
///
/// Ce que l'hôte ne peut pas faire sans calculer, et qui justifie que le moteur
/// s'en charge, c'est l'orientation : elle exige la base de la caméra, que
/// l'hôte devrait obtenir en réinversant la pose qu'il vient lui-même de
/// passer, donc en normalisant un quaternion par sa propre bibliothèque
/// mathématique. Deux liaisons ne rendraient plus la même image. Un
/// quadrilatère à **orientation libre** — une affiche, un impact, une tache au
/// sol — n'a pas ce problème et se soumet par les chemins texturés ordinaires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    /// Le centre, dans le repère de l'objet.
    pub center: Vec3,
    /// La demi-largeur, **en unités du monde**.
    ///
    /// Jamais en pixels : la taille dépendrait alors de la résolution interne,
    /// que l'hôte change en cours de partie.
    pub half_width: f32,
    /// La demi-hauteur, en unités du monde.
    pub half_height: f32,
    /// Le coin bas-gauche du rectangle de texture, en texels.
    pub u0: f32,
    /// L'ordonnée du même coin, en texels.
    pub v0: f32,
    /// Le coin haut-droit du rectangle de texture, en texels.
    pub u1: f32,
    /// L'ordonnée du même coin, en texels.
    pub v1: f32,
    /// Le roulis, angle binaire où 2³² vaut un tour.
    ///
    /// Il tourne le quadrilatère **dans son propre plan**, après l'orientation
    /// et avant la projection, et vaut pour les deux modes. Zéro est son
    /// neutre. C'est le format d'angle du noyau : l'hôte n'a aucune
    /// trigonométrie à faire, et aucune libm n'entre dans l'image.
    pub roll: Angle,
    /// Sa couleur, de même sens que celle d'un [`Triangle`] — ignorée dès que
    /// le lot porte une texture.
    pub color: Color,
}

#[cfg(test)]
mod tests;
