import { DiffMatchPatch, DOMPurify, localforage } from '../lib.js';
import { chat, event_types, eventSource, getCurrentChatId, reloadCurrentChat } from '../script.js';
import { t } from './i18n.js';
import { oai_settings } from './openai.js';
import { Popup, POPUP_TYPE } from './popup.js';
import { power_user, registerDebugFunction } from './power-user.js';
import { isMobile } from './RossAscends-mods.js';
import { renderTemplateAsync } from './templates.js';
import { getFriendlyTokenizerName, getTokenCountAsync } from './tokenizers.js';
import { copyText, uuidv4 } from './utils.js';

let PromptArrayItemForRawPromptDisplay;
let priorPromptArrayItemForRawPromptDisplay;

// Keep the upstream storage namespace so legacy SillyTavern itemized prompts can be migrated in-place.
const promptStorage = localforage.createInstance({ name: 'SillyTavern_Prompts' });

// Storage contract (TauriTavern):
// - `tt_prompts_index:<chatId>` stores a lightweight sorted index of { mesId, recordId }.
// - `tt_prompts_record:<chatId>:<recordId>` stores the full prompt record, loaded on-demand (e.g. inspector click).
// Legacy SillyTavern stores a full array under `<chatId>`; we migrate once and delete it to avoid cold-start bloat.
const TT_PROMPTS_INDEX_PREFIX = 'tt_prompts_index:';
const TT_PROMPTS_RECORD_PREFIX = 'tt_prompts_record:';

/**
 * @typedef {{ mesId: number, recordId: string }} ItemizedPromptIndexEntry
 */

// Historical name kept for upstream compatibility: this holds index entries (not full records) for the active chat.
export let itemizedPrompts = /** @type {ItemizedPromptIndexEntry[]} */ ([]);

/** @type {string|null} */
let activeChatId = null;
/** @type {Map<number, string>} */
let recordIdByMesId = new Map();
/** @type {Promise<unknown>} */
// Serialize index writes to keep localforage updates ordered when multiple upserts happen rapidly.
let indexWriteChain = Promise.resolve();

function getIndexKey(chatId) {
    return `${TT_PROMPTS_INDEX_PREFIX}${chatId}`;
}

function getRecordKey(chatId, recordId) {
    return `${TT_PROMPTS_RECORD_PREFIX}${chatId}:${recordId}`;
}

function getRecordKeyPrefix(chatId) {
    return `${TT_PROMPTS_RECORD_PREFIX}${chatId}:`;
}

function rebuildRecordIdMap(entries) {
    recordIdByMesId = new Map(entries.map((entry) => [Number(entry.mesId), entry.recordId]));
}

function setActiveIndex(chatId, entries) {
    activeChatId = chatId;
    itemizedPrompts = entries;
    itemizedPrompts.sort((a, b) => a.mesId - b.mesId);
    rebuildRecordIdMap(itemizedPrompts);
}

function clearActiveIndex() {
    activeChatId = null;
    itemizedPrompts = [];
    recordIdByMesId = new Map();
}

function enqueueIndexWrite(chatId, entriesSnapshot) {
    const snapshot = entriesSnapshot.map((entry) => ({ mesId: entry.mesId, recordId: entry.recordId }));
    const key = getIndexKey(chatId);
    const write = indexWriteChain.then(() => promptStorage.setItem(key, snapshot));
    indexWriteChain = write.catch((error) => {
        console.error('Error saving itemized prompts index', { chatId }, error);
    });
    return write;
}

async function loadPromptIndex(chatId) {
    const value = await promptStorage.getItem(getIndexKey(chatId));
    if (!value) {
        return null;
    }
    if (!Array.isArray(value)) {
        throw new Error(`Invalid tt_prompts_index for chatId: ${chatId}`);
    }
    return /** @type {ItemizedPromptIndexEntry[]} */ (value);
}

