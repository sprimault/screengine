// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Une image en cours de rendu, tuile par tuile.
//!
//! Entre le début et la fin, des tuiles distinctes peuvent se rendre depuis des
//! threads distincts : la [`Frame`] ne se lit qu'en partage, et chaque tuile
//! n'écrit que dans sa pile et dans son rectangle du tampon de l'hôte.

use core::sync::atomic::Ordering;

use crate::context::{
    BYTES_PER_PIXEL, CLEAR_COLOR, CLOSING, Context, OPAQUE, RECORDING, RENDERING,
};
use crate::error::{Argument, Error, Result};
use crate::light;
use crate::light::grade::{Identity, Transfer};
use crate::raster::{Lit, NO_LIGHTING, NO_TEXTURE, Rect, Sampling, Target, fill};

/// Le plus grand côté de tuile, qui dimensionne le tampon de travail posé sur
/// la pile.
const MAX_TILE: usize = 64;

/// Où une image écrit ses pixels : le tampon de l'hôte, ou une partie de
/// celui-ci.
///
/// Un trait plutôt qu'une tranche parce que deux tuiles de la même ligne
/// partagent des lignes du tampon : pour les rendre depuis deux threads, la
/// frontière C fournit à chacune ses propres morceaux de ligne, là où un appelant
/// Rust passe le tampon entier ou une bande.
pub trait Output {
    /// Refuse une sortie qui ne peut pas recevoir `rect`, avant que la moindre
    /// tuile ne soit prise.
    fn check(&self, rect: Rect) -> Result<()>;

    /// Les `width × 4` octets de la ligne `y` qui commencent à la colonne `x`,
    /// en coordonnées de l'image, ou `None` s'ils ne sont pas dans la sortie.
    fn span(&mut self, x: u32, y: u32, width: u32) -> Option<&mut [u8]>;
}

/// Un tampon d'hôte rangé par lignes, ou une bande de lignes consécutives.
#[derive(Debug)]
pub struct Rows<'a> {
    pixels: &'a mut [u8],
    stride: u32,
    first_row: u32,
}

impl<'a> Rows<'a> {
    /// Le tampon entier, `stride` pixels par ligne.
    pub fn new(pixels: &'a mut [u8], stride: u32) -> Self {
        Self::band(pixels, stride, 0)
    }

    /// Une bande du tampon dont la première ligne est la ligne `first_row` de
    /// l'image.
    ///
    /// C'est ce qui permet à un appelant Rust de rendre sur plusieurs threads
    /// sans `unsafe` : des bandes disjointes s'obtiennent par `chunks_mut`, et
    /// chaque thread rend les tuiles de la sienne.
    pub fn band(pixels: &'a mut [u8], stride: u32, first_row: u32) -> Self {
        Self {
            pixels,
            stride,
            first_row,
        }
    }

    /// Le décalage en octets du début de la ligne `y` de l'image.
    fn row_start(&self, y: u32) -> Option<usize> {
        (y.checked_sub(self.first_row)? as usize)
            .checked_mul(self.stride as usize)?
            .checked_mul(BYTES_PER_PIXEL)
    }
}

impl Output for Rows<'_> {
    fn check(&self, rect: Rect) -> Result<()> {
        if rect.x + rect.width > self.stride {
            return Err(Error::InvalidArgument(Argument::Stride));
        }
        if rect.y < self.first_row {
            return Err(Error::InvalidArgument(Argument::BufferLength));
        }
        // Seul le `stride` peut faire déborder le produit : la région est déjà
        // bornée par l'image.
        let end = self
            .row_start(rect.y + rect.height)
            .ok_or(Error::InvalidArgument(Argument::Stride))?;
        if self.pixels.len() < end {
            return Err(Error::InvalidArgument(Argument::BufferLength));
        }
        Ok(())
    }

    fn span(&mut self, x: u32, y: u32, width: u32) -> Option<&mut [u8]> {
        let start = self.row_start(y)? + x as usize * BYTES_PER_PIXEL;
        self.pixels
            .get_mut(start..start + width as usize * BYTES_PER_PIXEL)
    }
}

