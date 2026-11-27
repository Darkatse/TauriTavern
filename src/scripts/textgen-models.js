import { getPageForItem, initPagination } from './pagination.js';
import { DOMPurify } from '../lib.js';
import { isMobile } from './RossAscends-mods.js';
import { amount_gen, eventSource, event_types, getRequestHeaders, isConnectionValidationSuspended, max_context, online_status, setGenerationParamsFromPreset } from '../script.js';
import { textgenerationwebui_settings as textgen_settings, textgen_types } from './textgen-settings.js';
import { tokenizers } from './tokenizers.js';
import { renderTemplateAsync } from './templates.js';
import { POPUP_TYPE, callGenericPopup } from './popup.js';
import { t } from './i18n.js';
import { keepFocus, showNumericAdjustments } from './dom-handlers.js';
import { textValueMatcher } from './utils.js';
import { getNanoGptModelProviders, getOpenRouterModelProviders, getOpenRouterProviders } from '../tauri-bridge.js';

let mancerModels = [];
let togetherModels = [];
let infermaticAIModels = [];
let dreamGenModels = [];
let vllmModels = [];
let aphroditeModels = [];
let featherlessModels = [];
let tabbyModels = [];
let llamacppModels = [];
export let openRouterModels = [];

// Model select handlers normally reconnect immediately; profile replay defers that to the final validation.
function reconnectTextGenIfValidationAllowed() {
    if (!isConnectionValidationSuspended()) {
        $('#api_button_textgenerationwebui').trigger('click');
    }
}

/**
 * @typedef {object} ModelProvider
 * @property {string} id
 * @property {string} label
 */

/**
 * @typedef {object} ProviderChoice
 * @property {boolean} selectable
 * @property {ModelProvider[]} providers
 */

/**
 * @param {string} model
 * @returns {Promise<ProviderChoice>}
 */
async function getOpenRouterProviderChoice(model) {
    // The website setting has no model-specific provider list.
    const providers = model.includes('/') ? await getOpenRouterModelProviders(model) : await getOpenRouterProviders();
    return { selectable: true, providers };
}

/**
 * @param {string} model
 * @returns {Promise<ProviderChoice>}
 */
async function getNanoGptProviderChoice(model) {
    const { supportsProviderSelection, providers } = await getNanoGptModelProviders(model);
    return { selectable: supportsProviderSelection, providers };
}

/**
 * @type {Readonly<Record<string, { getChoice: (model: string) => Promise<ProviderChoice>, warning: string, fallbacks?: string }>>}
 */
const MODEL_PROVIDER_SELECTS = Object.freeze({
    '#openrouter_providers_chat': {
        getChoice: getOpenRouterProviderChoice,
        warning: '#openrouter_provider_warning_chat',
        fallbacks: '#openrouter_allow_fallbacks',
    },
    '#openrouter_providers_text': {
        getChoice: getOpenRouterProviderChoice,
        warning: '#openrouter_provider_warning_text',
        fallbacks: '#openrouter_allow_fallbacks_textgenerationwebui',
    },
    '#nanogpt_provider': {
        getChoice: getNanoGptProviderChoice,
        warning: '#nanogpt_provider_warning',
    },
});

/**
 * null means availability is unknown; an empty list is a successful lookup.
 * @type {Map<string, { providers: ModelProvider[] | null }>}
 */
const modelProviderLookups = new Map();

/**
 * @param {string} selector
 * @returns {JQuery<HTMLOptionElement>}
 */
function providerOptions(selector) {
    return $(selector).find('option').filter((_, option) => option.value !== '');
}

/**
 * @param {string} selector
 * @returns {string[]}
 */
function getSelectedModelProviders(selector) {
    return providerOptions(selector).get().filter(option => option.selected).map(option => option.value);
}

/**
 * An earlier lookup must not reset the newly applied selection.
 * @param {string} selector
 * @param {string[]} selected An empty id means Auto for NanoGPT.
 */
export function applySavedModelProviders(selector, selected) {
    modelProviderLookups.delete(selector);
    renderModelProviders(selector, selected);
}

/**
 * Rendering preserves saved priority and unavailable selections without writing settings.
 * @param {string} selector
 * @param {string[]} selected
 */