async function migrateLegacyPrompts(chatId) {
    // One-time migration: convert legacy per-chat array into (index + per-record) storage.
    // This still reads the legacy array once; subsequent loads only pull the small index.
    const legacy = await promptStorage.getItem(chatId);
    if (!legacy) {
        await enqueueIndexWrite(chatId, []);
        return [];
    }
    if (!Array.isArray(legacy)) {
        throw new Error(`Invalid legacy itemized prompts for chatId: ${chatId}`);
    }

    /** @type {ItemizedPromptIndexEntry[]} */
    const entries = [];
    const seen = new Set();

    for (let i = legacy.length - 1; i >= 0; i -= 1) {
        const record = legacy[i];
        const mesId = Number(record?.mesId);
        if (!Number.isFinite(mesId)) {
            throw new Error(`Invalid legacy itemized prompt: mesId missing for chatId: ${chatId}`);
        }
        if (seen.has(mesId)) {
            continue;
        }
        seen.add(mesId);
        const recordId = uuidv4();
        await promptStorage.setItem(getRecordKey(chatId, recordId), record);
        entries.push({ mesId, recordId });
    }

    entries.sort((a, b) => a.mesId - b.mesId);
    await enqueueIndexWrite(chatId, entries);
    await promptStorage.removeItem(chatId);
    return entries;
}

async function clonePromptStore(sourceChatId, targetChatId) {
    // Used by upstream flows that "save" prompts under a different chatId (e.g. clones/branches).
    // We keep recordIds stable and copy records so the target chat can lazily load them later.
    const targetRecordPrefix = getRecordKeyPrefix(targetChatId);
    const keys = await promptStorage.keys();
    await Promise.all(
        keys
            .filter((key) => key === targetChatId || key === getIndexKey(targetChatId) || key.startsWith(targetRecordPrefix))
            .map((key) => promptStorage.removeItem(key)),
    );

    const entriesSnapshot = itemizedPrompts.map((entry) => ({ mesId: entry.mesId, recordId: entry.recordId }));

    for (const entry of entriesSnapshot) {
        const record = await promptStorage.getItem(getRecordKey(sourceChatId, entry.recordId));
        if (record) {
            await promptStorage.setItem(getRecordKey(targetChatId, entry.recordId), record);
        }
    }

    await promptStorage.setItem(getIndexKey(targetChatId), entriesSnapshot);
}

/**
 * Gets the itemized prompts for a chat.
 * @param {string} chatId Chat ID to load
 */
export async function loadItemizedPrompts(chatId) {
    try {
        if (!chatId) {
            clearActiveIndex();
            return;
        }

        const index = await loadPromptIndex(chatId);
        if (index) {
            setActiveIndex(chatId, index);
            return;
        }

        const migrated = await migrateLegacyPrompts(chatId);
        setActiveIndex(chatId, migrated);
    } catch (error) {
        console.error('Error loading itemized prompts for chat', { chatId }, error);
        clearActiveIndex();
    }
}

/**
 * Saves the itemized prompts for a chat.
 * @param {string} chatId Chat ID to save itemized prompts for
 */
export async function saveItemizedPrompts(chatId) {
    try {
        if (!chatId) {
            return;
        }

        if (!activeChatId) {
            return;
        }

        if (chatId === activeChatId) {
            await enqueueIndexWrite(activeChatId, itemizedPrompts);
            return;
        }

        await clonePromptStore(activeChatId, chatId);
    } catch (error) {
        console.error('Error saving itemized prompts for chat', { chatId }, error);
    }
}

/**
 * Replaces the itemized prompt text for a message.
 * @param {number} mesId Message ID to get itemized prompt for
 * @param {string} promptText New raw prompt text
 * @returns
 */
export async function replaceItemizedPromptText(mesId, promptText) {
    if (!activeChatId) {
        return;
    }

    const recordId = recordIdByMesId.get(Number(mesId));
    if (!recordId) {
        return;
    }

    const record = await promptStorage.getItem(getRecordKey(activeChatId, recordId));
    if (!record) {
        return;
    }

    record.rawPrompt = promptText;
    await promptStorage.setItem(getRecordKey(activeChatId, recordId), record);
}

/**
 * Deletes the itemized prompts for a chat.
 * @param {string} chatId Chat ID to delete itemized prompts for
 */