/// Les tampons de couleur et de profondeur d'une région, vus comme un puits de
/// remplissage.
///
/// Deux tableaux et non un tableau de paires : le test de profondeur lit et
/// écrit une rangée de `u32` contigus, ce que le SIMD de l'étape 9 chargera d'un
/// bloc.
struct Scratch<'a> {
    color: &'a mut [u32],
    depth: &'a mut [u32],
    rect: Rect,
}

impl Scratch<'_> {
    /// L'indice d'un pixel de l'image dans les tampons de la région.
    fn index(&self, x: i32, y: i32) -> usize {
        let row = (y - self.rect.y as i32) as usize;
        let column = (x - self.rect.x as i32) as usize;
        row * self.rect.width as usize + column
    }
}

impl Target for Scratch<'_> {
    fn test(&mut self, x: i32, y: i32, z: u32) -> bool {
        // Strict : à profondeur égale, le triangle soumis le premier reste. Les
        // triangles se dessinent dans l'ordre de soumission, quelle que soit la
        // tuile, et c'est ce qui rend l'égalité indépendante du découpage.
        z > self.depth[self.index(x, y)]
    }

    fn write(&mut self, x: i32, y: i32, z: u32, color: u32) {
        let i = self.index(x, y);
        self.depth[i] = z;
        self.color[i] = color;
    }
}

/// Le bit de poids fort de `in_flight` : une tuile n'est pas revenue de son
/// rendu.
///
/// Dans le compteur plutôt qu'à côté, pour qu'une seule lecture réponde aux
/// deux questions que la fin d'image pose. Les trente et un bits restants
/// comptent des tuiles simultanées, donc des threads : la marge est sans
/// commune mesure avec ce qu'un hôte peut en lancer.
const FAULT: u32 = 1 << 31;

/// Décompte une tuile en cours, et note celle qui n'en ressort pas.
///
/// Le noyau ne sait pas ce qu'est une panique. Il sait seulement qu'un rendu de
/// tuile est entré sans appeler [`InFlight::done`], ce qui suffit : un dépliage
/// passe par `drop` sans passer par là.
///
/// **Le décompte et la défaillance sont la même valeur**, relâchés d'une seule
/// opération — voir [`FAULT`]. C'est ce qui rend la garantie indépendante de
/// qui gagne la course, là où deux valeurs laisseraient entre elles un instant
/// pendant lequel la fin d'image conclurait à tort.
struct InFlight<'a> {
    context: &'a Context,
    done: bool,
}

impl<'a> InFlight<'a> {
    /// Prend une tuile en compte pour la durée de son rendu.
    fn new(context: &'a Context) -> Self {
        context.in_flight.fetch_add(1, Ordering::SeqCst);
        Self {
            context,
            done: false,
        }
    }

    /// Note que le rendu est sorti normalement, quel que soit son résultat.
    ///
    /// Une erreur rendue est une sortie normale : l'appelant la reçoit et sait
    /// à quoi s'en tenir. Ce que ce garde attrape est l'absence de retour.
    fn done(&mut self) {
        self.done = true;
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mark = if self.done { 0 } else { FAULT };
        // **Une seule opération**, et c'est tout le mécanisme : le décompte et
        // la défaillance ne peuvent pas être observés séparément, donc il
        // n'existe aucun instant où la fin d'image verrait zéro tuile en vol
        // sans voir la tuile perdue. En deux atomiques, cet instant existe —
        // quelques instructions, mais c'est exactement le cas que la clause
        // sert à couvrir, et l'ordre correct ne se vérifierait par aucun test.
        let _ = self
            .context
            .in_flight
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| Some((v - 1) | mark));
    }
}

