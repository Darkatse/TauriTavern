import { allowlistSettingAllows, getActiveIosPolicyActivationReport } from './ios-policy.js';

const STYLE_ID = 'tt-ios-policy-ui-projection';

function ensureStyleElement() {
    let style = document.getElementById(STYLE_ID);
    if (style instanceof HTMLStyleElement) {
        return style;
    }

    style = document.createElement('style');
    style.id = STYLE_ID;
    document.head.appendChild(style);
    return style;
}

function requireElement(selector, context = document) {
    const element = context.querySelector(selector);
    if (!element) {
        throw new Error(`[TauriTavern][iOSPolicy] Expected element not found: ${selector}`);
    }
    return element;
}

function hideElement(element) {
    if (!(element instanceof HTMLElement)) {
        throw new Error('[TauriTavern][iOSPolicy] hideElement: element must be an HTMLElement');
    }

    element.hidden = true;
    element.setAttribute('data-tt-ios-policy-hidden', '1');
}

function normalizeChatCompletionSourceKey(value) {
    const normalized = String(value || '').trim().toLowerCase();
    if (normalized.startsWith('custom_')) {
        return 'custom';
    }
    return normalized;
}

function filterChatCompletionSourceOptions({ allowlist, endpointOverridesAllowed }) {
    const select = requireElement('#chat_completion_source');
    if (!(select instanceof HTMLSelectElement)) {
        throw new Error('[TauriTavern][iOSPolicy] #chat_completion_source must be a <select>');
    }

    const isAllowed = (value) => {
        const key = normalizeChatCompletionSourceKey(value);
        if (!key) {
            return false;
        }

        if (!endpointOverridesAllowed && key === 'custom') {
            return false;
        }

        return allowlistSettingAllows(allowlist, key);
    };

    const options = Array.from(select.options);
    for (const option of options) {
        if (!isAllowed(option.value)) {
            option.remove();
        }
    }

    if (select.options.length === 0) {
        throw new Error('[TauriTavern][iOSPolicy] Chat completion source list became empty after applying iOS policy');
    }

    if (!isAllowed(select.value)) {
        select.value = select.options[0].value;
    }
}

export function applyIosPolicyUiProjection() {
    const report = getActiveIosPolicyActivationReport();
    const caps = report?.capabilities ?? null;
    if (!caps) {
        return;
    }

    if (!(document.body instanceof HTMLBodyElement)) {
        throw new Error('[TauriTavern][iOSPolicy] document.body is unavailable');
    }

    document.body.dataset.ttIosPolicyScope = 'ios';
    if (typeof report.profile !== 'string' || !report.profile.trim()) {
        throw new Error('[TauriTavern][iOSPolicy] Active policy profile is missing');
    }
    document.body.dataset.ttIosPolicyProfile = report.profile;

    const cssRules = [];

    if (report.profile === 'ios_external_beta') {
        cssRules.push(`
            body[data-tt-ios-policy-scope="ios"][data-tt-ios-policy-profile="ios_external_beta"] #ttv-compat-row,
            body[data-tt-ios-policy-scope="ios"][data-tt-ios-policy-profile="ios_external_beta"] #ttv-discord-link,
            body[data-tt-ios-policy-scope="ios"][data-tt-ios-policy-profile="ios_external_beta"] #tt-welcome-discord-link {
                display: none !important;
            }
        `);
    }

    if (caps.extensions?.third_party_management === false) {
        document.body.dataset.ttIosPolicyNoThirdPartyExtensions = '1';
        cssRules.push(`
            body[data-tt-ios-policy-scope="ios"][data-tt-ios-policy-no-third-party-extensions="1"] #extensions_details,
            body[data-tt-ios-policy-scope="ios"][data-tt-ios-policy-no-third-party-extensions="1"] #third_party_extension_button,
            body[data-tt-ios-policy-scope="ios"][data-tt-ios-policy-no-third-party-extensions="1"] label[for="extensions_notify_updates"] {
                display: none !important;
            }
        `);

        hideElement(requireElement('#extensions_details'));
        hideElement(requireElement('#third_party_extension_button'));
        hideElement(requireElement('label[for="extensions_notify_updates"]'));
    }

    if (caps.content?.external_import === false) {
        document.body.dataset.ttIosPolicyNoExternalImport = '1';
        cssRules.push(`
            body[data-tt-ios-policy-scope="ios"][data-tt-ios-policy-no-external-import="1"] #external_import_button,
            body[data-tt-ios-policy-scope="ios"][data-tt-ios-policy-no-external-import="1"] .external_import_button {
                display: none !important;
            }
        `);

        hideElement(requireElement('#external_import_button'));
        for (const element of document.querySelectorAll('.external_import_button')) {
            if (element instanceof HTMLElement) {
                hideElement(element);
            }
        }

        const onboardingRoot = requireElement('#onboarding_template .onboarding');
        hideElement(requireElement('ul.justifyLeft.marginTopBot5', onboardingRoot));
        hideElement(requireElement('.textAlignCenter', onboardingRoot));
    }

    if (caps.prompts?.nsfw_prompt === false) {
        const textarea = requireElement('#nsfw_prompt_quick_edit_textarea');
        const container = textarea.closest('.range-block');
        if (!(container instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] NSFW prompt container not found');
        }
        hideElement(container);
    }

    if (caps.prompts?.jailbreak_prompt === false) {
        const textarea = requireElement('#jailbreak_prompt_quick_edit_textarea');
        const container = textarea.closest('.range-block');
        if (!(container instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] Jailbreak prompt container not found');
        }
        hideElement(container);

        const preferJailbreak = requireElement('#prefer_character_jailbreak');
        const label = preferJailbreak.closest('label');
        if (!(label instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] Jailbreak preference label not found');
        }
        hideElement(label);
    }

    const endpointOverridesAllowed = caps.llm?.endpoint_overrides !== false;
    filterChatCompletionSourceOptions({
        allowlist: caps.llm?.chat_completion_sources?.allowlist,
        endpointOverridesAllowed,
    });

    if (!endpointOverridesAllowed) {
        const reverseProxyUrl = requireElement('#openai_reverse_proxy');
        const reverseProxyDrawer = reverseProxyUrl.closest('.inline-drawer');
        if (!(reverseProxyDrawer instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] Reverse proxy drawer not found');
        }
        hideElement(reverseProxyDrawer);
        hideElement(requireElement('#ReverseProxyWarningMessage'));
        hideElement(requireElement('#ReverseProxyWarningMessage2'));
        hideElement(requireElement('#custom_form'));
    }

    if (caps.llm?.chat_completion_features?.web_search === false) {
        const checkbox = requireElement('#openai_enable_web_search');
        const container = checkbox.closest('.range-block');
        if (!(container instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] Web search settings container not found');
        }
        hideElement(container);
    }

    if (caps.llm?.chat_completion_features?.request_images === false) {
        hideElement(requireElement('#request_images_block'));
    }

    if (caps.llm?.text_completions?.enabled === false) {
        const textCompletionOption = requireElement('#main_api option[value="textgenerationwebui"]');
        textCompletionOption.remove();
        hideElement(requireElement('#textgenerationwebui_api'));
    }

    if (caps.ai?.image_generation === false) {
        hideElement(requireElement('#bg_chat_hint'));
    }

    const style = ensureStyleElement();
    style.textContent = cssRules.join('\n');
}
