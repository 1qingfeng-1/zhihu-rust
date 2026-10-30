package dev.gpui.mobile;

import android.app.Activity;
import android.app.Dialog;
import android.graphics.Color;
import android.view.ViewGroup;
import android.webkit.CookieManager;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * In-app Zhihu login and risk-verification WebView.
 *
 * The page shares the app's WebView cookie jar so the user can log in (or pass
 * a risk check) once and hand the cookies back to Rust, instead of trying to
 * copy cookies out of an external browser.
 */
public final class ZhihuLogin {
    private static final String MOBILE_UA =
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 "
                    + "(KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36";
    private static final String HOME = "https://www.zhihu.com/";
    private static final String SIGNIN = "https://www.zhihu.com/signin?next=%2F";

    private ZhihuLogin() {}

    private static void configure(WebView web) {
        WebSettings settings = web.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setUserAgentString(MOBILE_UA);
        settings.setDatabaseEnabled(true);
        CookieManager manager = CookieManager.getInstance();
        manager.setAcceptCookie(true);
        manager.setAcceptThirdPartyCookies(web, true);
    }

    /**
     * Keep the dialog's title bar and action button out of the status and
     * navigation bars: the app is edge-to-edge, so without this the close
     * button sits under the clock and cannot be tapped. The system bar sizes
     * come from framework resources, which is reliable inside a dialog where
     * window insets are not dispatched to the content.
     */
    private static int systemBarHeight(Activity activity, String name) {
        int id = activity.getResources().getIdentifier(name, "dimen", "android");
        return id > 0 ? activity.getResources().getDimensionPixelSize(id) : 0;
    }

    private static LinearLayout buildLayout(Activity activity, WebView web, String title,
                                            String actionLabel, Runnable action) {
        LinearLayout root = new LinearLayout(activity);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.BLACK);
        root.setPadding(0,
                systemBarHeight(activity, "status_bar_height"),
                0,
                systemBarHeight(activity, "navigation_bar_height"));
        root.setFitsSystemWindows(true);

        LinearLayout bar = new LinearLayout(activity);
        bar.setOrientation(LinearLayout.HORIZONTAL);
        bar.setPadding(32, 24, 32, 24);
        TextView label = new TextView(activity);
        label.setText(title);
        label.setTextColor(Color.WHITE);
        label.setTextSize(17);
        LinearLayout.LayoutParams labelParams =
                new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        bar.addView(label, labelParams);
        Button close = new Button(activity);
        close.setText("关闭");
        bar.addView(close);
        root.addView(bar, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        root.addView(web, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        Button actionButton = new Button(activity);
        actionButton.setText(actionLabel);
        root.addView(actionButton, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        actionButton.setOnClickListener(v -> action.run());
        return root;
    }

    /** Open the Zhihu sign-in page; "导入登录状态" reads cookies back. */
    public static void openWebLogin(Activity activity) {
        activity.runOnUiThread(() -> {
            Dialog dialog = new Dialog(activity, android.R.style.Theme_DeviceDefault_NoActionBar);
            WebView web = new WebView(activity);
            configure(web);
            CookieManager.getInstance().removeAllCookies(null);
            CookieManager.getInstance().flush();
            web.setWebViewClient(new WebViewClient());
            LinearLayout root = buildLayout(activity, web, "浏览器登录", "我已登录，导入",
                    () -> {
                        String cookies = CookieManager.getInstance().getCookie(HOME);
                        if (cookies == null || cookies.isEmpty()) {
                            cookies = CookieManager.getInstance().getCookie(SIGNIN);
                        }
                        if (cookies != null && !cookies.isEmpty()) {
                            nativeCookieHeader(cookies);
                        }
                        dialog.dismiss();
                    });
            Button close = (Button) ((LinearLayout) root.getChildAt(0)).getChildAt(1);
            close.setOnClickListener(v -> dialog.dismiss());
            dialog.setContentView(root);
            dialog.setOnDismissListener(d -> web.destroy());
            dialog.show();
            web.loadUrl(SIGNIN);
        });
    }

    /**
     * Open the risk-control page in a WebView that shares the QR session's
     * cookies. `cookiePairs` uses U+0001 between `name=value` pairs.
     */
    public static void openRiskControl(Activity activity, String url, String cookiePairs) {
        activity.runOnUiThread(() -> {
            Dialog dialog = new Dialog(activity, android.R.style.Theme_DeviceDefault_NoActionBar);
            WebView web = new WebView(activity);
            configure(web);
            CookieManager manager = CookieManager.getInstance();
            if (cookiePairs != null && !cookiePairs.isEmpty()) {
                for (String pair : cookiePairs.split("\u0001")) {
                    if (pair.isEmpty()) continue;
                    manager.setCookie("https://www.zhihu.com",
                            pair + "; Domain=.zhihu.com; Path=/");
                }
                manager.flush();
            }
            web.setWebViewClient(new WebViewClient());
            LinearLayout root = buildLayout(activity, web, "验证网络环境", "完成验证后继续扫码",
                    () -> {
                        String cookies = manager.getCookie(HOME);
                        if (cookies != null && !cookies.isEmpty()) {
                            nativeSessionCookies(cookies);
                        }
                        dialog.dismiss();
                    });
            Button close = (Button) ((LinearLayout) root.getChildAt(0)).getChildAt(1);
            close.setOnClickListener(v -> dialog.dismiss());
            dialog.setContentView(root);
            dialog.setOnDismissListener(d -> web.destroy());
            dialog.show();
            web.loadUrl(url == null || url.isEmpty() ? HOME : url);
        });
    }

    private static native void nativeCookieHeader(String cookies);

    private static native void nativeSessionCookies(String cookies);
}