impl Context {
    /// Rend la tuile `index` de l'image commencée dans `out`.
    ///
    /// Appelable depuis plusieurs threads à la fois, pour des index distincts.
    /// Une tuile se rend une fois par image : un index déjà pris, y compris
    /// par un appel simultané, rend [`Error::InvalidState`], comme une tuile
    /// hors d'une image commencée. La sortie est vérifiée avant la prise, pour
    /// qu'un refus ne consomme pas la tuile.
    pub fn tile<O: Output>(&self, index: u32, out: &mut O) -> Result<()> {
        // Compter avant de lire l'état, et la fin fait l'inverse : avec des
        // opérations séquentiellement cohérentes, une tuile qui voit encore le
        // rendu ouvert est forcément vue par la fin.
        //
        // Le corps est à part pour que le garde n'ait qu'un seul point de
        // sortie à marquer : un `return` de plus qui l'oublierait ferait passer
        // un refus ordinaire pour une tuile perdue.
        let mut in_flight = InFlight::new(self);
        let result = self.tile_inner(index, out);
        in_flight.done();
        result
    }

    /// Le corps de [`Context::tile`], une fois la tuile décomptée.
    fn tile_inner<O: Output>(&self, index: u32, out: &mut O) -> Result<()> {
        if self.state.load(Ordering::SeqCst) != RENDERING {
            return Err(Error::InvalidState);
        }
        if index >= self.grid.count() {
            return Err(Error::InvalidArgument(Argument::TileIndex));
        }
        let rect = self.grid.rect(index);
        // Les lignes de la tuile sur toute la largeur de l'image, pas le seul
        // rectangle : un `stride` plus court que l'image est une erreur de
        // l'hôte même pour une tuile qui n'atteint pas le bord.
        out.check(Rect {
            x: 0,
            width: self.grid.image().width,
            ..rect
        })?;
        if self.taken[index as usize].swap(true, Ordering::SeqCst) {
            return Err(Error::InvalidState);
        }
        self.render_tile(index, rect, out)
    }

