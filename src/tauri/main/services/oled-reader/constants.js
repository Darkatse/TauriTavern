// @ts-check

/**
 * Shared constants for the OLED red-black reader service.
 *
 * The reader turns the whole app into a pure-black canvas with single-hue red
 * text. Everything is driven from one root state ({ enabled, level }) so a
 * single control flips the entire UI, per the oled-red-black guidance.
 */

/** Class toggled on <html> that activates every override rule. */
export const ROOT_CLASS = 'tt-oled-reader';

/** <style> element id holding the override rules (injected once). */
export const STYLE_ELEMENT_ID = 'tt-oled-reader-style';

/** Floating widget container id. */
export const WIDGET_ID = 'tt-oled-reader-widget';
/** Toggle button id (the "reader" button). */
export const BUTTON_ID = 'tt-oled-reader-toggle';
/** Red-brightness slider id. */
export const SLIDER_ID = 'tt-oled-reader-level';

/** localStorage key. OLED mode is a per-device display preference, so it is
 * stored locally rather than synced through the typed backend settings. */
export const STORAGE_KEY = 'tauritavern:oled_reader';

/** Window event broadcast when the reader state changes, so any other surface
 * (e.g. a future settings panel entry) can stay in sync. */
export const OLED_READER_CHANGED_EVENT = 'tauritavern:oled_reader_changed';

/** Inline CSS custom properties carrying the computed palette. */
export const CSS_RED = '--tt-oled-red';
export const CSS_RED_DIM = '--tt-oled-red-dim';
export const CSS_CHANNEL = '--tt-oled-red-channel';

/** level bounds: 0.15 keeps the darkest setting readable (channel >= 99). */
export const MIN_LEVEL = 0.15;
export const MAX_LEVEL = 1;
export const DEFAULT_LEVEL = 0.7;
export const DEFAULT_ENABLED = false;
