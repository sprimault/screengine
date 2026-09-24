// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

package screengine.host;

import java.nio.ByteBuffer;
import java.util.Arrays;

/**
 * Hôte Android sans fenêtre : la frontière vue depuis l'ART, à travers JNI.
 *
 * <p>Lancé par {@code app_process} sur l'appareil ou l'émulateur, sans APK ni
 * activité. Il éprouve ce que l'exécutable C du même hôte ne voit pas : le
 * chargement par {@code System.load}, {@code JNI_OnLoad} et l'enregistrement des
 * méthodes, la version d'ABI reçue signée, et un tampon direct dont la base est
 * volontairement désalignée. Il écrit l'empreinte du triangle sur la sortie
 * standard, les échecs sur la sortie d'erreur.
 *
 * <p>La scène est celle de {@code screengine-conformance --print arete} :
 * 640×360, tuiles de 64.
 *
 * <p>Usage : {@code app_process <dir> screengine.host.Test <dir>}
 */
public final class Test {
    /** Largeur de la scène. */
    private static final int WIDTH = 640;

    /** Hauteur de la scène. */
    private static final int HEIGHT = 360;

    /** Côté de tuile. */
    private static final int TILE = 64;

    /** Le stride de l'hôte, plus grand que la largeur pour qu'une fin de ligne existe. */
    private static final int STRIDE = WIDTH + 3;

    /** Marge sentinelle avant et après la zone rendue, en octets. */
    private static final int GUARD = 64;

    /** Décalage ajouté à la marge : la base du rendu tombe sur une adresse impaire. */
    private static final int SKEW = 1;

    /** L'octet sentinelle, qui ne ressemble à aucune couleur du rendu. */
    private static final byte SENTINEL = (byte) 0xA5;

    /** Nombre de vérifications en échec. */
    private static int failures;

    /** Pas d'instance. */
    private Test() {}

    /**
     * Enregistre une vérification, et dit laquelle a échoué.
     *
     * @param ok le résultat
     * @param what ce qui est vérifié
     */
    private static void check(boolean ok, String what) {
        if (!ok) {
            System.err.println("échec : " + what);
            failures++;
        }
    }

    /**
     * Une configuration valide, champs réservés à zéro.
     *
     * @return les huit champs de {@code ScgContextConfig}
     */
    private static int[] sceneConfig() {
        return new int[] {WIDTH, HEIGHT, WIDTH, HEIGHT, TILE, 0, 0, 0};
    }

    /**
     * Soumet la scène {@code arete} : le quadrilatère de la conformance, en
     * coordonnées de monde, vu par la caméra par défaut depuis l'origine.
     *
     * <p>Les mêmes valeurs que la scène de référence, décrites ici en Java et
     * passées à travers JNI puis l'ABI : c'est la comparaison des empreintes
     * qui dit qu'elles arrivent intactes.
     *
     * @param ctx handle du contexte
     * @return le code de retour de la soumission
     */
    private static int submitScene(long ctx) {
        float[] model = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        float[] vertices = {
            2.0f, 2.5f, 1.6f,
            3.5f, -2.5f, 1.6f,
            3.5f, -2.5f, -1.6f,
            2.0f, 2.5f, -1.6f,
        };
        // L'arête des sommets 0 et 2, parcourue en sens opposés par les deux.
        int[] indices = {0, 2, 1, 0, 3, 2};
        byte[] colors = {
            (byte) 0xE0, (byte) 0xA0, 0x30, (byte) 0xFF,
            (byte) 0xA0, (byte) 0xE0, 0x30, (byte) 0xFF,
        };
        return Screengine.submit(ctx, model, vertices, indices, colors);
    }

    /** Côté de la texture du sol, en texels, et côté d'une de ses cases. */
    private static final int FLOOR_SIDE = 64;

    private static final int FLOOR_CELL = 8;