export async function deleteItemizedPrompts(chatId) {
    try {
        if (!chatId) {
            return;
        }

        const recordPrefix = getRecordKeyPrefix(chatId);
        const keys = await promptStorage.keys();
        await Promise.all(
            keys
                .filter((key) => key === chatId || key === getIndexKey(chatId) || key.startsWith(recordPrefix))
                .map((key) => promptStorage.removeItem(key)),
        );

        if (activeChatId === chatId) {
            clearActiveIndex();
        }
    } catch (error) {
        console.error('Error deleting itemized prompts for chat', { chatId }, error);
    }
}

/**
 * Empties the itemized prompts array and caches.
 */
export async function clearItemizedPrompts() {
    try {
        await promptStorage.clear();
        clearActiveIndex();
    } catch (error) {
        console.error('Error clearing itemized prompts', error);
    }
}

export function unloadItemizedPrompts() {
    // Only clears in-memory state; persistent storage is untouched.
    clearActiveIndex();
}

export function hasItemizedPromptForMessage(mesId) {
    // Fast path for UI: avoid loading any record data just to decide whether to render a button.
    return recordIdByMesId.has(Number(mesId));
}

export function upsertItemizedPromptRecord(record) {
    if (!activeChatId) {
        return;
    }

    const chatId = activeChatId;
    const mesId = Number(record?.mesId);
    if (!Number.isFinite(mesId)) {
        throw new Error('Cannot upsert itemized prompt record: mesId missing');
    }

    let recordId = recordIdByMesId.get(mesId);
    if (!recordId) {
        recordId = uuidv4();
        itemizedPrompts.push({ mesId, recordId });
        itemizedPrompts.sort((a, b) => a.mesId - b.mesId);
        rebuildRecordIdMap(itemizedPrompts);
    }

    void promptStorage.setItem(getRecordKey(chatId, recordId), record).catch((error) => {
        console.error('Error saving itemized prompt record', { chatId, mesId, recordId }, error);
    });

    void enqueueIndexWrite(chatId, itemizedPrompts);
}

