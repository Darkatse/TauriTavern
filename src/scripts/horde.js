import { toUserFacingErrorText } from './util/user-facing-error.js';
import {
    amount_gen,
    getRequestHeaders,
    main_api,
    max_context,
    resultCheckStatus,
    saveSettingsDebounced,
    setGenerationProgress,
    setOnlineStatus,
} from '../script.js';
import { SECRET_KEYS, readSecretState, secret_state, writeSecret } from './secrets.js';
import { isMobile } from './RossAscends-mods.js';
import { autoSelectInstructPreset } from './instruct-mode.js';
import { t } from './i18n.js';
import { kai_settings } from './kai-settings.js';
import EventSourceStream from './sse-stream.js';

export {
    MIN_LENGTH,
};

let models = [];

export let horde_settings = {
    models: [],
    auto_adjust_response_length: true,
    auto_adjust_context_length: false,
    trusted_workers_only: false,
};

const MIN_LENGTH = 16;

/**
 * Gets the available workers from Horde.
 * @param {boolean} force Do a force refresh of the workers
 * @returns {Promise<Array>} Array of workers
 */
async function getWorkers(force) {
    const response = await fetch('/api/horde/text-workers', {
        method: 'POST',
        headers: getRequestHeaders(),
        body: JSON.stringify({ force }),
    });
    return await response.json();
}

/**
 * Gets the available models from Horde.
 * @param {boolean} force Do a force refresh of the models
 * @returns {Promise<Array>} Array of models
 */
async function getModels(force) {
    const response = await fetch('/api/horde/text-models', {
        method: 'POST',
        headers: getRequestHeaders(),
        body: JSON.stringify({ force }),
    });
    const data = await response.json();
    console.log('getModels', data);
    return data;
}

/**
 * Checks if Horde is online.
 * @returns {Promise<boolean>} True if Horde is online, false otherwise
 */
export async function checkHordeStatus() {
    try {
        const response = await fetch('/api/horde/status', {
            method: 'POST',
            headers: getRequestHeaders({ omitContentType: true }),
        });

        if (!response.ok) {
            return false;
        }

        const data = await response.json();
        return data.ok;
    } catch (error) {
        console.error(error);
        return false;
    }
}

export async function getStatusHorde() {
    try {
        const hordeStatus = await checkHordeStatus();
        setOnlineStatus(hordeStatus ? t`Connected` : 'no_connection');
    } catch {
        setOnlineStatus('no_connection');
    }

    return resultCheckStatus();
}

function validateHordeModel() {
    let selectedModels = models.filter(m => horde_settings.models.includes(m.name));

    if (selectedModels.length === 0) {
        toastr.warning('No Horde model selected or the selected models are no longer available. Please choose another model');
        throw new Error('No Horde model available');
    }

    return selectedModels;
}

export async function adjustHordeGenerationParams(max_context_length, max_length) {
    console.log(max_context_length, max_length);
    const workers = await getWorkers(false);
    let maxContextLength = max_context_length;
    let maxLength = max_length;
    let availableWorkers = [];
    let selectedModels = validateHordeModel();

    if (selectedModels.length === 0) {
        return { maxContextLength, maxLength };
    }

    for (const model of selectedModels) {
        for (const worker of workers) {
            if (model.cluster === worker.cluster && worker.models.includes(model.name)) {
                // Skip workers that are not trusted if the option is enabled
                if (horde_settings.trusted_workers_only && !worker.trusted) {
                    continue;
                }

                availableWorkers.push(worker);
            }
        }
    }

    //get the minimum requires parameters, lowest common value for all selected
    for (const worker of availableWorkers) {
        if (horde_settings.auto_adjust_context_length) {
            maxContextLength = Math.min(worker.max_context_length, maxContextLength);
        }
        if (horde_settings.auto_adjust_response_length) {
            maxLength = Math.min(worker.max_length, maxLength);
        }
    }
    console.log(maxContextLength, maxLength);
    $('#adjustedHordeParams').text(t`Context` + `: ${maxContextLength}, ` + t`Response` + `: ${maxLength}`);
    return { maxContextLength, maxLength };
}

function setContextSizePreview() {
    if (horde_settings.models.length) {
        adjustHordeGenerationParams(max_context, amount_gen);
    } else {
        $('#adjustedHordeParams').text(t`Context` + ': --, ' + t`Response` + ': --');
    }
}

/** Generates text using the Horde API.
 * @param {string} prompt
 * @param params
 * @param signal
 * @param reportProgress
 * @returns {Promise<{text: *, workerName: string | undefined}>}
 * @throws {Error}
 */
