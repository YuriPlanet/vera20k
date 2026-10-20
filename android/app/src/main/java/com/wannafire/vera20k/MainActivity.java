package com.wannafire.vera20k;

import android.app.NativeActivity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.view.View;
import android.view.WindowManager;

/** Android owns its window; all game input and commands remain in Rust. */
public final class MainActivity extends NativeActivity {
    private boolean nativeReady;
    private static native void nativeActivityDestroy();

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        // NativeActivity loads libmain during super.onCreate. A failed native
        // load never reaches this assignment or calls our destroy entry point.
        nativeReady = true;
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
                | WindowManager.LayoutParams.FLAG_FULLSCREEN);
        WindowManager.LayoutParams attributes = getWindow().getAttributes();
        attributes.layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
        getWindow().setAttributes(attributes);
        immersive();
    }

    @Override public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (focused) immersive();
    }

    @SuppressWarnings("deprecation")
    private void immersive() {
        getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                | View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_LAYOUT_STABLE);
    }

    public void openWebsite() {
        runOnUiThread(() -> startActivity(new Intent(Intent.ACTION_VIEW,
                Uri.parse("https://youlidefuchou.com"))));
    }

    @Override protected void onDestroy() {
        if (nativeReady) {
            try {
                nativeActivityDestroy();
            } catch (UnsatisfiedLinkError error) {
                android.util.Log.e("VERA20k", "Native destroy callback unavailable", error);
            }
        }
        super.onDestroy();
    }
}