export async function itemizedParams(itemizedPrompts, thisPromptSet, incomingMesId) {
    const params = {
        charDescriptionTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].charDescription),
        charPersonalityTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].charPersonality),
        scenarioTextTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].scenarioText),
        userPersonaStringTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].userPersona),
        worldInfoStringTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].worldInfoString),
        allAnchorsTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].allAnchors),
        summarizeStringTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].summarizeString),
        authorsNoteStringTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].authorsNoteString),
        smartContextStringTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].smartContextString),
        beforeScenarioAnchorTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].beforeScenarioAnchor),
        afterScenarioAnchorTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].afterScenarioAnchor),
        zeroDepthAnchorTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].zeroDepthAnchor), // TODO: unused
        thisPrompt_padding: itemizedPrompts[thisPromptSet].padding,
        this_main_api: itemizedPrompts[thisPromptSet].main_api,
        chatInjects: await getTokenCountAsync(itemizedPrompts[thisPromptSet].chatInjects),
        chatVectorsStringTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].chatVectorsString),
        dataBankVectorsStringTokens: await getTokenCountAsync(itemizedPrompts[thisPromptSet].dataBankVectorsString),
        modelUsed: chat[incomingMesId]?.extra?.model,
        apiUsed: chat[incomingMesId]?.extra?.api,
        presetName: itemizedPrompts[thisPromptSet].presetName || t`(Unknown)`,
        messagesCount: String(itemizedPrompts[thisPromptSet].messagesCount ?? ''),
        examplesCount: String(itemizedPrompts[thisPromptSet].examplesCount ?? ''),
    };

    const getFriendlyName = (value) => $(`#rm_api_block select option[value="${value}"]`).first().text() || value;

    if (params.apiUsed) {
        params.apiUsed = getFriendlyName(params.apiUsed);
    }

    if (params.this_main_api) {
        params.mainApiFriendlyName = getFriendlyName(params.this_main_api);
    }

    if (params.chatInjects) {
        params.ActualChatHistoryTokens = params.ActualChatHistoryTokens - params.chatInjects;
    }

    if (params.this_main_api == 'openai') {
        //for OAI API
        //console.log('-- Counting OAI Tokens');

        //params.finalPromptTokens = itemizedPrompts[thisPromptSet].oaiTotalTokens;
        params.oaiMainTokens = itemizedPrompts[thisPromptSet].oaiMainTokens;
        params.oaiStartTokens = itemizedPrompts[thisPromptSet].oaiStartTokens;
        params.ActualChatHistoryTokens = itemizedPrompts[thisPromptSet].oaiConversationTokens;
        params.examplesStringTokens = itemizedPrompts[thisPromptSet].oaiExamplesTokens;
        params.oaiPromptTokens = itemizedPrompts[thisPromptSet].oaiPromptTokens - (params.afterScenarioAnchorTokens + params.beforeScenarioAnchorTokens) + params.examplesStringTokens;
        params.oaiBiasTokens = itemizedPrompts[thisPromptSet].oaiBiasTokens;
        params.oaiJailbreakTokens = itemizedPrompts[thisPromptSet].oaiJailbreakTokens;
        params.oaiNudgeTokens = itemizedPrompts[thisPromptSet].oaiNudgeTokens;
        params.oaiImpersonateTokens = itemizedPrompts[thisPromptSet].oaiImpersonateTokens;
        params.oaiNsfwTokens = itemizedPrompts[thisPromptSet].oaiNsfwTokens;
        params.finalPromptTokens =
            params.oaiStartTokens +
            params.oaiPromptTokens +
            params.oaiMainTokens +
            params.oaiNsfwTokens +
            params.oaiBiasTokens +
            params.oaiImpersonateTokens +
            params.oaiJailbreakTokens +
            params.oaiNudgeTokens +
            params.ActualChatHistoryTokens +
            //charDescriptionTokens +
            //charPersonalityTokens +
            //allAnchorsTokens +
            params.worldInfoStringTokens +
            params.beforeScenarioAnchorTokens +
            params.afterScenarioAnchorTokens;
        // Max context size - max completion tokens
        params.thisPrompt_max_context = (oai_settings.openai_max_context - oai_settings.openai_max_tokens);

        //console.log('-- applying % on OAI tokens');
        params.oaiStartTokensPercentage = ((params.oaiStartTokens / (params.finalPromptTokens)) * 100).toFixed(2);
        params.storyStringTokensPercentage = (((params.afterScenarioAnchorTokens + params.beforeScenarioAnchorTokens + params.oaiPromptTokens) / (params.finalPromptTokens)) * 100).toFixed(2);
        params.ActualChatHistoryTokensPercentage = ((params.ActualChatHistoryTokens / (params.finalPromptTokens)) * 100).toFixed(2);
        params.promptBiasTokensPercentage = ((params.oaiBiasTokens / (params.finalPromptTokens)) * 100).toFixed(2);
        params.worldInfoStringTokensPercentage = ((params.worldInfoStringTokens / (params.finalPromptTokens)) * 100).toFixed(2);
        params.allAnchorsTokensPercentage = ((params.allAnchorsTokens / (params.finalPromptTokens)) * 100).toFixed(2);
        params.selectedTokenizer = getFriendlyTokenizerName(params.this_main_api).tokenizerName;
        params.oaiSystemTokens = params.oaiImpersonateTokens + params.oaiJailbreakTokens + params.oaiNudgeTokens + params.oaiStartTokens + params.oaiNsfwTokens + params.oaiMainTokens;
        params.oaiSystemTokensPercentage = ((params.oaiSystemTokens / (params.finalPromptTokens)) * 100).toFixed(2);
    } else {
        //for non-OAI APIs
        //console.log('-- Counting non-OAI Tokens');
        params.finalPromptTokens = await getTokenCountAsync(itemizedPrompts[thisPromptSet].finalPrompt);
        params.storyStringTokens = await getTokenCountAsync(itemizedPrompts[thisPromptSet].storyString) - params.worldInfoStringTokens;
        params.examplesStringTokens = await getTokenCountAsync(itemizedPrompts[thisPromptSet].examplesString);
        params.mesSendStringTokens = await getTokenCountAsync(itemizedPrompts[thisPromptSet].mesSendString);
        params.ActualChatHistoryTokens = params.mesSendStringTokens - (params.allAnchorsTokens - (params.beforeScenarioAnchorTokens + params.afterScenarioAnchorTokens)) + power_user.token_padding;
        params.instructionTokens = await getTokenCountAsync(itemizedPrompts[thisPromptSet].instruction);
        params.promptBiasTokens = await getTokenCountAsync(itemizedPrompts[thisPromptSet].promptBias);

        params.totalTokensInPrompt =
            params.storyStringTokens +     //chardefs total
            params.worldInfoStringTokens +
            params.examplesStringTokens + // example messages
            params.ActualChatHistoryTokens +  //chat history
            params.allAnchorsTokens +      // AN and/or legacy anchors
            //afterScenarioAnchorTokens +       //only counts if AN is set to 'after scenario'
            //zeroDepthAnchorTokens +           //same as above, even if AN not on 0 depth
            params.promptBiasTokens;       //{{}}
        //- thisPrompt_padding;  //not sure this way of calculating is correct, but the math results in same value as 'finalPrompt'
        params.thisPrompt_max_context = itemizedPrompts[thisPromptSet].this_max_context;
        params.thisPrompt_actual = params.thisPrompt_max_context - params.thisPrompt_padding;

        //console.log('-- applying % on non-OAI tokens');
        params.storyStringTokensPercentage = ((params.storyStringTokens / (params.totalTokensInPrompt)) * 100).toFixed(2);
        params.ActualChatHistoryTokensPercentage = ((params.ActualChatHistoryTokens / (params.totalTokensInPrompt)) * 100).toFixed(2);
        params.promptBiasTokensPercentage = ((params.promptBiasTokens / (params.totalTokensInPrompt)) * 100).toFixed(2);
        params.worldInfoStringTokensPercentage = ((params.worldInfoStringTokens / (params.totalTokensInPrompt)) * 100).toFixed(2);
        params.allAnchorsTokensPercentage = ((params.allAnchorsTokens / (params.totalTokensInPrompt)) * 100).toFixed(2);
        params.selectedTokenizer = itemizedPrompts[thisPromptSet]?.tokenizer || getFriendlyTokenizerName(params.this_main_api).tokenizerName;
    }
    return params;
}