    /// Rend les tuiles que personne n'a prises, puis clôt l'image.
    ///
    /// Refuse par [`Error::InvalidState`] une image qui n'est pas commencée, ou
    /// dont une tuile se rend encore sur un autre thread ; l'image reste alors
    /// ouverte. Une sortie refusée la laisse ouverte aussi, pour que l'appelant
    /// corrige son tampon et recommence — **en rappelant cette fonction**, qui
    /// ne prend qu'un `&self` et reste donc disponible quoi qu'il arrive à la
    /// [`Frame`] qui l'avait appelée la première fois.
    pub fn end<O: Output>(&self, out: &mut O) -> Result<()> {
        if self
            .state
            .compare_exchange(RENDERING, CLOSING, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Error::InvalidState);
        }
        let result = self.finish(out);
        let next = if result.is_ok() { RECORDING } else { RENDERING };
        if next == RECORDING {
            // L'image est close : sa liste de dessin ne sert plus. La vider ici
            // demanderait un `&mut`, que la fin n'a pas ; le prochain appel
            // exclusif s'en charge.
            self.stale.store(true, Ordering::SeqCst);
        }
        self.state.store(next, Ordering::SeqCst);
        result
    }

    /// Le corps de [`Context::end`], une fois la main prise.
    fn finish<O: Output>(&self, out: &mut O) -> Result<()> {
        // Une seule lecture pour les deux questions : reste-t-il une tuile en
        // vol, et en a-t-on perdu une. Les séparer rouvrirait la fenêtre que
        // le garde ferme.
        let flight = self.in_flight.load(Ordering::SeqCst);
        if flight & !FAULT != 0 {
            return Err(Error::InvalidState);
        }
        if flight & FAULT != 0 {
            return Err(Error::Faulted);
        }
        out.check(self.grid.image())?;
        for index in 0..self.grid.count() {
            if !self.taken[index as usize].swap(true, Ordering::SeqCst) {
                self.render_tile(index, self.grid.rect(index), out)?;
            }
        }
        Ok(())
    }

    /// Rend une tuile dans des tampons de travail posés sur la pile : 32 Kio,
    /// sous le minimum que l'ABI exige du thread appelant.
    fn render_tile<O: Output>(&self, index: u32, rect: Rect, out: &mut O) -> Result<()> {
        let mut color = [0u32; MAX_TILE * MAX_TILE];
        let mut depth = [0u32; MAX_TILE * MAX_TILE];
        let pixels = rect.width as usize * rect.height as usize;
        let scratch = Scratch {
            color: &mut color[..pixels],
            depth: &mut depth[..pixels],
            rect,
        };
        self.draw(scratch, self.bins.tile(index), out)
    }

    /// Dessine `triangles` dans `scratch`, puis recopie la région dans `out`.
    ///
    /// `to_le_bytes` plutôt qu'une réinterprétation du tampon : l'ordre R, G, B,
    /// A en mémoire est celui de l'ABI, et le déduire de l'ordre natif de la
    /// cible marcherait partout aujourd'hui pour de mauvaises raisons.
    fn draw<O: Output>(
        &self,
        mut scratch: Scratch<'_>,
        triangles: impl Iterator<Item = u32>,
        out: &mut O,
    ) -> Result<()> {
        let rect = scratch.rect;
        scratch.color.fill(CLEAR_COLOR);
        // Zéro est infiniment loin : toute profondeur bornée par `to_depth`
        // le bat.
        scratch.depth.fill(0);
        // Le curseur des fenêtres de traversée. `Merge` rend les index par ordre
        // croissant — c'est l'invariant que le test strict de profondeur exige
        // déjà —, si bien qu'un curseur qui ne revient jamais en arrière suffit :
        // une comparaison par triangle, aucune recherche, et rien à ranger dans
        // le triangle préparé, qui est plein.
        let mut cursor = 0;
        for index in triangles {
            let triangle = &self.triangles[index as usize];
            // La fenêtre ne borne que la boucle, jamais les valeurs : c'est ce
            // qui rend l'image identique avec ou sans elle, exactement comme
            // elle l'est indépendamment du découpage en tuiles.
            let window = if index < self.visited_end {
                while cursor + 1 < self.visits.len()
                    && self.visits[cursor + 1].first_triangle <= index
                {
                    cursor += 1;
                }
                rect.intersect(self.visits[cursor].window)
            } else {
                rect
            };
            // L'index se résout ici, une fois par triangle : le rasteriseur
            // reçoit la texture et ne connaît ni la table ni le comptage de
            // références, qui appartiennent au contexte.
            let sampling = match triangle.texture() {
                NO_TEXTURE => None,
                index => self.textures.get(index as usize).map(|texture| Sampling {
                    texture: texture.as_ref(),
                    filter: self.filter,
                }),
            };
            // Deux index à résoudre, et non un : la place des plans dans le
            // tableau annexe, puis la lightmap que ces plans désignent.
            let lit = match triangle.lighting() {
                NO_LIGHTING => None,
                // La lightmap peut manquer : un triangle que seules des
                // lumières dynamiques éclairent porte quand même ses plans.
                slot => self.lighting.get(slot as usize).map(|planes| Lit {
                    lightmap: self
                        .textures
                        .get(planes.lightmap() as usize)
                        .map(|texture| texture.as_ref()),
                    planes,
                    overbright: self.overbright,
                }),
            };
            fill(&mut scratch, window, triangle, sampling, lit);
        }

        // Le post-traitement se choisit **une fois par tuile**, et la recopie
        // se monomorphise sur ce choix. Le tester par pixel le ferait payer à
        // toute scène, y compris à celles qui n'en ont pas.
        if self.grade.is_set() {
            self.blit(&scratch, rect, out, &self.grade)
        } else {
            self.blit(&scratch, rect, out, &Identity)
        }
    }

    /// Recopie la tuile rendue dans le tampon de l'hôte, en y appliquant le
    /// brouillard puis la courbe de sortie.
    ///
    /// L'ordre n'est pas indifférent : le brouillard est un mélange en
    /// couleurs directes, la courbe est la transformation de sortie, et elle
    /// vient donc en dernier — sans quoi le fond embrumé et la géométrie
    /// embrumée ne traverseraient pas la même chose.
    fn blit<O: Output, T: Transfer>(
        &self,
        scratch: &Scratch<'_>,
        rect: Rect,
        out: &mut O,
        transfer: &T,
    ) -> Result<()> {
        let width = rect.width as usize;
        let fog = self.fog.is_set().then_some((&self.fog, self.fog.color()));
        for (row, (source, depths)) in scratch
            .color
            .chunks_exact(width.max(1))
            .zip(scratch.depth.chunks_exact(width.max(1)))
            .enumerate()
        {
            let span = out
                .span(rect.x, rect.y + row as u32, rect.width)
                .ok_or(Error::InvalidArgument(Argument::BufferLength))?;
            // **Le brouillard s'applique ici**, et non dans le remplissage :
            // la profondeur de la tuile est déjà sous la main, un pixel que
            // rien n'a peint la garde nulle — donc infiniment lointaine —, et
            // le fond prend ainsi le brouillard plein sans traitement à part.
            // C'est cette absence de traitement à part qui supprime la couture
            // d'horizon.
            if let Some((fog, color)) = fog {
                let y = rect.y + row as u32;
                for (i, (pixel, slot)) in source
                    .iter()
                    .zip(span.chunks_exact_mut(BYTES_PER_PIXEL))
                    .enumerate()
                {
                    let x = rect.x + i as u32;
                    let factor = fog.factor(depths[i]);
                    let mixed = light::fog::blend(*pixel, color, factor, light::fog::dither(x, y));
                    slot.copy_from_slice(&(transfer.apply(mixed) | OPAQUE).to_le_bytes());
                }
                continue;
            }
            for (pixel, slot) in source.iter().zip(span.chunks_exact_mut(BYTES_PER_PIXEL)) {
                // L'alpha est forcé ici, au seul endroit que les deux chemins
                // de sortie traversent. Le contrat d'ABI le promet opaque, et
                // c'est ce qui permet aux hôtes d'annoncer l'opacité pour que
                // le compositeur saute le mélange — un canal laissé à la valeur
                // de l'hôte rendrait un décor troué dans un navigateur, seule
                // cible où il est réellement composité.
                slot.copy_from_slice(&(transfer.apply(*pixel) | OPAQUE).to_le_bytes());
            }
        }
        Ok(())
    }
}

