// Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

package screengine.host;

import android.app.Activity;
import android.graphics.Bitmap;
import android.graphics.drawable.BitmapDrawable;
import android.os.Bundle;
import android.widget.ImageView;
import android.widget.TextView;

/**
 * Affiche le triangle sur un appareil ou un émulateur.
 *
 * <p>Le moteur écrit directement dans la mémoire du bitmap. La mise à l'échelle
 * appartient à l'hôte : le bitmap est à la résolution interne, et la vue
 * l'agrandit sans filtrage.
 */
public final class MainActivity extends Activity {
    /** Largeur de la scène. */
    private static final int WIDTH = 640;

    /** Hauteur de la scène. */
    private static final int HEIGHT = 360;

    /** Côté de tuile. */
    private static final int TILE = 64;

    /** Rend une image et la place dans la fenêtre, ou affiche la cause d'un échec. */
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        System.loadLibrary("screengine_ffi");
        System.loadLibrary("screengine_jni");

        if (Screengine.abiVersionUnsigned() != Screengine.ABI_VERSION) {
            showText("ABI " + Screengine.abiVersionUnsigned() + ", attendue " + Screengine.ABI_VERSION);
            return;
        }

        long[] out = {0};
        if (Screengine.create(new int[] {WIDTH, HEIGHT, WIDTH, HEIGHT, TILE, 0, 0, 0}, out) != Screengine.OK) {
            showText(Screengine.lastError(0));
            return;
        }

        // ARGB_8888 est rangé R, G, B, A en mémoire, l'ordre de l'ABI : le nom
        // vient de l'entier de Color, pas de la disposition.
        Bitmap bitmap = Bitmap.createBitmap(WIDTH, HEIGHT, Bitmap.Config.ARGB_8888);
        bitmap.setHasAlpha(false);
        int code = Screengine.frameEndBitmap(out[0], bitmap);
        String message = Screengine.lastError(out[0]);
        Screengine.destroy(out[0]);
        if (code != Screengine.OK) {
            showText(message);
            return;
        }

        BitmapDrawable drawable = new BitmapDrawable(getResources(), bitmap);
        drawable.setFilterBitmap(false);
        ImageView view = new ImageView(this);
        view.setScaleType(ImageView.ScaleType.FIT_CENTER);
        view.setImageDrawable(drawable);
        setContentView(view);
    }

    /**
     * Remplace le contenu par un texte.
     *
     * @param text le texte
     */
    private void showText(String text) {
        TextView view = new TextView(this);
        view.setText(text);
        setContentView(view);
    }
}
