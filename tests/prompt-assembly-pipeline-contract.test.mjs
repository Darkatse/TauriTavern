import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
    INJECTION_POSITION,
    applyPromptManagerOverrides,
    getPromptInjectionGroups,
    getPromptInjectionPosition,
    isPromptInjectionPosition,
} from '../src/scripts/prompt-injections.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const YUAN_PRESET_PATH = path.join(REPO_ROOT, '.cache', 'Yuan ultranova 0.4.json');
const SILLYTAVERN_OPENAI_PATH = path.join(REPO_ROOT, 'sillytavern-1.16.0', 'public', 'scripts', 'openai.js');
const SILLYTAVERN_PROMPT_MANAGER_PATH = path.join(REPO_ROOT, 'sillytavern-1.16.0', 'public', 'scripts', 'PromptManager.js');

const DEFAULT_ORDER_ID = 100000;
const COMPLEX_YUAN_ORDER_ID = 100001;
const DEFAULT_INJECTION_ORDER = 100;
const MAX_EXTENSION_PROMPT_DEPTH = 4;

const FIXED_STANDALONE_PROMPTS = [
    'worldInfoBefore',
    'main',
    'worldInfoAfter',
    'charDescription',
    'charPersonality',
    'scenario',
    'personaDescription',
];

const SYSTEM_STANDALONE_PROMPTS = ['nsfw', 'jailbreak'];
const KNOWN_EXTENSION_PROMPTS = ['summary', 'authorsNote', 'vectorsMemory', 'vectorsDataBank', 'smartContext'];

const YUAN_COMPLEX_RELATIVE_USER_PROMPTS = [
    'c778f74a-bd06-4926-84da-4fab7b6286e4',
    '101ee207-f199-4266-b092-cbc7feeec239',
    'f7b95853-8054-4e2d-83aa-00bcbff62bfa',
    'd5184b2e-196b-4b02-ba76-e80154f3f4fc',
    'cb32d3a7-b02e-4816-a8cd-4921095983a2',
    '53dd67cf-c564-4dee-93b4-c2da918da832',
    '8210f8c2-d2a3-474b-97b7-5759b7fb0b17',
    '3e7813ed-61fb-4d2c-bfc1-b64eeac1e25e',
];

const YUAN_COMPLEX_ABSOLUTE_PROMPT = 'c2edd7c0-48f4-4c60-9b83-d06f014703c1';
const YUAN_COMPLEX_DISABLED_PROMPTS = [
    '42091d0a-4b08-454d-9eb0-1627fa8817b1',
    '18fb8ab5-3a9b-4eb9-9d53-022fbe3113e4',
];

test('SillyTavern 1.16.0 prompt-position baseline is the contract mirrored by this test', async (t) => {
    const [promptManagerSource, openaiSource] = await Promise.all([
        readOptionalFile(SILLYTAVERN_PROMPT_MANAGER_PATH),
        readOptionalFile(SILLYTAVERN_OPENAI_PATH),
    ]);

    if (!promptManagerSource || !openaiSource) {
        t.skip('sillytavern-1.16.0 symlink is not available');
        return;
    }

    assert.match(promptManagerSource, /RELATIVE:\s*0,\s*ABSOLUTE:\s*1/);
    assert.doesNotMatch(promptManagerSource, /ATTACH_EXISTING/);
    assert.match(openaiSource, /prompt\.injection_position !== INJECTION_POSITION\.ABSOLUTE/);
    assert.match(openaiSource, /prompt\.injection_position === INJECTION_POSITION\.ABSOLUTE/);
    assert.match(openaiSource, /prompt\.injection_position = collectionPrompt\.injection_position \?\? prompt\.injection_position/);
});

test('Yuan ultranova preset prompt assembly matches SillyTavern for every prompt order in the preset', async () => {
    const preset = await loadYuanPreset();

    for (const promptOrder of preset.prompt_order) {
        const orderId = Number(promptOrder.character_id);
        const sillyTavernPlan = buildPromptAssemblyPlan(preset, orderId, 'sillytavern');
        const tauriTavernPlan = buildPromptAssemblyPlan(preset, orderId, 'tauritavern');

        assert.deepEqual(tauriTavernPlan, sillyTavernPlan, `prompt order ${orderId} diverged`);
    }
});