export function findItemizedPromptSet(itemizedPrompts, incomingMesId) {
    let thisPromptSet = undefined;
    priorPromptArrayItemForRawPromptDisplay = -1;

    for (let i = 0; i < itemizedPrompts.length; i++) {
        console.log(`looking for ${incomingMesId} vs ${itemizedPrompts[i].mesId}`);
        if (itemizedPrompts[i].mesId === incomingMesId) {
            console.log(`found matching mesID ${i}`);
            thisPromptSet = i;
            PromptArrayItemForRawPromptDisplay = i;
            console.log(`wanting to raw display of ArrayItem: ${PromptArrayItemForRawPromptDisplay} which is mesID ${incomingMesId}`);
            console.log(itemizedPrompts[thisPromptSet]);
            break;
        } else if (itemizedPrompts[i].rawPrompt) {
            priorPromptArrayItemForRawPromptDisplay = i;
        }
    }
    return thisPromptSet;
}

async function promptItemizeRecords(promptRecords, requestedMesId) {
    console.log('PROMPT ITEMIZE ENTERED');
    var incomingMesId = Number(requestedMesId);
    console.debug(`looking for MesId ${incomingMesId}`);
    var thisPromptSet = findItemizedPromptSet(promptRecords, incomingMesId);

    if (thisPromptSet === undefined) {
        console.log(`couldnt find the right mesId. looked for ${incomingMesId}`);
        console.log(promptRecords);
        return null;
    }

    const params = await itemizedParams(promptRecords, thisPromptSet, incomingMesId);
    const flatten = (rawPrompt) => Array.isArray(rawPrompt) ? rawPrompt.map(x => x.content).join('\n') : rawPrompt;

    const template = params.this_main_api == 'openai'
        ? await renderTemplateAsync('itemizationChat', params)
        : await renderTemplateAsync('itemizationText', params);

    const popup = new Popup(template, POPUP_TYPE.TEXT);

    /** @type {HTMLElement} */
    const diffPrevPrompt = popup.dlg.querySelector('#diffPrevPrompt');
    if (priorPromptArrayItemForRawPromptDisplay >= 0) {
        diffPrevPrompt.style.display = '';
        diffPrevPrompt.addEventListener('click', function () {
            const dmp = new DiffMatchPatch();
            const text1 = flatten(promptRecords[priorPromptArrayItemForRawPromptDisplay].rawPrompt);
            const text2 = flatten(promptRecords[PromptArrayItemForRawPromptDisplay].rawPrompt);

            dmp.Diff_Timeout = 2.0;

            const d = dmp.diff_main(text1, text2);
            let ds = dmp.diff_prettyHtml(d);
            // make it readable
            ds = ds.replaceAll('background:#e6ffe6;', 'background:#b9f3b9; color:black;');
            ds = ds.replaceAll('background:#ffe6e6;', 'background:#f5b4b4; color:black;');
            ds = ds.replaceAll('&para;', '');
            const container = document.createElement('div');
            container.innerHTML = DOMPurify.sanitize(ds);
            const rawPromptWrapper = document.getElementById('rawPromptWrapper');
            rawPromptWrapper.replaceChildren(container);
            $('#rawPromptPopup').slideToggle();
        });
    } else {
        diffPrevPrompt.style.display = 'none';
    }
    popup.dlg.querySelector('#copyPromptToClipboard').addEventListener('pointerup', async function () {
        let rawPrompt = promptRecords[PromptArrayItemForRawPromptDisplay].rawPrompt;
        let rawPromptValues = rawPrompt;

        if (Array.isArray(rawPrompt)) {
            rawPromptValues = rawPrompt.map(x => x.content).join('\n');
        }

        await copyText(rawPromptValues);
        toastr.info(t`Copied!`);
    });

    popup.dlg.querySelector('#showRawPrompt').addEventListener('click', async function () {
        //console.log(promptRecords[PromptArrayItemForRawPromptDisplay].rawPrompt);
        console.log(PromptArrayItemForRawPromptDisplay);
        console.log(promptRecords);
        console.log(promptRecords[PromptArrayItemForRawPromptDisplay].rawPrompt);

        const rawPrompt = flatten(promptRecords[PromptArrayItemForRawPromptDisplay].rawPrompt);

        // Mobile needs special handholding. The side-view on the popup wouldn't work,
        // so we just show an additional popup for this.
        if (isMobile()) {
            const content = document.createElement('div');
            content.classList.add('tokenItemizingMaintext');
            content.innerText = rawPrompt;
            const popup = new Popup(content, POPUP_TYPE.TEXT, null, { allowVerticalScrolling: true, leftAlign: true });
            await popup.show();
            return;
        }

        //let DisplayStringifiedPrompt = JSON.stringify(itemizedPrompts[PromptArrayItemForRawPromptDisplay].rawPrompt).replace(/\n+/g, '<br>');
        const rawPromptWrapper = document.getElementById('rawPromptWrapper');
        rawPromptWrapper.innerText = rawPrompt;
        $('#rawPromptPopup').slideToggle();
    });

    await popup.show();
}