export async function generateHorde(prompt, params, signal, reportProgress) {
    validateHordeModel();
    const { prompt: _prompt, type, ...hordeParams } = params;

    const payload = {
        prompt,
        params: {
            ...hordeParams,
            // No idea what these do
            n: 1,
            frmtadsnsp: false,
            frmtrmblln: false,
            frmtrmspch: false,
            frmttriminc: false,
        },
        trusted_workers: horde_settings.trusted_workers_only,
        //"slow_workers": false,
        models: horde_settings.models,
        type,
    };

    const response = await fetch('/api/backends/koboldhorde/generate', {
        method: 'POST',
        headers: getRequestHeaders(),
        body: JSON.stringify(payload),
        signal,
    });

    /** @param {string} reason */
    const failGeneration = (reason) => {
        toastr.error(reason, 'Horde generation failed');
        throw new Error(`Horde generation failed: ${reason}`);
    };

    if (!response.ok) {
        const data = await response.json().catch(() => null);
        failGeneration(data?.error?.message || response.statusText);
    }

    const eventStream = new EventSourceStream();
    response.body.pipeThrough(eventStream);
    const reader = eventStream.readable.getReader();
    let generation = null;
    let firstQueuePosition = null;

    try {
        while (true) {
            const { done, value } = await reader.read();
            if (done || value.data === '[DONE]') {
                break;
            }

            const status = JSON.parse(value.data);
            generation = status.generations?.[0] ?? null;
            if (generation || !reportProgress) {
                continue;
            }

            if (!firstQueuePosition) {
                firstQueuePosition = status.queue_position;
                setGenerationProgress(0);
            } else if (status.queue_position >= 0) {
                setGenerationProgress(Math.round(100 - (status.queue_position / firstQueuePosition * 100)));
            }
        }
    } catch (error) {
        signal?.throwIfAborted();
        failGeneration(error instanceof Error ? error.message : String(error));
    }

    // The stream can close before the aborted signal rejects the reader.
    signal?.throwIfAborted();
    if (!generation) {
        throw new Error('Horde generation ended without a result');
    }

    reportProgress && setGenerationProgress(100);
    const workerName = generation.worker_name
        ? `Generated by Horde worker: ${generation.worker_name} [${generation.model}]`
        : undefined;
    return { text: generation.text, workerName };
}


/**
 * Displays the available models in the Horde model selection dropdown.
 * @param {boolean} force Force refresh of the models
 */
export async function getHordeModels(force) {
    const sortByPerformance = (a, b) => b.performance - a.performance;
    const sortByWhitelisted = (a, b) => b.is_whitelisted - a.is_whitelisted;
    const sortByPopular = (a, b) => b.tags?.includes('popular') - a.tags?.includes('popular');

    $('#horde_model').empty();
    models = (await getModels(force)).sort((a, b) => {
        return sortByWhitelisted(a, b) || sortByPopular(a, b) || sortByPerformance(a, b);
    });
    for (const model of models) {
        const option = document.createElement('option');
        option.value = model.name;
        option.innerText = hordeModelTextString(model);
        option.selected = horde_settings.models.includes(model.name);
        $('#horde_model').append(option);
    }

    // if previously selected is no longer available
    if (horde_settings.models.length && models.filter(m => horde_settings.models.includes(m.name)).length === 0) {
        horde_settings.models = [];
    }

    setContextSizePreview();
}

export function loadHordeSettings(settings) {
    if (settings.horde_settings) {
        Object.assign(horde_settings, settings.horde_settings);
    }

    $('#horde_auto_adjust_response_length').prop('checked', horde_settings.auto_adjust_response_length);
    $('#horde_auto_adjust_context_length').prop('checked', horde_settings.auto_adjust_context_length);
    $('#horde_trusted_workers_only').prop('checked', horde_settings.trusted_workers_only);
}

async function showKudos() {
    const response = await fetch('/api/horde/user-info', {
        method: 'POST',
        headers: getRequestHeaders({ omitContentType: true }),
    });

    if (!response.ok) {
        toastr.warning('Could not load user info from Horde. Please try again later.');
        return;
    }

    const data = await response.json();

    if (data.anonymous) {
        toastr.info('You are in anonymous mode. Set your personal Horde API key to see kudos.');
        return;
    }

    console.log('Horde user data', data.user, 'shared key data', data.sharedKey);
    const kudos = data.sharedKey?.kudos ?? data.user?.kudos ?? 0;
    toastr.info(`Kudos: ${kudos}`, data.user.username);
}

function hordeModelTextString(model) {
    const q = hordeModelQueueStateString(model);
    return `${model.name} (${q})`;
}