function renderModelProviders(selector, selected) {
    const saved = selected.filter(Boolean);
    const providers = modelProviderLookups.get(selector)?.providers ?? null;
    const labels = new Map(providers?.map(provider => [provider.id, provider.label]));
    const savedOptions = saved.map(id => {
        const label = labels.get(id) ?? id;
        const text = providers === null || labels.has(id) ? label : t`${label} (not offered for this model)`;
        return new Option(text, id, true, true);
    });
    const otherOptions = (providers ?? [])
        .filter(provider => !saved.includes(provider.id))
        .sort((a, b) => a.label.localeCompare(b.label))
        .map(provider => new Option(provider.label, provider.id));

    providerOptions(selector).remove();
    $(selector).append(savedOptions, otherOptions).trigger('change.select2');
    updateModelProvidersWarning(selector);
}

/**
 * @param {string} selector
 * @param {string} model
 */
export async function syncModelProviders(selector, model) {
    /** @type {{ providers: ModelProvider[] | null }} */
    const lookup = { providers: null };
    modelProviderLookups.set(selector, lookup);
    renderModelProviders(selector, getSelectedModelProviders(selector));

    try {
        const { selectable, providers } = await MODEL_PROVIDER_SELECTS[selector].getChoice(model);
        if (modelProviderLookups.get(selector) !== lookup) {
            return;
        }
        lookup.providers = providers;
        if (!selectable) {
            providerOptions(selector).prop('selected', false);
            $(selector).trigger('change');
        }
        renderModelProviders(selector, getSelectedModelProviders(selector));
    } catch (error) {
        console.error(`Failed to list the providers of ${model}`, error);
    }
}

/**
 * @param {string} selector
 */
export function updateModelProvidersWarning(selector) {
    const { warning, fallbacks } = MODEL_PROVIDER_SELECTS[selector];
    const providers = modelProviderLookups.get(selector)?.providers;
    const selected = getSelectedModelProviders(selector);
    const unserved = Array.isArray(providers)
        && selected.length > 0
        && selected.every(id => providers.every(provider => provider.id !== id));
    const fallsBack = fallbacks !== undefined && $(fallbacks).prop('checked');
    $(warning).toggleClass('displayNone', !unserved || fallsBack);
}

export async function loadOllamaModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid Ollama models data', data);
        return;
    }

    if (!data.find(x => x.id === textgen_settings.ollama_model)) {
        textgen_settings.ollama_model = data[0]?.id || '';
    }

    $('#ollama_model').empty();
    for (const model of data) {
        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.name;
        option.selected = model.id === textgen_settings.ollama_model;
        $('#ollama_model').append(option);
    }
}

export async function loadTabbyModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid Tabby models data', data);
        return;
    }

    tabbyModels = data;
    tabbyModels.sort((a, b) => a.id.localeCompare(b.id));
    tabbyModels.unshift({ id: '' });

    if (!tabbyModels.find(x => x.id === textgen_settings.tabby_model)) {
        textgen_settings.tabby_model = tabbyModels[0]?.id || '';
    }

    $('#tabby_model').empty();
    for (const model of tabbyModels) {
        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.id;
        option.selected = model.id === textgen_settings.tabby_model;
        $('#tabby_model').append(option);
    }
}

export async function loadLlamaCppModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid llama.cpp models data', data);
        return;
    }

    llamacppModels = data;
    llamacppModels.sort((a, b) => a.id.localeCompare(b.id));
    llamacppModels.unshift({ id: '' });

    if (!llamacppModels.find(x => x.id === textgen_settings.llamacpp_model)) {
        textgen_settings.llamacpp_model = llamacppModels[0]?.id || '';
    }

    $('#llamacpp_model').empty();
    for (const model of llamacppModels) {
        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.id;
        option.selected = model.id === textgen_settings.llamacpp_model;
        $('#llamacpp_model').append(option);
    }
}

export async function loadTogetherAIModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid Together AI models data', data);
        return;
    }

    data.sort((a, b) => a.id.localeCompare(b.id));
    togetherModels = data;

    if (!data.find(x => x.id === textgen_settings.togetherai_model)) {
        textgen_settings.togetherai_model = data[0]?.id || '';
    }

    $('#model_togetherai_select').empty();
    for (const model of data) {
        // Hey buddy, I think you've got the wrong door.
        if (model.type === 'image') {
            continue;
        }

        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.display_name;
        option.selected = model.id === textgen_settings.togetherai_model;
        $('#model_togetherai_select').append(option);
    }
}

export async function loadInfermaticAIModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid Infermatic AI models data', data);
        return;
    }

    data.sort((a, b) => a.id.localeCompare(b.id));
    infermaticAIModels = data;

    if (!data.find(x => x.id === textgen_settings.infermaticai_model)) {
        textgen_settings.infermaticai_model = data[0]?.id || '';
    }

    $('#model_infermaticai_select').empty();
    for (const model of data) {
        if (model.display_type === 'image') {
            continue;
        }

        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.id;
        option.selected = model.id === textgen_settings.infermaticai_model;
        $('#model_infermaticai_select').append(option);
    }
}

