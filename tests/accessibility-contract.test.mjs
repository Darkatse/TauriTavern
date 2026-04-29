import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { init, parse } from 'es-module-lexer';
import { setTimeout as delay } from 'node:timers/promises';
import { installFakeDom } from './helpers/fake-dom.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SCREEN_READER_MODULE_PATH = path.join(REPO_ROOT, 'src/scripts/a11y/screen-reader.js');
const EVENTS_MODULE_URL = pathToFileURL(path.join(REPO_ROOT, 'src/scripts/events.js')).href;
const ACCESSIBILITY_I18N_KEYS = [
    'Application navigation',
    'AI response configuration',
    'Main API',
    'Search Settings',
    'Search settings',
    'Announces generation status and important chat updates for screen readers. May add a small performance cost.',
    'Screen Reader Assistance',
    'Character management',
    'Previous swipe',
    'Copy message',
    'Open checkpoint chat',
    'Open checkpoint chat&#10;Shift+Click to replace the existing checkpoint with a new one',
    'Edit message',
    'Save edit',
    'Cancel edit',
    'Copy edited message',
    'Delete message',
    'Next swipe',
    'Accessibility option: Screen Reader Assistance for low-vision users',
    'Helpful for low-vision users. Announces generation status and important chat updates for screen readers. May add a small performance cost. You can turn it off later in User Settings.',
    'Chat workspace',
    'Chat messages',
    'Message composer',
    'Open message options',
    'Message ${0}',
    'narrator',
    'system',
    'user',
    'group member',
    'assistant',
    'Sent ${0}',
    'Swipe ${0} of ${1}',
    'Editing',
    'Editing message ${0}',
    'Move prompt up',
    'Move prompt down',
    'Move quick reply up',
    'Move quick reply down',
    'Move quick reply set up',
    'Move quick reply set down',
    'Move script up',
    'Move script down',
    'Moved to position ${0} of ${1}.',
    'AI is generating a response. Stop button is available.',
    'AI response ready.',
    'Generation stopped.',
    'Generation failed.',
    'API disconnected.',
    'API connected.',
    '1 matching setting.',
    '${0} matching settings.',
    'Response tokens',
    'Context tokens',
    'Chat width',
    'Font scale',
    'Blur strength',
    'Text shadow width',
    '${0} value',
];

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

async function readRepoJson(relativePath) {
    return JSON.parse(await readRepoFile(relativePath));
}

function getI18nPlaceholders(text) {
    return text.match(/\$\{\d+\}/g) ?? [];
}

async function resolveLocalImport(fromFile, specifier) {
    if (!specifier.startsWith('.')) {
        return null;
    }

    const basePath = path.resolve(path.dirname(fromFile), specifier);
    for (const candidate of [basePath, `${basePath}.js`, path.join(basePath, 'index.js')]) {
        try {
            const stats = await stat(candidate);
            if (stats.isFile()) {
                return candidate;
            }
        } catch {
            // Try the next standard ESM resolution candidate.
        }
    }

    throw new Error(`Unable to resolve local import "${specifier}" from ${fromFile}`);
}

async function collectLocalImports(entryRelativePath) {
    await init;

    const entryPath = path.join(REPO_ROOT, entryRelativePath);
    const seen = new Set();
    const stack = [entryPath];

    while (stack.length > 0) {
        const currentPath = stack.pop();
        if (seen.has(currentPath)) {
            continue;
        }
        seen.add(currentPath);

        const source = await readFile(currentPath, 'utf8');
        const [imports] = parse(source);

        for (const entry of imports) {
            const specifier = source.slice(entry.s, entry.e);
            const resolved = await resolveLocalImport(currentPath, specifier);
            if (resolved) {
                stack.push(resolved);
            }
        }
    }

    seen.delete(entryPath);
    return new Set([...seen].map(filePath => path.relative(REPO_ROOT, filePath).replaceAll(path.sep, '/')));
}

function getFunctionSlice(source, functionName, nextFunctionName) {
    const start = source.indexOf(`function ${functionName}`);
    assert.ok(start >= 0, `${functionName} should exist`);
    const end = source.indexOf(`function ${nextFunctionName}`, start);
    assert.ok(end > start, `${nextFunctionName} should follow ${functionName}`);
    return source.slice(start, end);
}

async function loadFreshScreenReader() {
    return import(`${pathToFileURL(SCREEN_READER_MODULE_PATH).href}?contract=${Date.now()}-${Math.random()}`);
}

async function getEventSource() {
    return (await import(EVENTS_MODULE_URL)).eventSource;
}

test('login accessibility graph remains shared and does not load the main app', async () => {
    const imports = await collectLocalImports('src/scripts/login.js');

    assert.ok(imports.has('src/scripts/a11y.js'));
    assert.ok(!imports.has('src/script.js'));
    assert.ok(!imports.has('src/scripts/a11y/screen-reader.js'));
});