export async function promptItemize(itemizedPromptsOrRecords, requestedMesId) {
    // Backward-compatible entry point:
    // - Upstream passes an array of records (with rawPrompt/finalPrompt); we render directly.
    // - TauriTavern passes the index array and lazily loads only the clicked record(s).
    const first = Array.isArray(itemizedPromptsOrRecords) ? itemizedPromptsOrRecords[0] : null;
    const looksLikeRecordArray = Boolean(first && typeof first === 'object' && ('rawPrompt' in first || 'finalPrompt' in first));
    if (looksLikeRecordArray) {
        return await promptItemizeRecords(itemizedPromptsOrRecords, requestedMesId);
    }

    if (!activeChatId) {
        return null;
    }

    const incomingMesId = Number(requestedMesId);
    if (!Number.isFinite(incomingMesId)) {
        return null;
    }

    const recordId = recordIdByMesId.get(incomingMesId);
    if (!recordId) {
        return null;
    }

    const record = await promptStorage.getItem(getRecordKey(activeChatId, recordId));
    if (!record) {
        return null;
    }

    let priorRecord = null;
    for (let i = itemizedPrompts.length - 1; i >= 0; i -= 1) {
        const entry = itemizedPrompts[i];
        if (entry.mesId < incomingMesId) {
            priorRecord = await promptStorage.getItem(getRecordKey(activeChatId, entry.recordId));
            break;
        }
    }

    const recordsForInspector = priorRecord ? [priorRecord, record] : [record];
    return await promptItemizeRecords(recordsForInspector, incomingMesId);
}

