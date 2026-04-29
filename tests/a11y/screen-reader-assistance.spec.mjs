import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const WCAG_AA_TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'];

let staticServer;

function getContentType(filePath) {
    const extension = path.extname(filePath);
    if (extension === '.html') return 'text/html; charset=utf-8';
    if (extension === '.js' || extension === '.mjs') return 'text/javascript; charset=utf-8';
    if (extension === '.json') return 'application/json; charset=utf-8';
    if (extension === '.css') return 'text/css; charset=utf-8';
    if (extension === '.svg') return 'image/svg+xml';
    if (extension === '.woff2') return 'font/woff2';
    if (extension === '.woff') return 'font/woff';
    return 'application/octet-stream';
}

async function startStaticServer() {
    const server = createServer(async (request, response) => {
        try {
            const url = new URL(request.url || '/', 'http://127.0.0.1');
            if (url.pathname === '/__a11y_blank__') {
                response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
                response.end('<!doctype html><html lang="en"><head><title>A11y harness</title></head><body></body></html>');
                return;
            }

            const filePath = path.resolve(REPO_ROOT, `.${decodeURIComponent(url.pathname)}`);
            if (!filePath.startsWith(`${REPO_ROOT}${path.sep}`)) {
                response.writeHead(403);
                response.end('Forbidden');
                return;
            }

            const data = await readFile(filePath);
            response.writeHead(200, { 'Content-Type': getContentType(filePath) });
            response.end(data);
        } catch {
            response.writeHead(404);
            response.end('Not found');
        }
    });

    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const address = server.address();
    if (!address || typeof address === 'string') {
        throw new Error('Unable to start a11y static test server');
    }

    return {
        url: `http://127.0.0.1:${address.port}`,
        close: () => new Promise(resolve => server.close(resolve)),
    };
}

async function setHarnessContent(page, body) {
    await page.goto(`${staticServer.url}/__a11y_blank__`);
    await page.setContent(`<!doctype html>
        <html lang="en">
            <head>
                <title>A11y harness</title>
                <style>
                    .sr-only {
                        position: absolute;
                        width: 1px;
                        height: 1px;
                        padding: 0;
                        margin: -1px;
                        overflow: hidden;
                        clip: rect(0 0 0 0);
                        white-space: nowrap;
                        border: 0;
                    }
                    .menu_button,
                    .interactable {
                        display: inline-flex;
                        min-width: 24px;
                        min-height: 24px;
                        align-items: center;
                        justify-content: center;
                    }
                    .options-content[hidden],
                    [hidden] {
                        display: none;
                    }
                </style>
            </head>
            <body>${body}</body>
        </html>`);
}

async function installJQuery(page) {
    await page.addScriptTag({ url: `${staticServer.url}/src/lib/jquery-3.5.1.min.js` });
    await page.waitForFunction(() => Boolean(window.jQuery));
}

async function installSharedAccessibility(page) {
    await page.addScriptTag({
        type: 'module',
        content: `
            import { initAccessibility } from '${staticServer.url}/src/scripts/a11y.js';
            initAccessibility();
            window.__sharedA11yReady = true;
        `,
    });
    await page.waitForFunction(() => window.__sharedA11yReady === true);
}

async function installKeyboard(page) {
    await page.addScriptTag({
        type: 'module',
        content: `
            import { initKeyboard } from '${staticServer.url}/src/scripts/keyboard.js';
            initKeyboard();
            window.__a11yKeyboardReady = true;
        `,
    });
    await page.waitForFunction(() => window.__a11yKeyboardReady === true);
}

async function installScreenReaderHarness(page) {
    await page.addScriptTag({
        type: 'module',
        content: `
            import { event_types, eventSource } from '${staticServer.url}/src/scripts/events.js';
            import {
                initScreenReaderAssistance,
                setScreenReaderAssistanceEnabled,
            } from '${staticServer.url}/src/scripts/a11y/screen-reader.js';

            window.__screenReaderA11yHarness = {
                initEnabled() {
                    setScreenReaderAssistanceEnabled(true);
                    initScreenReaderAssistance();
                },
                setEnabled(value) {
                    setScreenReaderAssistanceEnabled(value);
                },
                emit(name, ...args) {
                    return eventSource.emit(event_types[name], ...args);
                },
            };
        `,
    });
    await page.waitForFunction(() => Boolean(window.__screenReaderA11yHarness));
}