export function loadGenericModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid Generic models data', data);
        return;
    }

    data.sort((a, b) => a.id.localeCompare(b.id));
    const dataList = $('#generic_model_fill');
    dataList.empty();

    for (const model of data) {
        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.id;
        dataList.append(option);
    }
}

export async function loadDreamGenModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid DreamGen models data', data);
        return;
    }

    dreamGenModels = data;

    if (!data.find(x => x.id === textgen_settings.dreamgen_model)) {
        textgen_settings.dreamgen_model = data[0]?.id || '';
    }

    $('#model_dreamgen_select').empty();
    for (const model of data) {
        if (model.display_type === 'image') {
            continue;
        }

        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.id;
        option.selected = model.id === textgen_settings.dreamgen_model;
        $('#model_dreamgen_select').append(option);
    }
}

export async function loadMancerModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid Mancer models data', data);
        return;
    }

    data.sort((a, b) => a.name.localeCompare(b.name));
    mancerModels = data;

    if (!data.find(x => x.id === textgen_settings.mancer_model)) {
        textgen_settings.mancer_model = data[0]?.id || '';
    }

    $('#mancer_model').empty();
    for (const model of data) {
        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.name;
        option.selected = model.id === textgen_settings.mancer_model;
        $('#mancer_model').append(option);
    }
}

export async function loadOpenRouterModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid OpenRouter models data', data);
        return;
    }

    data.sort((a, b) => a.name.localeCompare(b.name));
    openRouterModels = data;

    if (!data.find(x => x.id === textgen_settings.openrouter_model)) {
        textgen_settings.openrouter_model = data[0]?.id || '';
    }

    $('#openrouter_model').empty();
    for (const model of data) {
        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.name;
        option.selected = model.id === textgen_settings.openrouter_model;
        $('#openrouter_model').append(option);
    }

    // Calculate the cost of the selected model + update on settings change
    calculateOpenRouterCost();
    syncModelProviders('#openrouter_providers_text', textgen_settings.openrouter_model);
}

export async function loadVllmModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid vLLM models data', data);
        return;
    }

    vllmModels = data;

    if (!data.find(x => x.id === textgen_settings.vllm_model)) {
        textgen_settings.vllm_model = data[0]?.id || '';
    }

    $('#vllm_model').empty();
    for (const model of data) {
        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.id;
        option.selected = model.id === textgen_settings.vllm_model;
        $('#vllm_model').append(option);
    }
}

export async function loadAphroditeModels(data) {
    if (!Array.isArray(data)) {
        console.error('Invalid Aphrodite models data', data);
        return;
    }

    aphroditeModels = data;

    if (!data.find(x => x.id === textgen_settings.aphrodite_model)) {
        textgen_settings.aphrodite_model = data[0]?.id || '';
    }

    $('#aphrodite_model').empty();
    for (const model of data) {
        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.id;
        option.selected = model.id === textgen_settings.aphrodite_model;
        $('#aphrodite_model').append(option);
    }
}