    /**
     * Le damier procédural, teinte pour teinte comme la suite de conformance
     * l'écrit : c'est lui qui décide de l'empreinte, et un liseré décalé d'un
     * texel la ferait diverger.
     *
     * @return les texels, quatre octets chacun, lignes jointives
     */
    private static byte[] makeChecker() {
        byte[] texels = new byte[FLOOR_SIDE * FLOOR_SIDE * 4];
        for (int v = 0; v < FLOOR_SIDE; v++) {
            for (int u = 0; u < FLOOR_SIDE; u++) {
                int base = (v * FLOOR_SIDE + u) * 4;
                boolean edge = u % FLOOR_CELL == 0 || v % FLOOR_CELL == 0;
                boolean dark = (u / FLOOR_CELL + v / FLOOR_CELL) % 2 == 0;
                if (edge) {
                    texels[base] = (byte) 0xF0;
                    texels[base + 1] = (byte) 0xE0;
                    texels[base + 2] = (byte) 0xA0;
                } else if (dark) {
                    texels[base] = 0x30;
                    texels[base + 1] = 0x38;
                    texels[base + 2] = 0x50;
                } else {
                    texels[base] = (byte) 0x90;
                    texels[base + 1] = 0x70;
                    texels[base + 2] = 0x50;
                }
                texels[base + 3] = (byte) 0xFF;
            }
        }
        return texels;
    }

    /**
     * Rend la scène texturée sous le filtrage demandé, ou {@code null} si le
     * rendu a échoué.
     *
     * <p>La texture est détruite avant le rendu, à dessein : le moteur en garde
     * sa propre référence jusqu'à la fin de l'image, et l'empreinte le prouve.
     *
     * <p>La géométrie ne dépend pas du filtrage : un écart entre les deux
     * empreintes ne peut donc venir que de lui.
     *
     * @param filter {@code SCG_FILTER_DITHER} ou {@code SCG_FILTER_BILINEAR}
     * @return l'empreinte
     */
    private static String renderTextured(int filter) {
        long texture = Screengine.textureLoad(FLOOR_SIDE, FLOOR_SIDE, makeChecker());
        check(texture != 0, "la texture se charge sans contexte");

        long[] out = {0};
        if (texture == 0 || Screengine.create(sceneConfig(), out) != Screengine.OK) {
            check(false, "création du contexte texturé");
            return null;
        }

        check(Screengine.setFilter(out[0], filter) == Screengine.OK, "le filtrage se règle");

        float[] model = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        // Cinq flottants par sommet : la position, puis u et v en texels, à
        // huit texels par unité de monde comme la scène de référence.
        float[] vertices = {
            2.0f, -24.0f, -1.2f, 2.0f * 8.0f, -24.0f * 8.0f,
            60.0f, -24.0f, -1.2f, 60.0f * 8.0f, -24.0f * 8.0f,
            60.0f, 24.0f, -1.2f, 60.0f * 8.0f, 24.0f * 8.0f,
            2.0f, 24.0f, -1.2f, 2.0f * 8.0f, 24.0f * 8.0f,
        };
        int[] indices = {0, 1, 2, 0, 2, 3};
        byte[] colors = {
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
        };
        check(Screengine.submitTextured(out[0], model, vertices, indices, colors, texture)
                == Screengine.OK, "le lot texturé est accepté");
        Screengine.textureDestroy(texture);

        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        ByteBuffer block = ByteBuffer.allocateDirect(body);
        int code = Screengine.frameEnd(out[0], block, 0, STRIDE);
        check(code == Screengine.OK, "l'image texturée se rend");

        String hash = fingerprint(block, 0, STRIDE);
        Screengine.destroy(out[0]);
        return code == Screengine.OK ? hash : null;
    }

