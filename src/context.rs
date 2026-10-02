// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Le contexte de rendu et sa configuration.

mod frame;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use crate::buffer::reserved;
use crate::error::{Argument, Error, Result};
use crate::format::{Cell, Mesh, Pose, World};
use crate::light::MAX_OVERBRIGHT;
use crate::light::dynamic::{self, MAX_LIGHTS};
use crate::light::fog::Fog;
use crate::light::grade::Grade;
use crate::math::fixed::MAX_TEXEL_COORD;
use crate::math::projection::ClipVertex;
use crate::math::{Affine3, Projection, Vec3};
use crate::raster::{
    Bins, Grid, Lighting, MAX_CLIP_TRIANGLES, MODULATED, NO_LIGHTING, NO_TEXTURE, Prepared, Rect,
    Segment, Vertex, clip, clip_segment, prepare, prepare_lit,
};
use crate::scene::{
    Camera, Color, DepthMode, Light, Line, Point, Sprite, SpriteOrientation, Triangle, VertexUv,
    VertexUv2,
};
use crate::texture::{Filter, Texture};
use crate::world::atlas::GUTTER;
use crate::world::lighting::Lightmaps;
use crate::world::traversal::{MAX_VISITS, Visit, traverse};

pub use frame::{Frame, Output, Rows};

/// La plus grande résolution interne qu'un contexte accepte, en pixels de côté.
///
/// Ce n'est pas une limite de confort. Les formats en virgule fixe de
/// `docs/rust.md` calculent leurs pires cas sur cette borne : au-delà, une
/// fonction de bord déborderait avant que quoi que ce soit d'autre ne le
/// signale.
pub const MAX_RESOLUTION: u32 = 2048;

/// Les deux tailles de tuile admises, en pixels de côté.
///
/// Une tuile de 32 ou 64 tient en L1 avec sa profondeur. Au-delà, l'intérêt
/// principal des tuiles — le cache — disparaît, et le tampon de travail posé
/// sur la pile de chaque appel grossirait avec elle.
pub const TILE_SIZES: [u32; 2] = [32, 64];

/// Quatre octets par pixel, R, G, B puis A en mémoire.
pub const BYTES_PER_PIXEL: usize = 4;

/// Les triangles qu'une image peut recevoir.
///
/// La capacité de triangles par défaut, quand la configuration passe zéro.
pub const TRIANGLE_CAPACITY: usize = 16_384;

/// Les primitives de tracé qu'une image peut recevoir.
///
/// La capacité par défaut, quand la configuration passe zéro. Quatre fois moins
/// que les triangles : un éditeur trace des repères et des arêtes sélectionnées,
/// pas un décor, et une cellule de cinquante surfaces n'en demande que quelques
/// centaines.
pub const LINE_CAPACITY: usize = 4_096;

/// Le noir opaque dont chaque image part.
const CLEAR_COLOR: u32 = OPAQUE;

/// Le canal alpha à fond, dans l'ordre mémoire des pixels de sortie.
///
/// Le contrat d'ABI promet l'alpha écrit à 255 partout, et la sortie l'y force
/// plutôt que de le faire promettre à chaque appelant.
const OPAQUE: u32 = 0xFF00_0000;

/// Ce qu'une traversée a pu montrer de la carte.
///
/// Un succès dans les trois cas : rien ici n'est une faute, et un appelant qui
/// n'en fait rien obtient une image juste. C'est ce que la frontière traduit en
/// codes positifs — un succès accompagné d'un statut —, et c'est pour ce jour que
/// l'ABI les avait réservés.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Tout ce que la caméra voit a été soumis.
    Complete,
    /// L'exploration s'est arrêtée à une de ses bornes.
    ///
    /// **Ce qui manque n'est pas au même endroit selon la borne atteinte**, et le
    /// statut ne les distingue pas : à la profondeur, la cellule du fond est
    /// dessinée entière et seuls ses portails restent pliés, donc le trou commence
    /// une cellule plus loin ; à la borne de visites, la cellule n'est pas
    /// enregistrée, donc pas dessinée, et le parcours étant en profondeur elle peut
    /// être une voisine de la caméra.
    ///
    /// C'est une condition de décor, jamais une faute d'appel — la profondeur et le
    /// nombre de visites sont des constantes du noyau, et l'hôte n'a aucun levier
    /// dessus.
    Incomplete,
    /// Aucune cellule n'a été donnée, et rien n'a été soumis.
    ///
    /// Le fond, l'alpha et le post-traitement s'écrivent comme pour une scène
    /// vide. Un hôte peut légitimement poser sa caméra dans un interstice d'une
    /// carte en cours d'édition, et le moteur ne se relocalise jamais de lui-même.
    NoCell,
}

/// Ce que reçoit la création d'un contexte.
///
/// La résolution maximale dimensionne tout ce que l'image consomme dès la
/// création. Changer de résolution sous ce maximum n'alloue donc rien, ce qui
/// est la seule façon de tenir « zéro allocation par image » quand l'hôte
/// ajuste sa résolution en cours de partie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Largeur maximale, en pixels. Entre 1 et [`MAX_RESOLUTION`].
    pub max_width: u32,
    /// Hauteur maximale, en pixels. Entre 1 et [`MAX_RESOLUTION`].
    pub max_height: u32,
    /// Largeur initiale, en pixels. Au plus `max_width`.
    pub width: u32,
    /// Hauteur initiale, en pixels. Au plus `max_height`.
    pub height: u32,
    /// Côté d'une tuile : 32 ou 64.
    pub tile_size: u32,
    /// Triangles qu'une image peut recevoir, ou `0` pour
    /// [`TRIANGLE_CAPACITY`].
    ///
    /// La valeur compte des triangles **préparés** : un triangle découpé par
    /// le plan proche en produit jusqu'à six, et c'est l'appel qui déborde qui
    /// est refusé, jamais une image entière déjà soumise.
    pub max_triangles: u32,
    /// Primitives de tracé qu'une image peut recevoir, ou `0` pour
    /// [`LINE_CAPACITY`].
    ///
    /// Un budget à part et non une part de `max_triangles` : une ligne n'est
    /// pas un triangle préparé, elle a sa propre liste et sa propre
    /// répartition. Les faire tenir dans le même budget obligerait un hôte à
    /// réserver pour le pire des deux, et un éditeur qui trace mille repères
    /// viderait la capacité de son décor sans comprendre pourquoi.
    ///
    /// Un point consomme une place, une ligne aussi.
    pub max_lines: u32,
}

impl Config {
    /// Refuse une configuration que le moteur ne peut pas honorer.
    fn validate(&self) -> Result<()> {
        let bounded = |v: u32, max: u32| v >= 1 && v <= max;

        if !bounded(self.max_width, MAX_RESOLUTION)
            || !bounded(self.max_height, MAX_RESOLUTION)
            || !bounded(self.width, self.max_width)
            || !bounded(self.height, self.max_height)
        {
            return Err(Error::InvalidArgument(Argument::Resolution));
        }
        if !TILE_SIZES.contains(&self.tile_size) {
            return Err(Error::InvalidArgument(Argument::TileSize));
        }
        Ok(())
    }

    /// La capacité de triangles effective, `0` valant le défaut.
    fn capacity(&self) -> usize {
        if self.max_triangles == 0 {
            TRIANGLE_CAPACITY
        } else {
            self.max_triangles as usize
        }
    }

    /// La capacité de primitives de tracé effective, `0` valant le défaut.
    fn line_capacity(&self) -> usize {
        if self.max_lines == 0 {
            LINE_CAPACITY
        } else {
            self.max_lines as usize
        }
    }
}

/// Un contexte de rendu.
///
/// Il ne porte aucun tampon à l'échelle de l'image : couleur et profondeur
/// vivent sur la pile de l'appel qui rend une tuile. Ce qu'il réserve à la
/// création — triangles préparés, répartition par tuile, drapeaux de tuile —
/// l'est pour la capacité et la résolution maximales, et plus jamais réalloué.
#[derive(Debug)]
pub struct Context {
    config: Config,
    width: u32,
    height: u32,
    /// Les triangles de l'image en cours, dans l'ordre de soumission.
    triangles: Vec<Prepared>,
    /// Les textures que l'image en cours emploie, une entrée par texture
    /// **distincte**, dans l'ordre de première soumission.
    ///
    /// Une table et non une référence dans chaque triangle préparé : le nombre
    /// d'incréments atomiques par image devient celui des textures plutôt que
    /// celui des triangles, et [`Prepared`] garde `Copy`, dont vivent les tests
    /// du rasteriseur. Elle tient une référence forte, si bien qu'une texture
    /// détruite pendant qu'une image la référence reste lisible.
    textures: Vec<Arc<Texture>>,
    /// Les plans d'éclairage des triangles qui en portent, indexés par la place
    /// que chacun retient.
    ///
    /// Un tableau annexe plutôt que deux plans de plus dans [`Prepared`] :
    /// voir [`Lighting`]. Il est réservé pour la capacité entière parce qu'une
    /// image peut n'avoir que des triangles éclairés, et grandir en cours
    /// d'image est exactement ce que le moteur s'interdit.
    lighting: Vec<Lighting>,
    bins: Bins,
    /// Les primitives de tracé de l'image en cours, dans l'ordre de soumission.
    ///
    /// Une seconde liste et non des entrées de la première : une ligne n'a ni
    /// couverture d'aire, ni plans d'attributs, ni texture, et le triangle
    /// préparé est plein à ses deux lignes de cache.
    segments: Vec<Segment>,
    /// La répartition des segments par tuile, parallèle à [`Context::bins`].
    segment_bins: Bins,
    /// Vrai pour chaque tuile déjà prise dans l'image en cours.
    ///
    /// Atomique parce que des tuiles distinctes se rendent depuis des threads
    /// distincts, et que c'est lui qui dit à la fin ce qui reste à rendre.
    taken: Vec<AtomicBool>,
    /// Le découpage de l'image en cours, fixé au début.
    grid: Grid,
    /// La caméra, qu'une image conserve d'un bout à l'autre.
    camera: Camera,
    /// Le mode d'échantillonnage, qu'une image conserve aussi.
    filter: Filter,
    /// Le décalage de sur-éclairement, de même.
    overbright: u32,
    /// Les lumières dynamiques de l'image, telles que l'hôte les a données.
    lights: Vec<Light>,
    /// Les mêmes, portées en espace de vue.
    ///
    /// Refaites à chaque lot plutôt qu'à chaque sommet : la vue ne change pas
    /// pendant une image, mais le tableau doit exister quelque part, et le
    /// contexte est le seul endroit qui n'alloue pas par image.
    placed: Vec<dynamic::Placed>,
    /// Les cellules que la dernière traversée a retenues, avec leur fenêtre.
    ///
    /// Dimensionnée à la création parce qu'elle ne peut l'être nulle part
    /// ailleurs : le contexte ne connaît pas encore la carte, et une soumission
    /// n'a pas le droit d'allouer. La liste sort de la traversée triée par index
    /// de cellule, fenêtres d'une même cellule déjà fusionnées.
    visits: Vec<Visit>,
    /// Le premier triangle que la traversée a produit.
    ///
    /// Symétrique de [`Context::visited_end`], et pour la même raison dans
    /// l'autre sens : un lot soumis **avant** la traversée dans la même image
    /// porte un index plus petit que tous les siens, et la première plage le
    /// rognerait à la fenêtre d'une cellule qui ne le contient pas. La fenêtre
    /// ne borne que la boucle et jamais les valeurs, si bien que ne pas
    /// l'appliquer est toujours juste — l'appliquer à tort supprime des pixels.
    visited_first: u32,
    /// Le premier triangle que la traversée n'a pas produit.
    ///
    /// Un lot soumis autrement dans la même image ne se borne par aucune
    /// fenêtre : sans cette limite, la dernière plage s'étendrait jusqu'au bout
    /// et le rognerait à la fenêtre d'une cellule qui ne le contient pas.
    visited_end: u32,
    /// Le brouillard, éteint par défaut.
    ///
    /// Sa table dépend du plan proche de la caméra, qui convertit une
    /// profondeur en distance : elle se refait quand l'un ou l'autre change,
    /// et jamais par image.
    fog: Fog,
    /// La rampe du brouillard, gardée pour la refaire quand la caméra change.
    fog_range: (f32, f32),
    /// La courbe de sortie, neutre par défaut.
    ///
    /// Elle ne dépend ni de la caméra ni de la scène : une fois réglée, sa
    /// table ne se refait plus jusqu'au réglage suivant.
    grade: Grade,
    /// Le monde vers l'espace de vue, recalculé avec la caméra.
    ///
    /// Gardée plutôt que recomposée à chaque soumission : la composer est une
    /// inversion, et la refaire par lot ferait dépendre l'image du découpage
    /// des lots — même résultat, mais plus rien ne le garantirait.
    view: Affine3,
    /// La projection, qui dépend de la résolution et de la caméra.
    projection: Projection,
    /// [`RECORDING`], [`RENDERING`] ou [`CLOSING`].
    state: AtomicU8,
    /// Les tuiles en cours de rendu, que la fin attend à zéro.
    in_flight: AtomicU32,
    /// Vrai quand la liste de dessin appartient à l'image déjà close.
    ///
    /// La fin d'image la périme mais ne peut pas la vider : elle ne tient
    /// qu'un `&self`, des tuiles pouvant encore la lire. Le prochain appel
    /// exclusif — une soumission, un début — la vide avant d'y toucher.
    stale: AtomicBool,
}

