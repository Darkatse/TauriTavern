// The data the perf suites run on. The frontend suite serves it through the Web Host; the backend replay reads the
// same data from a data root laid out like the native host's.
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { crc32, deflateSync } from 'node:zlib';
import { createWebHostFixture } from '../helpers/web-host-fixture.mjs';
import { defaultSettings, defaultTheme, systemExtensions } from '../helpers/frontend-content.mjs';
import { statusBarDocument } from './status-bar.mjs';

/**
 * A long chat and a large lorebook; the first entry holds 100k characters. Ledger is a heavy card: an embedded
 * lorebook of about 2M characters, and a chat whose last reply holds about 150k characters. The in-app assistant
 * has one Session of 20 tasks, each ten tool rounds and a reply: 440 messages, about 2 MB of history.
 */
export function dataset() {
    const paragraph = 'The old lighthouse keeper remembers every ship that passed the northern reef, and speaks of them as friends. ';
    const chat = [{ user_name: 'User', character_name: 'Review', chat_metadata: { integrity: 'review-integrity' } }];
    for (let i = 0; i < 80; i++) {
        chat.push({
            name: i % 2 ? 'User' : 'Review', is_user: i % 2 === 1, is_system: false, send_date: '2026-01-01T12:00:00Z',
            // Replies carry their model icon, as in a real chat, so a new reply's icon is already loaded.
            extra: i % 2 ? {} : { api: 'custom', model: 'replay' },
            mes: `*She glances at the map.* "${paragraph.repeat(3)}"\n\n**Note:** ${paragraph}`,
        });
    }
    const entries = {};
    for (let uid = 0; uid < 200; uid++) {
        entries[uid] = {
            uid, key: [`key${uid}`, `alias${uid}`], keysecondary: [], comment: `Entry ${uid}`, content: paragraph.repeat(8),
            constant: false, selective: true, order: 100, position: 0, disable: false, displayIndex: uid, probability: 100, useProbability: true,
        };
    }
    entries[0].content = `${paragraph.repeat(4)}\n\n`.repeat(250).slice(0, 100_000);
    const book = {
        entries: Array.from({ length: 1000 }, (_, id) => ({
            id, keys: [`ledger${id}`], content: paragraph.repeat(18), enabled: true, insertion_order: 100, extensions: {},
        })),
    };
    const ledgerCard = JSON.stringify({
        spec: 'chara_card_v2', spec_version: '2.0', name: 'Ledger', chat: 'ledger-chat', create_date: '2026-01-02T12:00:00Z',
        data: {
            name: 'Ledger', description: paragraph.repeat(20), personality: '', scenario: '', first_mes: paragraph.repeat(10),
            mes_example: '', creator: '', creator_notes: '', character_version: '', tags: [], alternate_greetings: [],
            system_prompt: '', post_history_instructions: '', character_book: book, extensions: { talkativeness: 0.5, fav: false },
        },
    });
    const ledgerChat = [{ user_name: 'User', character_name: 'Ledger', chat_metadata: { integrity: 'ledger-integrity' } }];
    for (let i = 0; i < 6; i++) {
        ledgerChat.push({
            name: i % 2 ? 'User' : 'Ledger', is_user: i % 2 === 1, is_system: false, send_date: `2026-01-02T12:0${i}:00Z`,
            extra: i % 2 ? {} : { api: 'custom', model: 'replay' },
            mes: i === 5 ? paragraph.repeat(1400) : paragraph.repeat(4),
        });
    }
    return { chat, lore: { entries }, ledgerCard, ledgerChat, ...frontendCard(), assistant: assistantSession(paragraph) };
}

/**
 * Atlas, a front-end card. A reply holds reasoning, a story body, a variable update and a status bar placeholder. The
 * card's regex scripts restyle the body and the update, and put the status bar in place of the latest two placeholders.
 */
