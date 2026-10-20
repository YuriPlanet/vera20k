package com.wannafire.vera20k;

import android.app.NativeActivity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.view.InputQueue;
import android.view.SurfaceHolder;
import android.view.View;
import android.view.WindowManager;

/** Android owns its window; all game input and commands remain in Rust. */
public final class MainActivity extends NativeActivity {
    private boolean nativeReady;
    private SurfaceHolder currentSurface;
    private InputQueue currentInputQueue;
    private static native void nativeActivityDestroy();

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        // NativeActivity opens the library with dlopen; also register it with
        // ART so Java can resolve our separate lifecycle JNI entry point.
        System.loadLibrary("main");
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

    @Override public void surfaceCreated(SurfaceHolder holder) {
        super.surfaceCreated(holder);
        currentSurface = holder;
    }

    @Override public void surfaceDestroyed(SurfaceHolder holder) {
        super.surfaceDestroyed(holder);
        if (currentSurface == holder) currentSurface = null;
    }

    @Override public void onInputQueueCreated(InputQueue queue) {
        super.onInputQueueCreated(queue);
        currentInputQueue = queue;
    }

    @Override public void onInputQueueDestroyed(InputQueue queue) {
        super.onInputQueueDestroyed(queue);
        if (currentInputQueue == queue) currentInputQueue = null;
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
        // The parent synchronously waits for native acknowledgements. Drain
        // remaining resources while the native looper still runs, before
        // requesting exit. These public callbacks also clear its mCur* fields.
        if (currentSurface != null) surfaceDestroyed(currentSurface);
        if (currentInputQueue != null) onInputQueueDestroyed(currentInputQueue);
        if (nativeReady) {
            try {
                nativeActivityDestroy();
            } catch (UnsatisfiedLinkError error) {
                android.util.Log.e("VERA20k", "Native destroy callback unavailable", error);
            }
        }
        super.onDestroy();
        if (nativeReady) {
            // winit permits one EventLoop per process. NativeActivity has now
            // joined the native owner and its asset worker; a later launch
            // must get a fresh process instead of recreating that EventLoop.
            android.os.Process.killProcess(android.os.Process.myPid());
        }
    }
}