test('shared a11y core has no app-level imports or public extension surface', async () => {
    await init;
    const source = await readRepoFile('src/scripts/a11y.js');
    const [imports, exports] = parse(source);

    assert.deepEqual(imports.map(entry => source.slice(entry.s, entry.e)), []);
    assert.deepEqual(exports.map(entry => entry.n).sort(), ['initAccessibility']);
    assert.doesNotMatch(source, /\bannounceA11y\b/);
    assert.doesNotMatch(source, /\bsetAccessibilityEnabled\b/);
    assert.doesNotMatch(source, /\bhandleDrawerFocus\b/);
    assert.doesNotMatch(source, /\bregisterA11ySelector\b/);
});

test('main generation and drawer flows no longer call screen-reader helpers directly', async () => {
    const source = await readRepoFile('src/script.js');

    assert.doesNotMatch(source, /\bannounceA11y\b/);
    assert.doesNotMatch(source, /\bsetAccessibilityEnabled\b/);
    assert.doesNotMatch(source, /\bhandleDrawerFocus\b/);
    assert.match(source, /import\s+\{\s*initAccessibility\s*\}\s+from\s+'\.\/scripts\/a11y\.js';/);
    assert.match(source, /import\s+\{\s*initScreenReaderAssistance,\s*setScreenReaderAssistanceEnabled\s*\}\s+from\s+'\.\/scripts\/a11y\/screen-reader\.js';/);
});

test('SillyTavern context does not expose an undocumented a11y API', async () => {
    const source = await readRepoFile('src/scripts/st-context.js');

    assert.doesNotMatch(source, /from\s+'\.\/a11y\.js'/);
    assert.doesNotMatch(source, /\ba11y\s*:/);
});