function frontendCard() {
    const lines = [
        '雾港的钟声在黄昏时分响起，湿冷的海风卷着盐味穿过狭窄的石巷。',
        '艾琳把斗篷的兜帽拉低了一些，目光越过码头上堆叠的木箱，落在那艘没有挂旗的货船上。',
        '洛岚靠在潮湿的墙边，手指轻轻敲着弓背，像是在数着守卫换岗的间隔。',
        '“商队最后一次出现，就是在这条街上。”维克托压低声音，把一张揉皱的地图摊在膝头。',
        '远处的灯塔亮了起来，光束扫过海面，又被浓雾一点点吞没。',
        '赛菲娅没有说话，只是把护符握在掌心，那块旧石头正在微微发热。',
        '酒馆里传来断断续续的歌声，夹杂着骰子落在木桌上的声响。',
        '“如果那封信是真的，今晚就会有人来取货。”艾琳的语气很平静，却没有给任何人反驳的余地。',
        '守卫提着灯笼从巷口经过，铁靴踩在积水里，溅起细碎的水花。',
        '诺德从屋檐上滑下来，落地时几乎没有发出声音，他朝众人比了一个手势。',
        '货船的舱门被推开了一条缝，昏黄的灯光从里面漏出来，照亮了甲板上的一串湿脚印。',
        '空气里忽然多了一股甜腻的香料味，那是赤砂集市才有的东西。',
        '“分头走，”维克托把地图折好塞进怀里，“一刻钟后在旧仓库碰头。”',
        '雨又下了起来，落在帆布上的声音像是有人在低声说话。',
        '艾琳最后看了一眼货船，转身没入雾中，身后只留下一串很快被雨水抹平的足迹。',
        '她知道，这一夜过后，雾港的许多秘密都将无处藏身。',
    ];
    const line = i => lines[i % lines.length];
    const prose = (from, count) => Array.from({ length: count }, (_, p) =>
        Array.from({ length: 3 + p % 3 }, (_, s) => line(from + p * 5 + s)).join('')).join('\n\n');
    const patch = turn => JSON.stringify([
        { op: 'replace', path: '/世界/时间', value: `霜月第${12 + turn}日 黄昏` },
        { op: 'replace', path: '/世界/地点', value: '雾港码头' },
        ...['艾琳', '洛岚', '维克托', '赛菲娅', '诺德'].flatMap((name, i) => [
            { op: 'delta', path: `/角色/${name}/体力`, value: -((turn + i) % 7) },
            { op: 'delta', path: `/角色/${name}/好感度`, value: (turn + i) % 3 },
            { op: 'replace', path: `/角色/${name}/状态`, value: ['警惕', '疲惫', '专注', '平静'][(turn + i) % 4] },
        ]),
        ...Array.from({ length: 6 }, (_, i) => ({ op: 'insert', path: `/日志/-`, value: `${line(turn + i)}` })),
    ], null, 2);
    const update = turn => `<UpdateVariable>
<Analysis>${line(turn + 3)}${line(turn + 7)}本轮时间推进到黄昏，众人在码头分头行动，体力略有消耗，与艾琳的信任上升。</Analysis>
<JSONPatch>
${patch(turn)}
</JSONPatch>
</UpdateVariable>`;
    const reasoning = turn => Array.from({ length: 110 }, (_, p) =>
        `${p + 1}. ${line(turn + p)}这一段需要承接上一轮的伏笔，同时控制节奏，不要让角色的反应显得突兀；变量方面要更新时间、地点与体力。`).join('\n\n');
    const reply = turn => `<content>\n${prose(turn, 16)}\n</content>\n\n${update(turn)}\n\n<StatusPlaceHolderImpl/>`;
    const userTurn = turn => `${line(turn + 1)}我跟着艾琳走进雾里，一边留意货船上的动静，一边低声问她接下来的打算。${line(turn + 9)}`.repeat(6);

    const script = (scriptName, findRegex, replaceString, options) => ({
        id: `atlas-${scriptName}`, scriptName, findRegex, replaceString, trimStrings: [], placement: [2], disabled: false,
        markdownOnly: false, promptOnly: false, runOnEdit: true, substituteRegex: 0, minDepth: null, maxDepth: null, ...options,
    });
    const panel = (title, state) => `<div style="width: 80%; margin: 20px auto;">
<details class="atlas-update ${state}" style="background: #2d2d2d; border-radius: 12px; box-shadow: 0 2px 8px rgba(0,0,0,0.3); transition: box-shadow 0.25s ease; overflow: hidden;">
<summary style="padding: 12px 16px; color: #e0e0e0; cursor: pointer; list-style: none; font-weight: 500; position: relative; background: #363636;"><small>${title}</small></summary>
<div style="max-height: 300px; overflow-y: auto; padding: 12px 16px; color: #b0b0b0; line-height: 1.6; font-size: 0.9em; white-space: pre-wrap; border-top: 1px solid #444;">$2</div>
</details>
</div>
<style>
.atlas-update summary::-webkit-details-marker { display: none; }
.atlas-update summary:hover { background: #3d3d3d !important; }
.atlas-update[open] summary { border-bottom: 1px solid #444; }
.atlas-update.${state} small::after { content: "${state === 'loading' ? ' …' : ' ▾'}"; color: #8a8a8a; }
</style>`;
    const page = `<div data-day style="width: 90%; max-width: 1200px; min-height: 200px; margin: 20px auto; position: relative; overflow: hidden; contain: layout paint; padding: 25px; background: linear-gradient(165deg, #f7e9d7, #e6d5c3); border: 12px solid #8b7355; border-radius: 8px; box-shadow: 0 0 40px rgba(139,115,85,0.3), inset 0 0 30px rgba(255,255,255,0.4); color: #4a3c2c;">
<div data-glow style="position: absolute; inset: 0; background: radial-gradient(120% 80% at 50% 50%, rgba(139,115,85,0.25), transparent 60%); opacity: 0.22; pointer-events: none; z-index: 0;"></div>
<div data-grid style="position: absolute; inset: 0; background: linear-gradient(90deg, rgba(139,115,85,0.06) 1px, transparent 1px), linear-gradient(rgba(139,115,85,0.06) 1px, transparent 1px); background-size: 24px 24px; pointer-events: none; z-index: 0;"></div>`;
    const display = { markdownOnly: true };
    const prompt = { promptOnly: true };
    // Stock-phrase cleanup runs on both the display and the prompt of recent replies.
    const trim = { markdownOnly: true, promptOnly: true, maxDepth: 6 };
    const regex = [
        script('[美化]变量更新中', String.raw`/<(update(?:variable)?)>(?!.*<\/\1>)\s*((?:(?!<\1>).)*)\s*$/gsi`, panel('变量更新中', 'loading'), display),
        script('[美化]完整变量更新', String.raw`/<(update(?:variable)?)>\s*((?:(?!<\1>).)*)\s*<\/\1>/gsi`, panel('变量更新情况', 'done'), display),
        script('[不发送]去除变量更新', String.raw`/<(update(?:variable)?)>(?:(?!.*<\/\1>)(?:(?!<\1>).)*$|(?:(?!<\1>).)*<\/\1>)/gsi`, '', prompt),
        script('[正文]日间美化', '/<content>/', page, { ...display, maxDepth: 2 }),
        script('[状态栏]', String.raw`/<StatusPlaceHolderImpl\/>/`, statusBarDocument(), { ...display, maxDepth: 2 }),
        script('[不发送]隐藏状态栏', String.raw`/<StatusPlaceHolderImpl\/>/g`, '', prompt),
        script('[杀八股]长表达', String.raw`/(?:石子[^\n]{0,20}?涟漪|涟漪[^\n]{0,20}?泛起|不是因为[^\n]{0,20}?而|不仅[^\n]{0,20}?更)/gi`, '', trim),
        script('[杀八股]孤立助词', String.raw`/([，。、；：？！])\s*(的|地|得|了(?![却无结断然当])|着)\s*/g`, '$1', trim),
        script('[杀八股]多余标点', String.raw`/([，。、；：？！])(?=[，。、；：？！])/g`, '', trim),
    ];
    const atlasCard = JSON.stringify({
        spec: 'chara_card_v2', spec_version: '2.0', name: 'Atlas', chat: 'atlas-chat', create_date: '2026-01-04T12:00:00Z',
        data: {
            name: 'Atlas', description: prose(0, 6), personality: '', scenario: prose(3, 2), first_mes: prose(5, 2),
            mes_example: '', creator: '', creator_notes: '', character_version: '', tags: [], alternate_greetings: [],
            system_prompt: '', post_history_instructions: '', extensions: { talkativeness: 0.5, fav: false, regex_scripts: regex },
        },
    });
    const atlasChat = [
        { user_name: 'User', character_name: 'Atlas', chat_metadata: { integrity: 'atlas-integrity' } },
        { name: 'Atlas', is_user: false, is_system: false, send_date: '2026-01-04T12:00:00Z', extra: {}, mes: prose(5, 2) },
    ];
    for (let turn = 0; turn < 20; turn++) {
        const at = minute => `2026-01-04T13:${String(minute).padStart(2, '0')}:00Z`;
        atlasChat.push({ name: 'User', is_user: true, is_system: false, send_date: at(turn * 2), extra: {}, mes: userTurn(turn) });
        atlasChat.push({
            name: 'Atlas', is_user: false, is_system: false, send_date: at(turn * 2 + 1), mes: reply(turn),
            extra: { api: 'custom', model: 'replay', reasoning: reasoning(turn), reasoning_type: 'model' },
        });
    }
    return { atlasCard, atlasChat, atlasReply: { reasoning: reasoning(20), content: reply(20), user: userTurn(20) } };
}

