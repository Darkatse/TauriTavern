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
        if (
          existingBridge &&
          typeof existingBridge.apply === 'function' &&
          typeof existingBridge.setImeTarget === 'function' &&
          typeof existingBridge.reapply === 'function'
        ) {
          return;
        }

        const IME_BOTTOM_VAR = '--tt-ime-bottom';

        const state = {
          baseViewportHeight: 0,
          baseViewportHeightCss: '',
          insetTop: '',
          insetRight: '',
          insetLeft: '',
          insetBottom: '',
          desiredImeTarget: null,
          appliedImeTarget: null,
          lastImeBottomCss: '0.00px',
          appliedImeBottomCss: '',
        };

        const setVarIfChanged = (target, cssName, stateKey, nextValue) => {
          if (state[stateKey] === nextValue) {
            return;
          }
          state[stateKey] = nextValue;
          target.style.setProperty(cssName, nextValue);
        };

        const requireDefaultImeTarget = () => {
          const fallback = document.getElementById('sheld');
          if (!(fallback instanceof HTMLElement)) {
            throw new Error('[TauriTavern] #sheld unavailable while applying IME insets.');
          }
          return fallback;
        };

        const resolveImeTarget = () => {
          const desired = state.desiredImeTarget;
          if (desired instanceof HTMLElement) {
            const root = document.documentElement;
            if (!root) {
              throw new Error('[TauriTavern] documentElement unavailable while resolving IME target.');
            }
            if (root.contains(desired)) {
              return desired;
            }
          }
          return requireDefaultImeTarget();
        };

        const applyImeBottom = (cssValue) => {
          state.lastImeBottomCss = cssValue;
          const target = resolveImeTarget();
          if (state.appliedImeTarget !== target) {
            if (state.appliedImeTarget instanceof HTMLElement) {
              state.appliedImeTarget.style.removeProperty(IME_BOTTOM_VAR);
            }
            state.appliedImeTarget = target;
            state.appliedImeBottomCss = '';
          }
          if (state.appliedImeBottomCss === cssValue) {
            return;
          }
          target.style.setProperty(IME_BOTTOM_VAR, cssValue);
          state.appliedImeBottomCss = cssValue;
        };

        const setImeTarget = (elementOrNull) => {
          if (elementOrNull === null) {
            state.desiredImeTarget = null;
          } else if (elementOrNull instanceof HTMLElement) {
            const root = document.documentElement;
            if (!root) {
              throw new Error('[TauriTavern] documentElement unavailable while setting IME target.');
            }
            if (!root.contains(elementOrNull)) {
              throw new Error('[TauriTavern] IME target must be connected to the document.');
            }
            state.desiredImeTarget = elementOrNull;
          } else {
            throw new Error('[TauriTavern] Invalid IME target (expected HTMLElement or null).');
          }

          applyImeBottom(state.lastImeBottomCss);
        };

        window.__TAURITAVERN_INSETS__ = {
          apply(insetTop, insetRight, insetLeft, insetBottom, imeBottom) {
            const root = document.documentElement;
            if (!root) {
              throw new Error('[TauriTavern] documentElement unavailable while applying insets.');
            }

            const viewportHeight =
              Number.isFinite(window.innerHeight) ? window.innerHeight : 0;
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

            setVarIfChanged(root, '--tt-inset-top', 'insetTop', insetTop.toFixed(2) + 'px');
            setVarIfChanged(root, '--tt-inset-right', 'insetRight', insetRight.toFixed(2) + 'px');
            setVarIfChanged(root, '--tt-inset-left', 'insetLeft', insetLeft.toFixed(2) + 'px');
            setVarIfChanged(root, '--tt-inset-bottom', 'insetBottom', insetBottom.toFixed(2) + 'px');

            const imeBottomCss = Math.max(0, imeBottom).toFixed(2) + 'px';
            applyImeBottom(imeBottomCss);
          },
          setImeTarget,
          reapply() {
            applyImeBottom(state.lastImeBottomCss);
          },
        };
      })();
      """.trimIndent()
  }
}
