const CONTROLLER_KEY = '__TAURITAVERN_MOBILE_TOP_SETTINGS_LAYOUT__';

const HOLDER_SAFE_OFFSET_VAR = '--tt-top-settings-holder-safe-offset';
const HOLDER_SAFE_OFFSET_EXPR = 'max(var(--tt-inset-top), 0px)';
const TOP_BAR_TOP_EXPR = '0px';
const TOP_BAR_MARGIN_TOP_EXPR = `var(${HOLDER_SAFE_OFFSET_VAR})`;
const TOP_BAR_PADDING_RIGHT_EXPR = 'max(var(--tt-inset-right), 0px)';
const TOP_BAR_PADDING_LEFT_EXPR = 'max(var(--tt-inset-left), 0px)';
const PANEL_TOP_VAR = '--tt-top-settings-panel-top';
const PANEL_MAX_HEIGHT_VAR = '--tt-top-settings-panel-max-height';

function requireHTMLElement(id) {
    const element = document.getElementById(id);
    if (!(element instanceof HTMLElement)) {
        throw new Error(`[TauriTavern] #${id} unavailable while installing mobile top settings layout controller.`);
    }

    return element;
}

function setStyleValue(element, property, value) {
    const currentValue = String(element.style.getPropertyValue(property) || '').trim();
    const currentPriority = element.style.getPropertyPriority(property) || '';
    if (currentValue === value && currentPriority === '') {
        return;
    }

    element.style.setProperty(property, value);
}

function collectTopSettingsDrawerContents(topSettingsHolder) {
    const drawerContents = [];

    for (const child of Array.from(topSettingsHolder.children)) {
        if (!(child instanceof HTMLElement) || !child.classList.contains('drawer')) {
            continue;
        }

        for (const grandchild of Array.from(child.children)) {
            if (!(grandchild instanceof HTMLElement) || !grandchild.classList.contains('drawer-content')) {
                continue;
            }

            drawerContents.push(grandchild);
        }
    }

    return drawerContents;
}

function applyRootContract(root) {
    setStyleValue(root, HOLDER_SAFE_OFFSET_VAR, HOLDER_SAFE_OFFSET_EXPR);
}

function applyTopBarLayout(element) {
    setStyleValue(element, 'top', TOP_BAR_TOP_EXPR);
    setStyleValue(element, 'margin-top', TOP_BAR_MARGIN_TOP_EXPR);
    setStyleValue(element, 'padding-right', TOP_BAR_PADDING_RIGHT_EXPR);
    setStyleValue(element, 'padding-left', TOP_BAR_PADDING_LEFT_EXPR);
}

function parsePixelValue(value) {
    const numericValue = Number.parseFloat(String(value || '').trim());
    if (!Number.isFinite(numericValue)) {
        return 0;
    }

    return numericValue;
}

function formatPixelValue(value) {
    const normalizedValue = Math.max(0, value);
    return `${normalizedValue}px`;
}

function applyPanelLayout(panel, holderBottom) {
    const computedStyle = window.getComputedStyle(panel);
    const marginTop = parsePixelValue(computedStyle.marginTop);
    const resolvedTop = Math.max(0, holderBottom - marginTop);

    setStyleValue(panel, PANEL_TOP_VAR, formatPixelValue(resolvedTop));
    setStyleValue(panel, PANEL_MAX_HEIGHT_VAR, `calc(var(--tt-base-viewport-height, var(--doc-height)) - ${formatPixelValue(holderBottom)})`);
    setStyleValue(panel, 'top', `var(${PANEL_TOP_VAR})`);
    setStyleValue(panel, 'max-height', `var(${PANEL_MAX_HEIGHT_VAR})`);
}

export function installMobileTopSettingsLayoutController() {
    if (window[CONTROLLER_KEY]) {
        return window[CONTROLLER_KEY];
    }

    let disposed = false;
    let holderObserver = null;
    let holderResizeObserver = null;

    const revalidate = () => {
        if (disposed) {
            return;
        }

        const root = document.documentElement;
        if (!(root instanceof HTMLElement)) {
            throw new Error('[TauriTavern] documentElement unavailable while applying mobile top settings layout contract.');
        }

        const topSettingsHolder = requireHTMLElement('top-settings-holder');
        const topBar = requireHTMLElement('top-bar');

        applyRootContract(root);
        applyTopBarLayout(topSettingsHolder);
        applyTopBarLayout(topBar);

        const holderBottom = topSettingsHolder.getBoundingClientRect().bottom;
        for (const drawerContent of collectTopSettingsDrawerContents(topSettingsHolder)) {
            if (drawerContent.classList.contains('fillLeft') || drawerContent.classList.contains('fillRight')) {
                continue;
            }

            applyPanelLayout(drawerContent, holderBottom);
        }
    };

    const start = () => {
        revalidate();

        const topSettingsHolder = requireHTMLElement('top-settings-holder');
        holderObserver = new MutationObserver(revalidate);
        holderObserver.observe(topSettingsHolder, {
            childList: true,
        });

        holderResizeObserver = new ResizeObserver(revalidate);
        holderResizeObserver.observe(topSettingsHolder);

        window.addEventListener('resize', revalidate);
        window.addEventListener('orientationchange', revalidate);
        window.visualViewport?.addEventListener('resize', revalidate);
        window.visualViewport?.addEventListener('scroll', revalidate);
    };

    if (document.body) {
        start();
    } else {
        document.addEventListener('DOMContentLoaded', start, { once: true });
    }

    const controller = {
        dispose() {
            disposed = true;
            holderObserver?.disconnect();
            holderResizeObserver?.disconnect();
            window.removeEventListener('resize', revalidate);
            window.removeEventListener('orientationchange', revalidate);
            window.visualViewport?.removeEventListener('resize', revalidate);
            window.visualViewport?.removeEventListener('scroll', revalidate);
            delete window[CONTROLLER_KEY];
        },
        revalidate,
    };

    window[CONTROLLER_KEY] = controller;
    return controller;
}