/** A Session as the native host stores it, with each task's run record and event journal. */
function assistantSession(paragraph) {
    const sessionId = 'session_perf';
    const time = seconds => new Date(Date.UTC(2026, 0, 3, 12) + seconds * 1000).toISOString();
    const session = { id: sessionId, createdAt: time(0), title: 'Reef log review', lastUsedAt: time(1000) };
    const messages = [], runs = [];
    for (let task = 0; task < 20; task++) {
        const runId = `run_perf_${task}`;
        const events = [];
        const event = (type, payload) => events.push({
            seq: events.length + 1, id: `evt_${task}_${events.length + 1}`, runId, timestamp: time(messages.length),
            level: 'info', type, payload,
        });
        const append = (role, parts, origin) => {
            messages.push({
                seq: messages.length + 1, runId, createdAt: time(messages.length),
                message: { role, parts, providerMetadata: null }, ...(origin ? { origin } : {}),
            });
            event('session_message_appended', { seq: messages.length, sessionId });
        };
        event('run_created', { target: { kind: 'session', sessionId }, workspaceId: sessionId });
        append('user', [{ type: 'text', text: `Task ${task}: ${paragraph}` }]);
        for (let round = 1; round <= 11; round++) {
            const origin = { invocationId: 'inv_root', round };
            const reasoning = { type: 'reasoning', text: `Round ${round} of task ${task}. ${paragraph.repeat(27)}`, provider_metadata: { source: 'reasoning_content' } };
            event('status_changed', { status: 'calling_model' });
            event('model_request_created', { ...origin, context: { tokens: 4000 * round, point: 110000 } });
            event('model_completed', origin);
            if (round === 11) {
                const reply = `**Task ${task} is done.** ${paragraph}\n\n- ${paragraph}\n- ${paragraph}\n\n\`\`\`js\nconst state = api.chat.current();\n\`\`\`\n\n${paragraph.repeat(6)}`;
                append('assistant', [reasoning, { type: 'text', text: reply }], origin);
                break;
            }
            const call = { callId: `call_${task}_${round}`, toolId: 'extension/in-app-agent:app.evaluate', arguments: { code: `// ${paragraph}\n`.repeat(10) } };
            append('assistant', [reasoning, { type: 'toolCall', call }], origin);
            event('status_changed', { status: 'dispatching_tool' });
            event('tool_call_requested', { ...origin, callId: call.callId, toolId: call.toolId });
            event('tool_call_completed', { ...origin, callId: call.callId, toolId: call.toolId, elapsedMs: 40 });
            const content = JSON.stringify({ round, log: paragraph.repeat(40) });
            append('tool', [{ type: 'toolResult', result: { callId: call.callId, toolId: call.toolId, content, structured: {}, isError: false, resourceRefs: [] } }], origin);
        }
        event('run_completed', {});
        const run = {
            id: runId, workspaceId: sessionId, target: { kind: 'session', sessionId }, profileId: 'in-app-assistant',
            status: 'completed', createdAt: events[0].timestamp, updatedAt: events.at(-1).timestamp,
        };
        runs.push({ run, events });
    }
    return { session, messages, runs };
}

