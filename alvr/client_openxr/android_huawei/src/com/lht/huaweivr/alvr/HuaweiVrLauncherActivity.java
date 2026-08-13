package com.lht.huaweivr.alvr;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.hardware.display.DisplayManager;
import android.net.wifi.WifiManager;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.view.Window;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import java.util.Locale;

/** Phone-side launcher. VR rendering remains in {@link HuaweiVrActivity}. */
public final class HuaweiVrLauncherActivity extends Activity {
    private static final int COLOR_BACKGROUND = Color.rgb(7, 20, 30);
    private static final int COLOR_CARD = Color.rgb(17, 42, 56);
    private static final int COLOR_PRIMARY = Color.rgb(44, 178, 199);
    private static final int COLOR_TEXT = Color.rgb(241, 248, 250);
    private static final int COLOR_SECONDARY = Color.rgb(174, 202, 211);

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        requestWindowFeature(Window.FEATURE_NO_TITLE);
        getWindow().setStatusBarColor(COLOR_BACKGROUND);
        getWindow().setNavigationBarColor(COLOR_BACKGROUND);
        setContentView(createContent());
    }

    private View createContent() {
        int horizontalPadding = dp(24);

        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(horizontalPadding, dp(28), horizontalPadding, dp(28));
        content.setBackgroundColor(COLOR_BACKGROUND);

        TextView brand = text("ALVR · HUAWEI VR Glass", 14, COLOR_PRIMARY);
        brand.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        content.addView(brand);

        TextView title = text("开始电脑 VR 串流", 30, COLOR_TEXT);
        title.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        LinearLayout.LayoutParams titleParams = wrapParams();
        titleParams.topMargin = dp(12);
        content.addView(title, titleParams);

        TextView subtitle = text(
                "先让电脑端 ALVR Dashboard 和 SteamVR 保持运行，再连接 VR Glass。",
                16,
                COLOR_SECONDARY);
        subtitle.setLineSpacing(0, 1.25f);
        LinearLayout.LayoutParams subtitleParams = matchWrapParams();
        subtitleParams.topMargin = dp(12);
        content.addView(subtitle, subtitleParams);

        content.addView(createStatusCard(), cardParams(dp(28)));

        Button enterButton = new Button(this);
        enterButton.setAllCaps(false);
        enterButton.setText("连接眼镜并进入 VR");
        enterButton.setTextSize(17);
        enterButton.setTextColor(Color.WHITE);
        enterButton.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        enterButton.setMinHeight(dp(54));
        enterButton.setBackground(roundedBackground(COLOR_PRIMARY, 16));
        enterButton.setOnClickListener(view -> launchHuaweiPrompt());
        LinearLayout.LayoutParams buttonParams = matchWrapParams();
        buttonParams.topMargin = dp(24);
        content.addView(enterButton, buttonParams);

        Button directButton = new Button(this);
        directButton.setAllCaps(false);
        directButton.setText("眼镜已连接：直接启动");
        directButton.setTextSize(15);
        directButton.setTextColor(COLOR_SECONDARY);
        directButton.setBackgroundColor(Color.TRANSPARENT);
        directButton.setOnClickListener(view -> launchVrActivity());
        LinearLayout.LayoutParams directParams = matchWrapParams();
        directParams.topMargin = dp(6);
        content.addView(directButton, directParams);

        TextView officialNote = text(
                "“连接眼镜并进入 VR”使用 HUAWEI VR SDK 3.5 官方 2D 提示页；插入眼镜后会绕过 VR Launcher，直接进入本应用。",
                13,
                COLOR_SECONDARY);
        officialNote.setLineSpacing(0, 1.2f);
        LinearLayout.LayoutParams noteParams = matchWrapParams();
        noteParams.topMargin = dp(18);
        content.addView(officialNote, noteParams);

        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.addView(content, new ScrollView.LayoutParams(
                ScrollView.LayoutParams.MATCH_PARENT,
                ScrollView.LayoutParams.WRAP_CONTENT));
        return scroll;
    }

    private View createStatusCard() {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(20), dp(18), dp(20), dp(18));
        card.setBackground(roundedBackground(COLOR_CARD, 18));

        TextView heading = text("连接前检查", 18, COLOR_TEXT);
        heading.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        card.addView(heading);

        TextView deviceStatus = text(getDeviceStatus(), 14, COLOR_PRIMARY);
        LinearLayout.LayoutParams statusParams = matchWrapParams();
        statusParams.topMargin = dp(10);
        card.addView(deviceStatus, statusParams);

        card.addView(step("1", "电脑与手机连接同一局域网"), stepParams());
        card.addView(step("2", "在电脑端信任这台手机并启动 SteamVR"), stepParams());
        card.addView(step("3", "连接 VR Glass，完成空间确认后进入应用"), stepParams());
        return card;
    }

    private String getDeviceStatus() {
        DisplayManager displayManager = (DisplayManager) getSystemService(Context.DISPLAY_SERVICE);
        boolean glassDetected = displayManager != null && displayManager.getDisplays().length > 1;
        return (glassDetected ? "VR Glass：已检测到" : "VR Glass：等待连接")
                + "    手机 IP："
                + getLocalIpAddress();
    }

    @SuppressWarnings("deprecation")
    private String getLocalIpAddress() {
        WifiManager wifiManager = (WifiManager) getApplicationContext()
                .getSystemService(Context.WIFI_SERVICE);
        if (wifiManager == null || wifiManager.getConnectionInfo() == null) {
            return "未连接 WLAN";
        }

        int address = wifiManager.getConnectionInfo().getIpAddress();
        if (address == 0) {
            return "未连接 WLAN";
        }

        return String.format(
                Locale.ROOT,
                "%d.%d.%d.%d",
                address & 0xff,
                address >> 8 & 0xff,
                address >> 16 & 0xff,
                address >> 24 & 0xff);
    }

    private View step(String number, String message) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);

        TextView badge = text(number, 14, Color.WHITE);
        badge.setGravity(Gravity.CENTER);
        badge.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        badge.setBackground(roundedBackground(COLOR_PRIMARY, 99));
        row.addView(badge, new LinearLayout.LayoutParams(dp(30), dp(30)));

        TextView label = text(message, 15, COLOR_TEXT);
        label.setLineSpacing(0, 1.15f);
        LinearLayout.LayoutParams labelParams = new LinearLayout.LayoutParams(
                0,
                LinearLayout.LayoutParams.WRAP_CONTENT,
                1f);
        labelParams.leftMargin = dp(14);
        row.addView(label, labelParams);
        return row;
    }

    private void launchHuaweiPrompt() {
        Intent prompt = new Intent("com.huawei.android.vr.PROMPT");
        prompt.setPackage(getPackageName());
        if (prompt.resolveActivity(getPackageManager()) != null) {
            startActivity(prompt);
        } else {
            Toast.makeText(this, "官方连接提示不可用，正在直接启动 VR。", Toast.LENGTH_SHORT).show();
            launchVrActivity();
        }
    }

    private void launchVrActivity() {
        startActivity(new Intent(this, HuaweiVrActivity.class));
    }

    private TextView text(String value, int sizeSp, int color) {
        TextView view = new TextView(this);
        view.setText(value);
        view.setTextSize(sizeSp);
        view.setTextColor(color);
        return view;
    }

    private GradientDrawable roundedBackground(int color, int radiusDp) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(color);
        drawable.setCornerRadius(dp(radiusDp));
        return drawable;
    }

    private LinearLayout.LayoutParams wrapParams() {
        return new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
    }

    private LinearLayout.LayoutParams matchWrapParams() {
        return new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
    }

    private LinearLayout.LayoutParams cardParams(int topMargin) {
        LinearLayout.LayoutParams params = matchWrapParams();
        params.topMargin = topMargin;
        return params;
    }

    private LinearLayout.LayoutParams stepParams() {
        LinearLayout.LayoutParams params = matchWrapParams();
        params.topMargin = dp(16);
        return params;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
