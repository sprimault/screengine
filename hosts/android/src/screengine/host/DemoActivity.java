// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

package screengine.host;

import android.app.Activity;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Rect;
import android.os.Bundle;
import android.view.MotionEvent;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.WindowManager;
import android.widget.TextView;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;

/**
 * Hôte Android de démonstration : une surface, deux zones tactiles, une boucle.
 *
 * <p><b>Ce que {@link MainActivity} ne montre pas.</b> Celle-ci rend une image
 * fixe et la pose dans une vue : c'est ce qu'il faut pour voir que le pont JNI
 * marche, et c'est inutilisable comme point de départ. Celle-ci est le point de
 * départ — elle charge un décor, l'affiche et s'y déplace.
 *
 * <p>Elle suit la page web et les deux démonstrations de bureau pas à pas. Ce
 * que le système apporte ici — une surface, des événements tactiles — n'appartient
 * pas au moteur, qui n'a ni l'un ni l'autre.
 */
public final class DemoActivity extends Activity implements SurfaceHolder.Callback, Runnable {
    /** Résolution interne, celle à laquelle le moteur rend. */
    private static final int WIDTH = 640;

    /** Hauteur interne. La surface est bien plus grande : l'hôte met à l'échelle. */
    private static final int HEIGHT = 360;

    /** Côté de tuile. */
    private static final int TILE = 64;

    /** Côté du damier des murs, en texels. */
    private static final int WALL_SIDE = 512;

    /**
     * Côté du damier du sol et du plafond.
     *
     * <p>Les côtés suivent la densité de plaquage de la carte — 256 texels par
     * unité de monde aux murs, 128 au sol : une case y fait un demi-mètre et un
     * quart. Un damier plus petit donnerait des cases de quelques centimètres,
     * que le mipmap ramènerait à un aplat.
     */
    private static final int FLOOR_SIDE = 256;

    /** Celui des caisses, dont le maillage a son propre plaquage. */
    private static final int CRATE_SIDE = 64;

    /** Vitesse de déplacement, en unités de monde par seconde. */
    private static final float SPEED = 6.0f;

    /** Vitesse de rotation, en radians par seconde, à pleine amplitude. */
    private static final float TURN = 2.0f;

    /**
     * Où sont posées les caisses : abscisse, ordonnée, et l'angle qui les
     * tourne. Les mêmes que les trois autres hôtes.
     */
    private static final float[][] CRATES = {
        {3.0f, 2.0f, 0.4f},
        {10.0f, 2.0f, -0.7f},
        {16.0f, 4.0f, 1.1f},
    };

    /**
     * L'échelle des caisses : le maillage fait deux unités de côté, les salles
     * quatre de haut. Le fichier ne se redimensionne pas — son empreinte est
     * figée —, donc l'échelle va dans la matrice de modèle.
     */
    private static final float CRATE_SCALE = 0.5f;

    /** La cote du centre d'une caisse, sa demi-hauteur au-dessus du sol. */
    private static final float CRATE_Z = 0.5f;

    /**
     * Où la caméra commence : dans la salle en L, à hauteur d'œil.
     *
     * <p>Le sol de ce décor est en zéro : une caméra laissée à l'origine serait
     * dans le plancher, hors de toute cellule, et la traversée ne rendrait rien.
     */
    private static final float[] START = {2.0f, 2.0f, 2.0f};

    /** La surface où l'image est recopiée. */
    private SurfaceView view;

    /** Le fil de rendu, vivant entre la création et la destruction de la surface. */
    private Thread thread;

    /** Arrête la boucle ; lu par le fil de rendu, écrit par celui de l'interface. */
    private volatile boolean running;

    /** Avance, de -1 en arrière à 1 en avant. Écrit par les événements tactiles. */
    private volatile float forward;

    /** Rotation, de -1 à droite à 1 à gauche. Même provenance. */
    private volatile float turn;