export function perfSettings() {
    const settings = structuredClone(defaultSettings);
    // With the browser flags in openFrontend, no animation spreads a step's work over time.
    settings.power_user.reduced_motion = true;
    // The embedded-lorebook import prompt shows once per card, and this user has already answered it for Ledger.
    settings.accountStorage = { ...settings.accountStorage, 'AlertWI_Ledger.png': 'true' };
    // This user allowed Atlas's own regex and uses Code Render.
    settings.extension_settings.character_allowed_regex = ['Atlas.png'];
    settings.extension_settings.code_render = { enabled: true, replace_last_message_by_default: false };
    // This user keeps a tool call and its result in the preset, switched off, so the prompt does not carry them. The
    // order is the one the Prompt Manager saves for a new user, followed by the two entries.
    const call = {
        identifier: 'readStateCall', name: 'Read state', role: 'assistant', content: '', system_prompt: false, marker: false,
        fields: { tool_calls: [{ id: 'call_read_state', type: 'function', function: { name: 'read_state', arguments: { target: '{{char}}' } } }] },
    };
    const result = {
        identifier: 'readStateResult', name: 'State', role: 'tool', content: '{{getvar::state}}', system_prompt: false, marker: false,
        fields: { tool_call_id: 'call_read_state' },
    };
    settings.oai_settings.prompts.push(call, result);
    settings.oai_settings.prompt_order.push({
        character_id: 100001,
        order: [
            'agentSystemPrompt', 'main', 'worldInfoBefore', 'personaDescription', 'charDescription', 'charPersonality', 'scenario',
            'enhanceDefinitions', 'nsfw', 'worldInfoAfter', 'dialogueExamples', 'agentTask', 'chatHistory', 'jailbreak', 'agentResults',
        ].map(identifier => ({ identifier, enabled: identifier !== 'enhanceDefinitions' }))
            .concat({ identifier: call.identifier, enabled: false }, { identifier: result.identifier, enabled: false }),
    });
    return settings;
}