test('Screen Reader Assistance settings default off for existing users and drop the old broad flag', async () => {
    const source = await readRepoFile('src/scripts/power-user.js');

    assert.match(source, /\bscreen_reader_assistance\s*:\s*false,/);
    assert.doesNotMatch(source, /\baccessibility_mode\s*:\s*true,/);
    assert.match(source, /delete power_user\.accessibility_mode;/);
    assert.match(source, /if \(power_user\.screen_reader_assistance === undefined\) {\s*power_user\.screen_reader_assistance = false;\s*} else if \(typeof power_user\.screen_reader_assistance !== 'boolean'\) {\s*throw new Error\('Invalid power_user\.screen_reader_assistance setting: expected boolean'\);\s*}/s);
    assert.match(source, /function switchScreenReaderAssistance\(\)\s*{\s*\$\(\'#screen_reader_assistance\'\)\.prop\('checked', power_user\.screen_reader_assistance\);\s*setScreenReaderAssistanceEnabled\(power_user\.screen_reader_assistance\);\s*}/s);
});

test('Screen Reader Assistance controls are wired for settings and onboarding', async () => {
    const html = await readRepoFile('src/index.html');
    const script = await readRepoFile('src/script.js');
    const popup = await readRepoFile('src/scripts/popup.js');
    const powerUser = await readRepoFile('src/scripts/power-user.js');

    const settingsControl = html.slice(html.indexOf('for="screen_reader_assistance"'), html.indexOf('for="reduced_motion"'));
    assert.match(settingsControl, /id="screen_reader_assistance"\s+type="checkbox"/);
    assert.doesNotMatch(settingsControl, /\schecked(?:\s|>|=)/);
    assert.match(settingsControl, /screen readers/i);
    assert.match(settingsControl, /performance cost/i);

    const onboardingTemplate = html.slice(html.indexOf('<div id="onboarding_template"'), html.indexOf('<div id="group_member_template"'));
    assert.doesNotMatch(onboardingTemplate, /for="onboarding_screen_reader_assistance"/);

    const popupTemplate = html.slice(html.indexOf('<template id="popup_template"'), html.indexOf('<div id="onboarding_template"'));
    assert.ok(popupTemplate.indexOf('popup-input text_pole') < popupTemplate.indexOf('popup-inputs'));
    assert.match(script, /const ONBOARDING_SCREEN_READER_ASSISTANCE_INPUT_ID = 'onboarding_screen_reader_assistance';/);
    assert.match(script, /const ONBOARDING_SCREEN_READER_ASSISTANCE_LABEL = 'Accessibility option: Screen Reader Assistance for low-vision users';/);
    assert.match(script, /const ONBOARDING_SCREEN_READER_ASSISTANCE_TOOLTIP = 'Helpful for low-vision users\. Announces generation status and important chat updates for screen readers\. May add a small performance cost\. You can turn it off later in User Settings\.';/);
    assert.match(popup, /tooltip\.classList\.add\('fa-solid', input\.tooltipIcon \?\? 'fa-circle-info', 'opacity50p'\);/);

    const doOnboarding = getFunctionSlice(script, 'doOnboarding', 'reloadLoop');
    assert.match(doOnboarding, /screenReaderAssistanceDefault = true/);
    assert.match(doOnboarding, /customInputs: \[\{\s*id: ONBOARDING_SCREEN_READER_ASSISTANCE_INPUT_ID,\s*label: ONBOARDING_SCREEN_READER_ASSISTANCE_LABEL,\s*tooltip: ONBOARDING_SCREEN_READER_ASSISTANCE_TOOLTIP,\s*tooltipIcon: 'fa-universal-access',\s*defaultState: screenReaderAssistanceDefault,\s*}\],/s);
    assert.match(doOnboarding, /popup\.inputResults\?\.get\(ONBOARDING_SCREEN_READER_ASSISTANCE_INPUT_ID\)/);
    assert.match(doOnboarding, /power_user\.screen_reader_assistance = screenReaderAssistance;/);
    assert.match(doOnboarding, /setScreenReaderAssistanceEnabled\(screenReaderAssistance\);/);
    assert.match(doOnboarding, /saveSettingsDebounced\(\);/);
    assert.match(script, /const hasScreenReaderAssistanceSetting = settings\.power_user && Object\.hasOwn\(settings\.power_user, 'screen_reader_assistance'\);\s*await doOnboarding\(user_avatar, {\s*screenReaderAssistanceDefault: hasScreenReaderAssistanceSetting \? power_user\.screen_reader_assistance : true,\s*}\);/s);

    assert.match(powerUser, /\$\(\'#screen_reader_assistance\'\)\.on\('input', function \(\) {\s*power_user\.screen_reader_assistance = !!\$\(this\)\.prop\('checked'\);\s*switchScreenReaderAssistance\(\);\s*saveSettingsDebounced\(\);\s*}\);/s);
});

test('Screen Reader Assistance lifecycle uses an explicit boolean contract', async () => {
    const dom = installFakeDom();
    const screenReader = await loadFreshScreenReader();

    try {
        assert.equal(screenReader.isScreenReaderAssistanceEnabled(), false);
        assert.throws(() => screenReader.setScreenReaderAssistanceEnabled('true'), TypeError);

        screenReader.setScreenReaderAssistanceEnabled(true);
        assert.equal(screenReader.isScreenReaderAssistanceEnabled(), true);

        screenReader.initScreenReaderAssistance();
        assert.ok(document.getElementById('screen_reader_assistance_live_region'));

        screenReader.setScreenReaderAssistanceEnabled(false);
        assert.equal(screenReader.isScreenReaderAssistanceEnabled(), false);
        assert.equal(document.getElementById('screen_reader_assistance_live_region'), null);
    } finally {
        if (screenReader.isScreenReaderAssistanceEnabled()) {
            screenReader.setScreenReaderAssistanceEnabled(false);
        }
        dom.cleanup();
    }
});

test('Screen Reader Assistance stays lifecycle-only and isolated from interaction semantics', async () => {
    await init;
    const source = await readRepoFile('src/scripts/a11y/screen-reader.js');
    const [imports] = parse(source);

    assert.deepEqual(imports.map(entry => source.slice(entry.s, entry.e)), ['../events.js', '../i18n.js']);
    assert.doesNotMatch(source, /from\s+'(?:\.\.\/)?(?:\.\.\/)?script\.js'/);
    assert.doesNotMatch(source, /\bgetContext\b/);
    assert.doesNotMatch(source, /\bpower_user\b/);
    assert.doesNotMatch(source, /\bwindow\.__TAURITAVERN__\b/);
    assert.doesNotMatch(source, /\b(document|window)\.addEventListener\b/);
    assert.doesNotMatch(source, /\.focus\s*\(/);
    assert.doesNotMatch(source, /\bconsole\.(?:log|debug|info|warn)\b/);
    assert.doesNotMatch(source, /\bMutationObserver\b/);
});

test('Screen Reader Assistance announces only visible generation lifecycle states', async () => {
    const dom = installFakeDom();
    const screenReader = await loadFreshScreenReader();
    const { event_types, eventSource } = await import(EVENTS_MODULE_URL);

    try {
        screenReader.setScreenReaderAssistanceEnabled(true);
        screenReader.initScreenReaderAssistance();

        const liveRegion = document.getElementById('screen_reader_assistance_live_region');
        assert.ok(liveRegion);

        await eventSource.emit(event_types.GENERATION_STARTED, 'normal', {}, false);
        assert.equal(liveRegion.textContent, 'AI is generating a response. Stop button is available.');

        await eventSource.emit(event_types.CHARACTER_MESSAGE_RENDERED, 3, 'normal');
        assert.equal(liveRegion.textContent, 'AI response ready.');

        await eventSource.emit(event_types.GENERATION_STARTED, 'quiet', {}, false);
        assert.equal(liveRegion.textContent, 'AI response ready.');

        await eventSource.emit(event_types.GENERATION_STARTED, 'normal', {}, true);
        assert.equal(liveRegion.textContent, 'AI response ready.');

        await eventSource.emit(event_types.GENERATION_STARTED, 'normal', { quiet_prompt: 'summarize privately', quietToLoud: false }, false);
        assert.equal(liveRegion.textContent, 'AI response ready.');
    } finally {
        if (screenReader.isScreenReaderAssistanceEnabled()) {
            screenReader.setScreenReaderAssistanceEnabled(false);
        }
        dom.cleanup();
    }
});

test('Screen Reader Assistance stop event cancels pending generation state without a ready announcement', async () => {
    const dom = installFakeDom();
    const screenReader = await loadFreshScreenReader();
    const { event_types, eventSource } = await import(EVENTS_MODULE_URL);

    try {
        screenReader.setScreenReaderAssistanceEnabled(true);
        screenReader.initScreenReaderAssistance();

        const liveRegion = document.getElementById('screen_reader_assistance_live_region');
        assert.ok(liveRegion);

        await eventSource.emit(event_types.GENERATION_STARTED, 'normal', {}, false);
        await eventSource.emit(event_types.GENERATION_ENDED, 3);
        await eventSource.emit(event_types.GENERATION_STOPPED);
        await delay(20);

        assert.equal(liveRegion.textContent, 'Generation stopped.');
    } finally {
        if (screenReader.isScreenReaderAssistanceEnabled()) {
            screenReader.setScreenReaderAssistanceEnabled(false);
        }
        dom.cleanup();
    }
});

test('Screen Reader Assistance announces generation failure and connection states without leaking content', async () => {
    const dom = installFakeDom();
    const screenReader = await loadFreshScreenReader();
    const { event_types, eventSource } = await import(EVENTS_MODULE_URL);

    try {
        screenReader.setScreenReaderAssistanceEnabled(true);
        screenReader.initScreenReaderAssistance();

        const liveRegion = document.getElementById('screen_reader_assistance_live_region');
        assert.ok(liveRegion);

        await eventSource.emit(event_types.GENERATION_STARTED, 'normal', { prompt: 'SECRET_PROMPT_TEXT' }, false);
        assert.equal(liveRegion.textContent, 'AI is generating a response. Stop button is available.');

        await eventSource.emit(event_types.GENERATION_FAILED, new Error('SECRET_FAILURE_TEXT'));
        assert.equal(liveRegion.textContent, 'Generation failed.');

        await eventSource.emit(event_types.CHARACTER_MESSAGE_RENDERED, 1, 'normal', 'SECRET_MESSAGE_TEXT');
        assert.equal(liveRegion.textContent, 'Generation failed.');

        await eventSource.emit(event_types.ONLINE_STATUS_CHANGED, 'no_connection');
        assert.equal(liveRegion.textContent, 'API disconnected.');

        await eventSource.emit(event_types.ONLINE_STATUS_CHANGED, 'Connected');
        assert.equal(liveRegion.textContent, 'API connected.');
    } finally {
        if (screenReader.isScreenReaderAssistanceEnabled()) {
            screenReader.setScreenReaderAssistanceEnabled(false);
        }
        dom.cleanup();
    }
});

test('Screen Reader Assistance cleanup removes event listeners and live region', async () => {
    const dom = installFakeDom();
    const screenReader = await loadFreshScreenReader();
    const eventSource = await getEventSource();
    const beforeCounts = Object.fromEntries(Object.entries(eventSource.events).map(([event, listeners]) => [event, listeners.length]));

    try {
        screenReader.setScreenReaderAssistanceEnabled(true);
        screenReader.initScreenReaderAssistance();
        assert.ok(document.getElementById('screen_reader_assistance_live_region'));

        screenReader.setScreenReaderAssistanceEnabled(false);
        assert.equal(document.getElementById('screen_reader_assistance_live_region'), null);

        for (const [event, listeners] of Object.entries(eventSource.events)) {
            assert.equal(listeners.length, beforeCounts[event] ?? 0, `listener count mismatch for ${event}`);
        }
    } finally {
        if (screenReader.isScreenReaderAssistanceEnabled()) {
            screenReader.setScreenReaderAssistanceEnabled(false);
        }
        dom.cleanup();
    }
});

test('Screen Reader Assistance does not log chat content while announcing', async () => {
    const dom = installFakeDom();
    const screenReader = await loadFreshScreenReader();
    const { event_types, eventSource } = await import(EVENTS_MODULE_URL);
    const originalDebug = console.debug;
    const originalLog = console.log;
    const logs = [];

    console.debug = (...args) => logs.push(args.map(String).join(' '));
    console.log = (...args) => logs.push(args.map(String).join(' '));

    try {
        screenReader.setScreenReaderAssistanceEnabled(true);
        screenReader.initScreenReaderAssistance();

        await eventSource.emit(event_types.GENERATION_STARTED, 'normal', { prompt: 'SECRET_PROMPT_TEXT' }, false);
        await eventSource.emit(event_types.CHARACTER_MESSAGE_RENDERED, 1, 'normal', 'SECRET_MESSAGE_TEXT');

        assert.equal(logs.some(line => line.includes('SECRET_PROMPT_TEXT') || line.includes('SECRET_MESSAGE_TEXT')), false);
    } finally {
        console.debug = originalDebug;
        console.log = originalLog;
        if (screenReader.isScreenReaderAssistanceEnabled()) {
            screenReader.setScreenReaderAssistanceEnabled(false);
        }
        dom.cleanup();
    }
});

test('Screen reader smoke coverage defines task-oriented flows with module ownership', async () => {
    const checklist = await readRepoFile('docs/AccessibilitySmokeChecklist.md');
    const plan = await readRepoFile('docs/BlindAccessibilityCompletionPlan.md');

    for (const phrase of [
        'VoiceOver',
        'NVDA',
        'Owner module',
        'onboarding',
        'send a message',
        'stop generation',
        'read the latest reply',
        'message actions',
        'left drawer',
        'right drawer',
        'Settings search',
        'Prompt Manager',
        'Quick Reply',
        'Regex',
    ]) {
        assert.match(checklist, new RegExp(phrase, 'i'));
    }

    assert.match(plan, /AccessibilitySmokeChecklist\.md/);
});

test('Keyboard contract supports named activation without editable-field side effects', async () => {
    const keyboard = await readRepoFile('src/scripts/keyboard.js');
    const html = await readRepoFile('src/index.html');
    const script = await readRepoFile('src/script.js');

    assert.match(keyboard, /\.mes_stop/);
    assert.match(keyboard, /function isActivationKey\([\s\S]*event\.key === 'Enter'[\s\S]*event\.key === ' '[\s\S]*event\.key === 'Spacebar'/);
    assert.match(keyboard, /function isEditableTarget/);
    assert.match(keyboard, /function isNativeKeyboardControl/);
    assert.match(keyboard, /function findKeyboardInteractable/);
    assert.match(keyboard, /setAttribute\('aria-disabled', 'true'\)/);
    assert.match(keyboard, /setAttribute\('aria-label', accessibleName\)/);
    assert.doesNotMatch(keyboard, /\[role=["']button["']\]/);
    assert.doesNotMatch(keyboard, /console\.debug\('Triggering click on keyboard-focused interactable control via Enter'/);

    assert.match(html, /id="options_button"[^>]*title="Open message options"/);
    assert.match(html, /id="mes_stop"[^>]*class="[^"]*\binteractable\b[^"]*"[^>]*title="Abort request"/);
    assert.match(html, /class="[^"]*\bswipe_left\b[^"]*"[^>]*title="Previous swipe"/);
    assert.match(html, /class="[^"]*\bswipe_right\b[^"]*"[^>]*title="Next swipe"/);
    assert.match(html, /class="[^"]*\bmes_bookmark\b[^"]*"[^>]*title="Open checkpoint chat"/);
    assert.match(script, /\.attr\('aria-label', t`Swipe \$\{currentSwipe\} of \$\{totalSwipes\}`\)/);
});

test('Main landmarks and owner-managed disclosure state are exposed', async () => {
    const html = await readRepoFile('src/index.html');
    const script = await readRepoFile('src/script.js');
    const utils = await readRepoFile('src/scripts/utils.js');
    const backgrounds = await readRepoFile('src/scripts/backgrounds.js');

    assert.match(html, /id="top-settings-holder"[^>]*role="banner"[^>]*aria-label="Application navigation"/);
    assert.match(html, /id="left-nav-panel"[^>]*role="region"[^>]*aria-label="AI response configuration"/);
    assert.match(html, /id="right-nav-panel"[^>]*aria-label="Character management"/);
    assert.match(html, /id="sheld"[^>]*role="main"[^>]*aria-label="Chat workspace"/);
    assert.match(html, /id="chat"[^>]*role="region"[^>]*aria-label="Chat messages"/);
    assert.match(html, /id="send_form"[^>]*role="form"[^>]*aria-label="Message composer"/);
    assert.doesNotMatch(html, /id="chat"[^>]*aria-live=/);

    assert.match(script, /function syncNavbarDrawerAccessibility/);
    assert.match(script, /setAttribute\('aria-controls', content\.id\)/);
    assert.match(script, /setAttribute\('aria-expanded', String\(isNavbarDrawerOpen\(drawerToggle\)\)\)/);
    assert.match(script, /syncAllNavbarDrawerAccessibility\(\);/);
    assert.match(script, /syncInlineDrawerAccessibility\(drawerEl, open\);/);
    assert.doesNotMatch(script, /inline-drawer-toggle:[\s\S]*console\.debug/);

    assert.match(utils, /export function syncInlineDrawerAccessibility/);
    assert.match(utils, /throw new Error\('inline drawer accessibility requires/);
    assert.match(utils, /setAttribute\('aria-controls', content\.id\)/);
    assert.match(utils, /setAttribute\('aria-expanded', String\(open\)\)/);
    assert.doesNotMatch(utils, /console\.debug\('toggleDrawer: No icon or content found/);

    assert.match(backgrounds, /function syncBackgroundTabAccessibility/);
    assert.match(backgrounds, /setAttribute\('aria-selected', selected \? 'true' : 'false'\)/);
    assert.match(backgrounds, /\$\(BG_TABS_SELECTOR\)\.on\('tabsactivate', syncBackgroundTabAccessibility\)\.tabs\(\);/);
});

test('Chat message semantics are exposed without turning streamed content into a live region', async () => {
    const html = await readRepoFile('src/index.html');
    const script = await readRepoFile('src/script.js');

    const messageTemplate = html.slice(html.indexOf('<div id="message_template"'), html.indexOf('<div id="group_avatars_template"'));

    assert.match(messageTemplate, /class="mes"[^>]*role="article"/);
    assert.doesNotMatch(messageTemplate, /class="mes_text"[^>]*aria-live=/);
    assert.match(messageTemplate, /title="Copy message"/);
    assert.match(messageTemplate, /title="Edit message"/);
    assert.match(messageTemplate, /title="Save edit"/);
    assert.match(messageTemplate, /title="Cancel edit"/);
    assert.match(messageTemplate, /title="Delete message"/);

    assert.match(script, /function syncMessageAccessibility/);
    assert.match(script, /setAttribute\('aria-label', parts\.join\(', '\)\)/);
    assert.match(script, /getMessageAccessibilityType\(message\)/);
    assert.match(script, /syncMessageAccessibility\(messageElement, mes, messageId\);/);
    assert.match(script, /syncMessageAccessibility\(swipeCounter\.closest\('\.mes'\), message, mesId\);/);
    assert.match(script, /editTextArea\.setAttribute\('aria-label', t`Editing message \$\{editMessageId \+ 1\}`\);/);
    assert.match(script, /messageElement\.attr\('data-editing', 'true'\);/);
    assert.match(script, /thisMesDiv\.removeAttr\('data-editing'\);/);
}
);

test('Generation controls expose stop and failure state without focus stealing', async () => {
    const events = await readRepoFile('src/scripts/events.js');
    const screenReader = await readRepoFile('src/scripts/a11y/screen-reader.js');
    const script = await readRepoFile('src/script.js');

    assert.match(events, /GENERATION_FAILED: 'generation_failed'/);
    assert.match(screenReader, /function onGenerationFailed/);
    assert.match(screenReader, /announce\(t`Generation failed\.`\);/);
    assert.match(screenReader, /listen\(event_types\.GENERATION_FAILED, onGenerationFailed\);/);
    assert.match(screenReader, /t`AI is generating a response\. Stop button is available\.`/);
    assert.match(screenReader, /listen\(event_types\.ONLINE_STATUS_CHANGED, onOnlineStatusChanged\);/);
    assert.match(script, /function showStopButton\(\)\s*{\s*\$\(\'#mes_stop\'\)\.css\(\{ 'display': 'flex' \}\)\.attr\(\{ 'aria-hidden': 'false', 'aria-disabled': 'false' \}\);/s);
    assert.match(script, /function hideStopButton\(\)\s*{\s*\/\/ prevent NOOP[\s\S]*\.attr\(\{ 'aria-hidden': 'true', 'aria-disabled': 'true' \}\);[\s\S]*eventSource\.emit\(event_types\.GENERATION_ENDED, chat\.length\);/);
    assert.match(script, /eventSource\.emit\(event_types\.GENERATION_FAILED\);/);
    assert.doesNotMatch(script, /mes_stop[\s\S]{0,120}\.focus\s*\(/);
});

test('Settings search and key form controls expose names and live result state', async () => {
    const html = await readRepoFile('src/index.html');
    const settingsSearch = await readRepoFile('src/scripts/setting-search.js');

    assert.match(html, /id="settingsSearch"[^>]*aria-label="Search settings"[^>]*aria-controls="user-settings-block-content"[^>]*aria-describedby="settingsSearchStatus"/);
    assert.match(html, /id="settingsSearchStatus"[^>]*class="sr-only"[^>]*role="status"[^>]*aria-live="polite"[^>]*aria-atomic="true"/);
    assert.match(html, /id="main_api"[^>]*aria-label="Main API"/);

    for (const id of ['amount_gen', 'max_context', 'chat_width_slider', 'font_scale', 'blur_strength', 'shadow_width']) {
        assert.match(settingsSearch, new RegExp(`syncRangePair\\('${id}'`));
    }

    assert.match(settingsSearch, /function syncRangeValueText/);
    assert.match(settingsSearch, /range\.setAttribute\('aria-valuetext', String\(range\.value\)\);/);
    assert.match(settingsSearch, /import\s+\{\s*t\s*\}\s+from\s+'\.\/i18n\.js';/);
    assert.match(settingsSearch, /counter\.setAttribute\('aria-label', t`\$\{label\} value`\);/);
    assert.match(settingsSearch, /settingsSearchStatus\.textContent = highlightedCount === 1 \? t`1 matching setting\.` : t`\$\{highlightedCount\} matching settings\.`;/);
    assert.match(settingsSearch, /\$\(document\)\.on\('input change', SETTINGS_RANGE_SELECTOR, function \(\) \{/);
    assert.match(settingsSearch, /initSettingsFormAccessibility\(\);/);
});

test('Accessibility user-facing strings are translated for Chinese locales', async () => {
    for (const locale of ['zh-cn', 'zh-tw']) {
        const localeData = await readRepoJson(`src/locales/${locale}.json`);

        for (const key of ACCESSIBILITY_I18N_KEYS) {
            const value = localeData[key];
            assert.equal(typeof value, 'string', `${locale} missing translation for "${key}"`);
            assert.notEqual(value.trim(), '', `${locale} translation is empty for "${key}"`);
            assert.notEqual(value, key, `${locale} translation still matches source text for "${key}"`);
            assert.deepEqual(getI18nPlaceholders(value), getI18nPlaceholders(key), `${locale} placeholder mismatch for "${key}"`);
        }
    }
});

test('Complex UI assistance does not directly reorder business DOM', async () => {
    const screenReader = await readRepoFile('src/scripts/a11y/screen-reader.js');
    const a11yCore = await readRepoFile('src/scripts/a11y.js');
    const combined = `${screenReader}\n${a11yCore}`;

    assert.doesNotMatch(combined, /\ba11y-sort-button\b/);
    assert.doesNotMatch(combined, /\bcompletion_prompt_manager_prompt\b/);
    assert.doesNotMatch(combined, /\bqr--(?:item|set-item)\b/);
    assert.doesNotMatch(combined, /\bregex-script-label\b/);
    assert.doesNotMatch(combined, /\.sortable\(/);
    assert.doesNotMatch(combined, /\bsortupdate\b/);
    assert.doesNotMatch(combined, /\b(insertBefore|insertAfter)\b/);
    assert.doesNotMatch(screenReader, /\bMutationObserver\b/);
});

test('Complex list sorting exposes owner APIs', async () => {
    const promptManager = await readRepoFile('src/scripts/PromptManager.js');
    const quickReplyConfig = await readRepoFile('src/scripts/extensions/quick-reply/src/QuickReplyConfig.js');
    const quickReplySet = await readRepoFile('src/scripts/extensions/quick-reply/src/QuickReplySet.js');
    const regex = await readRepoFile('src/scripts/extensions/regex/index.js');

    assert.match(promptManager, /async movePromptInActiveOrder\(identifier, direction\)/);
    assert.match(promptManager, /this\.getPromptOrderForCharacter\(this\.activeCharacter\)/);
    assert.match(promptManager, /this\.removePromptOrderForCharacter\(this\.activeCharacter\);/);
    assert.match(promptManager, /this\.addPromptOrderForCharacter\(this\.activeCharacter, updatedPromptOrder\);/);
    assert.match(promptManager, /await this\.saveServiceSettings\(\);/);
    assert.match(promptManager, /await this\.renderPromptManagerListItems\(\);/);

    assert.match(quickReplyConfig, /moveSetLink\(index, direction\)/);
    assert.match(quickReplyConfig, /this\.setList\.splice\(targetIndex, 0, this\.setList\.splice\(index, 1\)\[0\]\);/);
    assert.match(quickReplyConfig, /this\.update\(\);/);
    assert.match(quickReplyConfig, /this\.updateSetListDom\(\);/);

    assert.match(quickReplySet, /moveQuickReply\(id, direction\)/);
    assert.match(quickReplySet, /this\.qrList\.splice\(targetIndex, 0, this\.qrList\.splice\(index, 1\)\[0\]\);/);
    assert.match(quickReplySet, /this\.save\(\);/);
    assert.match(quickReplySet, /this\.rerenderSettings\(\);/);

    assert.match(regex, /export async function moveRegexScriptWithinType\(scriptId, scriptType, direction\)/);
    assert.match(regex, /const scripts = getScriptsByType\(scriptType\);/);
    assert.match(regex, /await saveScriptsByType\(updatedScripts, scriptType\);/);
    assert.match(regex, /saveSettingsDebounced\(\);/);
    assert.match(regex, /requestRegexChatRefresh\(\);/);
    assert.match(regex, /await loadRegexScripts\(\);/);
});

test('Module-owned keyboard sorting controls are rendered by their owner modules', async () => {
    const events = await readRepoFile('src/scripts/events.js');
    const screenReader = await readRepoFile('src/scripts/a11y/screen-reader.js');
    const promptManager = await readRepoFile('src/scripts/PromptManager.js');
    const promptManagerCss = await readRepoFile('src/css/promptmanager.css');
    const quickReplyIndex = await readRepoFile('src/scripts/extensions/quick-reply/index.js');
    const quickReplyConfig = await readRepoFile('src/scripts/extensions/quick-reply/src/QuickReplyConfig.js');
    const quickReplySetLink = await readRepoFile('src/scripts/extensions/quick-reply/src/QuickReplySetLink.js');
    const quickReplySet = await readRepoFile('src/scripts/extensions/quick-reply/src/QuickReplySet.js');
    const quickReply = await readRepoFile('src/scripts/extensions/quick-reply/src/QuickReply.js');
    const regexTemplate = await readRepoFile('src/scripts/extensions/regex/scriptTemplate.html');
    const regex = await readRepoFile('src/scripts/extensions/regex/index.js');

    assert.match(events, /SCREEN_READER_ASSISTANCE_CHANGED: 'screen_reader_assistance_changed'/);
    assert.match(screenReader, /eventSource\.emit\(event_types\.SCREEN_READER_ASSISTANCE_CHANGED, enabled\);/);

    assert.match(promptManager, /from '\.\/a11y\/screen-reader\.js'/);
    assert.match(promptManager, /const showScreenReaderSortUi = isScreenReaderAssistanceEnabled\(\);/);
    assert.match(promptManager, /prompt-manager-move-up-action/);
    assert.match(promptManager, /const moveUpSpanHtml = showScreenReaderSortUi \?/);
    assert.match(promptManager, /if \(showScreenReaderSortUi\) {\s*Array\.from\(promptManagerList\.querySelectorAll\('\.prompt-manager-move-up-action, \.prompt-manager-move-down-action'\)\)/s);
    assert.match(promptManager, /prompt-manager-move-down-action/);
    assert.match(promptManager, /function ensurePromptSortStatus/);
    assert.match(promptManager, /function focusPromptSortControl/);
    assert.match(promptManager, /eventSource\.on\(event_types\.SCREEN_READER_ASSISTANCE_CHANGED, \(\) => this\.renderDebounced\(false\)\);/);
    assert.match(promptManagerCss, /#completion_prompt_manager #completion_prompt_manager_list li\s*{\s*display: grid;\s*grid-template-columns: 4fr 80px 45px;/s);
    assert.match(promptManagerCss, /\.completion_prompt_manager_sort_assist li\s*{\s*grid-template-columns: 4fr 120px 45px;/s);

    assert.match(quickReplyIndex, /eventSource\.on\(event_types\.SCREEN_READER_ASSISTANCE_CHANGED, \(\) => {\s*QuickReplySet\.list\.forEach\(qrs => qrs\.unrenderSettings\(\)\);\s*manager\.rerender\(\);/s);
    assert.match(quickReplyConfig, /from '\.\.\/\.\.\/\.\.\/a11y\/screen-reader\.js'/);
    assert.match(quickReplyConfig, /from '\.\.\/\.\.\/\.\.\/i18n\.js'/);
    assert.match(quickReplyConfig, /const showScreenReaderSortUi = isScreenReaderAssistanceEnabled\(\);/);
    assert.match(quickReplySetLink, /qr--moveSetUp/);
    assert.match(quickReplySetLink, /from '\.\.\/\.\.\/\.\.\/i18n\.js'/);
    assert.match(quickReplySetLink, /t`Move quick reply set up`/);
    assert.match(quickReplySetLink, /renderSettings\(idx, total = idx \+ 1, showScreenReaderSortUi = false, visualIndex = idx\)/);
    assert.match(quickReplySetLink, /if \(showScreenReaderSortUi\) {/);
    assert.match(quickReplySetLink, /t`Move quick reply set down`/);
    assert.match(quickReplySetLink, /qr--moveSetDown/);
    assert.match(quickReplyConfig, /moveSetLinkFromUi\(index, direction\)/);
    assert.match(quickReplyConfig, /announceSortPosition\(result\.position, result\.total\);/);

    assert.match(quickReplySet, /from '\.\.\/\.\.\/\.\.\/a11y\/screen-reader\.js'/);
    assert.match(quickReplySet, /from '\.\.\/\.\.\/\.\.\/i18n\.js'/);
    assert.match(quickReplySet, /qr\.renderSettings\(idx, this\.qrList\.length, isScreenReaderAssistanceEnabled\(\)\)/);
    assert.match(quickReplySet, /unrenderSettings\(\)\s*{\s*this\.settingsDom\?\.remove\(\);/);
    assert.match(quickReply, /qr--moveUp/);
    assert.match(quickReply, /from '\.\.\/\.\.\/\.\.\/i18n\.js'/);
    assert.match(quickReply, /t`Move quick reply up`/);
    assert.match(quickReply, /renderSettings\(idx, total = idx \+ 1, showScreenReaderSortUi = false\)/);
    assert.match(quickReply, /if \(showScreenReaderSortUi\) {/);
    assert.match(quickReply, /t`Move quick reply down`/);
    assert.match(quickReply, /qr--moveDown/);
    assert.match(quickReplySet, /moveQuickReplyFromUi\(id, direction\)/);
    assert.match(quickReplySet, /focusQuickReplySortControl\(result\.id, direction\);/);

    assert.match(regexTemplate, /move_regex_up menu_button/);
    assert.match(regexTemplate, /data-i18n="\[title\]Move script up;\[aria-label\]Move script up"/);
    assert.match(regex, /from '\.\.\/\.\.\/a11y\/screen-reader\.js'/);
    assert.match(regex, /const showScreenReaderSortUi = isScreenReaderAssistanceEnabled\(\);/);
    assert.match(regex, /if \(!showScreenReaderSortUi\) {\s*moveUp\.remove\(\);\s*moveDown\.remove\(\);/s);
    assert.match(regexTemplate, /move_regex_down menu_button/);
    assert.match(regexTemplate, /data-i18n="\[title\]Move script down;\[aria-label\]Move script down"/);
    assert.match(regex, /function onRegexSortButtonClick/);
    assert.match(regex, /announceRegexSortPosition\(result\.position, result\.total\);/);
    assert.match(regex, /focusRegexSortControl\(result\.id, direction\);/);
    assert.match(regex, /eventSource\.on\(event_types\.SCREEN_READER_ASSISTANCE_CHANGED, async \(\) => await loadRegexScripts\(\)\);/);
});