export async function loadFeatherlessModels(data) {
    const searchBar = document.getElementById('featherless_model_search_bar');
    const modelCardBlock = document.getElementById('featherless_model_card_block');
    const paginationContainer = $('#featherless_model_pagination_container');
    const sortOrderSelect = document.getElementById('featherless_model_sort_order');
    const classSelect = document.getElementById('featherless_class_selection');
    const categoriesSelect = document.getElementById('featherless_category_selection');
    const paginationSettings = { storageKey: 'FeatherlessModels_PerPage', defaultPageSize: 10 };

    // Store the original models data for search and filtering
    let originalModels = [];

    if (!Array.isArray(data)) {
        console.error('Invalid Featherless models data', data);
        return;
    }

    originalModels = data;  // Store the original data for search
    featherlessModels = data;

    if (!data.find(x => x.id === textgen_settings.featherless_model)) {
        textgen_settings.featherless_model = data[0]?.id || '';
    }

    // Populate class select options with unique classes
    populateClassSelection(data);

    // Initialize pagination
    applyFiltersAndSort();

    // Function to set up pagination (also used for filtered results)
    function setupPagination(models, pageNumber) {
        initPagination(paginationContainer, {
            ...paginationSettings,
            dataSource: models,
            pageNumber,
            showPageNumbers: true,
            showSizeChanger: false,
            callback: function (modelsOnPage) {
                const retained = keepFocus(
                    () => modelCardBlock.querySelectorAll('.model-card'),
                    item => item.dataset.modelId,
                    () => {
                        modelCardBlock.innerHTML = '';

                        modelsOnPage.forEach(model => {
                            const card = document.createElement('button');
                            card.type = 'button';
                            card.setAttribute('aria-pressed', String(model.id === textgen_settings.featherless_model));
                            card.classList.add('model-card');
                            card.dataset.modelId = model.id;

                            const modelNameContainer = document.createElement('div');
                            modelNameContainer.classList.add('model-name-container');

                            const modelTitle = document.createElement('div');
                            modelTitle.classList.add('model-title');
                            modelTitle.textContent = model.id.replace(/_/g, '_\u200B');
                            modelNameContainer.appendChild(modelTitle);

                            const detailsContainer = document.createElement('div');
                            detailsContainer.classList.add('details-container');

                            const modelClassDiv = document.createElement('div');
                            modelClassDiv.classList.add('model-class');
                            modelClassDiv.textContent = t`Class` + `: ${model.model_class || 'N/A'}`;

                            const contextLengthDiv = document.createElement('div');
                            contextLengthDiv.classList.add('model-context-length');
                            contextLengthDiv.textContent = t`Context Length` + `: ${model.context_length}`;

                            const dateAddedDiv = document.createElement('div');
                            dateAddedDiv.classList.add('model-date-added');
                            dateAddedDiv.textContent = t`Added On` + `: ${new Date(model.created * 1000).toLocaleDateString()}`;

                            detailsContainer.appendChild(modelClassDiv);
                            detailsContainer.appendChild(contextLengthDiv);
                            detailsContainer.appendChild(dateAddedDiv);

                            card.appendChild(modelNameContainer);
                            card.appendChild(detailsContainer);

                            modelCardBlock.appendChild(card);

                            if (model.id === textgen_settings.featherless_model) {
                                card.classList.add('selected');
                            }

                            card.addEventListener('click', function () {
                                modelCardBlock.querySelectorAll('.model-card').forEach(c => {
                                    c.classList.remove('selected');
                                    c.setAttribute('aria-pressed', 'false');
                                });
                                card.setAttribute('aria-pressed', 'true');
                                card.classList.add('selected');
                                onFeatherlessModelSelect(model.id);
                            });
                        });
                    },
                );
                if (!retained) searchBar.focus();
            },
        });
    }

    // Unset previously added listeners
    $(searchBar).off('input');
    $(sortOrderSelect).off('change');
    $(classSelect).off('change');
    $(categoriesSelect).off('change');

    // Add event listener for input on the search bar
    searchBar.addEventListener('input', function () {
        applyFiltersAndSort();
    });

    // Add event listener for the sort order select
    sortOrderSelect.addEventListener('change', function () {
        applyFiltersAndSort();
    });

    // Add event listener for the class select
    classSelect.addEventListener('change', function () {
        applyFiltersAndSort();
    });

    categoriesSelect.addEventListener('change', function () {
        applyFiltersAndSort();
    });

    // Function to populate class selection dropdown
    function populateClassSelection(models) {
        const uniqueClasses = [...new Set(models.map(model => model.model_class).filter(Boolean))];  // Get unique class names
        uniqueClasses.sort((a, b) => a.localeCompare(b));
        uniqueClasses.forEach(className => {
            const option = document.createElement('option');
            option.value = className;
            option.textContent = className;
            classSelect.appendChild(option);
        });
    }

    // Function to apply sorting and filtering based on user input
    async function applyFiltersAndSort() {
        if (!(searchBar instanceof HTMLInputElement) ||
            !(sortOrderSelect instanceof HTMLSelectElement) ||
            !(classSelect instanceof HTMLSelectElement) ||
            !(categoriesSelect instanceof HTMLSelectElement)) {
            return;
        }
        const searchQuery = searchBar.value.toLowerCase();
        const selectedSortOrder = sortOrderSelect.value;
        const selectedClass = classSelect.value;
        const selectedCategory = categoriesSelect.value;
        let featherlessTop = [];
        let featherlessNew = [];

        if (selectedCategory === 'Top') {
            featherlessTop = await fetchFeatherlessStats();
        }
        const featherlessIds = featherlessTop.map(stat => stat.id);

        if (selectedCategory === 'New') {
            featherlessNew = await fetchFeatherlessNew();
        }
        const featherlessNewIds = featherlessNew.map(stat => stat.id);

        let filteredModels = originalModels.filter(model => {
            const matchesSearch = model.id.toLowerCase().includes(searchQuery);
            const matchesClass = selectedClass ? model.model_class === selectedClass : true;
            const matchesTop = featherlessIds.includes(model.id);
            const matchesNew = featherlessNewIds.includes(model.id);

            if (selectedCategory === 'All') {
                return matchesSearch && matchesClass;
            }
            else if (selectedCategory === 'Top') {
                return matchesSearch && matchesClass && matchesTop;
            }
            else if (selectedCategory === 'New') {
                return matchesSearch && matchesClass && matchesNew;
            }
            else {
                return matchesSearch && matchesClass;
            }
        });

        if (selectedSortOrder === 'asc') {
            filteredModels.sort((a, b) => a.id.localeCompare(b.id));
        } else if (selectedSortOrder === 'desc') {
            filteredModels.sort((a, b) => b.id.localeCompare(a.id));
        } else if (selectedSortOrder === 'date_asc') {
            filteredModels.sort((a, b) => a.created - b.created);
        } else if (selectedSortOrder === 'date_desc') {
            filteredModels.sort((a, b) => b.created - a.created);
        }

        const currentModelIndex = filteredModels.findIndex(x => x.id === textgen_settings.featherless_model);
        const pageNumber = currentModelIndex >= 0 ? getPageForItem(currentModelIndex, paginationSettings) : 1;
        setupPagination(filteredModels, pageNumber);
    }

    // Required to keep the /model command function
    $('#featherless_model').empty();
    for (const model of data) {
        const option = document.createElement('option');
        option.value = model.id;
        option.text = model.id;
        option.selected = model.id === textgen_settings.featherless_model;
        $('#featherless_model').append(option);
    }
}