    /**
     * Rend la scène {@code gamma}, ou {@code null} si le rendu a échoué.
     *
     * <p>La même géométrie que {@code texture}, passée par la courbe de sortie.
     * C'est le seul endroit où cet hôte règle une courbe, et il y éprouve aussi
     * le refus d'un champ réservé non nul — sans quoi la promesse d'extension
     * ne serait vérifiée depuis aucun langage à objets.
     *
     * @return l'empreinte, ou {@code null}
     */
    private static String renderGraded() {
        long texture = Screengine.textureLoad(FLOOR_SIDE, FLOOR_SIDE, makeChecker());
        check(texture != 0, "la texture de la scène étalonnée se charge");

        long[] out = {0};
        if (texture == 0 || Screengine.create(sceneConfig(), out) != Screengine.OK) {
            check(false, "création du contexte étalonné");
            return null;
        }

        // Gamma, trois gains, trois décalages : les mêmes valeurs que la scène
        // de référence, et des décalages distincts pour qu'une permutation des
        // tables se voie.
        float[] grade = {2.2f, 1.15f, 1.0f, 0.85f, 0.04f, -0.02f, 0.08f};

        check(Screengine.setGrade(out[0], grade, 1) == Screengine.ERR_INVALID_ARGUMENT,
                "un champ réservé non nul est refusé");
        check(Screengine.clearGrade(out[0]) == Screengine.OK,
                "l'extinction sans courbe passe");
        check(Screengine.setGrade(out[0], grade, 0) == Screengine.OK, "la courbe se règle");

        float[] model = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        float[] vertices = {
            2.0f, -24.0f, -1.2f, 2.0f * 8.0f, -24.0f * 8.0f,
            60.0f, -24.0f, -1.2f, 60.0f * 8.0f, -24.0f * 8.0f,
            60.0f, 24.0f, -1.2f, 60.0f * 8.0f, 24.0f * 8.0f,
            2.0f, 24.0f, -1.2f, 2.0f * 8.0f, 24.0f * 8.0f,
        };
        int[] indices = {0, 1, 2, 0, 2, 3};
        byte[] colors = {
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
        };
        check(Screengine.submitTextured(out[0], model, vertices, indices, colors, texture)
                == Screengine.OK, "le lot de la scène étalonnée est accepté");
        Screengine.textureDestroy(texture);

        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        ByteBuffer block = ByteBuffer.allocateDirect(body);
        int code = Screengine.frameEnd(out[0], block, 0, STRIDE);
        check(code == Screengine.OK, "l'image étalonnée se rend");

        String hash = fingerprint(block, 0, STRIDE);
        Screengine.destroy(out[0]);
        return code == Screengine.OK ? hash : null;
    }

    /**
     * Rend la scène éclairée par des lumières, ou {@code null} si le rendu a
     * échoué.
     *
     * <p>Le sol et le mur sont découpés en panneaux : l'atténuation étant par
     * sommet, une surface d'un seul quadrilatère ne rendrait qu'un dégradé
     * entre ses quatre coins. Le sol va au-delà de la portée des trois
     * lumières, si bien que ses derniers panneaux s'éteignent — en continuité.
     *
     * @return l'empreinte
     */
    private static String renderLights() {
        long[] out = {0};
        if (Screengine.create(sceneConfig(), out) != Screengine.OK) {
            check(false, "création du contexte éclairé");
            return null;
        }

        // Quatre flottants de pose et trois octets de couleur par lumière,
        // aux mêmes valeurs que la scène de conformance.
        float[] poses = {
            8.0f, -3.0f, 1.5f, 12.0f,
            16.0f, 3.0f, 1.5f, 12.0f,
            24.0f, -2.0f, 2.5f, 14.0f,
        };
        byte[] colors = {
            (byte) 0xFF, (byte) 0x30, (byte) 0x20,
            (byte) 0x20, (byte) 0xFF, (byte) 0x40,
            (byte) 0x30, (byte) 0x50, (byte) 0xFF,
        };
        // Des tableaux de longueurs incompatibles sont refusés : le pont ne
        // peut pas deviner combien de lumières on lui donne.
        check(Screengine.setLights(out[0], poses, new byte[] {0, 0, 0})
                == Screengine.ERR_INVALID_ARGUMENT, "des tableaux dépareillés sont refusés");
        check(Screengine.setLights(out[0], poses, colors) == Screengine.OK,
                "les lumières se règlent");

        final int panels = 16;
        final float nearEdge = 2.0f;
        final float farEdge = 50.0f;
        final float step = (farEdge - nearEdge) / panels;
        float[] model = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        int[] indices = {0, 1, 2, 0, 2, 3};
        byte[] floorColor = {
            (byte) 0xB0, (byte) 0xB0, (byte) 0xB0, (byte) 0xFF,
            (byte) 0xB0, (byte) 0xB0, (byte) 0xB0, (byte) 0xFF,
        };
        byte[] wallColor = {
            (byte) 0x90, (byte) 0x90, (byte) 0x98, (byte) 0xFF,
            (byte) 0x90, (byte) 0x90, (byte) 0x98, (byte) 0xFF,
        };

        int submitted = Screengine.OK;
        for (int i = 0; i < panels && submitted == Screengine.OK; i++) {
            float a = nearEdge + i * step;
            float b = nearEdge + (i + 1) * step;
            float[] floorV = {
                a, -7.0f, -1.2f, b, -7.0f, -1.2f,
                b, 7.0f, -1.2f, a, 7.0f, -1.2f,
            };
            submitted = Screengine.submit(out[0], model, floorV, indices, floorColor);
            if (submitted == Screengine.OK) {
                float[] wallV = {
                    a, -7.0f, 4.0f, b, -7.0f, 4.0f,
                    b, -7.0f, -1.2f, a, -7.0f, -1.2f,
                };
                submitted = Screengine.submit(out[0], model, wallV, indices, wallColor);
            }
        }
        check(submitted == Screengine.OK, "les panneaux éclairés sont acceptés");

        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        ByteBuffer block = ByteBuffer.allocateDirect(body);
        int code = Screengine.frameEnd(out[0], block, 0, STRIDE);
        check(code == Screengine.OK, "l'image éclairée se rend");

        String hash = fingerprint(block, 0, STRIDE);
        Screengine.destroy(out[0]);
        return code == Screengine.OK ? hash : null;
    }