test('Yuan ultranova complex prompt order keeps the expected assembly shape', async () => {
    const preset = await loadYuanPreset();
    const plan = buildPromptAssemblyPlan(preset, COMPLEX_YUAN_ORDER_ID, 'tauritavern');

    assert.equal(plan.activePromptOrderId, COMPLEX_YUAN_ORDER_ID);
    assert.deepEqual(plan.disabledPromptIds, YUAN_COMPLEX_DISABLED_PROMPTS);
    assert.deepEqual(plan.groups.userRelativePromptIds, YUAN_COMPLEX_RELATIVE_USER_PROMPTS);
    assert.deepEqual(plan.groups.absolutePromptIds, [YUAN_COMPLEX_ABSOLUTE_PROMPT]);
    assert.deepEqual(plan.groups.attachedPromptIds, []);
    assert.deepEqual(plan.groups.legacyRelativeWithoutExplicitPosition, ['chatHistory', 'dialogueExamples']);

    assert.deepEqual(plan.standalonePromptIds, [
        ...FIXED_STANDALONE_PROMPTS,
        ...SYSTEM_STANDALONE_PROMPTS,
        ...YUAN_COMPLEX_RELATIVE_USER_PROMPTS,
        'enhanceDefinitions',
        'bias',
    ]);

    assert.equal(plan.absolutePrompts[0].identifier, YUAN_COMPLEX_ABSOLUTE_PROMPT);
    assert.equal(plan.absolutePrompts[0].injectionDepth, 1);
    assert.equal(plan.absolutePrompts[0].injectionOrder, -99999);
    assert.equal(plan.absolutePrompts[0].role, 'user');

    assert.deepEqual(plan.collectionSlots, [
        { identifier: 'chatHistory', index: 13 },
        { identifier: 'dialogueExamples', index: 15 },
    ]);

    assert.deepEqual(plan.controlPromptIds, ['quietPrompt']);
    assert.deepEqual(plan.groupNudgePromptIds, ['groupNudge']);
    assert.deepEqual(plan.relativeExtensionInsertions.map(insertion => [insertion.identifier, insertion.position]), [
        ['summary', 'start'],
        ['authorsNote', 'end'],
        ['custom_extension', 'end'],
    ]);

    const injectedMessage = plan.inChatMessages.find(message => message.injected);
    assert.ok(injectedMessage, 'expected the depth prompt to be inserted into chat history');
    assert.equal(injectedMessage.role, 'user');
    assert.equal(injectedMessage.contentHash, plan.absolutePrompts[0].contentHash);
});

test('Yuan ultranova default order also preserves legacy relative marker semantics', async () => {
    const preset = await loadYuanPreset();
    const plan = buildPromptAssemblyPlan(preset, DEFAULT_ORDER_ID, 'tauritavern');

    assert.equal(plan.activePromptOrderId, DEFAULT_ORDER_ID);
    assert.deepEqual(plan.groups.absolutePromptIds, []);
    assert.deepEqual(plan.groups.attachedPromptIds, []);
    assert.deepEqual(plan.groups.legacyRelativeWithoutExplicitPosition, ['dialogueExamples', 'chatHistory']);
    assert.ok(plan.standalonePromptIds.includes('main'));
    assert.ok(!plan.standalonePromptIds.includes('enhanceDefinitions'));
});

async function loadYuanPreset() {
    const text = await readOptionalFile(YUAN_PRESET_PATH);
    return text ? JSON.parse(text) : createYuanPresetFallback();
}

async function readOptionalFile(filePath) {
    try {
        return await readFile(filePath, 'utf8');
    } catch (error) {
        if (error?.code === 'ENOENT') {
            return null;
        }

        throw error;
    }
}