async function fetchFeatherlessStats() {
    const response = await fetch('https://api.featherless.ai/feather/popular');
    const data = await response.json();
    return data.popular;
}

async function fetchFeatherlessNew() {
    const response = await fetch('https://api.featherless.ai/feather/models?sort=-created_at&perPage=20');
    const data = await response.json();
    return data.items;
}

function applyModelContextLength(max_length) {
    const adjustments = [];
    setGenerationParamsFromPreset({ max_length }, adjustments);
    showNumericAdjustments(adjustments);
}

function onFeatherlessModelSelect(modelId) {
    const model = featherlessModels.find(x => x.id === modelId);
    textgen_settings.featherless_model = modelId;
    $('#featherless_model').val(modelId);
    reconnectTextGenIfValidationAllowed();
    applyModelContextLength(model.context_length);
}

let featherlessIsGridView = false;  // Default state set to grid view

// Ensure the correct initial view is applied when the page loads
document.addEventListener('DOMContentLoaded', function () {
    const modelCardBlock = document.getElementById('featherless_model_card_block');
    modelCardBlock.classList.add('list-view');

    const toggleButton = document.getElementById('featherless_model_grid_toggle');
    toggleButton.setAttribute('aria-pressed', String(featherlessIsGridView));
    toggleButton.addEventListener('click', function () {
        // Toggle between grid and list view
        if (featherlessIsGridView) {
            modelCardBlock.classList.remove('grid-view');
            modelCardBlock.classList.add('list-view');
        } else {
            modelCardBlock.classList.remove('list-view');
            modelCardBlock.classList.add('grid-view');
        }

        featherlessIsGridView = !featherlessIsGridView;
        this.setAttribute('aria-pressed', String(featherlessIsGridView));
    });
});
function onMancerModelSelect() {
    const modelId = String($('#mancer_model').val());
    textgen_settings.mancer_model = modelId;
    reconnectTextGenIfValidationAllowed();

    const limits = mancerModels.find(x => x.id === modelId)?.limits;
    applyModelContextLength(limits.context);
}

function onTogetherModelSelect() {
    const modelName = String($('#model_togetherai_select').val());
    textgen_settings.togetherai_model = modelName;
    reconnectTextGenIfValidationAllowed();
    const model = togetherModels.find(x => x.id === modelName);
    applyModelContextLength(model.context_length);
}

function onInfermaticAIModelSelect() {
    const modelName = String($('#model_infermaticai_select').val());
    textgen_settings.infermaticai_model = modelName;
    reconnectTextGenIfValidationAllowed();
    const model = infermaticAIModels.find(x => x.id === modelName);
    applyModelContextLength(model.context_length);
}

function onDreamGenModelSelect() {
    const modelName = String($('#model_dreamgen_select').val());
    textgen_settings.dreamgen_model = modelName;
    reconnectTextGenIfValidationAllowed();
    // TODO(DreamGen): Consider retuning max_tokens from API and setting it here.
}

function onOllamaModelSelect() {
    const modelId = String($('#ollama_model').val());
    textgen_settings.ollama_model = modelId;
    reconnectTextGenIfValidationAllowed();
}

