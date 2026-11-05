package com.cabalmesh.app

import android.os.Bundle
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.ScriptHandler
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature

class MainActivity : TauriActivity() {
  private var insetScript: ScriptHandler? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  /**
   * Publishes the window insets to CSS.
   *
   * The layout is edge-to-edge and relies on `env(safe-area-inset-*)` to keep
   * the header and tab bar clear of the system UI. That works on iOS. On
   * Android the WebView only ever reports a **display cutout** through those
   * properties — the status bar and the gesture pill are window insets, which
   * it does not surface at all. So `safe-area-inset-bottom` resolves to `0px`
   * and the tab labels render underneath the gesture bar.
   *
   * The insets are pushed in from here as `--android-inset-*`, which
   * `mobile.css` folds in with `max()`. Padding the WebView instead would fix
   * the collision by giving up edge-to-edge, which is the design.
   *
   * # Why a document-start script and not just `evaluateJavascript`
   *
   * Insets are delivered during the first layout pass, **before Tauri
   * navigates**, so the WebView is still on `about:blank` at that moment. An
   * inline style set there is thrown away by the navigation that follows, and
   * the result looks exactly like insets that were never applied. Registering
   * the assignment as a document-start script makes it run again for the real
   * document — and for every reload after it.
   *
   * `evaluateJavascript` is still called alongside, for when insets change
   * while a page is already loaded: a rotation, or the keyboard opening.
   */
  override fun onWebViewCreate(webView: WebView) {
    ViewCompat.setOnApplyWindowInsetsListener(webView) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      // Android reports insets in physical pixels; CSS wants density-independent ones.
      val density = view.resources.displayMetrics.density
      fun dp(value: Int) = (value / density).toInt()

      val script = """
        (function () {
          var root = document.documentElement.style;
          root.setProperty('--android-inset-top', '${dp(bars.top)}px');
          root.setProperty('--android-inset-bottom', '${dp(bars.bottom)}px');
          root.setProperty('--android-inset-left', '${dp(bars.left)}px');
          root.setProperty('--android-inset-right', '${dp(bars.right)}px');
        })();
      """.trimIndent()

      if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
        // Replaced rather than added to: leaving the old one registered would
        // have a stale set of values racing the new one on the next load.
        insetScript?.remove()
        insetScript = WebViewCompat.addDocumentStartJavaScript(webView, script, setOf("*"))
      }
      webView.evaluateJavascript(script, null)

      // Passed through rather than consumed: consuming them would stop every
      // other view in the tree from seeing the same insets.
      insets
    }
  }
}
