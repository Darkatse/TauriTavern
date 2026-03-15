const OVERLAY_ID = 'tt-startup-status-overlay';

function ensureOverlayElement() {
    const existing = document.getElementById(OVERLAY_ID);
    if (existing instanceof HTMLDivElement) {
        return existing;
    }

    const element = document.createElement('div');
    element.id = OVERLAY_ID;
    element.style.position = 'fixed';
    element.style.left = '12px';
    element.style.bottom = '12px';
    element.style.zIndex = '999999';
    element.style.maxWidth = 'min(92vw, 560px)';
    element.style.padding = '6px 10px';
    element.style.borderRadius = '10px';
    element.style.background = 'rgba(0, 0, 0, 0.55)';
    element.style.color = '#fff';
    element.style.font = '12px/1.4 system-ui, -apple-system, Segoe UI, Roboto, sans-serif';
    element.style.pointerEvents = 'none';
    element.style.whiteSpace = 'pre-wrap';
    element.textContent = '';

    document.body.appendChild(element);
    return element;
}

export function createStartupStatusOverlay() {
    const element = ensureOverlayElement();

    return {
        setText(text) {
            element.textContent = String(text ?? '');
        },
        remove() {
            element.remove();
        },
    };
}