function onTabbyModelSelect() {
    const modelId = String($('#tabby_model').val());
    textgen_settings.tabby_model = modelId;
    reconnectTextGenIfValidationAllowed();
}

function onLlamaCppModelSelect() {
    const modelId = String($('#llamacpp_model').val());
    textgen_settings.llamacpp_model = modelId;
    reconnectTextGenIfValidationAllowed();
}

function onOpenRouterModelSelect() {
    const modelId = String($('#openrouter_model').val());
    textgen_settings.openrouter_model = modelId;
    reconnectTextGenIfValidationAllowed();
    const model = openRouterModels.find(x => x.id === modelId);
    syncModelProviders('#openrouter_providers_text', modelId);
    applyModelContextLength(model.context_length);
}

function onVllmModelSelect() {
    const modelId = String($('#vllm_model').val());
    textgen_settings.vllm_model = modelId;
    reconnectTextGenIfValidationAllowed();
}

function onAphroditeModelSelect() {
    const modelId = String($('#aphrodite_model').val());
    textgen_settings.aphrodite_model = modelId;
    reconnectTextGenIfValidationAllowed();
}

function getMancerModelTemplate(option) {
    const model = mancerModels.find(x => x.id === option?.element?.value);

    if (!option.id || !model) {
        return option.text;
    }

    const creditsPerPrompt = (model.limits?.context - model.limits?.completion) * model.pricing?.prompt;
    const creditsPerCompletion = model.limits?.completion * model.pricing?.completion;
    const creditsTotal = Math.round(creditsPerPrompt + creditsPerCompletion).toFixed(0);

    return $((`
        <div class="flex-container flexFlowColumn">
            <div><strong>${DOMPurify.sanitize(model.name)}</strong> | <span>${model.limits?.context} ctx</span> / <span>${model.limits?.completion} res</span> | <small>Credits per request (max): ${creditsTotal}</small></div>
        </div>
    `));
}

function getTogetherModelTemplate(option) {
    const model = togetherModels.find(x => x.id === option?.element?.value);

    if (!option.id || !model) {
        return option.text;
    }

    return $((`
        <div class="flex-container flexFlowColumn">
            <div><strong>${DOMPurify.sanitize(model.id)}</strong> | <span>${model.context_length || '???'} tokens</span></div>
            <div><small>${DOMPurify.sanitize(model.description)}</small></div>
        </div>
    `));
}

function getInfermaticAIModelTemplate(option) {
    const model = infermaticAIModels.find(x => x.id === option?.element?.value);

    if (!option.id || !model) {
        return option.text;
    }

    return $((`
        <div class="flex-container flexFlowColumn">
            <div><strong>${DOMPurify.sanitize(model.id)}</strong></div>
        </div>
    `));
}

function getDreamGenModelTemplate(option) {
    const model = dreamGenModels.find(x => x.id === option?.element?.value);

    if (!option.id || !model) {
        return option.text;
    }

    return $((`
        <div class="flex-container flexFlowColumn">
            <div><strong>${DOMPurify.sanitize(model.id)}</strong></div>
        </div>
    `));
}

function getOpenRouterModelTemplate(option) {
    const model = openRouterModels.find(x => x.id === option?.element?.value);

    if (!option.id || !model) {
        return option.text;
    }

    let tokens_dollar = Number(1 / (1000 * model.pricing?.prompt));
    let tokens_rounded = (Math.round(tokens_dollar * 1000) / 1000).toFixed(0);

    const price = 0 === Number(model.pricing?.prompt) ? 'Free' : `${tokens_rounded}k t/$ `;

    return $((`
        <div class="flex-container flexFlowColumn" title="${DOMPurify.sanitize(model.id)}">
            <div><strong>${DOMPurify.sanitize(model.name)}</strong> | ${model.context_length} ctx | <small>${price}</small></div>
        </div>
    `));
}

function getVllmModelTemplate(option) {
    const model = vllmModels.find(x => x.id === option?.element?.value);

    if (!option.id || !model) {
        return option.text;
    }

    return $((`
        <div class="flex-container flexFlowColumn">
            <div><strong>${DOMPurify.sanitize(model.id)}</strong></div>
        </div>
    `));
}

function getAphroditeModelTemplate(option) {
    const model = aphroditeModels.find(x => x.id === option?.element?.value);

    if (!option.id || !model) {
        return option.text;
    }

    return $((`
        <div class="flex-container flexFlowColumn">
            <div><strong>${DOMPurify.sanitize(model.id)}</strong></div>
        </div>
    `));
}