    /**
     * Rend la scène embrumée, ou {@code null} si le rendu a échoué.
     *
     * <p>Le fond n'est effacé de rien : c'est le moteur qui lui donne la
     * couleur du brouillard, parce qu'un pixel non peint est infiniment
     * lointain. Un hôte qui effacerait lui-même redessinerait la couture qu'on
     * cherche à supprimer, et l'empreinte le dirait.
     *
     * @return l'empreinte
     */
    private static String renderFog() {
        long texture = Screengine.textureLoad(FLOOR_SIDE, FLOOR_SIDE, makeChecker());
        check(texture != 0, "la texture du sol embrumé se charge");

        long[] out = {0};
        if (texture == 0 || Screengine.create(sceneConfig(), out) != Screengine.OK) {
            check(false, "création du contexte embrumé");
            return null;
        }

        // Une rampe vide est refusée : c'est une division par zéro, et
        // l'appelant voulait vraisemblablement éteindre le brouillard.
        check(Screengine.setFog(out[0], 0x30, 0x38, 0x48, 10.0f, 10.0f)
                == Screengine.ERR_INVALID_ARGUMENT, "une rampe vide est refusée");
        // Éteindre un brouillard qui n'existe pas n'est pas une erreur.
        check(Screengine.clearFog(out[0]) == Screengine.OK,
                "l'extinction sans brouillard passe");
        check(Screengine.setFog(out[0], 0x30, 0x38, 0x48, 3.0f, 14.0f) == Screengine.OK,
                "le brouillard se règle");

        float[] model = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        // Le sol est plus long que la rampe : sa moitié lointaine se confond
        // avec le fond, sa moitié proche garde son damier.
        float[] vertices = {
            1.0f, -20.0f, -1.2f, 1.0f * 8.0f, -20.0f * 8.0f,
            50.0f, -20.0f, -1.2f, 50.0f * 8.0f, -20.0f * 8.0f,
            50.0f, 20.0f, -1.2f, 50.0f * 8.0f, 20.0f * 8.0f,
            1.0f, 20.0f, -1.2f, 1.0f * 8.0f, 20.0f * 8.0f,
        };
        int[] indices = {0, 1, 2, 0, 2, 3};
        byte[] colors = {
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
        };
        check(Screengine.submitTextured(out[0], model, vertices, indices, colors, texture)
                == Screengine.OK, "le sol embrumé est accepté");
        Screengine.textureDestroy(texture);

        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        ByteBuffer block = ByteBuffer.allocateDirect(body);
        int code = Screengine.frameEnd(out[0], block, 0, STRIDE);
        check(code == Screengine.OK, "l'image embrumée se rend");

        String hash = fingerprint(block, 0, STRIDE);
        Screengine.destroy(out[0]);
        return code == Screengine.OK ? hash : null;
    }

    /** Côté de la lightmap de la scène {@code lumiere}, en texels. */
    private static final int LIGHT_SIDE = 16;

    /**
     * Le dégradé de lightmap, recopié de la suite de conformance.
     *
     * <p>Les deux axes n'y font pas la même chose : un dégradé symétrique
     * laisserait passer un axe échangé entre les deux jeux de coordonnées.
     *
     * @return {@code LIGHT_SIDE} au carré texels de quatre octets
     */
    private static byte[] makeGradient() {
        byte[] luxels = new byte[LIGHT_SIDE * LIGHT_SIDE * 4];
        for (int v = 0; v < LIGHT_SIDE; v++) {
            for (int u = 0; u < LIGHT_SIDE; u++) {
                int base = (v * LIGHT_SIDE + u) * 4;
                luxels[base] = (byte) (32 + u * 223 / (LIGHT_SIDE - 1));
                luxels[base + 1] = (byte) (32 + (u + v) / 2 * 223 / (LIGHT_SIDE - 1));
                luxels[base + 2] = (byte) (32 + v * 223 / (LIGHT_SIDE - 1));
                luxels[base + 3] = (byte) 0xFF;
            }
        }
        return luxels;
    }