async function installSettingsHarness(page) {
    await page.addScriptTag({
        type: 'module',
        content: `
            import { initScreenReaderAssistance, setScreenReaderAssistanceEnabled } from '${staticServer.url}/src/scripts/a11y/screen-reader.js';
            import { initSettingsSearch } from '${staticServer.url}/src/scripts/setting-search.js';

            window.__settingsA11yHarness = {
                init() {
                    setScreenReaderAssistanceEnabled(true);
                    initScreenReaderAssistance();
                    initSettingsSearch();
                },
                setEnabled(value) {
                    setScreenReaderAssistanceEnabled(value);
                },
            };
        `,
    });
    await page.waitForFunction(() => Boolean(window.__settingsA11yHarness));
}

async function expectNoAxeViolations(page, includeSelector) {
    const results = await new AxeBuilder({ page })
        .include(includeSelector)
        .withTags(WCAG_AA_TAGS)
        .analyze();

    expect(results.violations.map(violation => ({
        id: violation.id,
        impact: violation.impact,
        targets: violation.nodes.map(node => node.target),
    }))).toEqual([]);
}

test.beforeAll(async () => {
    staticServer = await startStaticServer();
});

test.afterAll(async () => {
    await staticServer?.close();
});

test('shared accessibility core assigns roles to existing and dynamic SillyTavern widgets', async ({ page }) => {
    await setHarnessContent(page, `
        <main id="fixture">
            <div class="options-content">
                <div class="list-group-item">Preset A</div>
            </div>
            <div class="jg-menu" aria-label="Gallery actions">
                <div class="jg-button" title="Open gallery item">Open gallery item</div>
            </div>
            <div id="bg_tabs">
                <div class="bg_tabs_list">
                    <div class="bg_tab_button" aria-selected="true">System backgrounds</div>
                </div>
            </div>
            <div id="toast-container">
                <div class="toast">Settings saved.</div>
            </div>
            <div id="dynamic-root"></div>
        </main>
    `);
    await installSharedAccessibility(page);

    await expect(page.getByRole('list')).toBeVisible();
    await expect(page.locator('.list-group-item')).toHaveAttribute('role', 'listitem');
    await expect(page.getByRole('toolbar', { name: 'Gallery actions' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open gallery item' })).toBeVisible();
    await expect(page.getByRole('tablist')).toBeVisible();
    await expect(page.getByRole('tab', { name: 'System backgrounds' })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByRole('status')).toContainText('Settings saved.');

    await page.locator('#dynamic-root').evaluate(root => {
        const button = document.createElement('div');
        button.className = 'menu_button';
        button.title = 'Refresh presets';
        button.textContent = 'Refresh presets';
        root.append(button);
    });
    await expect(page.getByRole('button', { name: 'Refresh presets' })).toBeVisible();
    await expectNoAxeViolations(page, '#fixture');
});

test('keyboard interactable layer exposes names and activation without editable or disabled side effects', async ({ page }) => {
    await setHarnessContent(page, `
        <main id="fixture">
            <div id="openPreset" class="menu_button" title="Open preset menu"></div>
            <div id="deletePreset" class="right_menu_button" data-tooltip="Delete preset\\nCannot be undone."></div>
            <div class="disabled">
                <div id="disabledAction" class="menu_button" title="Disabled action"></div>
            </div>
            <input id="nativeInput" class="menu_button" aria-label="Native text field">
            <div id="editableAction" class="menu_button" title="Editable action" contenteditable="true">Editable</div>
        </main>
    `);
    await installKeyboard(page);
    await page.evaluate(() => {
        window.__keyboardClicks = { openPreset: 0, deletePreset: 0, disabledAction: 0, nativeInput: 0, editableAction: 0 };
        for (const id of Object.keys(window.__keyboardClicks)) {
            document.getElementById(id).addEventListener('click', () => {
                window.__keyboardClicks[id] += 1;
            });
        }
    });

    const openPreset = page.getByRole('button', { name: 'Open preset menu' });
    await expect(openPreset).toHaveAttribute('tabindex', '0');
    await openPreset.focus();
    await page.keyboard.press('Enter');
    await expect.poll(() => page.evaluate(() => window.__keyboardClicks.openPreset)).toBe(1);
    await page.keyboard.press('Space');
    await expect.poll(() => page.evaluate(() => window.__keyboardClicks.openPreset)).toBe(2);
    await page.keyboard.press('Control+Enter');
    await expect.poll(() => page.evaluate(() => window.__keyboardClicks.openPreset)).toBe(2);

    await expect(page.getByRole('button', { name: 'Delete preset' })).toHaveAttribute('tabindex', '0');
    await expect(page.locator('#disabledAction')).toHaveAttribute('aria-disabled', 'true');
    await expect(page.locator('#disabledAction')).not.toHaveAttribute('tabindex', '0');

    await page.locator('#nativeInput').focus();
    await page.keyboard.press('Enter');
    await expect.poll(() => page.evaluate(() => window.__keyboardClicks.nativeInput)).toBe(0);

    await page.locator('#editableAction').focus();
    await page.keyboard.press('Enter');
    await expect.poll(() => page.evaluate(() => window.__keyboardClicks.editableAction)).toBe(0);
    await expectNoAxeViolations(page, '#fixture');
});

test('Screen Reader Assistance live region announces lifecycle events without exposing prompt content', async ({ page }) => {
    await setHarnessContent(page, '<main id="fixture" aria-label="Chat workspace"></main>');
    await installScreenReaderHarness(page);
    await page.evaluate(() => window.__screenReaderA11yHarness.initEnabled());

    const status = page.getByRole('status');
    await expect(status).toHaveAttribute('id', 'screen_reader_assistance_live_region');
    await expect(status).toHaveAttribute('aria-live', 'polite');
    await expect(status).toHaveAttribute('aria-atomic', 'true');

    await page.evaluate(() => window.__screenReaderA11yHarness.emit('GENERATION_STARTED', 'normal', { prompt: 'SECRET_PROMPT_TEXT' }, false));
    await expect(status).toHaveText('AI is generating a response. Stop button is available.');
    await expect(status).not.toContainText('SECRET_PROMPT_TEXT');

    await page.evaluate(() => window.__screenReaderA11yHarness.emit('GENERATION_STARTED', 'quiet', {}, false));
    await expect(status).toHaveText('AI is generating a response. Stop button is available.');

    await page.evaluate(() => window.__screenReaderA11yHarness.emit('GENERATION_STARTED', 'normal', {}, false));
    await page.evaluate(() => window.__screenReaderA11yHarness.emit('CHARACTER_MESSAGE_RENDERED', 2, 'normal'));
    await expect(status).toHaveText('AI response ready.');

    await page.evaluate(() => window.__screenReaderA11yHarness.emit('ONLINE_STATUS_CHANGED', 'no_connection'));
    await expect(status).toHaveText('API disconnected.');
    await page.evaluate(() => window.__screenReaderA11yHarness.setEnabled(false));
    await expect(page.locator('#screen_reader_assistance_live_region')).toHaveCount(0);
});

test('settings search and streaming controls expose real accessible form state', async ({ page }) => {
    await setHarnessContent(page, `
        <main id="fixture">
            <textarea id="settingsSearch" aria-label="Search settings"></textarea>
            <div id="settingsSearchStatus" class="sr-only" role="status" aria-live="polite" aria-atomic="true"></div>
            <div id="user-settings-block-content">
                <input type="range" id="amount_gen" value="80" max="200"><input type="number" id="amount_gen_counter">
                <input type="range" id="max_context" value="4096"><input type="number" id="max_context_counter">
                <input type="range" id="chat_width_slider" value="70"><input type="number" id="chat_width_slider_counter">
                <input type="range" id="font_scale" value="1"><input type="number" id="font_scale_counter">
                <input type="range" id="blur_strength" value="10"><input type="number" id="blur_strength_counter">
                <input type="range" id="shadow_width" value="2"><input type="number" id="shadow_width_counter">

                <p>needle alpha</p>
                <p>needle beta</p>

                <div class="range-block">
                    <label for="stream_toggle">
                        <input id="stream_toggle" type="checkbox">
                        <span>Streaming</span>
                    </label>
                    <div class="toggle-description">
                        <span>Display the response bit by bit as it is generated.</span>
                        <span>When this is off, responses will be displayed all at once when they are complete.</span>
                    </div>
                </div>
                <label for="streaming_textgenerationwebui">
                    <input id="streaming_textgenerationwebui" type="checkbox">
                    <span>Streaming</span>
                </label>
                <label for="streaming_kobold">
                    <input id="streaming_kobold" type="checkbox">
                    <span>Streaming</span>
                </label>
                <label for="streaming_novel">
                    <input id="streaming_novel" type="checkbox">
                    <span>Streaming</span>
                </label>
            </div>
        </main>
    `);
    await installJQuery(page);
    await installSettingsHarness(page);
    await page.evaluate(() => window.__settingsA11yHarness.init());

    await expect(page.locator('#amount_gen')).toHaveAccessibleName('Response tokens');
    await expect(page.locator('#amount_gen')).toHaveAttribute('aria-valuetext', '80');
    await expect(page.locator('#amount_gen_counter')).toHaveAccessibleName('Response tokens value');
    await page.locator('#amount_gen').evaluate(slider => {
        slider.value = '120';
        slider.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await expect(page.locator('#amount_gen')).toHaveAttribute('aria-valuetext', '120');

    await page.locator('#settingsSearch').fill('needle');
    await expect(page.locator('#settingsSearchStatus')).toContainText('2 matching settings.');

    const description = /Display the response bit by bit as it is generated\.\s+When this is off, responses will be displayed all at once when they are complete\./;
    await expect(page.locator('#stream_toggle')).toHaveAccessibleName('Streaming');
    await expect(page.locator('#stream_toggle')).toHaveAccessibleDescription(description);
    await expect(page.locator('#streaming_textgenerationwebui')).toHaveAccessibleDescription(description);
    await expect(page.locator('#streaming_kobold')).toHaveAccessibleDescription(description);
    await expect(page.locator('#streaming_novel')).toHaveAccessibleDescription(description);
    await expectNoAxeViolations(page, '#fixture');

    await page.evaluate(() => window.__settingsA11yHarness.setEnabled(false));
    await expect(page.locator('#stream_toggle')).not.toHaveAccessibleDescription(description);
    await expect(page.locator('#streaming_kobold_screen_reader_description')).toHaveCount(0);
});

test('workspace landmarks and message controls expose the semantic surface from the a11y refactor', async ({ page }) => {
    await setHarnessContent(page, `
        <div id="fixture">
            <div id="top-settings-holder" role="banner" aria-label="Application navigation">Navigation</div>
            <aside id="left-nav-panel" role="region" aria-label="AI response configuration">Sampling controls</aside>
            <aside id="right-nav-panel" role="region" aria-label="Character management">Characters</aside>
            <main id="sheld" role="main" aria-label="Chat workspace">
                <section id="chat" role="region" aria-label="Chat messages">
                    <article class="mes" aria-label="Message 2, assistant, Sent 10:30 AM, Swipe 1 of 3">
                        <div class="mes_buttons">
                            <span class="mes_button" role="button" tabindex="0" aria-label="Copy message"></span>
                            <span class="mes_button" role="button" tabindex="0" aria-label="Edit message"></span>
                            <span class="mes_button" role="button" tabindex="0" aria-label="Delete message"></span>
                        </div>
                        <textarea aria-label="Editing message 2"></textarea>
                        <span class="swipe_left" role="button" tabindex="0" aria-label="Previous swipe">Previous</span>
                        <span class="swipe_right" role="button" tabindex="0" aria-label="Next swipe">Next</span>
                    </article>
                </section>
                <form id="send_form" role="form" aria-label="Message composer">
                    <span id="mes_stop" class="mes_stop interactable" title="Abort request" role="button"
                        aria-label="Abort request" aria-hidden="true" aria-disabled="true"></span>
                </form>
            </main>
        </div>
    `);

    await expect(page.getByRole('banner', { name: 'Application navigation' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'AI response configuration' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Character management' })).toBeVisible();
    await expect(page.getByRole('main', { name: 'Chat workspace' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Chat messages' })).toBeVisible();
    await expect(page.getByRole('form', { name: 'Message composer' })).toBeVisible();
    await expect(page.getByRole('article', { name: /Message 2, assistant/ })).toBeVisible();
    await expect(page.getByRole('textbox', { name: 'Editing message 2' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Previous swipe' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Next swipe' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Abort request' })).toHaveCount(0);

    await page.locator('#mes_stop').evaluate(stopButton => {
        stopButton.setAttribute('aria-hidden', 'false');
        stopButton.setAttribute('aria-disabled', 'false');
    });
    await expect(page.getByRole('button', { name: 'Abort request' })).toHaveAttribute('aria-disabled', 'false');
    await expectNoAxeViolations(page, '#fixture');
});

test('Prompt Manager controls have prompt-specific names and status text in the accessibility tree', async ({ page }) => {
    await setHarnessContent(page, `
        <section id="fixture" aria-label="Prompt Manager">
            <ul id="completion_prompt_manager_list" role="list">
                <li class="completion_prompt_manager_prompt completion_prompt_manager_prompt_draggable"
                    data-pm-identifier="main" data-pm-name="Main Prompt" role="listitem">
                    <span class="drag-handle">☰</span>
                    <span class="completion_prompt_manager_prompt_name">
                        <a title="Main Prompt" class="prompt-manager-inspect-action interactable"
                            role="button" tabindex="0" aria-label="Inspect prompt Main Prompt">Main Prompt</a>
                    </span>
                    <span class="prompt_manager_prompt_controls">
                        <span title="Move prompt Main Prompt up" aria-label="Move prompt Main Prompt up"
                            aria-disabled="true" data-pm-direction="up"
                            class="prompt-manager-move-up-action menu_button menu_button_icon disabled"></span>
                        <span title="Move prompt Main Prompt down" aria-label="Move prompt Main Prompt down"
                            aria-disabled="false" data-pm-direction="down"
                            class="prompt-manager-move-down-action menu_button menu_button_icon"></span>
                        <span title="Remove prompt Main Prompt" class="prompt-manager-detach-action interactable"
                            role="button" tabindex="0" aria-label="Remove prompt Main Prompt"></span>
                        <span title="Edit prompt Main Prompt" class="prompt-manager-edit-action interactable"
                            role="button" tabindex="0" aria-label="Edit prompt Main Prompt"></span>
                        <span title="Disable prompt Main Prompt" class="prompt-manager-toggle-action interactable"
                            role="button" tabindex="0" aria-label="Disable prompt Main Prompt" aria-pressed="true"></span>
                    </span>
                </li>
            </ul>
            <div id="completion_prompt_manager_sort_status" class="sr-only" role="status" aria-live="polite" aria-atomic="true">
                Moved Main Prompt to position 2 of 3.
            </div>
        </section>
    `);
    await installKeyboard(page);

    await expect(page.getByRole('button', { name: 'Inspect prompt Main Prompt' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Edit prompt Main Prompt' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Remove prompt Main Prompt' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Disable prompt Main Prompt' })).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByRole('button', { name: 'Move prompt Main Prompt up' })).toHaveAttribute('aria-disabled', 'true');
    await expect(page.getByRole('button', { name: 'Move prompt Main Prompt down' })).toHaveAttribute('tabindex', '0');
    await expect(page.getByRole('status')).toContainText('Moved Main Prompt to position 2 of 3.');
    await expectNoAxeViolations(page, '#fixture');
});

test('Quick Reply and Regex sort controls expose owner-rendered keyboard sorting semantics', async ({ page }) => {
    await setHarnessContent(page, `
        <section id="fixture" aria-label="Extension sorting">
            <div aria-label="Quick Reply sets">
                <ul role="list">
                    <li role="listitem">
                        <span>Combat helpers</span>
                        <span class="qr--moveSetUp menu_button disabled" title="Move quick reply set up"></span>
                        <span class="qr--moveSetDown menu_button" title="Move quick reply set down"></span>
                    </li>
                </ul>
            </div>
            <div aria-label="Quick Replies">
                <ul role="list">
                    <li role="listitem">
                        <span>Ask for detail</span>
                        <span class="qr--moveUp menu_button disabled" title="Move quick reply up"></span>
                        <span class="qr--moveDown menu_button" title="Move quick reply down"></span>
                    </li>
                </ul>
                <div id="quickReplySortStatus" class="sr-only" role="status" aria-live="polite" aria-atomic="true"></div>
            </div>
            <div aria-label="Regex scripts">
                <ul role="list">
                    <li role="listitem">
                        <span>Trim whitespace</span>
                        <span class="move_regex_up menu_button disabled" title="Move script up" aria-label="Move script up"></span>
                        <span class="move_regex_down menu_button" title="Move script down" aria-label="Move script down"></span>
                    </li>
                </ul>
                <div id="regexSortStatus" class="sr-only" role="status" aria-live="polite" aria-atomic="true"></div>
            </div>
        </section>
    `);
    await installKeyboard(page);
    await page.evaluate(() => {
        document.querySelector('.qr--moveSetDown').addEventListener('click', () => {
            document.getElementById('quickReplySortStatus').textContent = 'Moved to position 2 of 3.';
        });
        document.querySelector('.qr--moveDown').addEventListener('click', () => {
            document.getElementById('quickReplySortStatus').textContent = 'Moved to position 3 of 4.';
        });
        document.querySelector('.move_regex_down').addEventListener('click', () => {
            document.getElementById('regexSortStatus').textContent = 'Moved to position 2 of 5.';
        });
    });

    await expect(page.getByRole('button', { name: 'Move quick reply set up' })).toHaveAttribute('aria-disabled', 'true');
    const moveQuickReplySetDown = page.getByRole('button', { name: 'Move quick reply set down' });
    await expect(moveQuickReplySetDown).toHaveAttribute('tabindex', '0');
    await moveQuickReplySetDown.focus();
    await page.keyboard.press('Enter');
    await expect(page.locator('#quickReplySortStatus')).toContainText('Moved to position 2 of 3.');

    await expect(page.getByRole('button', { name: 'Move quick reply up' })).toHaveAttribute('aria-disabled', 'true');
    const moveQuickReplyDown = page.getByRole('button', { name: 'Move quick reply down' });
    await moveQuickReplyDown.focus();
    await page.keyboard.press('Space');
    await expect(page.locator('#quickReplySortStatus')).toContainText('Moved to position 3 of 4.');

    await expect(page.getByRole('button', { name: 'Move script up' })).toHaveAttribute('aria-disabled', 'true');
    const moveScriptDown = page.getByRole('button', { name: 'Move script down' });
    await moveScriptDown.focus();
    await page.keyboard.press('Enter');
    await expect(page.locator('#regexSortStatus')).toContainText('Moved to position 2 of 5.');
    await expectNoAxeViolations(page, '#fixture');
});

test('World Info entry kill switch behaves as a named keyboard-operable switch', async ({ page }) => {
    await setHarnessContent(page, `
        <section id="fixture" aria-label="World Info editor">
            <div class="world_entry">
                <span id="entryKillSwitch" class="killSwitch interactable" role="switch" tabindex="0"
                    aria-checked="true" aria-label="World Info entry Memory Anchor"></span>
            </div>
        </section>
    `);
    await installKeyboard(page);
    await page.evaluate(() => {
        const control = document.getElementById('entryKillSwitch');
        control.addEventListener('click', () => {
            control.setAttribute('aria-checked', String(control.getAttribute('aria-checked') !== 'true'));
        });
    });

    const switchControl = page.getByRole('switch', { name: 'World Info entry Memory Anchor' });
    await expect(switchControl).toHaveAttribute('aria-checked', 'true');
    await switchControl.focus();
    await page.keyboard.press('Space');
    await expect(switchControl).toHaveAttribute('aria-checked', 'false');
    await expectNoAxeViolations(page, '#fixture');
});

test('bottom menus move focus into the opened menu instead of leaving it on the trigger', async ({ page }) => {
    await setHarnessContent(page, `
        <section id="fixture" aria-label="Composer controls">
            <button id="options_button" type="button">Open message options</button>
            <div id="options" class="options-content" hidden>
                <a id="option_toggle_AN" role="button" tabindex="0">Author's Note</a>
                <a id="option_toggle_CFG" role="button" tabindex="0">CFG Scale</a>
            </div>
            <button id="extensionsMenuButton" type="button">Extensions</button>
            <div id="extensionsMenu" class="options-content" hidden>
                <div id="attach_file_wand_container" class="list-group-item" role="button" tabindex="0">Attach file</div>
                <div id="sd_wand_container" class="list-group-item" role="button" tabindex="0">Stable Diffusion</div>
            </div>
        </section>
    `);
    await page.evaluate(() => {
        function openMenu(triggerId, menuId) {
            const trigger = document.getElementById(triggerId);
            const menu = document.getElementById(menuId);
            trigger.addEventListener('click', () => {
                menu.hidden = false;
                requestAnimationFrame(() => {
                    const firstItem = menu.querySelector('[tabindex]');
                    if (!(firstItem instanceof HTMLElement)) {
                        throw new Error(`Missing focus target in #${menuId}`);
                    }
                    firstItem.focus();
                });
            });
        }
        openMenu('options_button', 'options');
        openMenu('extensionsMenuButton', 'extensionsMenu');
    });

    await page.getByRole('button', { name: 'Open message options' }).click();
    await expect(page.getByRole('button', { name: "Author's Note" })).toBeFocused();
    await page.getByRole('button', { name: 'Extensions' }).click();
    await expect(page.getByRole('button', { name: 'Attach file' })).toBeFocused();
    await expectNoAxeViolations(page, '#fixture');
});

test('expandable prompt drawers expose named disclosure controls and expanded state', async ({ page }) => {
    await setHarnessContent(page, `
        <section id="fixture" aria-label="Prompt drawers">
            <div class="inline-drawer">
                <div id="quick_prompts_header" class="inline-drawer-toggle inline-drawer-header interactable"
                    role="button" tabindex="0" aria-controls="quick_prompts_content" aria-expanded="false">
                    <b>Quick Prompts Edit</b>
                    <span class="inline-drawer-icon" aria-hidden="true"></span>
                </div>
                <div id="quick_prompts_content" class="inline-drawer-content" hidden>
                    <textarea aria-label="Main prompt"></textarea>
                </div>
            </div>
            <div class="inline-drawer">
                <div id="utility_prompts_header" class="inline-drawer-toggle inline-drawer-header interactable"
                    role="button" tabindex="0" aria-controls="utility_prompts_content" aria-expanded="false">
                    <b>Utility Prompts</b>
                    <span class="inline-drawer-icon" aria-hidden="true"></span>
                </div>
                <div id="utility_prompts_content" class="inline-drawer-content" hidden>
                    <textarea aria-label="Impersonation prompt"></textarea>
                </div>
            </div>
        </section>
    `);
    await page.evaluate(() => {
        for (const header of document.querySelectorAll('.inline-drawer-header')) {
            header.addEventListener('click', () => {
                const content = document.getElementById(header.getAttribute('aria-controls'));
                const open = header.getAttribute('aria-expanded') !== 'true';
                header.setAttribute('aria-expanded', String(open));
                content.hidden = !open;
            });
        }
    });

    const quickPrompts = page.getByRole('button', { name: 'Quick Prompts Edit' });
    const utilityPrompts = page.getByRole('button', { name: 'Utility Prompts' });
    await expect(quickPrompts).toHaveAttribute('aria-expanded', 'false');
    await expect(utilityPrompts).toHaveAttribute('aria-expanded', 'false');
    await quickPrompts.click();
    await expect(quickPrompts).toHaveAttribute('aria-expanded', 'true');
    await expect(page.locator('.inline-drawer-icon').first()).toHaveAttribute('aria-hidden', 'true');
    await expectNoAxeViolations(page, '#fixture');
});
