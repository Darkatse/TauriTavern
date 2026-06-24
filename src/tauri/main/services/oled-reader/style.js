// @ts-check

import { CSS_RED, CSS_RED_DIM, ROOT_CLASS, WIDGET_ID, BUTTON_ID, SLIDER_ID } from './constants.js';

/**
 * Build the CSS that powers the reader.
 *
 * Strategy: SillyTavern drives ~all colors from `--SmartTheme*` custom
 * properties that it writes inline on <html> via setProperty(). Author
 * declarations marked `!important` outrank inline non-important declarations,
 * so re-pointing those variables here flips the whole UI and survives theme
 * switches. A handful of hardcoded colors (kbd, scrollbars, selection,
 * wallpaper) are overridden explicitly.
 *
 * `red` / `dimRed` come from inline `--tt-oled-red*` vars set per the current
 * level, so the slider can dim the hue without rebuilding this sheet.
 *
 * @returns {string}
 */
export function buildOledCss() {
    const red = `var(${CSS_RED})`;
    const dim = `var(${CSS_RED_DIM})`;
    const root = `html.${ROOT_CLASS}`;

    return `
/* ===== TauriTavern OLED red-black reader ===== */
${root} {
    /* foreground: single red hue */
    --SmartThemeBodyColor: ${red} !important;
    --SmartThemeEmColor: ${dim} !important;
    --SmartThemeUnderlineColor: ${red} !important;
    --SmartThemeQuoteColor: ${red} !important;

    /* surfaces: pure black so OLED pixels switch off */
    --SmartThemeBlurTintColor: #000000 !important;
    --SmartThemeChatTintColor: #000000 !important;
    --SmartThemeUserMesBlurTintColor: #000000 !important;
    --SmartThemeBotMesBlurTintColor: #000000 !important;
    --SmartThemeFastUIBGColor: #000000 !important;

    /* structure via thin red borders, no glow */
    --SmartThemeBorderColor: ${dim} !important;
    --SmartThemeShadowColor: rgba(0, 0, 0, 0) !important;

    /* checkbox: black box, red tick */
    --SmartThemeCheckboxBgColorR: 0 !important;
    --SmartThemeCheckboxBgColorG: 0 !important;
    --SmartThemeCheckboxBgColorB: 0 !important;
    --SmartThemeCheckboxTickColor: ${red} !important;

    /* utility accents collapsed onto the single hue */
    --interactable-outline-color: ${red} !important;
    --interactable-outline-color-faint: ${dim} !important;
    --white100: ${red} !important;
    --white70a: ${red} !important;
    --white60a: ${red} !important;
    --white50a: ${red} !important;
    --white30a: ${dim} !important;
    --white20a: ${dim} !important;
    --grey75: ${red} !important;
    --grey70: ${red} !important;
    --grey50: ${dim} !important;
    --grey30: ${dim} !important;
    --grey10: #000000 !important;
    --golden: ${red} !important;
    --ivory: ${red} !important;
    --active: ${red} !important;
    --preferred: ${red} !important;
    --warning: ${red} !important;
    --okGreen70a: ${dim} !important;
    --crimson70a: ${dim} !important;

    background-color: #000000 !important;
    accent-color: ${red} !important;
    color-scheme: dark;
}

${root} body {
    background-color: #000000 !important;
    background-image: none !important;
    color: ${red};
}

/* kill the wallpaper layer — it is a large light source */
${root} #bg1,
${root} #bg2,
${root} #bg_custom {
    background-image: none !important;
    background-color: #000000 !important;
    filter: brightness(0) !important;
}

/* hardcoded spots that do not flow through the theme variables */
${root} kbd {
    background-color: #000000 !important;
    color: ${red} !important;
    border-color: ${dim} !important;
}

${root} ::selection {
    background: ${red} !important;
    color: #000000 !important;
}

${root} * {
    scrollbar-color: ${dim} #000000 !important;
}
${root} ::-webkit-scrollbar,
${root} ::-webkit-scrollbar-track {
    background: #000000 !important;
}
${root} ::-webkit-scrollbar-thumb {
    background: ${dim} !important;
}

/* ===== floating reader control (always present so it can switch ON) ===== */
#${WIDGET_ID} {
    position: fixed;
    left: calc(env(safe-area-inset-left, 0px) + 10px);
    bottom: calc(env(safe-area-inset-bottom, 0px) + 10px);
    z-index: 2147483600;
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: var(--mainFontFamily, "Noto Sans", sans-serif);
    pointer-events: none;
}
#${WIDGET_ID} > * {
    pointer-events: auto;
}
#${BUTTON_ID} {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    font-size: 16px;
    line-height: 1;
    color: #d0d0c8;
    background: rgba(0, 0, 0, 0.55);
    border: 1px solid rgba(180, 180, 180, 0.45);
    backdrop-filter: blur(4px);
    opacity: 0.55;
    transition: opacity 0.15s ease;
}
#${BUTTON_ID}:hover {
    opacity: 1;
}
#${SLIDER_ID} {
    display: none;
    width: 110px;
    height: 18px;
}
${root} #${WIDGET_ID} #${BUTTON_ID} {
    color: ${red};
    background: #000000;
    border-color: ${red};
    opacity: 1;
}
${root} #${WIDGET_ID} #${SLIDER_ID} {
    display: block;
    accent-color: ${red};
}
`;
}