/// Un sommet porté en espace de vue, ses coordonnées de texture intactes.
///
/// La transformation ne touche que la position ; `u` et `v` traversent sans
/// être modifiés, et c'est ce qui rend leur bornage vérifiable une fois pour
/// toutes à la soumission.
#[derive(Debug, Clone, Copy)]
struct ClipSource {
    /// La position en espace de vue.
    view: Vec3,
    /// L'abscisse de texture, en texels.
    u: f32,
    /// L'ordonnée de texture, en texels.
    v: f32,
    /// L'abscisse de lightmap, nulle sur un lot qui n'en porte pas.
    u2: f32,
    /// L'ordonnée de lightmap, nulle sur un lot qui n'en porte pas.
    v2: f32,
    /// Ce que les lumières dynamiques ajoutent à ce sommet, par canal.
    ///
    /// Calculé ici, en espace de vue, une fois par sommet soumis : le
    /// découpage l'interpole ensuite comme une coordonnée.
    light: [f32; 3],
}

/// Le contexte accepte la scène : aucune image n'est commencée.
const RECORDING: u8 = 0;

/// L'image est répartie, et ses tuiles se rendent.
const RENDERING: u8 = 1;

/// La fin d'image a pris la main : plus aucune tuile ne commence.
const CLOSING: u8 = 2;

/// Les textures distinctes qu'une image peut employer, pour une capacité de
/// `triangles` triangles préparés.
///
/// **Ce plafond se déduit, il n'est pas un paramètre.** Un lot pose au moins un
/// triangle et prend au plus **deux** entrées — la sienne et celle de sa
/// lightmap, qui vivent dans la même table : les textures distinctes d'une
/// image sont donc au plus le double de ses triangles préparés. Un champ de
/// configuration n'apprendrait rien au moteur, et `ScgContextConfig` n'a plus
/// que deux champs réservés avant qu'une extension exige une structure et une
/// fonction nouvelles.
///
/// Le facteur deux n'est pas une marge : sans lui, un hôte qui règle sa
/// capacité sur le compte de triangles d'une carte — ce que l'ABI lui dit de
/// faire — se voit refuser un décor dont les surfaces sont des triangles à
/// matériau propre, alors que sa capacité de triangles suffit exactement.
///
/// Le plafond dur vient des deux sentinelles : [`NO_TEXTURE`] occupe `0x7FFF`
/// et le bit de poids fort porte la modulation, il reste donc 32767 index. À
/// huit octets l'entrée, la table coûte 256 Kio à la capacité par défaut,
/// contre plus de deux mégaoctets de triangles préparés et de bacs déjà
/// réservés.
fn texture_capacity(triangles: usize) -> usize {
    // `usize` fait 32 bits sur wasm32 et armv7, et la capacité vient d'un
    // `u32` : le double déborde avant d'être plafonné.
    triangles.saturating_mul(2).min(NO_TEXTURE as usize)
}

/// Une position entre deux poses, `p + (q − p)·t`, composante par composante.
///
/// **L'ordre est contractuel**, comme tout ordre d'opérations flottant du
/// projet : une variante qui l'écrirait autrement rendrait d'autres bits.
/// Jamais de `mul_add` — sur une cible sans instruction de fusion il retombe
/// sur la libm, et sur une autre il ne rend pas les mêmes bits qu'une
/// multiplication suivie d'une addition.
///
/// **Elle n'est pas exacte en `t = 1`**, et c'est pour cela que l'appelant
/// tranche ce cas avant de l'appeler : `p + (q − p)` s'écarte de `q` dès que la
/// soustraction perd des bits, ce qu'un écart d'échelle entre les deux poses
/// suffit à produire.
fn blend(p: Vec3, q: Vec3, t: f32) -> Vec3 {
    Vec3::new(
        p.x + (q.x - p.x) * t,
        p.y + (q.y - p.y) * t,
        p.z + (q.z - p.z) * t,
    )
}

/// Les triangles éclairés qu'une image peut porter, pour une capacité de
/// `triangles` triangles préparés.
///
/// Même plafond dur et pour la même raison que la table de textures :
/// [`NO_LIGHTING`] occupe `u16::MAX`, et c'est un `u16` que [`Prepared`] garde
/// dans les deux octets qu'il avait en bourrage. Au-delà, un lot éclairé se
/// refuse par la capacité de triangles — c'en est une, celle des triangles qui
/// portent un second jeu de coordonnées.
///
/// Une capacité qui dépasse ce plafond n'est donc pas une capacité éclairée
/// équivalente ; en dessous, et c'est le cas de la valeur par défaut, les deux
/// se confondent et aucun triangle ne se refuse pour cette raison.
fn lighting_capacity(triangles: usize) -> usize {
    triangles.min(NO_LIGHTING as usize)
}

impl Context {
    /// Crée un contexte, ou refuse la configuration.
    ///
    /// C'est l'un des appels nommés où l'allocation est permise : tout ce que
    /// l'image consomme se réserve ici, pour la résolution maximale.
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;

        let tiles = Grid::new(config.max_width, config.max_height, config.tile_size).count();
        let mut taken = reserved(tiles as usize)?;
        taken.resize_with(tiles as usize, AtomicBool::default);