/// Une image entre son début et sa fin.
///
/// `Sync` : des tuiles d'index distincts se rendent depuis des threads
/// distincts. Elle emprunte le contexte, donc rien d'autre ne le touche tant
/// qu'elle vit, et le compilateur tient la séquence que la frontière C doit
/// vérifier à l'exécution.
#[derive(Debug)]
pub struct Frame<'a> {
    context: &'a Context,
    /// Vrai quand une fin d'image a été tentée et refusée.
    ///
    /// Le `Drop` referme l'image d'une `Frame` simplement abandonnée — sans
    /// quoi le contexte resterait en rendu pour toujours. Mais une fin refusée
    /// n'est pas un abandon : le noyau laisse alors l'image ouverte exprès,
    /// pour que l'appelant corrige sa sortie et rappelle [`Context::end`]. La
    /// refermer ici lui retirerait cette reprise, et sa scène avec.
    attempted: bool,
}

impl<'a> Frame<'a> {
    /// Ouvre le rendu d'une image déjà répartie.
    pub(super) fn new(context: &'a Context) -> Self {
        Self {
            context,
            attempted: false,
        }
    }

    /// Le nombre de tuiles de l'image, numérotées ligne par ligne.
    pub fn tile_count(&self) -> u32 {
        self.context.grid.count()
    }

    /// Rend la tuile `index` dans `out`. Voir [`Context::tile`].
    pub fn tile<O: Output>(&self, index: u32, out: &mut O) -> Result<()> {
        self.context.tile(index, out)
    }

