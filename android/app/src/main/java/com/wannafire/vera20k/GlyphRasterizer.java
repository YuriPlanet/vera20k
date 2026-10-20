package com.wannafire.vera20k;

import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.Typeface;
import android.os.Build;
import java.util.Locale;

/** System Medium sans-serif coverage masks, fitted to the retained GAME.FNT cell. */
public final class GlyphRasterizer {
    private final int rows;
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.SUBPIXEL_TEXT_FLAG);
    private final Rect bounds = new Rect();
    private Bitmap bitmap;
    private Canvas canvas;
    private int[] pixels;

    public GlyphRasterizer(int rows) {
        if (rows <= 0 || rows > 128) throw new IllegalArgumentException("Invalid font height");
        this.rows = rows;
        Typeface medium = Typeface.create("sans-serif-medium", Typeface.NORMAL);
        if (Build.VERSION.SDK_INT >= 28) medium = Typeface.create(medium, 500, false);
        paint.setTypeface(medium);
        paint.setTextLocale(Locale.SIMPLIFIED_CHINESE);
        paint.setTextSize(rows);
        paint.setColor(Color.WHITE);
        paint.setHinting(Paint.HINTING_ON);
    }

    /** Alpha only: Rust expands white RGB separately to avoid double premultiplication. */
    public byte[] rasterize(int codepoint, int width) {
        if (width <= 0 || width > 128 || codepoint < 0 || codepoint > 0xffff
                || (codepoint >= 0xd800 && codepoint <= 0xdfff)) return null;
        String text = String.valueOf((char) codepoint);
        if (!paint.hasGlyph(text)) return null;
        if (bitmap == null || bitmap.getWidth() != width) {
            if (bitmap != null) bitmap.recycle();
            bitmap = Bitmap.createBitmap(width, rows, Bitmap.Config.ARGB_8888);
            canvas = new Canvas(bitmap);
            pixels = new int[width * rows];
        }
        bitmap.eraseColor(Color.TRANSPARENT);
        paint.getTextBounds(text, 0, text.length(), bounds);
        if (!bounds.isEmpty()) {
            // Paint bounds are relative to the baseline. Fit width and height
            // independently so narrow native Latin cells keep readable height.
            float left = Math.min(0f, bounds.left);
            float right = Math.max(bounds.right, paint.measureText(text));
            float span = Math.max(1f, right - left);
            float sx = Math.min(1f, Math.max(0.25f, width - 0.5f) / span);
            float sy = Math.min(1f, Math.max(0.25f, rows - 0.5f) / bounds.height());
            float x = (width - span * sx) * 0.5f - left * sx;
            float y = (rows - bounds.height() * sy) * 0.5f - bounds.top * sy;
            canvas.save();
            canvas.translate(x, y);
            canvas.scale(sx, sy);
            canvas.drawText(text, 0f, 0f, paint);
            canvas.restore();
        }
        bitmap.getPixels(pixels, 0, width, 0, 0, width, rows);
        byte[] alpha = new byte[width * rows];
        for (int i = 0; i < alpha.length; i++) alpha[i] = (byte) (pixels[i] >>> 24);
        return alpha;
    }
}