function createYuanPresetFallback() {
    const prompt = (identifier, fields = {}) => ({
        identifier,
        name: identifier,
        content: identifier.toUpperCase(),
        role: 'user',
        system_prompt: false,
        marker: false,
        injection_position: INJECTION_POSITION.RELATIVE,
        injection_depth: 4,
        injection_order: DEFAULT_INJECTION_ORDER,
        ...fields,
    });

    const marker = identifier => prompt(identifier, {
        content: '',
        system_prompt: true,
        marker: true,
    });

    return {
        prompts: [
            prompt('main', { system_prompt: true, content: 'MAIN' }),
            prompt('enhanceDefinitions', { system_prompt: true, content: '' }),
            prompt('nsfw', { system_prompt: true, content: '' }),
            prompt('jailbreak', { system_prompt: true, content: '' }),
            marker('worldInfoBefore'),
            marker('worldInfoAfter'),
            marker('charDescription'),
            marker('charPersonality'),
            marker('scenario'),
            marker('personaDescription'),
            prompt('chatHistory', { content: '', system_prompt: true, marker: true, role: undefined, injection_position: undefined, injection_depth: undefined }),
            prompt('dialogueExamples', { content: '', system_prompt: true, marker: true, role: undefined, injection_position: undefined, injection_depth: undefined }),
            prompt('c778f74a-bd06-4926-84da-4fab7b6286e4'),
            prompt('42091d0a-4b08-454d-9eb0-1627fa8817b1', { role: 'assistant' }),
            prompt('101ee207-f199-4266-b092-cbc7feeec239'),
            prompt('f7b95853-8054-4e2d-83aa-00bcbff62bfa'),
            prompt('d5184b2e-196b-4b02-ba76-e80154f3f4fc'),
            prompt(YUAN_COMPLEX_ABSOLUTE_PROMPT, {
                injection_position: INJECTION_POSITION.ABSOLUTE,
                injection_depth: 1,
                injection_order: -99999,
            }),
            prompt('cb32d3a7-b02e-4816-a8cd-4921095983a2'),
            prompt('53dd67cf-c564-4dee-93b4-c2da918da832'),
            prompt('8210f8c2-d2a3-474b-97b7-5759b7fb0b17'),
            prompt('3e7813ed-61fb-4d2c-bfc1-b64eeac1e25e'),
            prompt('18fb8ab5-3a9b-4eb9-9d53-022fbe3113e4', { role: 'assistant' }),
        ],
        prompt_order: [
            {
                character_id: DEFAULT_ORDER_ID,
                order: [
                    { enabled: true, identifier: 'main' },
                    { enabled: true, identifier: 'worldInfoBefore' },
                    { enabled: true, identifier: 'charDescription' },
                    { enabled: true, identifier: 'charPersonality' },
                    { enabled: true, identifier: 'scenario' },
                    { enabled: false, identifier: 'enhanceDefinitions' },
                    { enabled: true, identifier: 'nsfw' },
                    { enabled: true, identifier: 'worldInfoAfter' },
                    { enabled: true, identifier: 'dialogueExamples' },
                    { enabled: true, identifier: 'chatHistory' },
                    { enabled: true, identifier: 'jailbreak' },
                ],
            },
            {
                character_id: COMPLEX_YUAN_ORDER_ID,
                order: [
                    { enabled: true, identifier: 'main' },
                    { enabled: true, identifier: 'c778f74a-bd06-4926-84da-4fab7b6286e4' },
                    { enabled: false, identifier: '42091d0a-4b08-454d-9eb0-1627fa8817b1' },
                    { enabled: true, identifier: '101ee207-f199-4266-b092-cbc7feeec239' },
                    { enabled: true, identifier: 'worldInfoBefore' },
                    { enabled: true, identifier: 'personaDescription' },
                    { enabled: true, identifier: 'charDescription' },
                    { enabled: true, identifier: 'charPersonality' },
                    { enabled: true, identifier: 'scenario' },
                    { enabled: true, identifier: 'enhanceDefinitions' },
                    { enabled: true, identifier: 'nsfw' },
                    { enabled: true, identifier: 'worldInfoAfter' },
                    { enabled: true, identifier: 'f7b95853-8054-4e2d-83aa-00bcbff62bfa' },
                    { enabled: true, identifier: 'd5184b2e-196b-4b02-ba76-e80154f3f4fc' },
                    { enabled: true, identifier: 'chatHistory' },
                    { enabled: true, identifier: 'jailbreak' },
                    { enabled: true, identifier: 'dialogueExamples' },
                    { enabled: true, identifier: YUAN_COMPLEX_ABSOLUTE_PROMPT },
                    { enabled: true, identifier: 'cb32d3a7-b02e-4816-a8cd-4921095983a2' },
                    { enabled: true, identifier: '53dd67cf-c564-4dee-93b4-c2da918da832' },
                    { enabled: true, identifier: '8210f8c2-d2a3-474b-97b7-5759b7fb0b17' },
                    { enabled: true, identifier: '3e7813ed-61fb-4d2c-bfc1-b64eeac1e25e' },
                    { enabled: false, identifier: '18fb8ab5-3a9b-4eb9-9d53-022fbe3113e4' },
                ],
            },
        ],
    };
}

