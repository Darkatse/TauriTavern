// @ts-check

import { BUTTON_ID, MAX_LEVEL, MIN_LEVEL, SLIDER_ID, WIDGET_ID } from './constants.js';

/**
 * @typedef {object} OledReaderState
 * @property {boolean} enabled
 * @property {number} level
 */

/**
 * @typedef {object} WidgetHandlers
 * @property {() => OledReaderState} getState
 * @property {() => void} onToggle
 * @property {(level: number) => void} onLevel
 */

/**
 * Mount the floating reader control (toggle button + brightness slider).
 * Idempotent; safe to call once <body> exists.
 * @param {WidgetHandlers} handlers
 */
export function mountWidget(handlers) {
    if (document.getElementById(WIDGET_ID) || !document.body) {
        return;
    }

    const widget = document.createElement('div');
    widget.id = WIDGET_ID;

    const button = document.createElement('button');
    button.id = BUTTON_ID;
    button.type = 'button';
    button.title = 'OLED red-black reader';
    button.setAttribute('aria-label', 'Toggle OLED red-black reader');
    const icon = document.createElement('i');
    icon.className = 'fa-solid fa-circle-half-stroke';
    button.appendChild(icon);
    button.addEventListener('click', () => handlers.onToggle());

    const slider = document.createElement('input');
    slider.id = SLIDER_ID;
    slider.type = 'range';
    slider.min = String(MIN_LEVEL);
    slider.max = String(MAX_LEVEL);
    slider.step = '0.01';
    slider.title = 'Red brightness';
    slider.setAttribute('aria-label', 'Red brightness');
    slider.addEventListener('input', () => handlers.onLevel(Number(slider.value)));

    widget.appendChild(button);
    widget.appendChild(slider);
    document.body.appendChild(widget);

    syncWidget(handlers.getState());
}

/**
 * Reflect current state into the widget controls.
 * @param {OledReaderState} state
 */
export function syncWidget(state) {
    const button = document.getElementById(BUTTON_ID);
    if (button) {
        button.setAttribute('aria-pressed', String(state.enabled));
    }
    const slider = document.getElementById(SLIDER_ID);
    if (slider instanceof HTMLInputElement) {
        slider.value = String(state.level);
    }
}
