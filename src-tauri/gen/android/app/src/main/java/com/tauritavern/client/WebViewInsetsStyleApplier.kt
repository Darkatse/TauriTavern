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
    val safeTop = toCssPxNumber(snapshot.systemBars.top)
    val safeRight = toCssPxNumber(snapshot.systemBars.right)
    val safeLeft = toCssPxNumber(snapshot.systemBars.left)
    val safeBottom = toCssPxNumber(snapshot.systemBars.bottom)
    val imeBottom = toCssPxNumber(snapshot.imeBottom)

    return """
      (() => {
        const bridge = window.__TAURITAVERN_INSETS__;
        if (!bridge || typeof bridge.apply !== 'function') return;
        bridge.apply($safeTop, $safeRight, $safeLeft, $safeBottom, $imeBottom);
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
          safeAreaTop: '',
          safeAreaRight: '',
          safeAreaLeft: '',
          safeAreaBottom: '',
          imeBottom: '',
          imeTargetId: '',
          imeTargetRef: null,
        };

        const setVarIfChanged = (target, cssName, stateKey, nextValue) => {
          if (!target || state[stateKey] === nextValue) {
            return;
          }
          state[stateKey] = nextValue;
          target.style.setProperty(cssName, nextValue);
        };

        const resolveImeTarget = (root) => document.getElementById('sheld') || root;

        window.__TAURITAVERN_INSETS__ = {
          apply(safeTop, safeRight, safeLeft, safeBottom, imeBottom) {
            const root = document.documentElement;
            if (!root) return;

            const viewport = window.visualViewport;
            const viewportHeight =
              viewport && Number.isFinite(viewport.height) ? viewport.height : window.innerHeight;
            const imeVisible = imeBottom > 0;

            if (!imeVisible && viewportHeight > 0) {
              state.baseViewportHeight = viewportHeight;
            } else if (!state.baseViewportHeight && viewportHeight > 0) {
              state.baseViewportHeight = viewportHeight;
            }

            const viewportShrink =
              imeVisible && state.baseViewportHeight > 0
                ? Math.max(0, state.baseViewportHeight - viewportHeight)
                : 0;
            const effectiveImeBottom = Math.max(0, imeBottom - viewportShrink);

            setVarIfChanged(root, '--tt-safe-area-top', 'safeAreaTop', safeTop.toFixed(2) + 'px');
            setVarIfChanged(root, '--tt-safe-area-right', 'safeAreaRight', safeRight.toFixed(2) + 'px');
            setVarIfChanged(root, '--tt-safe-area-left', 'safeAreaLeft', safeLeft.toFixed(2) + 'px');
            setVarIfChanged(
              root,
              '--tt-safe-area-bottom',
              'safeAreaBottom',
              safeBottom.toFixed(2) + 'px',
            );

            const imeBottomCss = effectiveImeBottom.toFixed(2) + 'px';
            const imeTarget = resolveImeTarget(root);
            const imeTargetId = imeTarget === root ? 'root' : 'sheld';

            if (
              state.imeTargetRef &&
              state.imeTargetRef !== imeTarget &&
              state.imeTargetRef.isConnected
            ) {
              state.imeTargetRef.style.removeProperty('--tt-ime-bottom');
            }

            if (state.imeBottom !== imeBottomCss || state.imeTargetId !== imeTargetId) {
              state.imeBottom = imeBottomCss;
              state.imeTargetId = imeTargetId;
              state.imeTargetRef = imeTarget;
              imeTarget.style.setProperty('--tt-ime-bottom', imeBottomCss);
            }
          },
        };
      })();
      """.trimIndent()
  }
}