function buildPromptAssemblyPlan(preset, activePromptOrderId, mode) {
    const prompts = buildActivePromptCollection(preset, activePromptOrderId, mode);
    mergeGeneratedPrompts(prompts, mode);

    const groups = getGroups(prompts, mode);
    const standaloneTrace = buildStandaloneTrace(prompts, groups, mode);
    const relativeExtensionInsertions = buildRelativeExtensionInsertions(prompts, standaloneTrace, mode);
    const absolutePrompts = groups.absolutePrompts.map(prompt => summarizePrompt(prompt, mode));
    const inChatMessages = simulateInChatInjection(groups.absolutePrompts, createSampleChat()).map(summarizeMessage);

    return {
        activePromptOrderId,
        disabledPromptIds: prompts.disabledPromptIds,
        preparedCollection: prompts.collection.map(prompt => summarizePrompt(prompt, mode)),
        groups: {
            userRelativePromptIds: groups.userRelativePromptIds,
            absolutePromptIds: groups.absolutePrompts.map(prompt => prompt.identifier),
            attachedPromptIds: groups.attachedPrompts.map(prompt => prompt.identifier),
            legacyRelativeWithoutExplicitPosition: prompts.collection
                .filter(prompt => prompt.__contractSource === 'preset')
                .filter(prompt => prompt.injection_position === undefined && isRelativePrompt(prompt, mode))
                .map(prompt => prompt.identifier),
        },
        standalonePromptIds: standaloneTrace.map(entry => entry.identifier),
        standaloneTrace,
        absolutePrompts,
        attachedPrompts: groups.attachedPrompts.map(prompt => summarizePrompt(prompt, mode)),
        relativeExtensionInsertions,
        collectionSlots: ['chatHistory', 'dialogueExamples']
            .filter(identifier => prompts.has(identifier))
            .map(identifier => ({ identifier, index: prompts.index(identifier) })),
        controlPromptIds: getRelativePromptIds(prompts, ['impersonate', 'quietPrompt'], mode)
            .filter(identifier => identifier !== 'impersonate'),
        groupNudgePromptIds: getRelativePromptIds(prompts, ['groupNudge'], mode),
        inChatMessages,
    };
}

function buildActivePromptCollection(preset, activePromptOrderId, mode) {
    const promptOrder = preset.prompt_order.find(order => Number(order.character_id) === Number(activePromptOrderId));
    assert.ok(promptOrder, `missing prompt order ${activePromptOrderId}`);

    const promptById = new Map(preset.prompts.map(prompt => [prompt.identifier, prompt]));
    const prompts = new PromptCollectionContract();

    for (const entry of promptOrder.order) {
        const prompt = promptById.get(entry.identifier);

        if (!prompt) {
            continue;
        }

        if (entry.enabled && shouldTrigger(prompt, 'normal')) {
            prompts.add(markSource(preparePrompt(prompt, mode), 'preset'));
        } else if (entry.identifier === 'main') {
            prompts.add(markSource(preparePrompt({ ...prompt, content: '' }, mode), 'preset'));
        } else {
            prompts.disabledPromptIds.push(entry.identifier);
        }
    }

    return prompts;
}

function mergeGeneratedPrompts(prompts, mode) {
    for (const generatedPrompt of createGeneratedPrompts()) {
        const prompt = { ...generatedPrompt };
        const collectionPrompt = prompts.get(prompt.identifier);

        if (mode === 'tauritavern') {
            applyPromptManagerOverrides(prompt, collectionPrompt);
        } else {
            applySillyTavernPromptManagerOverrides(prompt, collectionPrompt);
        }

        const preparedPrompt = markSource(preparePrompt(prompt, mode), 'generated');
        const markerIndex = prompts.index(prompt.identifier);

        if (markerIndex !== -1) {
            prompts.collection[markerIndex] = preparedPrompt;
        } else {
            prompts.add(preparedPrompt);
        }
    }
}