    /**
     * L'écart, en pixels, qui donne la pleine amplitude. Tiré de la densité :
     * en pixels bruts, la même valeur serait un geste ample sur un écran peu
     * dense et un frémissement sur un écran fin.
     */
    private float range;

    /** Le pointeur qui tient la zone gauche, ou -1. */
    private int leftPointer = -1;

    /** L'ordonnée où il s'est posé, l'origine de son manche. */
    private float leftOrigin;

    /** Le pointeur qui tient la zone droite, ou -1. */
    private int rightPointer = -1;

    /** L'abscisse où il s'est posé. */
    private float rightOrigin;

    /** Crée la surface et s'abonne à son cycle de vie. */
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        System.loadLibrary("screengine");
        System.loadLibrary("screengine_jni");

        // Une démonstration qu'on regarde tourner : l'écran s'éteindrait au
        // bout de quinze secondes, puisque personne ne touche l'appareil.
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);

        range = 120.0f * getResources().getDisplayMetrics().density;
        view = new SurfaceView(this);
        view.getHolder().addCallback(this);
        setContentView(view);
    }

    /**
     * Les deux zones tactiles : moitié gauche pour avancer et reculer, moitié
     * droite pour tourner.
     *
     * <p><b>Chaque zone est relative à son point de pose.</b> Là où le doigt se
     * pose devient l'origine, et son écart donne le sens et l'amplitude. Une
     * origine fixe — le centre de la zone — a été essayée et ne tient pas en
     * main : le pouce ne tombe jamais deux fois au même endroit, et il faudrait
     * dessiner un repère pour qu'il sache où viser. Un manche relatif se passe
     * d'affichage, puisque son repère est l'endroit où on vient de poser le
     * doigt.
     *
     * @param event l'événement
     * @return toujours vrai, l'activité consommant tout ce qui la touche
     */
    @Override
    public boolean onTouchEvent(MotionEvent event) {
        int action = event.getActionMasked();
        if (action == MotionEvent.ACTION_CANCEL) {
            leftPointer = -1;
            rightPointer = -1;
        } else if (action == MotionEvent.ACTION_DOWN || action == MotionEvent.ACTION_POINTER_DOWN) {
            int index = event.getActionIndex();
            // Le premier doigt posé dans une zone la tient jusqu'à ce qu'il se
            // lève : un second doigt au même endroit ne déplace pas l'origine
            // sous celui qui est déjà en train de conduire.
            if (event.getX(index) < view.getWidth() / 2.0f) {
                if (leftPointer < 0) {
                    leftPointer = event.getPointerId(index);
                    leftOrigin = event.getY(index);
                }
            } else if (rightPointer < 0) {
                rightPointer = event.getPointerId(index);
                rightOrigin = event.getX(index);
            }
        } else if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_POINTER_UP) {
            int id = event.getPointerId(event.getActionIndex());
            if (id == leftPointer) {
                leftPointer = -1;
            }
            if (id == rightPointer) {
                rightPointer = -1;
            }
        }

        forward = leftPointer < 0 ? 0.0f : offset(event, leftPointer, leftOrigin, false);
        turn = rightPointer < 0 ? 0.0f : offset(event, rightPointer, rightOrigin, true);
        return true;
    }

    /**
     * L'écart d'un pointeur à son origine, ramené dans [-1, 1].
     *
     * <p>Le signe est celui qu'attend le monde : vers le haut avance, vers la
     * gauche tourne à gauche.
     *
     * @param event l'événement en cours
     * @param pointer l'identifiant du pointeur
     * @param origin l'abscisse ou l'ordonnée où il s'est posé
     * @param horizontal vrai pour lire l'abscisse, faux pour l'ordonnée
     * @return l'amplitude, nulle si le pointeur a disparu de l'événement
     */
    private float offset(MotionEvent event, int pointer, float origin, boolean horizontal) {
        int index = event.findPointerIndex(pointer);
        if (index < 0) {
            return 0.0f;
        }
        float current = horizontal ? event.getX(index) : event.getY(index);
        return clamp((origin - current) / range);
    }

    /**
     * Ramène une valeur dans [-1, 1].
     *
     * @param value la valeur
     * @return la valeur bornée
     */
    private static float clamp(float value) {
        return Math.max(-1.0f, Math.min(1.0f, value));
    }

    /** Démarre le fil de rendu. */
    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        running = true;
        thread = new Thread(this, "screengine-demo");
        thread.start();
    }

    /**
     * Rien à faire : l'image est à résolution fixe, et c'est le rectangle de
     * destination qui suit la surface, recalculé à chaque image.
     */
    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
    }

    /**
     * Arrête le fil et l'attend : la surface n'est plus valide au retour, et
     * une image en cours de recopie écrirait dans ce qui n'existe plus.
     */
    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        running = false;
        try {
            thread.join();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    /**
     * Charge un fichier des ressources de l'application.
     *
     * <p>L'hôte lit les fichiers, jamais le moteur : ce que la frontière reçoit
     * est un bloc d'octets.
     *
     * @param name le nom du fichier, sous {@code assets/}
     * @return son contenu, ou {@code null}
     */
    private byte[] readAsset(String name) {
        try {
            InputStream stream = getAssets().open(name);
            try {
                ByteArrayOutputStream out = new ByteArrayOutputStream();
                byte[] chunk = new byte[16384];
                for (int read = stream.read(chunk); read > 0; read = stream.read(chunk)) {
                    out.write(chunk, 0, read);
                }
                return out.toByteArray();
            } finally {
                stream.close();
            }
        } catch (IOException e) {
            return null;
        }
    }

    /**
     * Un damier de {@code cell} texels de case, le même que la suite de
     * conformance, teinte pour teinte.
     *
     * @param side le côté de la texture, puissance de deux
     * @param cell le côté d'une case
     * @return le handle de texture, ou 0
     */
    private static long loadChecker(int side, int cell) {
        byte[] texels = new byte[side * side * 4];
        for (int v = 0; v < side; v++) {
            for (int u = 0; u < side; u++) {
                int base = (v * side + u) * 4;
                boolean edge = u % cell == 0 || v % cell == 0;
                boolean dark = (u / cell + v / cell) % 2 == 0;
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
        return Screengine.textureLoad(side, side, texels);
    }

    /**
     * La matrice d'une caisse : une rotation autour de la verticale mise à
     * l'échelle, puis une translation. Par colonnes, comme l'ABI l'attend.
     *
     * @param placement abscisse, ordonnée, angle
     * @return les seize coefficients
     */
    private static float[] crateModel(float[] placement) {
        float c = (float) Math.cos(placement[2]) * CRATE_SCALE;
        float s = (float) Math.sin(placement[2]) * CRATE_SCALE;
        return new float[] {
            c, s, 0, 0,
            -s, c, 0, 0,
            0, 0, CRATE_SCALE, 0,
            placement[0], placement[1], CRATE_Z, 1,
        };
    }

    /**
     * Charge, rend et recopie jusqu'à la destruction de la surface.
     *
     * <p>Tout est chargé ici et non dans {@code onCreate} : les ressources
     * appartiennent au fil qui rend, et il n'y a personne d'autre pour les
     * détruire quand il s'arrête.
     */
    @Override
    public void run() {
        long[] created = {0};
        if (Screengine.create(new int[] {WIDTH, HEIGHT, WIDTH, HEIGHT, TILE, 0, 0, 0}, created)
                != Screengine.OK) {
            show(Screengine.lastError(0));
            return;
        }
        long ctx = created[0];

        byte[] bytes = readAsset("salles.world");
        long world = bytes != null ? Screengine.worldLoad(bytes) : 0;
        if (world == 0) {
            show("carte illisible : " + Screengine.lastError(0));
            Screengine.destroy(ctx);
            return;
        }

        // Les lightmaps, cuites une fois pour toutes les cellules avant la
        // première image : une lightmap est un cache de la carte et non de la
        // vue, et la cuire en chemin ferait allouer un atlas au milieu d'une
        // image. Un échec n'arrête pas la démonstration — le décor part alors
        // non éclairé, ce que la soumission accepte.
        long lighting = Screengine.lightingCreate(world);
        for (int i = 0; lighting != 0 && i < Screengine.worldCellCount(world); i++) {
            Screengine.lightingBuild(lighting, Screengine.worldCellId(world, i));
        }

        // Une texture par matériau, dans l'ordre que la carte déclare. L'hôte
        // lit les noms, décide de ce qu'il charge, et passe les handles dans cet
        // ordre — le moteur ne connaît que des emplacements à remplir.
        int materials = Screengine.worldMaterialCount(world);
        long[] slots = new long[Math.max(materials, 0)];
        for (int i = 0; i < slots.length; i++) {
            boolean wall = "mur".equals(Screengine.worldMaterialName(world, i));
            slots[i] = loadChecker(wall ? WALL_SIDE : FLOOR_SIDE, wall ? 128 : 32);
        }

        bytes = readAsset("caisse.mesh");
        long crate = bytes != null ? Screengine.meshLoad(bytes) : 0;
        if (crate == 0) {
            show("maillage illisible : " + Screengine.lastError(0));
            release(ctx, world, 0, slots, 0, lighting);
            return;
        }
        // Le même damier sur les deux emplacements du maillage.
        long crateSide = loadChecker(CRATE_SIDE, 8);
        long[] crateSlots = {crateSide, crateSide};

        Bitmap bitmap = Bitmap.createBitmap(WIDTH, HEIGHT, Bitmap.Config.ARGB_8888);
        bitmap.setHasAlpha(false);
        Paint blit = new Paint();
        blit.setFilterBitmap(false);
        Paint text = new Paint();
        text.setColor(Color.WHITE);
        text.setTextSize(36.0f);
        Rect source = new Rect(0, 0, WIDTH, HEIGHT);

        float[] position = {START[0], START[1], START[2]};
        float[] previousPosition = new float[3];
        int cell = Screengine.worldLocate(world, position);
        float angle = 0.0f;
        float[] identity = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
        long previous = System.nanoTime();
        long since = previous;
        int frames = 0;
        int rate = 0;
        // Le temps passé dans le moteur, cumulé sur la seconde en cours. La
        // cadence affichée est celle de l'écran — unlockCanvasAndPost attend le
        // balayage —, et ne dit donc rien de ce que coûte une image. C'est cette
        // seconde mesure qui se compare d'une étape à l'autre, et sur téléphone
        // c'est elle qui décide de la batterie.
        long engine = 0;
        float cost = 0.0f;
        String failure = null;

        while (running && failure == null) {
            long now = System.nanoTime();
            float dt = (float) ((now - previous) / 1e9);
            previous = now;
            if (dt > 0.1f) {
                dt = 0.1f;
            }

            angle += turn * TURN * dt;
            System.arraycopy(position, 0, previousPosition, 0, 3);
            position[0] += (float) Math.cos(angle) * forward * SPEED * dt;
            position[1] += (float) Math.sin(angle) * forward * SPEED * dt;

            // La cellule se suit par le déplacement, et c'est l'activité qui la
            // garde : le moteur ne retient aucune caméra. Zéro veut dire « sorti
            // du décor » et ne s'écrit pas.
            int found = Screengine.worldTrack(world, cell, previousPosition, position);
            if (found != 0) {
                cell = found;
            }

            float[] camera = {
                position[0], position[1], position[2],
                0.0f, 0.0f, (float) Math.sin(angle / 2.0f), (float) Math.cos(angle / 2.0f),
                1.2f, 0.1f,
            };
            if (Screengine.setCamera(ctx, camera) != Screengine.OK) {
                failure = Screengine.lastError(ctx);
                break;
            }
            if (Screengine.submitWorldVisible(ctx, identity, world, slots, lighting, cell)
                    != Screengine.OK) {
                failure = Screengine.lastError(ctx);
                break;
            }
            for (float[] placement : CRATES) {
                if (Screengine.submitMesh(ctx, crateModel(placement), crate, crateSlots)
                        != Screengine.OK) {
                    failure = Screengine.lastError(ctx);
                    break;
                }
            }
            long before = System.nanoTime();
            if (failure != null || Screengine.frameBitmap(ctx, bitmap) != Screengine.OK) {
                failure = failure != null ? failure : Screengine.lastError(ctx);
                break;
            }
            engine += System.nanoTime() - before;

            frames++;
            if (now - since >= 1000000000L) {
                cost = engine / (frames * 1000000.0f);
                rate = (int) (frames * 1000000000L / (now - since));
                frames = 0;
                engine = 0;
                since = now;
            }

            SurfaceHolder holder = view.getHolder();
            Canvas canvas = holder.lockCanvas();
            if (canvas == null) {
                continue;
            }
            Rect target = fit(canvas);
            // Le fond est réeffacé à chaque image : la surface tourne sur
            // plusieurs tampons, et ce qui borde l'image y garderait sinon ce
            // qu'une image précédente y avait laissé.
            canvas.drawColor(Color.BLACK);
            canvas.drawBitmap(bitmap, source, target, blit);
            canvas.drawText(String.format("%d images/s — moteur %.1f ms — glisser à gauche pour"
                    + " avancer, à droite pour tourner", rate, cost),
                    target.left + 16.0f, target.top + 48.0f, text);
            holder.unlockCanvasAndPost(canvas);
        }

        release(ctx, world, crate, slots, crateSlots[0], lighting);
        if (failure != null) {
            show(failure);
        }
    }

    /**
     * Le rectangle de destination : l'image entière, centrée, à l'agrandissement
     * entier le plus grand qui tienne.
     *
     * <p>Entier parce qu'un facteur fractionnaire rendrait les pixels inégaux —
     * une colonne sur trois deux fois plus large —, ce qui se voit immédiatement
     * sur une image de rendu logiciel.
     *
     * @param canvas la toile de la surface
     * @return le rectangle où recopier
     */
    private static Rect fit(Canvas canvas) {
        int scale = Math.max(1, Math.min(canvas.getWidth() / WIDTH, canvas.getHeight() / HEIGHT));
        int width = WIDTH * scale;
        int height = HEIGHT * scale;
        int left = (canvas.getWidth() - width) / 2;
        int top = (canvas.getHeight() - height) / 2;
        return new Rect(left, top, left + width, top + height);
    }

    /**
     * Rend tout ce qui a été chargé. Un handle nul ne fait rien, ce qui permet
     * d'appeler cette méthode depuis n'importe quel abandon.
     *
     * @param ctx le contexte
     * @param world la carte
     * @param mesh le maillage des caisses
     * @param slots les textures de la carte
     * @param crateSlot la texture des caisses
     * @param lighting le porteur des lightmaps, ou 0
     */
    private static void release(
            long ctx, long world, long mesh, long[] slots, long crateSlot, long lighting) {
        for (long texture : slots) {
            Screengine.textureDestroy(texture);
        }
        Screengine.textureDestroy(crateSlot);
        Screengine.meshDestroy(mesh);
        Screengine.lightingDestroy(lighting);
        Screengine.worldDestroy(world);
        Screengine.destroy(ctx);
    }

    /**
     * Remplace la surface par un texte, depuis le fil de l'interface.
     *
     * <p>Appelée depuis le fil de rendu, où toucher une vue ne serait pas permis.
     *
     * @param message ce qui a échoué
     */
    private void show(final String message) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                TextView label = new TextView(DemoActivity.this);
                label.setText(message);
                setContentView(label);
            }
        });
    }
}