    /// Rend les tuiles que personne n'a prises, puis clôt l'image.
    ///
    /// Appelée seule, elle rend l'image entière : c'est ce qui garde valide un
    /// hôte qui n'appelle jamais [`Frame::tile`].
    ///
    /// **Elle consomme la `Frame`, y compris quand elle échoue** — une sortie
    /// trop courte, un `stride` faux. L'image, elle, reste alors ouverte, et se
    /// termine en rappelant [`Context::end`] avec la sortie corrigée : cette
    /// dernière ne prend qu'un `&self`, donc elle reste disponible une fois la
    /// `Frame` partie. C'est le pendant exact d'un hôte C qui rappelle
    /// `scg_frame_end` après un refus.
    ///
    /// Écarté : rendre la `Frame` à côté de l'erreur. Ça alourdirait la
    /// signature de tout le monde, et casserait `?`, pour un cas qui a une
    /// issue. Écarté aussi, prendre `&self` ici : la consommation est ce qui
    /// garantit qu'une image close ne resserve pas.
    pub fn end<O: Output>(mut self, out: &mut O) -> Result<()> {
        // Noté avant l'appel, et non après : le `Drop` qui suit doit voir la
        // tentative même si celle-ci panique.
        self.attempted = true;
        self.context.end(out)
    }

    /// Rend une région quelconque de l'image, sans passer par la répartition.
    ///
    /// Le chemin de référence des tuiles : tous les triangles, dans l'ordre de
    /// soumission, dans des tampons de couleur et de profondeur fournis par
    /// l'appelant, au moins aussi grands que la région. Une tuile qui en
    /// diffère a perdu un triangle à la répartition, ou dépend de son
    /// découpage. Ne prend aucune tuile.
    ///
    /// Deux tampons de longueurs différentes sont refusés plutôt que ramenés
    /// au plus court : c'est la même erreur d'appelant qu'un `stride` faux, et
    /// elle se dit au même endroit.
    pub fn region<O: Output>(
        &self,
        rect: Rect,
        color: &mut [u32],
        depth: &mut [u32],
        out: &mut O,
    ) -> Result<()> {
        let image = self.context.grid.image();
        let inside = rect
            .x
            .checked_add(rect.width)
            .zip(rect.y.checked_add(rect.height))
            .is_some_and(|(right, bottom)| right <= image.width && bottom <= image.height);
        if !inside {
            return Err(Error::InvalidArgument(Argument::Region));
        }
        let pixels = rect.width as usize * rect.height as usize;
        if color.len() != depth.len() || color.len() < pixels {
            return Err(Error::InvalidArgument(Argument::ScratchLength));
        }
        out.check(rect)?;
        let scratch = Scratch {
            color: &mut color[..pixels],
            depth: &mut depth[..pixels],
            rect,
        };
        let triangles = 0..self.context.triangles.len() as u32;
        self.context.draw(scratch, triangles, out)
    }
}

/// Une `Frame` abandonnée sans fin referme l'image, pour que le contexte en
/// accepte une autre. Rien n'a pu la lire entre-temps : elle empruntait le
/// contexte.
impl Drop for Frame<'_> {
    fn drop(&mut self) {
        // Une fin tentée décide elle-même de l'état : réussie, elle a déjà
        // refermé ; refusée, elle a laissé l'image ouverte **exprès**, pour que
        // l'appelant corrige sa sortie et rappelle [`Context::end`]. Refermer
        // ici lui retirerait cette reprise et périmerait sa scène — et le
        // chemin Rust ne vaudrait plus le chemin C, où un hôte rappelle
        // simplement `scg_frame_end`.
        if self.attempted {
            return;
        }
        // Une `Frame` abandonnée, elle, doit refermer : sans quoi le contexte
        // resterait en rendu pour toujours, sans personne pour l'en sortir.
        self.context.stale.store(true, Ordering::SeqCst);
        self.context.state.store(RECORDING, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