function createGeneratedPrompts() {
    return [
        { role: 'system', content: 'WORLD_INFO_BEFORE', identifier: 'worldInfoBefore' },
        { role: 'system', content: 'WORLD_INFO_AFTER', identifier: 'worldInfoAfter' },
        { role: 'system', content: 'CHAR_DESCRIPTION', identifier: 'charDescription' },
        { role: 'system', content: 'CHAR_PERSONALITY', identifier: 'charPersonality' },
        { role: 'system', content: 'SCENARIO', identifier: 'scenario' },
        { role: 'system', content: 'IMPERSONATE', identifier: 'impersonate' },
        { role: 'system', content: 'QUIET_PROMPT', identifier: 'quietPrompt' },
        { role: 'system', content: 'GROUP_NUDGE', identifier: 'groupNudge' },
        { role: 'assistant', content: 'BIAS', identifier: 'bias' },
        { role: 'system', content: 'PERSONA_DESCRIPTION', identifier: 'personaDescription' },
        { role: 'system', content: 'SUMMARY', identifier: 'summary', position: 'start' },
        { role: 'user', content: 'AUTHORS_NOTE', identifier: 'authorsNote', position: 'end' },
        { role: 'system', content: 'CUSTOM_EXTENSION', identifier: 'custom_extension', position: 'end', extension: true },
    ];
}

function buildStandaloneTrace(prompts, groups, mode) {
    const trace = [];
    const addToChatCompletion = (identifier, target = null) => {
        if (!prompts.has(identifier)) {
            return;
        }

        const prompt = prompts.get(identifier);
        if (!isRelativePrompt(prompt, mode)) {
            return;
        }

        trace.push({
            identifier,
            target,
            collectionIndex: target ? prompts.index(target) : prompts.index(identifier),
            prompt: summarizePrompt(prompt, mode),
        });
    };

    for (const identifier of FIXED_STANDALONE_PROMPTS) {
        addToChatCompletion(identifier);
    }

    for (const identifier of [...SYSTEM_STANDALONE_PROMPTS, ...groups.userRelativePromptIds]) {
        addToChatCompletion(identifier);
    }

    if (prompts.has('enhanceDefinitions')) {
        addToChatCompletion('enhanceDefinitions');
    }

    addToChatCompletion('bias');

    return trace;
}

function buildRelativeExtensionInsertions(prompts, standaloneTrace, mode) {
    const mainWasAdded = standaloneTrace.some(entry => entry.identifier === 'main');
    const insertions = [];
    const injectToMain = (prompt) => {
        if (!prompt?.position || !isRelativePrompt(prompt, mode)) {
            return;
        }

        insertions.push({
            identifier: prompt.identifier,
            position: prompt.position,
            destination: mainWasAdded ? 'main' : 'absolutePrompts',
            prompt: summarizePrompt(prompt, mode),
        });
    };

    for (const identifier of KNOWN_EXTENSION_PROMPTS) {
        if (prompts.has(identifier)) {
            injectToMain(prompts.get(identifier));
        }
    }

    for (const prompt of prompts.collection.filter(prompt => prompt.extension && prompt.position)) {
        injectToMain(prompt);
    }

    return insertions;
}

function getGroups(prompts, mode) {
    if (mode === 'tauritavern') {
        return getPromptInjectionGroups(prompts);
    }

    return {
        userRelativePromptIds: prompts.collection
            .filter(prompt => prompt.system_prompt === false && prompt.injection_position !== INJECTION_POSITION.ABSOLUTE)
            .map(prompt => prompt.identifier),
        absolutePrompts: prompts.collection
            .filter(prompt => prompt.injection_position === INJECTION_POSITION.ABSOLUTE),
        attachedPrompts: [],
    };
}

function getRelativePromptIds(prompts, identifiers, mode) {
    return identifiers.filter(identifier => {
        const prompt = prompts.get(identifier);
        return prompt && isRelativePrompt(prompt, mode);
    });
}

function isRelativePrompt(prompt, mode) {
    if (mode === 'tauritavern') {
        return isPromptInjectionPosition(prompt, INJECTION_POSITION.RELATIVE);
    }

    return prompt?.injection_position !== INJECTION_POSITION.ABSOLUTE;
}

function getNormalizedPosition(prompt, mode) {
    if (mode === 'tauritavern') {
        return getPromptInjectionPosition(prompt);
    }

    return prompt?.injection_position === INJECTION_POSITION.ABSOLUTE
        ? INJECTION_POSITION.ABSOLUTE
        : INJECTION_POSITION.RELATIVE;
}

function applySillyTavernPromptManagerOverrides(prompt, collectionPrompt) {
    if (!collectionPrompt) {
        return prompt;
    }

    for (const key of ['injection_position', 'injection_depth', 'injection_order', 'role']) {
        if (collectionPrompt[key] !== undefined) {
            prompt[key] = collectionPrompt[key];
        }
    }

    return prompt;
}

