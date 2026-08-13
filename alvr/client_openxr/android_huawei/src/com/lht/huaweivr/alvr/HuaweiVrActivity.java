package com.lht.huaweivr.alvr;

import android.app.Activity;
import android.content.pm.ActivityInfo;
import android.os.Bundle;
import android.util.Log;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.Window;
import android.view.WindowManager;

import com.huawei.hvr.LibUpdateClient;

/** Huawei VR Glass entry point matching the lifecycle used by the official HVRSDK demo. */
public final class HuaweiVrActivity extends Activity implements SurfaceHolder.Callback {
    private static final String TAG = "HuaweiVrActivity";

    private boolean nativeLibraryLoaded;
    private boolean nativeStarted;
    private boolean nativeStopping;

    private native void nativeStart(Surface surface);
    private native void nativeStop();

    private void stopNativeIfNeeded() {
        if (nativeLibraryLoaded && nativeStarted && !nativeStopping) {
            nativeStopping = true;
            Log.i(TAG, "Stopping ALVR native runtime");
            nativeStop();
            nativeStarted = false;
        }
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        requestWindowFeature(Window.FEATURE_NO_TITLE);
        getWindow().setFlags(
                WindowManager.LayoutParams.FLAG_FULLSCREEN,
                WindowManager.LayoutParams.FLAG_FULLSCREEN);
        setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE);

        SurfaceView view = new SurfaceView(this);
        setContentView(view);
        view.getHolder().addCallback(this);

        Log.i(TAG, "Initializing Huawei VR SDK Service bridge");
        new LibUpdateClient(this).runUpdate();
        System.loadLibrary("alvr_client_openxr");
        nativeLibraryLoaded = true;
        Log.i(TAG, "Huawei bridge and ALVR native library initialized");
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        Log.i(TAG, "Surface created");
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        Log.i(TAG, "Surface changed: " + width + "x" + height + ", format=" + format);
        if (nativeLibraryLoaded && !nativeStarted) {
            nativeStarted = true;
            nativeStart(holder.getSurface());
        }
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        Log.i(TAG, "Surface destroyed");
    }

    @Override
    protected void onStop() {
        Log.i(TAG, "Activity stopped; shutting down ALVR before returning to Glass Home");
        stopNativeIfNeeded();
        super.onStop();
        finishAndRemoveTask();
        android.os.Process.killProcess(android.os.Process.myPid());
    }

    @Override
    protected void onDestroy() {
        stopNativeIfNeeded();
        super.onDestroy();
    }
}