/** The page's Web Host data: the fixture's arguments plus the dataset. */
export function pageData() {
    return { settings: perfSettings(), theme: defaultTheme, extensions: systemExtensions, ...dataset() };
}

const jsonl = records => records.map(record => JSON.stringify(record)).join('\n');
// The native host appends the assistant's records one by one, each ending its line.
const appended = records => records.map(record => `${JSON.stringify(record)}\n`).join('');

/** A 1x1 PNG carrying a character card in its `chara` text chunk, as SillyTavern writes cards. */
function cardPng(cardJson) {
    const chunk = (type, data) => {
        const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
        const length = Buffer.alloc(4), checksum = Buffer.alloc(4);
        length.writeUInt32BE(data.length);
        checksum.writeUInt32BE(crc32(body));
        return Buffer.concat([length, body, checksum]);
    };
    const header = Buffer.from([0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    return Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        chunk('IHDR', header),
        chunk('tEXt', Buffer.from(`chara\0${Buffer.from(cardJson).toString('base64')}`, 'latin1')),
        chunk('IDAT', deflateSync(Buffer.from([0, 0, 0, 0, 0]))),
        chunk('IEND', Buffer.alloc(0)),
    ]);
}

/** Writes the dataset under `root` in the native host's data directory layout. */
export async function writeDataRoot(root) {
    const data = pageData();
    const fixture = createWebHostFixture(data.settings, data.theme, data.extensions);
    const get = url => JSON.parse(fixture.backend.handle({ method: 'POST', url, headers: {}, body: '{}' }).body);
    const { session, messages, runs } = data.assistant;
    const agent = '_tauritavern/agent-workspaces';
    const files = {
        'default-user/settings.json': get('/api/settings/get').settings,
        'default-user/characters/Review.png': cardPng(get('/api/characters/get').json_data),
        'default-user/characters/Ledger.png': cardPng(data.ledgerCard),
        'default-user/characters/Atlas.png': cardPng(data.atlasCard),
        'default-user/chats/Review/review-chat.jsonl': jsonl(data.chat),
        'default-user/chats/Ledger/ledger-chat.jsonl': jsonl(data.ledgerChat),
        'default-user/chats/Atlas/atlas-chat.jsonl': jsonl(data.atlasChat),
        'default-user/worlds/Review lore.json': JSON.stringify(data.lore),
        [`${agent}/sessions/${session.id}/session.json`]: JSON.stringify(session),
        [`${agent}/sessions/${session.id}/history.jsonl`]: appended(messages),
    };
    for (const { run, events } of runs) {
        files[`${agent}/sessions/${session.id}/runs/${run.id}/run.json`] = JSON.stringify(run);
        files[`${agent}/sessions/${session.id}/runs/${run.id}/events.jsonl`] = appended(events);
        files[`${agent}/index/session-runs/${run.id}.json`] = JSON.stringify(run);
    }
    for (const [file, content] of Object.entries(files)) {
        await mkdir(path.dirname(path.join(root, file)), { recursive: true });
        await writeFile(path.join(root, file), content);
    }
}