function hordeModelQueueStateString(model) {
    return `ETA: ${model.eta}s, Speed: ${model.performance}, Queue: ${model.queued}, Workers: ${model.count}`;
}

export function isHordeGenerationNotAllowed() {
    if (main_api == 'koboldhorde' && kai_settings.preset_settings == 'gui') {
        toastr.error(t`GUI Settings preset is not supported for Horde. Please select another preset.`);
        return true;
    }

    return false;
}

function getHordeModelTemplate(option) {
    const model = models.find(x => x.name === option?.element?.value);

    if (!option.id || !model) {
        console.debug('No model found for option', option, option?.element?.value);
        console.debug('Models', models);
        return option.text;
    }

    const strip = html => {
        const tmp = document.createElement('DIV');
        tmp.innerHTML = html || '';
        return tmp.textContent || tmp.innerText || '';
    };

    // how much do we trust the metadata from the models repo? about this much
    const displayName = strip(model.display_name || model.name).replace(/.*\//g, '');
    const description = strip(model.description);
    const tags = model.tags ? model.tags.map(strip) : [];
    const url = strip(model.url);
    const style = strip(model.style);

    const workerInfo = hordeModelQueueStateString(model);
    const isPopular = model.tags?.includes('popular');
    const descriptionDiv = description ? `<div class="horde-model-description">${description}</div>` : '';
    const tagSpans = tags.length > 0 &&
        `${tags.map(tag => `<span class="tag tag_name">${tag}</span>`).join('')}</span>` || '';

    const modelDetailsLink = url && `<a href="${url}" target="_blank" rel="noopener noreferrer" class="model-details-link fa-solid fa-circle-question"> </a>`;
    const capitalize = s => s ? s[0].toUpperCase() + s.slice(1) : '';
    const innerContent = [
        `<strong>${displayName}</strong> ${modelDetailsLink}`,
        style ? `${capitalize(style)}` : '',
        tagSpans ? `<span class="tags tags_inline inline-flex margin-r2">${tagSpans}</span>` : '',
    ].filter(Boolean).join(' | ');

    return $((`
        <div class="flex-container flexFlowColumn">
            <div>
                ${isPopular ? '<span class="fa-fw fa-solid fa-star" title="Popular"></span>' : ''}
                ${innerContent}
            </div>
            ${descriptionDiv}
            <div><small>${workerInfo}</small></div>
        </div>
    `));
}

export function initHorde() {
    $('#horde_model').on('mousedown change', async function (e) {
        console.log('Horde model change', e);
        const modelValue = $('#horde_model').val();
        horde_settings.models = Array.isArray(modelValue) ? modelValue : [];
        console.log('Updated Horde models', horde_settings.models);

        // Try select instruct preset
        autoSelectInstructPreset(horde_settings.models.join(' '));
        if (horde_settings.models.length) {
            adjustHordeGenerationParams(max_context, amount_gen);
        } else {
            $('#adjustedHordeParams').text(t`Context` + ': --, ' + t`Response` + ': --');
        }

        saveSettingsDebounced();
    });

    $('#horde_auto_adjust_response_length').on('input', function () {
        horde_settings.auto_adjust_response_length = !!$(this).prop('checked');
        setContextSizePreview();
        saveSettingsDebounced();
    });

    $('#horde_auto_adjust_context_length').on('input', function () {
        horde_settings.auto_adjust_context_length = !!$(this).prop('checked');
        setContextSizePreview();
        saveSettingsDebounced();
    });

    $('#horde_trusted_workers_only').on('input', function () {
        horde_settings.trusted_workers_only = !!$(this).prop('checked');
        setContextSizePreview();
        saveSettingsDebounced();
    });

    $('#horde_api_key_button').on('click', async function () {
        try {
            await readSecretState();
            const key = String($('#horde_api_key').val()).trim();
            if (!key && !secret_state[SECRET_KEYS.HORDE]) {
                toastr.warning(t`Please enter your Horde API key`);
                return;
            }
            if (key) await writeSecret(SECRET_KEYS.HORDE, key);
            await getStatusHorde();
        } catch (error) {
            toastr.error(toUserFacingErrorText(error));
        }
    });

    $('#horde_refresh').on('click', () => getHordeModels(true));
    $('#horde_kudos').on('click', showKudos);

    // Not needed on mobile
    if (!isMobile()) {
        $('#horde_model').select2({
            width: '100%',
            placeholder: t`Select Horde models`,
            allowClear: true,
            closeOnSelect: false,
            templateSelection: function (data) {
                // Customize the pillbox text by shortening the full text
                return data.id;
            },
            templateResult: getHordeModelTemplate,
        });
    }
}