async function downloadOllamaModel() {
    try {
        const serverUrl = textgen_settings.server_urls[textgen_types.OLLAMA];

        if (!serverUrl) {
            toastr.info('Please connect to an Ollama server first.');
            return;
        }

        const html = `Enter a model tag, for example <code>llama2:latest</code>.<br>
        See <a target="_blank" href="https://ollama.ai/library">Library</a> for available models.`;
        const name = await callGenericPopup(html, POPUP_TYPE.INPUT, '', { okButton: 'Download' });

        if (!name) {
            return;
        }

        toastr.info('Download may take a while, please wait...', 'Working on it');

        const response = await fetch('/api/backends/text-completions/ollama/download', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify({
                name: name,
                api_server: serverUrl,
            }),
        });

        if (!response.ok) {
            throw new Error(response.statusText);
        }

        // Force refresh the model list
        toastr.success('Download complete. Please select the model from the dropdown.');
        reconnectTextGenIfValidationAllowed();
    } catch (err) {
        console.error(err);
        toastr.error('Failed to download Ollama model. Please try again.');
    }
}

async function downloadTabbyModel() {
    try {
        const serverUrl = textgen_settings.server_urls[textgen_types.TABBY];

        if (online_status === 'no_connection' || !serverUrl) {
            toastr.info('Please connect to a TabbyAPI server first.');
            return;
        }

        const downloadHtml = $(await renderTemplateAsync('tabbyDownloader'));
        const popupResult = await callGenericPopup(downloadHtml, POPUP_TYPE.CONFIRM, '', { okButton: 'Download', cancelButton: 'Cancel' });

        // User cancelled the download
        if (!popupResult) {
            return;
        }

        const repoId = downloadHtml.find('input[name="hf_repo_id"]').val().toString();
        if (!repoId) {
            toastr.error('A HuggingFace repo ID must be provided. Skipping Download.');
            return;
        }

        if (repoId.split('/').length !== 2) {
            toastr.error('A HuggingFace repo ID must be formatted as Author/Name. Please try again.');
            return;
        }

        const params = {
            repo_id: repoId,
            folder_name: downloadHtml.find('input[name="folder_name"]').val() || undefined,
            revision: downloadHtml.find('input[name="revision"]').val() || undefined,
            token: downloadHtml.find('input[name="hf_token"]').val() || undefined,
        };

        for (const suffix of ['include', 'exclude']) {
            const patterns = downloadHtml.find(`textarea[name="tabby_download_${suffix}"]`).val().toString();
            if (patterns) {
                params[suffix] = patterns.split('\n');
            }
        }

        // Params for the server side of ST
        params.api_server = serverUrl;
        params.api_type = textgen_settings.type;

        toastr.info('Downloading. Check the Tabby console for progress reports.');

        const response = await fetch('/api/backends/text-completions/tabby/download', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify(params),
        });

        if (response.status === 403) {
            toastr.error('The provided key has invalid permissions. Please use an admin key for downloading.');
            return;
        } else if (!response.ok) {
            throw new Error(response.statusText);
        }

        toastr.success('Download complete.');
    } catch (err) {
        console.error(err);
        toastr.error('Failed to download HuggingFace model in TabbyAPI. Please try again.');
    }
}

function calculateOpenRouterCost() {
    if (textgen_settings.type !== textgen_types.OPENROUTER) {
        return;
    }

    let cost = 'Unknown';
    const model = openRouterModels.find(x => x.id === textgen_settings.openrouter_model);

    if (model?.pricing) {
        const completionCost = Number(model.pricing.completion);
        const promptCost = Number(model.pricing.prompt);
        const completionTokens = amount_gen;
        const promptTokens = (max_context - completionTokens);
        const totalCost = (completionCost * completionTokens) + (promptCost * promptTokens);
        if (!isNaN(totalCost)) {
            cost = '$' + totalCost.toFixed(3);
        }
    }

    $('#or_prompt_cost').text(cost);

    // Schedule an update when settings change
    eventSource.removeListener(event_types.SETTINGS_UPDATED, calculateOpenRouterCost);
    eventSource.once(event_types.SETTINGS_UPDATED, calculateOpenRouterCost);
}