export function initItemizedPrompts() {
    registerDebugFunction('clearPrompts', 'Delete itemized prompts', 'Deletes all itemized prompts from the local storage.', async () => {
        await clearItemizedPrompts();
        toastr.info('Itemized prompts deleted.');
        if (getCurrentChatId()) {
            await reloadCurrentChat();
        }
    });

    $(document).on('pointerup', '.mes_prompt', async function () {
        let mesIdForItemization = $(this).closest('.mes').attr('mesId');
        console.log(`looking for mesID: ${mesIdForItemization}`);
        if (itemizedPrompts.length !== undefined && itemizedPrompts.length !== 0) {
            await promptItemize(itemizedPrompts, mesIdForItemization);
        }
    });

    eventSource.on(event_types.CHAT_DELETED, async (name) => {
        await deleteItemizedPrompts(name);
    });
    eventSource.on(event_types.GROUP_CHAT_DELETED, async (name) => {
        await deleteItemizedPrompts(name);
    });
}

/**
 * Swaps the itemized prompts between two messages. Useful when moving messages around in the chat.
 * @param {number} sourceMessageId Source message ID
 * @param {number} targetMessageId Target message ID
 */
export function swapItemizedPrompts(sourceMessageId, targetMessageId) {
    if (!activeChatId) {
        return;
    }

    const chatId = activeChatId;
    const sourceEntry = itemizedPrompts.find((x) => x.mesId === sourceMessageId);
    const targetEntry = itemizedPrompts.find((x) => x.mesId === targetMessageId);
    if (!sourceEntry && !targetEntry) {
        return;
    }
    sourceEntry && (sourceEntry.mesId = targetMessageId);
    targetEntry && (targetEntry.mesId = sourceMessageId);
    itemizedPrompts.sort((a, b) => a.mesId - b.mesId);
    rebuildRecordIdMap(itemizedPrompts);
    void enqueueIndexWrite(chatId, itemizedPrompts);
}

/**
 * Deletes the itemized prompt for a specific message.
 * Shifts down other itemized prompts as necessary.
 * @param {number} messageId Message ID to delete itemized prompt for
 */
export function deleteItemizedPromptForMessage(messageId) {
    if (!activeChatId) {
        return;
    }

    const chatId = activeChatId;
    const mesId = Number(messageId);
    const deletedRecordId = recordIdByMesId.get(mesId) ?? null;

    itemizedPrompts = itemizedPrompts.filter((x) => x.mesId !== mesId);
    for (const entry of itemizedPrompts) {
        if (entry.mesId > mesId) {
            entry.mesId -= 1;
        }
    }

    itemizedPrompts.sort((a, b) => a.mesId - b.mesId);
    rebuildRecordIdMap(itemizedPrompts);
    void enqueueIndexWrite(chatId, itemizedPrompts);

    if (deletedRecordId) {
        void promptStorage.removeItem(getRecordKey(chatId, deletedRecordId)).catch((error) => {
            console.error('Error deleting itemized prompt record', { chatId, mesId, recordId: deletedRecordId }, error);
        });
    }
}