function preparePrompt(prompt, mode) {
    const preparedPrompt = {
        identifier: prompt.identifier,
        role: prompt.role,
        content: prompt.content,
        name: prompt.name,
        system_prompt: prompt.system_prompt,
        position: prompt.position,
        injection_depth: prompt.injection_depth,
        injection_position: prompt.injection_position,
        forbid_overrides: prompt.forbid_overrides,
        extension: prompt.extension ?? false,
        injection_order: prompt.injection_order ?? DEFAULT_INJECTION_ORDER,
        injection_trigger: prompt.injection_trigger ?? [],
    };

    if (mode === 'tauritavern') {
        preparedPrompt.attach_role = prompt.attach_role ?? 'user';
        preparedPrompt.attach_index = prompt.attach_index ?? 1;
        preparedPrompt.attach_side = prompt.attach_side ?? 'end';
    }

    return preparedPrompt;
}

function markSource(prompt, source) {
    prompt.__contractSource = source;
    return prompt;
}

function shouldTrigger(prompt, generationType) {
    if (!Array.isArray(prompt?.injection_trigger)) {
        return true;
    }

    if (!prompt.injection_trigger.length) {
        return true;
    }

    return prompt.injection_trigger.includes(generationType);
}

function simulateInChatInjection(prompts, messages) {
    let totalInsertedMessages = 0;

    for (let depth = 0; depth <= MAX_EXTENSION_PROMPT_DEPTH; depth++) {
        const depthPrompts = prompts.filter(prompt => prompt.injection_depth === depth && prompt.content);
        const orderGroups = { [String(DEFAULT_INJECTION_ORDER)]: [] };

        for (const prompt of depthPrompts) {
            const order = String(prompt.injection_order ?? DEFAULT_INJECTION_ORDER);
            orderGroups[order] ??= [];
            orderGroups[order].push(prompt);
        }

        const roleMessages = [];
        const orders = Object.keys(orderGroups).sort((a, b) => Number(b) - Number(a));

        for (const order of orders) {
            const orderPrompts = orderGroups[order];

            for (const role of ['system', 'user', 'assistant']) {
                const content = orderPrompts
                    .filter(prompt => prompt.role === role)
                    .map(prompt => prompt.content)
                    .join('\n')
                    .trim();

                if (content) {
                    roleMessages.push({ role, content, injected: true });
                }
            }
        }

        if (roleMessages.length) {
            const injectIndex = depth + totalInsertedMessages;
            messages.splice(injectIndex, 0, ...roleMessages);
            totalInsertedMessages += roleMessages.length;
        }
    }

    return messages.reverse();
}

function createSampleChat() {
    return [
        { role: 'assistant', content: 'ASSISTANT_NEWEST' },
        { role: 'user', content: 'USER_NEWEST' },
        { role: 'assistant', content: 'ASSISTANT_OLDER' },
        { role: 'user', content: 'USER_OLDER' },
    ];
}

function summarizePrompt(prompt, mode) {
    const position = getNormalizedPosition(prompt, mode);

    return {
        identifier: prompt.identifier,
        role: prompt.role ?? null,
        systemPrompt: prompt.system_prompt === true,
        extension: prompt.extension === true,
        relativeExtensionPosition: prompt.position ?? null,
        injectionPosition: position,
        injectionDepth: prompt.injection_depth ?? null,
        injectionOrder: prompt.injection_order ?? null,
        contentLength: String(prompt.content ?? '').length,
        contentHash: hashContent(prompt.content),
        attach: position === INJECTION_POSITION.ATTACH_EXISTING
            ? {
                role: prompt.attach_role ?? null,
                index: prompt.attach_index ?? null,
                side: prompt.attach_side ?? null,
            }
            : null,
    };
}

function summarizeMessage(message) {
    return {
        role: message.role,
        injected: message.injected === true,
        contentLength: String(message.content ?? '').length,
        contentHash: hashContent(message.content),
    };
}

function hashContent(content) {
    return createHash('sha256')
        .update(String(content ?? ''))
        .digest('hex');
}

class PromptCollectionContract {
    collection = [];
    overriddenPrompts = [];
    disabledPromptIds = [];

    add(...prompts) {
        this.collection.push(...prompts);
    }

    has(identifier) {
        return this.index(identifier) !== -1;
    }

    get(identifier) {
        return this.collection.find(prompt => prompt.identifier === identifier);
    }

    index(identifier) {
        return this.collection.findIndex(prompt => prompt.identifier === identifier);
    }
}