        let camera = Camera::DEFAULT;
        let capacity = config.capacity();
        let lines = config.line_capacity();
        Ok(Self {
            config,
            width: config.width,
            height: config.height,
            triangles: reserved(capacity)?,
            textures: reserved(texture_capacity(capacity))?,
            lighting: reserved(lighting_capacity(capacity))?,
            bins: Bins::new(tiles as usize, capacity)?,
            segments: reserved(lines)?,
            segment_bins: Bins::new(tiles as usize, lines)?,
            taken,
            grid: Grid::new(config.width, config.height, config.tile_size),
            camera,
            filter: Filter::default(),
            overbright: 0,
            lights: reserved(MAX_LIGHTS)?,
            placed: reserved(MAX_LIGHTS)?,
            visits: reserved(MAX_VISITS)?,
            visited_first: 0,
            visited_end: 0,
            fog: Fog::new()?,
            fog_range: (0.0, 0.0),
            grade: Grade::new()?,
            view: camera.view(),
            projection: Projection::new(config.width, config.height, camera.fov_y, camera.near)?,
            state: AtomicU8::new(RECORDING),
            in_flight: AtomicU32::new(0),
            stale: AtomicBool::new(false),
        })
    }

    /// La caméra courante.
    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Change la caméra, ou refuse son champ de vision et son plan proche.
    ///
    /// Refusée pendant le rendu, comme toute écriture dans l'état du contexte,
    /// et refusée dès qu'un triangle de l'image en cours est retenu.
    ///
    /// La caméra vaut pour l'image entière, et chaque soumission projette
    /// immédiatement : changer de caméra au milieu laisserait dans la même
    /// image deux espaces écran, chacun juste et l'ensemble faux. Rien en aval
    /// ne pourrait le rattraper, la géométrie source n'étant pas conservée.
    ///
    /// Le quaternion n'est pas exigé unitaire, il est normalisé ici ; en
    /// revanche `fov_y` est refusé **sur les radians**, avant toute conversion
    /// en angle binaire, qui replierait un champ de vision de trois demi-tours
    /// en un demi-tour parfaitement acceptable.
    pub fn set_camera(&mut self, camera: Camera) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.require_empty_frame()?;
        self.projection = Projection::new(self.width, self.height, camera.fov_y, camera.near)?;
        self.view = camera.view();
        let moved_near = self.camera.near != camera.near;
        self.camera = camera;
        // **La table du brouillard dépend du plan proche**, qui convertit une
        // profondeur en distance. Sans ce refait, une caméra dont le plan
        // proche change déplacerait toute la rampe sans que rien ne le dise :
        // le brouillard resterait cohérent avec lui-même, et faux par rapport
        // aux distances que l'hôte a demandées.
        if moved_near && self.fog.is_set() {
            let (start, end) = self.fog_range;
            let color = self.fog.color();
            self.fog.set(color, camera.near, start, end)?;
        }
        Ok(())
    }

    /// Le mode d'échantillonnage courant.
    pub fn filter(&self) -> Filter {
        self.filter
    }

    /// Change le mode d'échantillonnage des textures.
    ///
    /// Refusé pendant le rendu, et pour une raison qui lui est propre : les
    /// tuiles d'une image se rendent depuis des threads que le noyau ne
    /// connaît pas, si bien qu'un filtre changé au milieu laisserait dans la
    /// même image des tuiles lues autrement — une image que rien ne décrit, et
    /// qui dépendrait de l'ordre où l'hôte a pris ses tuiles.
    pub fn set_filter(&mut self, filter: Filter) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.filter = filter;
        Ok(())
    }

    /// Le décalage de sur-éclairement courant, de zéro à [`MAX_OVERBRIGHT`].
    pub fn overbright(&self) -> u32 {
        self.overbright
    }

    /// Change le décalage de sur-éclairement.
    ///
    /// Zéro par défaut : la combinaison rend alors le texel intact sous pleine
    /// lumière, et jamais plus clair. **C'est le réglage juste et une scène
    /// terne** — une lightmap réelle n'atteint le blanc nulle part, si bien que
    /// toute surface est plus sombre que sa texture. Un décalage de un ou deux
    /// rend la dynamique d'une scène éclairée, au prix d'une saturation là où
    /// la lumière est forte.
    ///
    /// Refusé pendant le rendu, pour la même raison que le filtre : une image
    /// dont les tuiles n'auraient pas toutes le même réglage n'est décrite par
    /// rien.
    pub fn set_overbright(&mut self, overbright: u32) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        if overbright > MAX_OVERBRIGHT {
            return Err(Error::InvalidArgument(Argument::Overbright));
        }
        self.overbright = overbright;
        Ok(())
    }

    /// Règle le brouillard : sa couleur, et les distances de vue entre
    /// lesquelles il s'épaissit.
    ///
    /// Éteint par défaut, et éteint par [`Context::clear_fog`]. Les distances
    /// sont en unités de monde, comptées depuis la caméra ; `start` ne peut
    /// pas être négatif, et `end` doit être strictement au-delà.
    ///
    /// **Le fond de l'image prend le brouillard plein**, sans que l'hôte ait
    /// à effacer avec sa couleur : un pixel qu'aucun triangle n'a peint est
    /// infiniment lointain, et c'est ce qui supprime la couture d'horizon.
    ///
    /// Refusé pendant le rendu, comme les autres réglages d'image.
    pub fn set_fog(&mut self, color: Color, start: f32, end: f32) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.fog.set(color.packed(), self.camera.near, start, end)?;
        self.fog_range = (start, end);
        Ok(())
    }

    /// Éteint le brouillard.
    pub fn clear_fog(&mut self) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.fog.clear();
        Ok(())
    }

    /// Vrai si le brouillard est réglé.
    pub fn has_fog(&self) -> bool {
        self.fog.is_set()
    }

    /// Règle la courbe de sortie : une affine par canal, puis le gamma.
    ///
    /// Chaque canal subit `x·gain + offset`, ramené dans `[0, 1]`, puis
    /// `^(1/gamma)` — `gamma` étant celui de l'écran, dans le sens où **2,2
    /// éclaircit**. L'affine vient **avant** le gamma : elle corrige la source,
    /// il encode pour l'écran, et c'est dans cet ordre qu'on les lit. Gain et
    /// gamma commutent d'ailleurs à reparamétrisation près, si bien qu'aucune
    /// image n'est perdue d'un ordre à l'autre ; ce qui change est ce que le
    /// nombre veut dire.
    ///
    /// Refusée pendant le rendu, comme tout réglage que les tuiles d'une même
    /// image doivent partager.
    pub fn set_grade(&mut self, gamma: f32, gains: [f32; 3], offsets: [f32; 3]) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.grade.set(gamma, gains, offsets)
    }

    /// Rend la sortie à son état neutre.
    ///
    /// Sur un contexte qui n'a jamais rien réglé, ce n'est pas une erreur : un
    /// hôte qui remet sa courbe à plat en fin de niveau n'a pas à se souvenir
    /// s'il l'avait réglée.
    pub fn clear_grade(&mut self) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.grade.clear();
        Ok(())
    }

    /// Vrai si une courbe de sortie est réglée.
    pub fn has_grade(&self) -> bool {
        self.grade.is_set()
    }

    /// Remplace les lumières dynamiques de l'image.
    ///
    /// Au plus [`MAX_LIGHTS`] ; au-delà, le lot entier est refusé plutôt que
    /// tronqué — une scène à demi éclairée ne se distingue pas d'une scène
    /// dont on a mal réglé les rayons.
    ///
    /// **L'atténuation se calcule par sommet**, à la soumission : les lumières
    /// réglées après un lot ne l'éclairent pas. C'est ce qui permet à une
    /// torche portée par le joueur d'éclairer le décor sans que le décor soit
    /// resoumis, à condition de la régler avant lui.
    ///
    /// Une lumière de rayon nul, négatif ou non fini est refusée : elle
    /// n'éclaire rien et ferait diviser par zéro.
    ///
    /// Refusé pendant le rendu, comme les autres réglages d'image.
    pub fn set_lights(&mut self, lights: &[Light]) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        if lights.len() > MAX_LIGHTS {
            return Err(Error::InvalidArgument(Argument::LightCapacity));
        }
        for light in lights {
            let finite = light.position.x.is_finite()
                && light.position.y.is_finite()
                && light.position.z.is_finite();
            // `is_finite` avant la comparaison : il écarte `NaN`, que celle-ci
            // laisserait passer puisqu'elle est fausse dans les deux sens.
            if !finite || !light.radius.is_finite() || light.radius <= 0.0 {
                return Err(Error::InvalidArgument(Argument::Light));
            }
        }
        self.lights.clear();
        self.lights.extend_from_slice(lights);
        Ok(())
    }

    /// Les lumières dynamiques courantes.
    pub fn lights(&self) -> &[Light] {
        &self.lights
    }

    /// Porte les lumières en espace de vue, pour le lot qui commence.
    ///
    /// Une fois par lot et non par sommet : la vue ne change pas pendant une
    /// image, et huit transformations ne se mesurent pas.
    fn place_lights(&mut self) {
        self.placed.clear();
        for light in &self.lights {
            let view = self.view.transform_point(light.position);
            if let Some(placed) = dynamic::Placed::new(light, view) {
                self.placed.push(placed);
            }
        }
    }

    /// La configuration reçue à la création.
    ///
    /// Ses champs `width` et `height` restent ceux de la création : c'est
    /// [`Context::resolution`] qui suit les changements, et confondre les deux
    /// ferait dimensionner un tampon d'hôte sur une valeur périmée.
    pub fn config(&self) -> Config {
        self.config
    }

    /// La résolution interne courante, en pixels.
    pub fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Vérifie qu'une sortie peut recevoir l'image entière à la résolution
    /// courante.
    ///
    /// À appeler **avant** d'ouvrir une image que l'appelant n'a pas commencée
    /// lui-même : une fin qui échoue rend la main à l'état de rendu, ce qui est
    /// juste pour une image demandée et absurde pour celle qu'on vient
    /// d'ouvrir pour elle. Le contexte resterait en rendu sur un simple
    /// `stride` fautif, et tout appel exclusif serait refusé ensuite.
    ///
    /// Elle est publique parce que la frontière C ouvre l'image elle-même, et
    /// qu'on ne lui fait pas reconstruire le rectangle de l'image : ce serait
    /// de la logique dans une couche qui n'en porte pas.
    pub fn check_output<O: Output>(&self, out: &O) -> Result<()> {
        out.check(Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        })
    }

    /// Change la résolution interne, sous le maximum fixé à la création.
    ///
    /// Aucune allocation : tout ce que l'image consomme est dimensionné sur la
    /// résolution maximale, et la grille se reconstruit au début de chaque
    /// image. Au-delà du maximum, ou sur une dimension nulle, rend
    /// [`Error::InvalidArgument`] et le contexte garde la résolution qu'il
    /// avait.
    ///
    /// Refusée pendant le rendu et après une soumission, pour la raison qui
    /// vaut pour la caméra : la projection s'applique au moment de la
    /// soumission, et deux résolutions dans la même image ne décriraient rien.
    ///
    /// Le tampon de l'hôte ne suit pas tout seul. Après une hausse, un tampon
    /// laissé à sa taille d'avant est trop court d'autant, et c'est à
    /// l'appelant de le redimensionner.
    pub fn set_resolution(&mut self, width: u32, height: u32) -> Result<()> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.require_empty_frame()?;
        let bounded = |v: u32, max: u32| v >= 1 && v <= max;
        if !bounded(width, self.config.max_width) || !bounded(height, self.config.max_height) {
            return Err(Error::InvalidArgument(Argument::Resolution));
        }
        // La projection est le seul état dérivé de la résolution qu'aucune
        // image ne rafraîchit : la grille se refait à chaque début, les
        // tampons sont dimensionnés au maximum. L'oublier rendrait une image
        // cohérente avec elle-même, à la mauvaise échelle et décentrée — et la
        // bande de garde découperait autour de l'ancien centre, ce qu'aucune
        // borne du rasteriseur ne signalerait.
        self.projection = Projection::new(width, height, self.camera.fov_y, self.camera.near)?;
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// Commence une image : scelle la scène, la répartit par tuile, et rend le
    /// nombre de tuiles.
    ///
    /// La répartition est la seule phase qui écrit dans un état partagé, et
    /// elle se termine ici, avant toute tuile. Une image déjà commencée rend
    /// [`Error::InvalidState`] : sa répartition est peut-être lue par une tuile
    /// sur un autre thread.
    ///
    /// C'est la forme qu'emploie la frontière C, qui garde l'image ouverte d'un
    /// appel à l'autre. Un appelant Rust lui préfère [`Context::frame_begin`],
    /// dont la [`Frame`] rend la séquence vérifiable à la compilation.
    ///
    /// L'image est faite de ce qui a été soumis depuis la fin de la précédente.
    /// Rien de soumis donne une image de fond, sans erreur : c'est une scène
    /// vide, pas un appel fautif.
    pub fn begin(&mut self) -> Result<u32> {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.drop_closed_frame();
        self.seal();
        Ok(self.grid.count())
    }

    /// Vide la liste de dessin si elle appartient à une image déjà close.
    ///
    /// Le vidage se fait ici et non à la fin de l'image parce que la fin ne
    /// tient qu'un `&self`. C'est aussi ce qui permet à un hôte de soumettre
    /// dès le retour de la fin sans que sa scène soit jetée au début suivant.
    fn drop_closed_frame(&mut self) {
        if *self.stale.get_mut() {
            self.triangles.clear();
            self.segments.clear();
            self.lighting.clear();
            // Les textures meurent avec les triangles qui les référencent :
            // les garder ferait vivre une ressource que plus rien ne dessine.
            self.textures.clear();
            // Et les fenêtres avec eux : gardées, elles borneraient les triangles
            // d'une image suivante que la traversée n'a pas produits.
            self.visits.clear();
            self.visited_first = 0;
            self.visited_end = 0;
            *self.stale.get_mut() = false;
        }
    }

    /// Refuse un réglage qui vaut pour l'image entière quand l'image en cours
    /// tient déjà de la géométrie.
    ///
    /// La liste d'une image close ne compte pas : elle appartient à la
    /// précédente, et un hôte qui soumet dès le retour de la fin doit pouvoir
    /// régler sa vue avant de le faire.
    ///
    /// Le test porte sur les triangles **retenus**, et non sur les lots reçus :
    /// un lot dont pas un triangle n'a survécu à la projection ne laisse rien
    /// dans l'image, donc rien à mêler.
    fn require_empty_frame(&mut self) -> Result<()> {
        self.drop_closed_frame();
        // Les segments comptent autant que les triangles : ils sont projetés à
        // la soumission eux aussi, donc une caméra changée après eux laisserait
        // deux espaces écran dans la même image.
        if self.triangles.is_empty() && self.segments.is_empty() {
            Ok(())
        } else {
            Err(Error::InvalidState)
        }
    }

    /// Commence une image et rend de quoi en rendre les tuiles.
    ///
    /// La [`Frame`] emprunte le contexte : aucun autre appel n'est possible tant
    /// qu'elle vit, et sa destruction referme l'image si personne ne l'a fait.
    pub fn frame_begin(&mut self) -> Result<Frame<'_>> {
        self.begin()?;
        Ok(Frame::new(self))
    }

    /// Vrai entre le début et la fin d'une image.
    pub fn is_rendering(&self) -> bool {
        self.state.load(Ordering::SeqCst) != RECORDING
    }

    /// Répartit les triangles soumis et ouvre le rendu des tuiles.
    fn seal(&mut self) {
        self.grid = Grid::new(self.width, self.height, self.config.tile_size);
        self.bins.build(&self.grid, &self.triangles);
        // Les segments se répartissent dans la même phase et par le même
        // pavage : ils sont dessinés après les triangles dans chaque tuile, et
        // leur ordre de soumission est contractuel comme le leur.
        let segments = &self.segments;
        self.segment_bins
            .build_bounds(&self.grid, segments.len(), |i| segments[i].bounds());
        for flag in &mut self.taken[..self.grid.count() as usize] {
            *flag.get_mut() = false;
        }
        *self.state.get_mut() = RENDERING;
    }

    /// Rend une image entière dans le tampon de l'hôte, tuile par tuile.
    ///
    /// Sans début, elle le fait elle-même ; après un début, elle rend les tuiles
    /// que personne n'a prises.
    ///
    /// `stride` est en pixels et vaut au moins la largeur courante. Le tampon
    /// fait au moins `stride × hauteur` pixels de quatre octets ; la frontière C
    /// ne reçoit pas sa longueur et en fait une précondition, alors qu'un
    /// appelant Rust la porte avec la tranche — c'est le seul contrôle des deux
    /// qui distingue les deux chemins.
    pub fn frame_end(&mut self, pixels: &mut [u8], stride: u32) -> Result<()> {
        if !self.is_rendering() {
            // La sortie se vérifie **avant** d'ouvrir l'image. Ouvrir d'abord
            // ferait qu'un `stride` refusé laisse le contexte en rendu : la
            // fin échouée y rend la main à l'état de rendu, ce qui est juste
            // pour une image que l'hôte a commencée lui-même, et absurde pour
            // celle-ci, qu'il n'a jamais demandée. Tout appel exclusif
            // deviendrait alors `InvalidState`, sans que rien ne dise pourquoi.
            self.check_output(&Rows::new(pixels, stride))?;
            self.begin()?;
        }
        self.end(&mut Rows::new(pixels, stride))
    }

    /// Soumet un lot de triangles, chacun transformé par `model` puis par la
    /// caméra.
    ///
    /// `model` porte l'objet vers le monde, et le noyau compose la vue :
    /// une matrice modèle-vue reçue toute faite obligerait chaque hôte à
    /// inverser la pose de la caméra lui-même, donc à normaliser un quaternion
    /// par sa propre bibliothèque mathématique, et deux liaisons ne rendraient
    /// plus la même image.
    ///
    /// **Un lot est accepté ou refusé en entier.** Un lot à demi soumis
    /// laisserait dans l'image un mur dont il manque la moitié, sans que l'hôte
    /// sache où la coupure est tombée.
    ///
    /// Un triangle dont un sommet ne se projette pas — coordonnée démesurée ou
    /// non finie — disparaît sans erreur : c'est une donnée, pas un défaut du
    /// moteur. Un indice hors du tableau de sommets, lui, est une erreur de
    /// l'appelant.
    pub fn submit(
        &mut self,
        model: Affine3,
        vertices: &[Vec3],
        triangles: &[Triangle],
    ) -> Result<()> {
        self.submit_each(model, triangles.len(), |i| {
            let triangle = triangles[i];
            let mut corners = [Vec3::ZERO; 3];
            for (corner, &index) in corners.iter_mut().zip(&triangle.indices) {
                *corner = *vertices
                    .get(index as usize)
                    .ok_or(Error::InvalidArgument(Argument::VertexIndex))?;
            }
            Ok((corners, triangle.color))
        })
    }

    /// Soumet un lot dont chaque triangle se lit par une fonction d'accès.
    ///
    /// La forme générale, dont [`Context::submit`] n'est que la façade sur deux
    /// tranches. Elle existe pour la frontière C, qui reçoit des tableaux de
    /// structures `#[repr(C)]` qui lui appartiennent : sans elle, il lui
    /// faudrait soit les copier — une allocation par image —, soit
    /// réinterpréter ses tranches, ce qui imposerait au noyau une disposition
    /// mémoire qu'il n'a pas choisie.
    ///
    /// `read` est appelée une fois par triangle, dans l'ordre, et son erreur
    /// refuse le lot entier.
    pub fn submit_each<F>(&mut self, model: Affine3, count: usize, read: F) -> Result<()>
    where
        F: Fn(usize) -> Result<([Vec3; 3], Color)>,
    {
        self.submit_each_uv(model, count, None, |i| {
            let (corners, color) = read(i)?;
            Ok((corners.map(VertexUv::untextured), color))
        })
    }

    /// Soumet un lot éclairé par une lightmap, dont les sommets portent un
    /// second jeu de coordonnées.
    ///
    /// Même contrat que [`Context::submit_each_uv`] pour le reste. La lightmap
    /// vaut pour le lot entier, comme la texture, et **c'est sa présence qui
    /// décide de l'éclairage** : un lot passe par ici parce qu'il en a une, et
    /// il n'existe aucun autre moyen d'en porter une.
    ///
    /// La texture, elle, reste facultative : un mur uni éclairé est le cas le
    /// plus courant d'un décor, et il rend alors `couleur × lightmap`.
    ///
    /// Le nombre de triangles **éclairés** d'une image est plafonné par la
    /// sentinelle du tableau annexe, plus bas que la capacité quand celle-ci
    /// dépasse 65535 ; le dépassement se refuse par la capacité de triangles.
    pub fn submit_each_lit<F>(
        &mut self,
        model: Affine3,
        count: usize,
        texture: Option<&Arc<Texture>>,
        lightmap: &Arc<Texture>,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> Result<([VertexUv2; 3], Color)>,
    {
        self.submit_lot(model, count, texture, Some(lightmap), 0, read)
    }

    /// Soumet un lot éclairé par indices, la façade de
    /// [`Context::submit_each_lit`] sur deux tranches.
    pub fn submit_lit(
        &mut self,
        model: Affine3,
        vertices: &[VertexUv2],
        triangles: &[Triangle],
        texture: Option<&Arc<Texture>>,
        lightmap: &Arc<Texture>,
    ) -> Result<()> {
        self.submit_each_lit(model, triangles.len(), texture, lightmap, |i| {
            let triangle = triangles[i];
            let mut corners = [VertexUv2::unlit(VertexUv::untextured(Vec3::ZERO)); 3];
            for (corner, &index) in corners.iter_mut().zip(&triangle.indices) {
                *corner = *vertices
                    .get(index as usize)
                    .ok_or(Error::InvalidArgument(Argument::VertexIndex))?;
            }
            Ok((corners, triangle.color))
        })
    }

    /// Soumet un maillage chargé, un lot par groupe de surface.
    ///
    /// `texture` rend la texture d'un emplacement, ou `None` pour « sans
    /// texture » : une fonction d'accès et non une tranche, pour que la table de
    /// l'hôte se lise sur place — la recopier dans un tampon intermédiaire
    /// serait une allocation sur le chemin le plus banal de l'étape.
    ///
    /// **Refusé en entier ou pas du tout.** Le dépassement de capacité ne se voit
    /// pas à l'entrée : un triangle soumis consomme plusieurs places quand le
    /// découpage le multiplie, si bien qu'un groupe peut échouer après que
    /// d'autres ont été posés. D'où la marque et la troncature, plutôt qu'un
    /// contrôle a priori qui, dimensionné sur le pire cas du découpage,
    /// refuserait des maillages tenant largement — et rendrait inutilisable le
    /// compte de triangles sur lequel l'hôte dimensionne son contexte.
    ///
    /// Rien n'est revalidé : les indices et le pavage des groupes tiennent du
    /// chargement, et c'est ce qui permet d'indexer sans contrôle.
    pub fn submit_mesh<'t, F>(&mut self, model: Affine3, mesh: &Mesh, texture: F) -> Result<()>
    where
        F: Fn(u32) -> Option<&'t Arc<Texture>>,
    {
        // La normale de la première trame : c'est elle que le rendu non animé
        // dessine, et `vertices()` porte déjà sa position pour la même raison.
        let poses = mesh.frame(0);
        self.submit_mesh_with(model, mesh, texture, |i| {
            let normal = poses.map_or(Vec3::ZERO, |poses| poses[i].normal);
            VertexUv2::shaded(mesh.vertices()[i], normal)
        })
    }

    /// Le corps commun des trois soumissions de maillage : seule la façon de
    /// lire un sommet les distingue.
    ///
    /// Un seul corps et non trois, parce que c'est lui qui porte le refus en
    /// entier — un maillage qui ne tient pas dans la capacité restante ne doit
    /// rien laisser derrière lui, pas même les groupes déjà posés. Recopié, ce
    /// rattrapage finirait par manquer sur le chemin ajouté en dernier.
    fn submit_mesh_with<'t, F, V>(
        &mut self,
        model: Affine3,
        mesh: &Mesh,
        texture: F,
        vertex: V,
    ) -> Result<()>
    where
        F: Fn(u32) -> Option<&'t Arc<Texture>>,
        V: Fn(usize) -> VertexUv2,
    {
        // **Avant de prendre la marque**, et c'est tout l'intérêt de la prendre :
        // les listes d'une image close ne sont vidées qu'au premier triangle
        // posé, si bien qu'une marque lue avant désigne une longueur que la
        // liste n'a plus. Le `truncate` du refus ne ferait alors rien, et un
        // maillage refusé resterait à moitié posé.
        self.drop_closed_frame();
        let (mark, textures) = (self.triangles.len(), self.textures.len());
        let lights = self.lighting.len();

        for group in mesh.groups() {
            let first = group.first_triangle as usize;
            let result = self.submit_each_shaded(
                model,
                group.triangle_count as usize,
                texture(group.texture_slot),
                |i| {
                    let triangle = mesh.triangles()[first + i];
                    let mut corners = [VertexUv2::unlit(VertexUv::untextured(Vec3::ZERO)); 3];
                    for (corner, &index) in corners.iter_mut().zip(&triangle.indices) {
                        *corner = vertex(index as usize);
                    }
                    Ok((corners, triangle.color))
                },
            );
            if result.is_err() {
                self.triangles.truncate(mark);
                self.lighting.truncate(lights);
                self.textures.truncate(textures);
                return result;
            }
        }
        Ok(())
    }

    /// Soumet le maillage dans une pose donnée, sans interpoler.
    ///
    /// Les coordonnées de texture viennent des sommets assemblés au chargement :
    /// elles n'animent pas, et les relire ailleurs serait une seconde source
    /// pour la même valeur.
    fn submit_posed<'t, F>(
        &mut self,
        model: Affine3,
        mesh: &Mesh,
        texture: F,
        poses: &[Pose],
    ) -> Result<()>
    where
        F: Fn(u32) -> Option<&'t Arc<Texture>>,
    {
        self.submit_mesh_with(model, mesh, texture, |i| {
            VertexUv2::shaded(
                VertexUv {
                    position: poses[i].position,
                    u: mesh.vertices()[i].u,
                    v: mesh.vertices()[i].v,
                },
                poses[i].normal,
            )
        })
    }

    /// Soumet le maillage entre deux poses, par `a + (b − a)·t`.
    ///
    /// **Composante par composante, dans l'ordre x, y, z**, et cet ordre est
    /// contractuel comme tout ordre d'opérations flottant du projet : une
    /// variante qui l'écrirait autrement rendrait d'autres bits. Jamais de
    /// `mul_add` — sur une cible sans instruction de fusion il retombe sur la
    /// libm, et sur une autre il ne rend pas les mêmes bits qu'une
    /// multiplication suivie d'une addition.
    ///
    /// Les cas dégénérés n'arrivent pas ici : l'appelant les a tranchés.
    fn submit_interpolated<'t, F>(
        &mut self,
        model: Affine3,
        mesh: &Mesh,
        texture: F,
        a: &[Pose],
        b: &[Pose],
        t: f32,
    ) -> Result<()>
    where
        F: Fn(u32) -> Option<&'t Arc<Texture>>,
    {
        self.submit_mesh_with(model, mesh, texture, |i| {
            VertexUv2::shaded(
                VertexUv {
                    position: blend(a[i].position, b[i].position, t),
                    u: mesh.vertices()[i].u,
                    v: mesh.vertices()[i].v,
                },
                // **La normale passe par le même mélange**, et le moteur la
                // renormalise plus loin : une normale interpolée entre deux
                // trames n'est plus unitaire, et c'est pourquoi rien n'exige
                // qu'elle le soit à l'entrée.
                blend(a[i].normal, b[i].normal, t),
            )
        })
    }

    /// Soumet un maillage entre deux de ses trames.
    ///
    /// **`frame_a == frame_b` rend exactement ce que [`Context::submit_mesh`]
    /// rend de cette trame, pour tout `t`.** Ce n'est pas une commodité : c'est
    /// le théorème contre lequel ce chemin se valide, comme la traversée par
    /// portails se valide contre le chemin brut. Il contraint l'écriture de
    /// l'interpolation, et c'est pour lui que les trois cas ci-dessous se
    /// tranchent une fois par lot.
    ///
    /// Deux indices explicites et non « la trame et la suivante » : un hôte
    /// boucle de la dernière à la première, ou mêle deux trames non adjacentes,
    /// sans que le moteur connaisse la moindre notion de séquence — rien du jeu
    /// ne traverse.
    ///
    /// `t` hors de `[0, 1]` est **refusé, jamais ramené dans l'intervalle** : un
    /// bornage silencieux rendrait une pose extrapolée sans le dire, et
    /// extrapoler est une décision de jeu.
    pub fn submit_mesh_frame<'t, F>(
        &mut self,
        model: Affine3,
        mesh: &Mesh,
        texture: F,
        frame_a: u32,
        frame_b: u32,
        factor: f32,
    ) -> Result<()>
    where
        F: Fn(u32) -> Option<&'t Arc<Texture>>,
    {
        // `is_nan` nommément et avant les comparaisons de bornes : toute
        // comparaison avec lui est fausse, et un refus écrit `t <= 1` le
        // laisserait passer.
        if factor.is_nan() || !(0.0..=1.0).contains(&factor) {
            return Err(Error::InvalidArgument(Argument::FrameFactor));
        }
        let (a, b) = (mesh.frame(frame_a), mesh.frame(frame_b));
        let (Some(a), Some(b)) = (a, b) else {
            return Err(Error::InvalidArgument(Argument::FrameIndex));
        };

        // **Les cas se tranchent ici, une fois par lot**, et jamais par sommet :
        // le facteur ne change pas d'un sommet à l'autre.
        //
        // **Un seul des trois est nécessaire à l'exactitude, et c'est `t = 1`**
        // — mesuré, pas supposé. Avec la forme `p + (q − p)·t`, deux trames
        // identiques donnent `q − p = 0` exactement et rendent `p` au bit près,
        // et `t = 0` de même : ces deux cas-là sont une **économie**, pas une
        // garantie. En `t = 1`, en revanche, `p + (q − p)` n'est pas `q` dès
        // que la soustraction perd des bits, et c'est lui qui fait du théorème
        // ci-dessus une conséquence de l'écriture plutôt qu'une chance.
        let poses: &[Pose] = if frame_a == frame_b || factor == 0.0 {
            a
        } else if factor == 1.0 {
            b
        } else {
            return self.submit_interpolated(model, mesh, texture, a, b, factor);
        };
        self.submit_posed(model, mesh, texture, poses)
    }

    /// Soumet une carte entière, un lot par surface.
    ///
    /// `texture` rend la texture d'un matériau, par son rang dans la table de la
    /// carte, ou `None` pour « sans texture ». Même forme que pour un maillage,
    /// et pour la même raison : la table de l'hôte se lit sur place.
    ///
    /// **Toutes les cellules, aucune élimination.** La traversée par portails
    /// ne remplace pas ce chemin : elle s'ajoute, et c'est contre lui qu'elle se
    /// valide — sur une scène où tout est visible, les deux rendent la même
    /// image. Rien ici ne prend de cellule de départ, un paramètre qui ne
    /// servirait pas étant un paramètre dont le sens changerait.
    ///
    /// **Refusée en entier ou pas du tout**, comme un maillage, et par le même
    /// mécanisme.
    pub fn submit_world<'t, F>(&mut self, model: Affine3, world: &World, texture: F) -> Result<()>
    where
        F: Fn(u32) -> Option<&'t Arc<Texture>>,
    {
        self.submit_world_lit(model, world, texture, None)
    }

    /// Le même décor entier, éclairé par les lightmaps qu'on lui passe.
    ///
    /// **Elle existe pour que l'égalité avec la traversée soit vérifiable
    /// éclairée.** Sans elle, le chemin brut rendrait un décor éteint et le
    /// chemin traversé un décor allumé : les deux images différeraient pour une
    /// raison étrangère à ce qu'on compare, et le seul contrôle qui attrape une
    /// fenêtre trop étroite ne vaudrait que pour un décor sans lightmap.
    ///
    /// `lighting` nul rend exactement ce que [`Context::submit_world`] rend, qui
    /// l'appelle ainsi.
    pub fn submit_world_lit<'t, F>(
        &mut self,
        model: Affine3,
        world: &World,
        texture: F,
        lighting: Option<&Lightmaps>,
    ) -> Result<()>
    where
        F: Fn(u32) -> Option<&'t Arc<Texture>>,
    {
        // Avant la marque, pour la raison écrite dans `submit_mesh_with`.
        self.drop_closed_frame();
        let (mark, textures) = (self.triangles.len(), self.textures.len());
        let lights = self.lighting.len();

        for (index, cell) in world.cells().iter().enumerate() {
            if let Err(error) = self.submit_cell(model, cell, index as u32, lighting, &texture) {
                self.triangles.truncate(mark);
                self.lighting.truncate(lights);
                self.textures.truncate(textures);
                return Err(error);
            }
        }
        Ok(())
    }

    /// Les surfaces d'une cellule, éclairées ou non selon ce que l'atlas porte.
    ///
    /// **Le corps commun des trois soumissions de décor**, et non une commodité :
    /// la projection des coordonnées de lightmap, le demi-luxel et le retrait
    /// d'une surface qui refuse son atlas s'écrivent une fois. Recopiés, ils
    /// divergeraient d'un chemin à l'autre, et c'est l'image du chemin le moins
    /// emprunté qui deviendrait fausse.
    ///
    /// Ne rattrape rien : c'est l'appelant qui tronque, lui seul sachant où son
    /// lot commence.
    fn submit_cell<'t, F>(
        &mut self,
        model: Affine3,
        cell: &Cell,
        index: u32,
        lighting: Option<&Lightmaps>,
        texture: &F,
    ) -> Result<()>
    where
        F: Fn(u32) -> Option<&'t Arc<Texture>>,
    {
        for (rank_in_cell, surface) in cell.surfaces.iter().enumerate() {
            let first = surface.first_triangle as usize;
            // La cellule a-t-elle son atlas, et cette surface accepte-t-elle une
            // lightmap ? Sinon le lot part par le chemin non éclairé — **un
            // niveau partiellement rallumé reste affichable**, ce qui est
            // exactement le moment où un éditeur a besoin de le voir.
            let lit = lighting
                .and_then(|lighting| lighting.of(index))
                .filter(|_| !surface.skips_lightmap())
                .map(|(texture, atlas)| (texture, atlas.slots[rank_in_cell]));

            match lit {
                Some((lightmap, slot)) => {
                    let extent = surface.luxels;
                    self.submit_each_lit(
                        model,
                        surface.triangle_count as usize,
                        texture(surface.material),
                        lightmap,
                        |i| {
                            let triangle = cell.triangles[first + i];
                            let mut corners =
                                [VertexUv2::unlit(VertexUv::untextured(Vec3::ZERO)); 3];
                            for (corner, &index) in corners.iter_mut().zip(&triangle) {
                                let vertex = cell.vertices[index as usize];
                                // Les coordonnées locales de la surface, plus
                                // l'origine de son rectangle : la carte reste
                                // indépendante d'un rangement que le premier
                                // déplacement de sommet changerait. Le demi-luxel
                                // n'est pas décoratif — le bilinéaire retranche
                                // déjà un demi-texel, et le centre du luxel (0,0)
                                // est en (0,5 ; 0,5).
                                let (lu, lv) = surface.lightmap.project(vertex.position);
                                *corner = VertexUv2 {
                                    position: vertex.position,
                                    u: vertex.u,
                                    v: vertex.v,
                                    u2: lu - extent.min_u as f32 + (slot.x + GUTTER) as f32 + 0.5,
                                    v2: lv - extent.min_v as f32 + (slot.y + GUTTER) as f32 + 0.5,
                                    // Le décor n'a pas de normale par sommet : son
                                    // angle est déjà dans la lightmap, cuite avec
                                    // son terme de Lambert.
                                    normal: Vec3::ZERO,
                                };
                            }
                            Ok((corners, Color::new(0xFF, 0xFF, 0xFF, 0xFF)))
                        },
                    )?;
                }
                None => {
                    self.submit_each_uv(
                        model,
                        surface.triangle_count as usize,
                        texture(surface.material),
                        |i| {
                            let triangle = cell.triangles[first + i];
                            let mut corners = [VertexUv::untextured(Vec3::ZERO); 3];
                            for (corner, &index) in corners.iter_mut().zip(&triangle) {
                                *corner = cell.vertices[index as usize];
                            }
                            // Le décor sort en blanc : la couleur du sommet
                            // n'existe pas dans une carte, où c'est le matériau
                            // qui habille, et la lightmap qui éclairera.
                            Ok((corners, Color::new(0xFF, 0xFF, 0xFF, 0xFF)))
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Soumet ce qu'une caméra voit d'une carte, depuis la cellule où elle est.
    ///
    /// **La traversée décide d'abord, la soumission suit.** Chaque cellule
    /// retenue est soumise **une fois**, dans l'ordre du fichier : c'est cet ordre
    /// qui départage deux surfaces coplanaires, et en ordre de traversée il
    /// dépendrait de la position de la caméra. Le total soumis reste donc inférieur
    /// ou égal à ce que rend [`World::triangle_count`], qui demeure un
    /// dimensionnement valide de la capacité du contexte.
    ///
    /// `cell_id` à zéro vaut « aucune cellule » : rien n'est soumis et
    /// [`Visibility::NoCell`] le dit. Un identifiant qui ne désigne aucune cellule
    /// est une erreur, lui — c'est la différence entre « la caméra n'est nulle
    /// part » et « cette cellule n'existe pas », qui n'ont pas la même réponse.
    pub fn submit_world_visible<'t, F>(
        &mut self,
        model: Affine3,
        world: &World,
        cell_id: u32,
        lighting: Option<&Lightmaps>,
        texture: F,
    ) -> Result<Visibility>
    where
        F: Fn(u32) -> Option<&'t Arc<Texture>>,
    {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        if cell_id == 0 {
            return Ok(Visibility::NoCell);
        }
        // **Avant la traversée, et non à la première surface soumise.** Le
        // nettoyage d'une image close vide la liste des visites ; laissé où les
        // autres soumissions le font, il l'effacerait entre le remplissage par la
        // traversée et la boucle qui la relit, et la seconde image d'un même
        // contexte s'arrêterait sur un index hors borne.
        self.drop_closed_frame();
        let start = world.cell_of(cell_id).ok_or(Error::UnknownResource)?;

        let image = Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        };
        let truncated = traverse(
            world,
            start,
            image,
            self.view,
            &self.projection,
            &mut self.visits,
        );

        let (mark, textures) = (self.triangles.len(), self.textures.len());
        let lights = self.lighting.len();
        let cells = world.cells();
        // Ce que l'image porte déjà n'appartient à aucune cellule visitée.
        self.visited_first = mark as u32;

        for rank in 0..self.visits.len() {
            let visit = self.visits[rank];
            // La plage s'ouvre ici, avant que la cellule ne prépare un seul
            // triangle : c'est par elle que le remplissage retrouvera la fenêtre.
            self.visits[rank].first_triangle = self.triangles.len() as u32;
            let cell = &cells[visit.cell as usize];
            if let Err(error) = self.submit_cell(model, cell, visit.cell, lighting, &texture) {
                self.triangles.truncate(mark);
                self.lighting.truncate(lights);
                self.textures.truncate(textures);
                // **Les trois ensemble, ou le remplissage lit une liste
                // vidée.** Une plage laissée derrière par une traversée
                // précédente de la même image désignerait encore des triangles,
                // et le curseur irait chercher leur fenêtre dans la liste que
                // cette ligne efface.
                self.visits.clear();
                self.visited_first = 0;
                self.visited_end = 0;
                return Err(error);
            }
        }

        self.visited_end = self.triangles.len() as u32;
        Ok(if truncated {
            Visibility::Incomplete
        } else {
            Visibility::Complete
        })
    }

    /// Soumet un lot de triangles habillés d'une texture.
    ///
    /// **La texture vaut pour le lot entier**, et non pour chaque triangle :
    /// une surface continue se soumet en un seul franchissement, et c'est aussi
    /// la forme qu'impose la frontière C, dont la structure de triangle est
    /// publiée et ne peut plus gagner de champ.
    ///
    /// Le moteur en garde une référence forte jusqu'à la fin de l'image :
    /// l'hôte peut la libérer de son côté sans que l'image en cours change.
    pub fn submit_textured(
        &mut self,
        model: Affine3,
        vertices: &[VertexUv],
        triangles: &[Triangle],
        texture: &Arc<Texture>,
    ) -> Result<()> {
        self.submit_indexed(model, vertices, triangles, Some(texture))
    }

    /// Soumet un lot de triangles dont les sommets portent leurs coordonnées de
    /// texture.
    ///
    /// Même contrat que [`Context::submit`] pour le reste : `model` porte
    /// l'objet vers le monde, et le lot est accepté ou refusé en entier.
    pub fn submit_uv(
        &mut self,
        model: Affine3,
        vertices: &[VertexUv],
        triangles: &[Triangle],
    ) -> Result<()> {
        self.submit_indexed(model, vertices, triangles, None)
    }

    /// Le corps commun de [`Context::submit_uv`] et
    /// [`Context::submit_textured`] : la seule différence entre les deux est la
    /// texture.
    fn submit_indexed(
        &mut self,
        model: Affine3,
        vertices: &[VertexUv],
        triangles: &[Triangle],
        texture: Option<&Arc<Texture>>,
    ) -> Result<()> {
        self.submit_each_uv(model, triangles.len(), texture, |i| {
            let triangle = triangles[i];
            let mut corners = [VertexUv::untextured(Vec3::ZERO); 3];
            for (corner, &index) in corners.iter_mut().zip(&triangle.indices) {
                *corner = *vertices
                    .get(index as usize)
                    .ok_or(Error::InvalidArgument(Argument::VertexIndex))?;
            }
            Ok((corners, triangle.color))
        })
    }

    /// La forme générale de [`Context::submit_each`], dont les sommets portent
    /// leurs coordonnées de texture et le lot son habillage.
    pub fn submit_each_uv<F>(
        &mut self,
        model: Affine3,
        count: usize,
        texture: Option<&Arc<Texture>>,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> Result<([VertexUv; 3], Color)>,
    {
        self.submit_lot(model, count, texture, None, 0, |i| {
            let (corners, color) = read(i)?;
            Ok((corners.map(VertexUv2::unlit), color))
        })
    }

    /// Soumet un lot **modulé** : chaque pixel multiplie ce qui est déjà dans
    /// le tampon, au lieu de l'écraser.
    ///
    /// **Le décor se soumet avant ses taches.** Une surface modulée teste sa
    /// profondeur sans la réécrire, et son test n'est pas strict : c'est ce qui
    /// laisse une tache coplanaire gagner sur le sol qu'elle marque. L'ordre de
    /// soumission décide, et il est déjà contractuel — rien de neuf n'est
    /// promis ici.
    ///
    /// Le cas d'usage est l'ombre d'un objet mobile, dont le jeu décide la
    /// place : le moteur ne fournit que la primitive, un polygone qui assombrit
    /// au lieu de recouvrir. 255 est le neutre, comme pour une lightmap, et une
    /// surface modulée n'éclaircit jamais.
    pub fn submit_blended(
        &mut self,
        model: Affine3,
        vertices: &[VertexUv],
        triangles: &[Triangle],
        texture: Option<&Arc<Texture>>,
    ) -> Result<()> {
        self.submit_each_blended(model, triangles.len(), texture, |i| {
            let triangle = triangles[i];
            let mut corners = [VertexUv::untextured(Vec3::ZERO); 3];
            for (corner, &index) in corners.iter_mut().zip(&triangle.indices) {
                *corner = *vertices
                    .get(index as usize)
                    .ok_or(Error::InvalidArgument(Argument::VertexIndex))?;
            }
            Ok((corners, triangle.color))
        })
    }

    /// Soumet un lot dont les sommets portent une normale.
    ///
    /// Les lumières dynamiques tiennent alors compte de l'orientation de la
    /// surface : une face qui tourne le dos à une lumière ne reçoit rien, là où
    /// sans normale elle recevait autant que ses voisines à égale distance.
    ///
    /// La normale n'est pas exigée unitaire — le moteur la normalise, et il le
    /// faut : une normale interpolée entre deux trames ne l'est plus. Une
    /// normale de longueur nulle vaut « pas de normale », et l'éclairage
    /// retombe sur la distance seule.
    pub fn submit_each_shaded<F>(
        &mut self,
        model: Affine3,
        count: usize,
        texture: Option<&Arc<Texture>>,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> Result<([VertexUv2; 3], Color)>,
    {
        self.submit_lot(model, count, texture, None, 0, read)
    }

    /// Soumet des quadrilatères que le moteur oriente sur la caméra.
    ///
    /// L'hôte donne un centre et deux demi-extensions ; le moteur construit le
    /// quadrilatère. C'est la seule chose de cette famille qu'un hôte ne peut
    /// pas faire sans calculer — voir [`Sprite`].
    ///
    /// **`model` place le centre, et lui seul.** Sa partie linéaire n'oriente
    /// pas le quadrilatère : c'est la caméra qui l'oriente, par définition. Le
    /// paramètre reste pour la symétrie de la famille des soumissions.
    ///
    /// Un sprite consomme **deux triangles** de la capacité, avant découpe.
    pub fn submit_sprites(
        &mut self,
        model: Affine3,
        sprites: &[Sprite],
        texture: Option<&Arc<Texture>>,
        orientation: SpriteOrientation,
    ) -> Result<()> {
        self.submit_each_sprite(model, sprites.len(), texture, orientation, |i| {
            Ok(sprites[i])
        })
    }

    /// La même, par fonction d'accès : c'est la forme qu'emploie la frontière,
    /// qui lit les structures de l'hôte sur place.
    pub fn submit_each_sprite<F>(
        &mut self,
        model: Affine3,
        count: usize,
        texture: Option<&Arc<Texture>>,
        orientation: SpriteOrientation,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> Result<Sprite>,
    {
        // **La base se calcule une fois par lot, et c'est ce qui la rend
        // gratuite.** Elle ne dépend que de la caméra : la prendre par sprite
        // paierait la même racine inverse autant de fois qu'il y a
        // d'étincelles dans une image.
        let Some((right, up)) = self.sprite_basis(orientation) else {
            // L'axial dégénéré : la caméra regarde à la verticale exacte, et le
            // quadrilatère n'a plus de largeur. Il disparaît sans erreur, comme
            // un triangle qui ne se projette pas — c'est une donnée, pas une
            // faute d'appel.
            return Ok(());
        };
        // Les coins **en espace monde**, que la transformation du lot portera
        // ensuite en espace de vue comme ceux de n'importe quelle soumission :
        // un seul chemin de validation, de capacité et de troncature. Le centre
        // passe par `model`, les axes non — c'est la clause « `model` place le
        // centre, et lui seul ».
        self.submit_each_uv(Affine3::IDENTITY, count * 2, texture, |i| {
            let sprite = read(i / 2)?;
            let center = model.transform_point(sprite.center);
            // Le roulis tourne les deux demi-extensions dans le plan du
            // quadrilatère : après l'orientation, avant la projection, et sans
            // division ni racine.
            let (sin, cos) = (sprite.roll.sin(), sprite.roll.cos());
            let across = (right * cos + up * sin) * sprite.half_width;
            let along = (up * cos - right * sin) * sprite.half_height;

            let corner = |sx: f32, sy: f32, u: f32, v: f32| VertexUv {
                position: center + across * sx + along * sy,
                u,
                v,
            };
            // Deux triangles pour un quadrilatère, en sens antihoraire vus de
            // face. Le second commence au coin déjà posé : un sprite consomme
            // exactement deux places, ce que la documentation promet.
            let corners = if i % 2 == 0 {
                [
                    corner(-1.0, -1.0, sprite.u0, sprite.v1),
                    corner(1.0, -1.0, sprite.u1, sprite.v1),
                    corner(1.0, 1.0, sprite.u1, sprite.v0),
                ]
            } else {
                [
                    corner(-1.0, -1.0, sprite.u0, sprite.v1),
                    corner(1.0, 1.0, sprite.u1, sprite.v0),
                    corner(-1.0, 1.0, sprite.u0, sprite.v0),
                ]
            };
            Ok((corners, sprite.color))
        })
    }

    /// Les axes du plan d'un sprite, en espace monde, ou `None` si l'axial
    /// dégénère.
    ///
    /// La vue est rigide, donc son inverse porte les axes de la caméra dans le
    /// monde. L'axe vertical de l'écran est le **−Y de vue**, celui-ci
    /// descendant.
    fn sprite_basis(&self, orientation: SpriteOrientation) -> Option<(Vec3, Vec3)> {
        let camera = self.view.inverse_rigid();
        let right = camera.transform_vector(Vec3::new(1.0, 0.0, 0.0));
        match orientation {
            SpriteOrientation::Facing => {
                Some((right, -camera.transform_vector(Vec3::new(0.0, 1.0, 0.0))))
            }
            SpriteOrientation::Axial => {
                // Debout : le haut est le Z du monde, et la largeur lui est
                // perpendiculaire tout en restant face à la caméra. Prendre
                // l'axe avant de la caméra plutôt que la direction vers chaque
                // sprite garde la base commune au lot — et garde donc une seule
                // racine inverse pour toute une nuée.
                let up = Vec3::new(0.0, 0.0, 1.0);
                let forward = camera.transform_vector(Vec3::new(0.0, 0.0, 1.0));
                // **`forward × up`, et non l'inverse** : celui-ci rendrait un
                // axe de largeur opposé à celui du plein-face, donc un
                // quadrilatère dont la normale fuit la caméra — vu de dos, il
                // serait éliminé, et l'axial ne dessinerait jamais rien.
                //
                // `normalize` rend le vecteur nul sous le seuil de la racine
                // inverse du noyau, et c'est exactement le cas d'une caméra à la
                // verticale : le produit vectoriel s'y annule.
                let across = forward.cross(up).normalize();
                if across.dot(across) == 0.0 {
                    return None;
                }
                Some((across, up))
            }
        }
    }

    /// Soumet un lot modulé par fonction d'accès, la forme que la frontière
    /// emploie : elle lit les structures de l'hôte sur place, sans les recopier
    /// dans un tampon intermédiaire.
    pub fn submit_each_blended<F>(
        &mut self,
        model: Affine3,
        count: usize,
        texture: Option<&Arc<Texture>>,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> Result<([VertexUv; 3], Color)>,
    {
        self.submit_lot(model, count, texture, None, MODULATED, |i| {
            let (corners, color) = read(i)?;
            Ok((corners.map(VertexUv2::unlit), color))
        })
    }

    /// Le corps commun des deux soumissions par fonction d'accès : le second
    /// jeu de coordonnées est toujours là, la lightmap dit seulement si ses
    /// plans se construisent.
    fn submit_lot<F>(
        &mut self,
        model: Affine3,
        count: usize,
        texture: Option<&Arc<Texture>>,
        lightmap: Option<&Arc<Texture>>,
        blend: u16,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> Result<([VertexUv2; 3], Color)>,
    {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.drop_closed_frame();
        let (mark, textures) = (self.triangles.len(), self.textures.len());
        let lights = self.lighting.len();
        let result = self
            .record_texture(texture)
            .and_then(|index| {
                // La lightmap est une entrée de la même table : le rasteriseur
                // ne lit qu'un seul genre de ressource, et un décor qui
                // partage une lightmap entre plusieurs murs n'en garde qu'une
                // copie par la même déduplication.
                let lit = match lightmap {
                    Some(lightmap) => Some(self.record_texture(Some(lightmap))?),
                    None => None,
                };
                Ok((index, lit))
            })
            // Le mode voyage dans l'index de texture, faute d'un octet libre
            // dans le triangle préparé : le bit de poids fort le porte, et
            // `Prepared::texture` le retire avant toute indexation.
            .and_then(|(index, lit)| self.submit_batch(model, count, index | blend, lit, read));
        // Un lot refusé, mais aussi un lot accepté dont pas un triangle n'a
        // survécu à la projection : dans les deux cas la table garderait une
        // texture que plus rien ne référence, et le plafond ne se déduirait
        // plus du nombre de triangles préparés — il suffirait de soumettre des
        // lots invisibles pour le remplir.
        if result.is_err() || self.triangles.len() == mark {
            self.triangles.truncate(mark);
            self.lighting.truncate(lights);
            // La texture n'est retirée que si ce lot l'a ajoutée : déjà
            // présente, la table n'a pas grandi et la troncature ne fait rien.
            self.textures.truncate(textures);
        }
        result
    }

    /// L'index d'une texture dans la table de l'image, en l'y ajoutant si elle
    /// n'y est pas encore.
    ///
    /// La déduplication se fait par identité de l'allocation et non par
    /// contenu : deux textures identiques chargées séparément sont deux
    /// ressources, et les confondre demanderait de comparer des mégaoctets à
    /// chaque lot. Le balayage est linéaire parce qu'il a lieu une fois par
    /// lot, jamais par triangle.
    fn record_texture(&mut self, texture: Option<&Arc<Texture>>) -> Result<u16> {
        let Some(texture) = texture else {
            return Ok(NO_TEXTURE);
        };
        if let Some(index) = self.textures.iter().position(|t| Arc::ptr_eq(t, texture)) {
            return Ok(index as u16);
        }
        if self.textures.len() >= texture_capacity(self.config.capacity()) {
            return Err(Error::InvalidArgument(Argument::TextureCapacity));
        }
        self.textures.push(Arc::clone(texture));
        Ok((self.textures.len() - 1) as u16)
    }

    /// Le corps de [`Context::submit_each_uv`], qui peut laisser le lot à
    /// moitié posé.
    fn submit_batch<F>(
        &mut self,
        model: Affine3,
        count: usize,
        texture: u16,
        lit: Option<u16>,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> Result<([VertexUv2; 3], Color)>,
    {
        let transform = self.view.product(model);
        // Une fois par lot, comme les lumières : trois produits vectoriels ne
        // se mesurent pas devant le nombre de sommets qu'ils vont porter. La
        // vue étant rigide et directe, ses cofacteurs sont elle-même et son
        // déterminant vaut un : les prendre sur la composée revient à les
        // prendre sur la seule matrice modèle, sans la composer à part.
        // **Une matrice miroir retourne le sens de parcours de toutes les
        // faces**, et le test de dos du rasteriseur n'a alors plus rien à
        // départager : les faces tournées vers la caméra y passent pour des
        // faces arrière, celles du fond pour des faces avant, et on voit
        // l'intérieur de l'objet. Le lot se soumet donc avec deux de ses
        // sommets échangés, ce qui rend au sens de parcours ce que le miroir
        // lui a pris. Échanger ici plutôt que lever le test au rasteriseur :
        // la négation des fonctions de bord doit valoir pour tous les
        // triangles, sans quoi deux voisins revendiqueraient leur arête commune
        // deux fois ou pas du tout.
        //
        // Le déterminant vient du même appel que les normales : il décide des
        // deux, et deux calculs pourraient en diverger sur un lot presque plat.
        let (normals, determinant) = transform.cofactors();
        let mirrored = determinant < 0.0;
        // Une fois par lot : la vue ne change pas pendant une image, et huit
        // transformations rigides ne se mesurent pas devant le nombre de
        // sommets qu'elles vont éclairer.
        self.place_lights();
        for i in 0..count {
            let (corners, color) = read(i)?;
            // Avant la transformation : c'est la valeur écrite par l'hôte qu'on
            // refuse, pas ce que la caméra en fait. Un sommet fini que la
            // matrice porte à l'infini reste une condition de vue, et son
            // triangle disparaît plus bas sans erreur.
            if corners.iter().any(|c| {
                !c.position.x.is_finite() || !c.position.y.is_finite() || !c.position.z.is_finite()
            }) {
                return Err(Error::InvalidArgument(Argument::VertexCoordinate));
            }
            // Les coordonnées de texture, elles, ne dépendent d'aucune
            // transformation : la borne porte sur la valeur exacte que l'hôte a
            // écrite, et le découpage n'en produira que des combinaisons
            // convexes. Revalider en aval serait de la défensive sur une valeur
            // qui ne peut plus sortir que par un défaut du moteur.
            let off = |c: f32| c.is_nan() || c.abs() > MAX_TEXEL_COORD;
            if corners
                .iter()
                .any(|c| off(c.u) || off(c.v) || off(c.u2) || off(c.v2))
            {
                return Err(Error::InvalidArgument(Argument::TextureCoordinate));
            }
            let mut sources = corners.map(|c| {
                let view = transform.transform_point(c.position);
                ClipSource {
                    view,
                    u: c.u,
                    v: c.v,
                    u2: c.u2,
                    v2: c.v2,
                    // **Les lumières sont déjà en espace de vue** : la
                    // transformation a eu lieu une fois pour le lot, et la
                    // distance y est la même qu'en monde puisque la vue
                    // est rigide.
                    // La normale passe par les cofacteurs et non par la
                    // transformation elle-même : la matrice modèle n'est
                    // tenue qu'à être affine, et une échelle non uniforme
                    // inclinerait la normale du mauvais côté.
                    light: dynamic::sum(&self.placed, view, normals.transform_vector(c.normal)),
                }
            });
            if mirrored {
                sources.swap(1, 2);
            }
            self.submit_view(sources, color, texture, lit)?;
        }
        Ok(())
    }

    /// Ajoute un triangle déjà projeté à l'image en cours.
    ///
    /// La couleur y est déjà l'entier du rasteriseur : [`Color`] appartient à
    /// la scène, et se convertit au plus tôt.
    ///
    /// Un triangle éclairé range ses plans dans le tableau annexe à la place
    /// qu'il retient. Le rangement se fait après les deux refus : un triangle
    /// qu'on n'ajoute pas ne doit rien laisser derrière lui, et le tableau
    /// annexe n'a aucun moyen de désigner ses trous.
    fn push(&mut self, v: [Vertex; 3], color: u32, texture: u16, lit: Option<u16>) -> Result<()> {
        let slot = self.lighting.len();
        // **Dès qu'une lumière est réglée, elle vaut pour toute la scène**, y
        // compris pour les triangles hors de sa portée. Sans cela, un triangle
        // qu'aucune lumière n'atteint garderait sa couleur pleine pendant que
        // son voisin à demi éclairé s'assombrit là où la lumière ne porte
        // pas : la transition entre les deux serait une marche franche, au
        // milieu d'une surface continue.
        let shaded = !self.placed.is_empty();
        // Le drapeau des plans, lui, reste par triangle : celui qu'aucune
        // lumière n'atteint garde des plans nuls, et le remplissage le sait.
        let glowing = v.iter().any(|vertex| vertex.light != [0; 3]);
        let prepared = if let Some(lightmap) = lit.or(shaded.then_some(NO_TEXTURE)) {
            if slot >= lighting_capacity(self.config.capacity()) {
                return Err(Error::InvalidArgument(Argument::TriangleCapacity));
            }
            // `slot` est sous la sentinelle, donc sous `u16::MAX`.
            prepare_lit(v, color, texture, lightmap, glowing, slot as u16)
                .map(|(p, l)| (p, Some(l)))
        } else {
            prepare(v, color, texture).map(|p| (p, None))
        };
        let Some((triangle, lighting)) = prepared else {
            return Ok(());
        };
        if self.triangles.len() >= self.config.capacity() {
            return Err(Error::InvalidArgument(Argument::TriangleCapacity));
        }
        self.triangles.push(triangle);
        if let Some(lighting) = lighting {
            self.lighting.push(lighting);
        }
        Ok(())
    }

    /// Soumet un triangle donné en espace de vue : découpe, projection,
    /// préparation.
    ///
    /// **La découpe a lieu ici et non au début d'image**, parce que c'est la
    /// seule place qui rende vraie la clause de capacité : un triangle découpé
    /// en produit jusqu'à six, et le refus doit tomber sur l'appel qui déborde
    /// plutôt que sur une image entière déjà soumise. La capacité compte donc
    /// des triangles préparés, pas soumis.
    ///
    /// Un sommet qu'on ne peut pas projeter fait disparaître le triangle sans
    /// erreur : c'est une donnée, pas un défaut du moteur.
    /// Soumet des lignes, dans le repère que `model` porte.
    ///
    /// **Le lot est accepté ou refusé en entier**, comme tout lot : une
    /// coordonnée non finie ou un dépassement de capacité le rejette sans rien
    /// laisser dans l'image. Une ligne qui ne se projette pas — derrière le plan
    /// proche, hors de la bande de garde — disparaît sans erreur : c'est une
    /// donnée.
    ///
    /// Un segment découpé reste un segment : il consomme une place, et une
    /// seule, ce qui rend la capacité lisible pour l'appelant — contrairement
    /// aux triangles, dont le découpage multiplie les places consommées.
    pub fn submit_lines(&mut self, model: Affine3, lines: &[Line], depth: DepthMode) -> Result<()> {
        self.submit_segments(model, lines.len(), depth, |i| {
            let line = &lines[i];
            (line.a, line.b, line.color)
        })
    }

    /// Soumet des points, dans le repère que `model` porte.
    ///
    /// Un point est un segment de longueur nulle du côté de l'hôte, mais pas du
    /// côté du tracé : la règle du losange n'allumerait rien pour lui. Il est
    /// donc porté jusqu'au pixel qui le contient par un chemin à lui, et c'est
    /// la seule chose qui distingue les deux soumissions.
    pub fn submit_points(
        &mut self,
        model: Affine3,
        points: &[Point],
        depth: DepthMode,
    ) -> Result<()> {
        self.submit_segments(model, points.len(), depth, |i| {
            let point = &points[i];
            (point.at, point.at, point.color)
        })
    }

    /// Soumet des lignes lues une par une, sans tranche à traverser.
    ///
    /// La forme qu'emploie la frontière C : elle lit **ses** structures sur
    /// place, sans les recopier — ce serait une allocation par image — ni les
    /// réinterpréter, ce qui imposerait au noyau une disposition mémoire qu'il
    /// n'a pas choisie. La forme par tranche reste celle d'un appelant Rust.
    pub fn submit_each_line<F>(
        &mut self,
        model: Affine3,
        count: usize,
        depth: DepthMode,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> Line,
    {
        self.submit_segments(model, count, depth, |i| {
            let line = read(i);
            (line.a, line.b, line.color)
        })
    }

    /// Soumet des points lus un par un, pour la raison de
    /// [`Context::submit_each_line`].
    pub fn submit_each_point<F>(
        &mut self,
        model: Affine3,
        count: usize,
        depth: DepthMode,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> Point,
    {
        self.submit_segments(model, count, depth, |i| {
            let point = read(i);
            (point.at, point.at, point.color)
        })
    }

    /// Le corps commun des quatre soumissions de tracé.
    ///
    /// Un point arrive ici avec ses deux extrémités confondues, ce que la
    /// préparation reconnaît et traite à part.
    fn submit_segments<F>(
        &mut self,
        model: Affine3,
        count: usize,
        depth: DepthMode,
        read: F,
    ) -> Result<()>
    where
        F: Fn(usize) -> (Vec3, Vec3, Color),
    {
        if *self.state.get_mut() != RECORDING {
            return Err(Error::InvalidState);
        }
        self.drop_closed_frame();

        let mark = self.segments.len();
        let transform = self.view.product(model);
        for i in 0..count {
            let (a, b, color) = read(i);
            // Avant la transformation, comme pour un sommet de triangle : c'est
            // la valeur écrite par l'hôte qu'on refuse, pas ce que la caméra en
            // fait.
            if [a, b]
                .iter()
                .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
            {
                self.segments.truncate(mark);
                return Err(Error::InvalidArgument(Argument::VertexCoordinate));
            }
            if let Err(error) = self.push_segment(transform, a, b, color, depth) {
                self.segments.truncate(mark);
                return Err(error);
            }
        }
        Ok(())
    }

    /// Projette un segment et le retient, ou ne retient rien s'il sort du tronc.
    fn push_segment(
        &mut self,
        transform: Affine3,
        a: Vec3,
        b: Vec3,
        color: Color,
        depth: DepthMode,
    ) -> Result<()> {
        let point = a == b;
        let to_clip = |p: Vec3| {
            self.projection
                .to_clip(transform.transform_point(p), 0.0, 0.0, 0.0, 0.0, [0.0; 3])
        };
        let (Some(ca), Some(cb)) = (to_clip(a), to_clip(b)) else {
            return Ok(());
        };
        let Some((ca, cb)) = clip_segment(ca, cb, self.projection.frustum()) else {
            return Ok(());
        };

        let (pa, pb) = (self.projection.to_vertex(ca), self.projection.to_vertex(cb));
        // **Un point porte son drapeau, il ne se déguise pas en segment court.**
        // La première écriture lui donnait un seizième de pixel de long, en
        // comptant sur la règle du losange pour allumer le pixel qui le
        // contient : un tel segment n'en sort jamais, donc il n'allumait rien.
        // La scène de conformance du tracé l'a montré avant qu'une empreinte le
        // fige — c'est ce pour quoi elle existe.
        if self.segments.len() >= self.config.line_capacity() {
            return Err(Error::InvalidArgument(Argument::LineCapacity));
        }
        self.segments.push(Segment {
            x0: pa.x,
            y0: pa.y,
            z0: pa.z,
            x1: pb.x,
            y1: pb.y,
            z1: pb.z,
            color: color.packed(),
            tested: depth == DepthMode::Tested,
            point,
        });
        Ok(())
    }

    fn submit_view(
        &mut self,
        view: [ClipSource; 3],
        color: Color,
        texture: u16,
        lit: Option<u16>,
    ) -> Result<()> {
        let mut homogeneous = [ClipVertex::ZERO; 3];
        for (slot, source) in homogeneous.iter_mut().zip(view) {
            match self.projection.to_clip(
                source.view,
                source.u,
                source.v,
                source.u2,
                source.v2,
                source.light,
            ) {
                Some(vertex) => *slot = vertex,
                None => return Ok(()),
            }
        }

        let polygon = clip(homogeneous, self.projection.frustum());
        // La borne est démontrée par la géométrie du découpage ; c'est ici
        // qu'elle décide du nombre de places qu'un triangle soumis consomme
        // dans la capacité.
        debug_assert!(polygon.triangle_count() <= MAX_CLIP_TRIANGLES);
        for i in 0..polygon.triangle_count() {
            let vertices = polygon
                .triangle(i)
                .map(|c| Vertex::from(self.projection.to_vertex(c)));
            self.push(vertices, color.packed(), texture, lit)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod mesh_tests;
#[cfg(test)]
mod tests;