export function getCurrentOpenRouterModelTokenizer() {
    const modelId = textgen_settings.openrouter_model;
    const model = openRouterModels.find(x => x.id === modelId);
    if (modelId?.includes('jamba')) {
        return tokenizers.JAMBA;
    }
    switch (model?.architecture?.tokenizer) {
        case 'Llama2':
            return tokenizers.LLAMA;
        case 'Llama3':
            return tokenizers.LLAMA3;
        case 'Yi':
            return tokenizers.YI;
        case 'Mistral':
            return tokenizers.MISTRAL;
        case 'Gemini':
            return tokenizers.GEMMA;
        case 'Claude':
            return tokenizers.CLAUDE;
        case 'Cohere':
            return tokenizers.COMMAND_R;
        case 'Qwen':
            return tokenizers.QWEN2;
        default:
            return tokenizers.OPENAI;
    }
}

export function getCurrentDreamGenModelTokenizer() {
    const modelId = textgen_settings.dreamgen_model;
    const model = dreamGenModels.find(x => x.id === modelId);
    if (model.id.startsWith('lucid-v1-medium') || model.id.startsWith('lucid-v1-base')) {
        return tokenizers.MISTRAL;
    } else if (model.id.startsWith('lucid-v1-extra-large') || model.id.startsWith('lucid-v1-max')) {
        return tokenizers.LLAMA3;
    } else {
        return tokenizers.MISTRAL;
    }
}

export function initTextGenModels() {
    $('#mancer_model').on('change', onMancerModelSelect);
    $('#model_togetherai_select').on('change', onTogetherModelSelect);
    $('#model_infermaticai_select').on('change', onInfermaticAIModelSelect);
    $('#model_dreamgen_select').on('change', onDreamGenModelSelect);
    $('#ollama_model').on('change', onOllamaModelSelect);
    $('#openrouter_model').on('change', onOpenRouterModelSelect);
    $('#ollama_download_model').on('click', downloadOllamaModel);
    $('#vllm_model').on('change', onVllmModelSelect);
    $('#aphrodite_model').on('change', onAphroditeModelSelect);
    $('#tabby_download_model').on('click', downloadTabbyModel);
    $('#tabby_model').on('change', onTabbyModelSelect);
    $('#llamacpp_model').on('change', onLlamaCppModelSelect);
    $('#featherless_model').on('change', () => onFeatherlessModelSelect(String($('#featherless_model').val())));

    const providersSelect = $('.openrouter_providers');
    const nanoGptProvidersSelect = $('#nanogpt_provider');

    if (!isMobile()) {
        $('#mancer_model').select2({
            placeholder: t`Select a model`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            templateResult: getMancerModelTemplate,
        });
        $('#model_togetherai_select').select2({
            placeholder: t`Select a model`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            templateResult: getTogetherModelTemplate,
        });
        $('#ollama_model').select2({
            placeholder: t`Select a model`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
        });
        $('#tabby_model').select2({
            placeholder: t`[Currently loaded]`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            allowClear: true,
        });
        $('#llamacpp_model').select2({
            placeholder: t`[Currently loaded]`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            allowClear: true,
        });
        $('#model_infermaticai_select').select2({
            placeholder: t`Select a model`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            templateResult: getInfermaticAIModelTemplate,
        });
        $('#model_dreamgen_select').select2({
            placeholder: t`Select a model`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            templateResult: getDreamGenModelTemplate,
        });
        $('#openrouter_model').select2({
            placeholder: t`Select a model`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            templateResult: getOpenRouterModelTemplate,
            matcher: textValueMatcher,
        });
        $('#vllm_model').select2({
            placeholder: t`Select a model`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            templateResult: getVllmModelTemplate,
        });
        $('#aphrodite_model').select2({
            placeholder: t`Select a model`,
            searchInputPlaceholder: t`Search models...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            templateResult: getAphroditeModelTemplate,
        });
        $('.openrouter_quantizations').select2({
            closeOnSelect: false,
            placeholder: t`Select quantizations. No selection = all quantizations.`,
            searchInputCssClass: 'text_pole',
            searchInputPlaceholder: t`Search quantizations...`,
            width: '100%',
        });
        providersSelect.select2({
            sorter: data => data.sort((a, b) => a.text.localeCompare(b.text)),
            placeholder: t`Select providers. No selection = all providers.`,
            searchInputPlaceholder: t`Search providers...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            closeOnSelect: false,
        });
        providersSelect.on('select2:select', function (/** @type {any} */ evt) {
            const element = evt.params.data.element;
            const $element = $(element);

            $element.detach();
            $(this).append($element);
            $(this).trigger('change');
        });
        nanoGptProvidersSelect.select2({
            sorter: data => data.sort((a, b) => a.text.localeCompare(b.text)),
            placeholder: t`Select providers. No selection = all providers.`,
            searchInputPlaceholder: t`Search providers...`,
            searchInputCssClass: 'text_pole',
            width: '100%',
            allowClear: true,
        });
    }
}
