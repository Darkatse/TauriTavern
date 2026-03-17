package com.tauritavern.client

import android.content.res.Resources
import android.webkit.WebView
import androidx.core.graphics.Insets
import java.util.Locale

data class InsetsSnapshot(val systemBars: Insets, val imeBottom: Int)

class WebViewInsetsStyleApplier(private val resources: Resources) {
  private var isHelperInjected: Boolean = false

  fun onWebViewContextReset() {
    isHelperInjected = false
  }

  fun apply(targetWebView: WebView, snapshot: InsetsSnapshot) {
    if (!isHelperInjected) {
      targetWebView.evaluateJavascript(INSTALL_HELPER_SCRIPT, null)
      isHelperInjected = true
    }

    targetWebView.evaluateJavascript(buildApplyScript(snapshot), null)
  }

  private fun buildApplyScript(snapshot: InsetsSnapshot): String {
    val insetTop = toCssPxNumber(snapshot.systemBars.top)
    val insetRight = toCssPxNumber(snapshot.systemBars.right)
    val insetLeft = toCssPxNumber(snapshot.systemBars.left)
    val insetBottom = toCssPxNumber(snapshot.systemBars.bottom)
    val imeBottom = toCssPxNumber(snapshot.imeBottom)

    return """
      (() => {
        const bridge = window.__TAURITAVERN_INSETS__;
        if (!bridge || typeof bridge.apply !== 'function') {
          throw new Error('[TauriTavern] Android insets bridge unavailable.');
        }
        bridge.apply($insetTop, $insetRight, $insetLeft, $insetBottom, $imeBottom);
      })();
      """.trimIndent()
  }

  private fun toCssPxNumber(value: Int): String =
    String.format(Locale.US, "%.4f", value / resources.displayMetrics.density)

  companion object {
    private val INSTALL_HELPER_SCRIPT =
      """
      (() => {
        const existingBridge = window.__TAURITAVERN_INSETS__;
        if (existingBridge && typeof existingBridge.apply === 'function') {
          return;
        }

        const state = {
          baseViewportHeight: 0,
          baseViewportHeightCss: '',
          insetTop: '',
          insetRight: '',
          insetLeft: '',
          insetBottom: '',
          imeBottom: '',
        };

        const setVarIfChanged = (target, cssName, stateKey, nextValue) => {
          if (state[stateKey] === nextValue) {
            return;
          }
          state[stateKey] = nextValue;
          target.style.setProperty(cssName, nextValue);
        };

        window.__TAURITAVERN_INSETS__ = {
          apply(insetTop, insetRight, insetLeft, insetBottom, imeBottom) {
            const root = document.documentElement;
            if (!root) {
              throw new Error('[TauriTavern] documentElement unavailable while applying insets.');
            }

            const viewport = window.visualViewport;
            const viewportHeight =
              viewport && Number.isFinite(viewport.height) ? viewport.height : window.innerHeight;
            const imeVisible = imeBottom > 0;

            if (!imeVisible && viewportHeight > 0) {
              state.baseViewportHeight = viewportHeight;
            } else if (!state.baseViewportHeight && viewportHeight > 0) {
              state.baseViewportHeight = viewportHeight;
            }

            if (state.baseViewportHeight > 0) {
              setVarIfChanged(
                root,
                '--tt-base-viewport-height',
                'baseViewportHeightCss',
                state.baseViewportHeight.toFixed(2) + 'px',
              );
            }

            const viewportShrink =
              imeVisible && state.baseViewportHeight > 0
                ? Math.max(0, state.baseViewportHeight - viewportHeight)
                : 0;
            const effectiveImeBottom = Math.max(0, imeBottom - viewportShrink);

            setVarIfChanged(root, '--tt-inset-top', 'insetTop', insetTop.toFixed(2) + 'px');
            setVarIfChanged(root, '--tt-inset-right', 'insetRight', insetRight.toFixed(2) + 'px');
            setVarIfChanged(root, '--tt-inset-left', 'insetLeft', insetLeft.toFixed(2) + 'px');
            setVarIfChanged(root, '--tt-inset-bottom', 'insetBottom', insetBottom.toFixed(2) + 'px');

            const imeBottomCss = effectiveImeBottom.toFixed(2) + 'px';
            const imeTarget = document.getElementById('sheld');
            if (!imeTarget) {
              throw new Error('[TauriTavern] #sheld unavailable while applying IME insets.');
            }
            setVarIfChanged(imeTarget, '--tt-ime-bottom', 'imeBottom', imeBottomCss);
          },
        };
      })();
      """.trimIndent()
  }
}