    /**
     * Rend la scène éclairée, ou {@code null} si le rendu a échoué.
     *
     * <p>Deux lots : le sol, texturé et éclairé, puis le mur, éclairé seul. Le
     * second passe une texture nulle, ce que ce point d'entrée accepte là où
     * {@code submitTextured} la refuse — l'asymétrie que le header signale, et
     * que cet hôte exerce pour de bon.
     *
     * @return l'empreinte
     */
    private static String renderLit(int overbright) {
        long texture = Screengine.textureLoad(FLOOR_SIDE, FLOOR_SIDE, makeChecker());
        check(texture != 0, "la texture du sol se charge");
        long lightmap = Screengine.textureLoad(LIGHT_SIDE, LIGHT_SIDE, makeGradient());
        check(lightmap != 0, "la lightmap se charge par le meme chemin");

        long[] out = {0};
        if (texture == 0 || lightmap == 0
                || Screengine.create(sceneConfig(), out) != Screengine.OK) {
            check(false, "création du contexte éclairé");
            return null;
        }

        float[] model = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        // Sept flottants par sommet : la position, u et v en texels, puis u2
        // et v2 en texels de la lightmap. Celles-ci vont d'un demi-texel à un
        // demi-texel du bord opposé : une lightmap ne se pave pas, et le
        // bilinéaire irait autrement chercher son voisin par le repli.
        float[] floor = {
            2.0f, -16.0f, -1.2f, 2.0f * 8.0f, -16.0f * 8.0f, 0.5f, 0.5f,
            40.0f, -16.0f, -1.2f, 40.0f * 8.0f, -16.0f * 8.0f, 15.5f, 0.5f,
            40.0f, 16.0f, -1.2f, 40.0f * 8.0f, 16.0f * 8.0f, 15.5f, 15.5f,
            2.0f, 16.0f, -1.2f, 2.0f * 8.0f, 16.0f * 8.0f, 0.5f, 15.5f,
        };
        float[] wall = {
            40.0f, -16.0f, -1.2f, 0.0f, 0.0f, 0.5f, 0.5f,
            40.0f, -16.0f, 10.0f, 0.0f, 0.0f, 15.5f, 0.5f,
            40.0f, 16.0f, 10.0f, 0.0f, 0.0f, 15.5f, 15.5f,
            40.0f, 16.0f, -1.2f, 0.0f, 0.0f, 0.5f, 15.5f,
        };
        int[] indices = {0, 1, 2, 0, 2, 3};
        byte[] white = {
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
            (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
        };
        byte[] stone = {
            (byte) 0xC0, (byte) 0xB0, (byte) 0x90, (byte) 0xFF,
            (byte) 0xC0, (byte) 0xB0, (byte) 0x90, (byte) 0xFF,
        };

        // Le sur-éclairement, seul réglage de l'étape 3 qu'aucun hôte
        // n'empruntait : trois valeurs permises, et toute autre refusée plutôt
        // que rabattue.
        check(Screengine.setOverbright(out[0], 3) == Screengine.ERR_INVALID_ARGUMENT,
                "un sur-éclairement de trois est refusé");
        check(Screengine.setOverbright(out[0], overbright) == Screengine.OK,
                "le sur-éclairement se règle");

        // Une lightmap nulle est refusée, elle : sans elle, ce lot n'a rien à
        // faire sur ce chemin.
        check(Screengine.submitLit(out[0], model, floor, indices, white, texture, 0)
                == Screengine.ERR_NULL, "une lightmap nulle est refusée");
        check(Screengine.submitLit(out[0], model, floor, indices, white, texture, lightmap)
                == Screengine.OK, "le sol texturé et éclairé est accepté");
        check(Screengine.submitLit(out[0], model, wall, indices, stone, 0, lightmap)
                == Screengine.OK, "le mur uni et éclairé est accepté");
        Screengine.textureDestroy(texture);
        Screengine.textureDestroy(lightmap);

        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        ByteBuffer block = ByteBuffer.allocateDirect(body);
        int code = Screengine.frameEnd(out[0], block, 0, STRIDE);
        check(code == Screengine.OK, "l'image éclairée se rend");

        String hash = fingerprint(block, 0, STRIDE);
        Screengine.destroy(out[0]);
        return code == Screengine.OK ? hash : null;
    }

    /**
     * Vrai si le message est non vide quand on l'attend, et sans caractère de
     * contrôle venu d'un tampon non initialisé.
     *
     * @param message le message lu
     * @param expectText s'il doit être non vide
     * @return le résultat
     */
    private static boolean messageOk(String message, boolean expectText) {
        if (message == null || expectText == message.isEmpty()) {
            return false;
        }
        for (int i = 0; i < message.length(); i++) {
            if (message.charAt(i) < 0x20) {
                return false;
            }
        }
        return true;
    }

    /** Les refus, vus depuis Java : codes exacts et messages lisibles. */
    private static void checkRefusals() {
        long[] out = {0x5A5A5A5AL};

        check(Screengine.create(null, out) == Screengine.ERR_NULL, "configuration nulle refusée par SCG_ERR_NULL");
        check(messageOk(Screengine.lastError(0), true), "message sans contexte après une configuration nulle");

        int[] config = sceneConfig();
        config[4] = 48;
        check(Screengine.create(config, out) == Screengine.ERR_INVALID_ARGUMENT, "taille de tuile 48 refusée");
        check(out[0] == 0x5A5A5A5AL, "rien n'est écrit dans le paramètre de sortie après un refus");

        config = sceneConfig();
        config[6] = 1;
        check(Screengine.create(config, out) == Screengine.ERR_INVALID_ARGUMENT, "champ réservé non nul refusé");

        check(Screengine.create(sceneConfig(), null) == Screengine.ERR_NULL, "paramètre de sortie nul refusé");

        check(Screengine.create(sceneConfig(), out) == Screengine.OK, "configuration valide acceptée");
        long ctx = out[0];
        check(messageOk(Screengine.lastError(ctx), false), "message vide après un succès");
        check(messageOk(Screengine.lastError(0), false), "message sans contexte vidé par un appel réussi");

        check(Screengine.setResolution(ctx, WIDTH, HEIGHT) == Screengine.OK, "la résolution du maximum est acceptée");
        check(Screengine.setResolution(ctx, WIDTH + 1, HEIGHT) == Screengine.ERR_INVALID_ARGUMENT, "largeur au-delà du maximum refusée");
        check(Screengine.setResolution(ctx, WIDTH, HEIGHT + 1) == Screengine.ERR_INVALID_ARGUMENT, "hauteur au-delà du maximum refusée");
        check(Screengine.setResolution(ctx, 0, HEIGHT) == Screengine.ERR_INVALID_ARGUMENT, "largeur nulle refusée");
        check(messageOk(Screengine.lastError(ctx), true), "message du contexte après une résolution refusée");
        check(Screengine.setResolution(0, WIDTH, HEIGHT) == Screengine.ERR_NULL, "contexte nul refusé par setResolution");

        ByteBuffer small = ByteBuffer.allocateDirect(4 * 4);
        check(Screengine.frameEnd(ctx, null, 0, WIDTH) == Screengine.ERR_NULL, "tampon nul refusé");
        check(Screengine.frameEnd(ctx, small, 0, WIDTH - 1) == Screengine.ERR_INVALID_ARGUMENT, "stride inférieur à la largeur refusé");
        check(messageOk(Screengine.lastError(ctx), true), "message du contexte après un stride refusé");
        check(Screengine.frameEnd(0, small, 0, WIDTH) == Screengine.ERR_NULL, "contexte nul refusé");

        // La caméra : aucune scène de conformance n'en règle, celle du chemin
        // Rust tournant son modèle et non son point de vue. Son contrat se
        // vérifie donc ici plutôt que par une image.
        float[] camera = {0, 0, 0, 0, 0, 0, 1, 1.2f, 0.1f};
        check(Screengine.setCamera(ctx, camera) == Screengine.OK, "la caméra se règle");
        check(Screengine.setCamera(ctx, null) == Screengine.ERR_NULL, "caméra nulle refusée");
        check(Screengine.setCamera(0, camera) == Screengine.ERR_NULL,
                "contexte nul refusé par setCamera");

        float[] wide = camera.clone();
        wide[7] = 4.0f;
        check(Screengine.setCamera(ctx, wide) == Screengine.ERR_INVALID_ARGUMENT,
                "champ de vision au-delà de pi refusé");
        float[] flat = camera.clone();
        flat[8] = 0.0f;
        check(Screengine.setCamera(ctx, flat) == Screengine.ERR_INVALID_ARGUMENT,
                "plan proche nul refusé");

        // Et refusée dès qu'un triangle de l'image en cours est retenu : chaque
        // soumission projette immédiatement, si bien qu'une caméra changée au
        // milieu laisserait deux espaces écran dans la même image.
        check(submitScene(ctx) == Screengine.OK, "un triangle est retenu");
        check(Screengine.setCamera(ctx, camera) == Screengine.ERR_INVALID_STATE,
                "la caméra est refusée après une soumission retenue");

        Screengine.destroy(ctx);
        Screengine.destroy(0);
    }

    /** L'allocation pour le compte de l'hôte : alignement de l'ABI, cas limites. */
    private static void checkBuffers() {
        long len = (long) WIDTH * HEIGHT * Screengine.BYTES_PER_PIXEL;
        long buffer = Screengine.bufferAlloc(len);
        check(buffer != 0, "scg_buffer_alloc rend un tampon");
        if (buffer != 0) {
            check((buffer & (Screengine.BUFFER_ALIGNMENT - 1)) == 0, "tampon aligné sur SCG_BUFFER_ALIGNMENT");
            Screengine.bufferFree(buffer, len);
        }
        check(Screengine.bufferAlloc(0) == 0, "une allocation de zéro octet rend 0");
        Screengine.bufferFree(0, 0);
    }

    /**
     * FNV-1a 64 bits dans la forme de la conformance. Le produit de deux
     * {@code long} se replie modulo 2⁶⁴, ce que FNV exige.
     *
     * @param pixels tampon
     * @param base position de la zone rendue
     * @param stride pas de ligne en pixels
     * @return seize chiffres hexadécimaux minuscules
     */
    private static String fingerprint(ByteBuffer pixels, int base, int stride) {
        long hash = 0xcbf29ce484222325L;
        for (int dim : new int[] {WIDTH, HEIGHT}) {
            for (int shift = 0; shift < 32; shift += 8) {
                hash = (hash ^ ((dim >>> shift) & 0xFF)) * 0x100000001b3L;
            }
        }
        for (int y = 0; y < HEIGHT; y++) {
            int row = base + y * stride * Screengine.BYTES_PER_PIXEL;
            for (int i = 0; i < WIDTH * Screengine.BYTES_PER_PIXEL; i++) {
                hash = (hash ^ (pixels.get(row + i) & 0xFF)) * 0x100000001b3L;
            }
        }
        return String.format("%016x", hash);
    }

    /**
     * Rend le triangle dans un tampon direct entouré de sentinelles, à une base
     * désalignée, vérifie que rien n'est écrit hors de la zone utile, et rend
     * l'empreinte, ou {@code null} si le rendu a échoué.
     *
     * @return l'empreinte
     */
    private static String render() {
        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        int base = SKEW + GUARD;
        ByteBuffer block = ByteBuffer.allocateDirect(base + body + GUARD);
        byte[] fill = new byte[block.capacity()];
        Arrays.fill(fill, SENTINEL);
        block.put(fill).clear();

        long[] out = {0};
        if (Screengine.create(sceneConfig(), out) != Screengine.OK) {
            check(false, "création du contexte de rendu");
            return null;
        }
        check(submitScene(out[0]) == Screengine.OK, "scène soumise");
        int code = Screengine.frameEnd(out[0], block, base, STRIDE);
        check(code == Screengine.OK, "scg_frame_end aboutit sur une base désalignée");

        boolean intact = true;
        for (int i = 0; i < base; i++) {
            intact &= block.get(i) == SENTINEL;
        }
        for (int i = 0; i < GUARD; i++) {
            intact &= block.get(base + body + i) == SENTINEL;
        }
        for (int y = 0; y < HEIGHT; y++) {
            int tail = base + y * STRIDE * Screengine.BYTES_PER_PIXEL + WIDTH * Screengine.BYTES_PER_PIXEL;
            for (int i = 0; i < (STRIDE - WIDTH) * Screengine.BYTES_PER_PIXEL; i++) {
                intact &= block.get(tail + i) == SENTINEL;
            }
        }
        check(intact, "rien n'est écrit hors de la zone utile, marges et fins de ligne comprises");

        boolean opaque = true;
        for (int y = 0; y < HEIGHT; y++) {
            int row = base + y * STRIDE * Screengine.BYTES_PER_PIXEL;
            for (int x = 0; x < WIDTH; x++) {
                opaque &= block.get(row + x * Screengine.BYTES_PER_PIXEL + 3) == (byte) 0xFF;
            }
        }
        check(opaque, "l'alpha est écrit à 255 sur chaque pixel");

        String hash = fingerprint(block, base, STRIDE);
        Screengine.destroy(out[0]);
        return code == Screengine.OK ? hash : null;
    }

    /**
     * La résolution interne change sans recréer le contexte, et l'image ne
     * dépend pas de celle qu'il avait à l'ouverture.
     *
     * Le contexte s'ouvre en 1×1 sous un maximum de WIDTH×HEIGHT, puis passe à
     * la résolution de la scène : son empreinte doit être celle qu'un contexte
     * ouvert directement dessus a rendue. Ouvrir en 1×1 plutôt qu'à la
     * résolution finale est ce qui rend une projection laissée périmée visible
     * ici.
     *
     * @param expected l'empreinte du contexte neuf
     */
    private static void checkResize(String expected) {
        int body = STRIDE * HEIGHT * Screengine.BYTES_PER_PIXEL;
        ByteBuffer block = ByteBuffer.allocateDirect(body);

        int[] config = sceneConfig();
        config[2] = 1;
        config[3] = 1;

        long[] out = {0};
        if (Screengine.create(config, out) != Screengine.OK) {
            check(false, "création du contexte redimensionnable");
            return;
        }

        check(Screengine.setResolution(out[0], WIDTH, HEIGHT) == Screengine.OK, "la résolution passe de 1×1 au maximum");
        check(submitScene(out[0]) == Screengine.OK, "scène soumise après redimensionnement");
        check(Screengine.frameEnd(out[0], block, 0, STRIDE) == Screengine.OK, "image rendue après redimensionnement");
        check(fingerprint(block, 0, STRIDE).equals(expected),
                "un contexte redimensionné rend l'empreinte d'un contexte neuf");

        // Après une soumission, la résolution est figée pour l'image en cours,
        // comme la caméra : la projection a déjà eu lieu.
        check(submitScene(out[0]) == Screengine.OK, "seconde scène soumise");
        check(Screengine.setResolution(out[0], 1, 1) == Screengine.ERR_INVALID_STATE, "résolution refusée après une soumission");

        Screengine.destroy(out[0]);
    }

    /**
     * Toutes les vérifications, puis l'empreinte sur la sortie standard.
     *
     * @param args le répertoire des bibliothèques
     */
    public static void main(String[] args) {
        if (args.length != 1) {
            System.err.println("usage : app_process <dir> screengine.host.Test <dir>");
            System.exit(2);
        }
        Screengine.load(args[0]);
        check(Screengine.abiVersionUnsigned() == Screengine.ABI_VERSION, "la bibliothèque chargée est celle du header");

        checkRefusals();
        checkBuffers();
        String hash = render();
        if (hash != null) {
            checkResize(hash);
        }
        String textured = renderTextured(Screengine.FILTER_DITHER);
        String bilinear = renderTextured(Screengine.FILTER_BILINEAR);
        String graded = renderGraded();
        String lit = renderLit(0);
        // La même scène au sur-éclairement maximal : seul le réglage du
        // contexte les sépare, donc une divergence ne peut venir que de lui.
        String overbright = renderLit(2);
        String fog = renderFog();
        String lights = renderLights();

        if (failures > 0 || hash == null || textured == null || bilinear == null
                || graded == null || lit == null || overbright == null || fog == null
                || lights == null) {
            System.err.println(failures + " vérification(s) en échec");
            System.exit(1);
        }
        System.out.println(hash);
        System.out.println(textured);
        System.out.println(bilinear);
        System.out.println(graded);
        System.out.println(lit);
        System.out.println(overbright);
        System.out.println(fog);
        System.out.println(lights);
        System.exit(0);
    }
}
