import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('core message roots reconcile through ChatSurface instead of direct DOM mutation', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const groups = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');
    const slash = await readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands.js'), 'utf8');
    const bounded = await readFile(
        path.join(REPO_ROOT, 'src/tauri/main/services/chat-surface/bounded-chat-surface.js'),
        'utf8',
    );
    const install = await readFile(
        path.join(REPO_ROOT, 'src/tauri/main/services/chat-surface/install.js'),
        'utf8',
    );

    const showMore = script.slice(
        script.indexOf('export async function showMoreMessages'),
        script.indexOf('export async function printMessages'),
    );
    assert.match(showMore, /chatSurface\.reconcileMounted/);
    assert.doesNotMatch(showMore, /updateMessageElement|\.prepend\(|showMoreButton\.after/);

    const redisplay = script.slice(
        script.indexOf('export async function redisplayChat'),
        script.indexOf('export function scrollOnMediaLoad'),
    );
    assert.match(redisplay, /chatSurface\.render/);
    assert.match(redisplay, /targetChat !== chat[\s\S]*only accepts the canonical chat array/);
    assert.doesNotMatch(redisplay, /\.remove\(|chatElement\.append/);

    const deleteMessage = script.slice(
        script.indexOf('export async function deleteMessage'),
        script.indexOf('export const reloadChatMutex'),
    );
    assert.match(deleteMessage, /!Number\.isInteger\(id\)[\s\S]*id >= chat\.length[\s\S]*throw new RangeError/);
    assert.match(deleteMessage, /chat\.splice\(id, 1\);[\s\S]*updateViewMessageIds\(\)/);
    assert.doesNotMatch(deleteMessage, /const minId = getFirstDisplayedMessageId|const startIndex =/);
    assert.doesNotMatch(deleteMessage, /messageElement\.length === 0[\s\S]*return/);

    const editMove = script.slice(
        script.indexOf('async function messageEditMove'),
        script.indexOf('\n/**', script.indexOf('async function messageEditMove')),
    );
    assert.match(editMove, /getMessageElement\(sourceId\)/);
    assert.match(editMove, /getMessageElement\(targetId\)/);
    assert.match(editMove, /!sourceElement \|\| !targetElement/);
    assert.match(editMove, /captureTextarea\('#curEditTextarea'\)/);
    assert.match(editMove, /captureTextarea\('\.reasoning_edit_textarea'\)/);
    assert.match(editMove, /await messageEdit\(targetId\)/);
    assert.match(editMove, /setSelectionRange\(state\.selectionStart, state\.selectionEnd, state\.selectionDirection\)/);

    const editArrows = script.slice(
        script.indexOf('export function updateEditArrowClasses'),
        script.indexOf('\n/**', script.indexOf('export function updateEditArrowClasses')),
    );
    assert.match(editArrows, /new Set\(chatSurface\.getMountedMessageIds\(\)\)/);
    assert.match(editArrows, /!mountedIds\.has\(messageId \+ 1\)/);
    assert.match(editArrows, /!mountedIds\.has\(messageId - 1\)/);

    const editCopy = script.slice(
        script.indexOf("$(document).on('click', '.mes_edit_copy'"),
        script.indexOf("$(document).on('click', '.mes_edit_delete'"),
    );
    assert.match(editCopy, /reconcileMountedChatSurface\([\s\S]*updateEditArrowClasses\(\)/);

    const editEntry = script.slice(
        script.indexOf('export async function messageEdit'),
        script.indexOf('async function messageEditCancel'),
    );
    assert.match(editEntry, /replaceTransientMesTextHtmlWithRuntimePolicy\(/);
    assert.doesNotMatch(script, /ttMessageEditStash|ttGuardEmbeddedRuntimeMoves/);

    assert.match(install, /onProjectionCommitted: syncMountedViewState/);
    const viewStateSync = script.slice(
        script.indexOf('function syncMountedChatViewState'),
        script.indexOf('function shouldUseBoundedChatSurface'),
    );
    assert.match(viewStateSync, /applyCharacterTagsToMessageDivs/);
    assert.match(viewStateSync, /syncMountedDeleteState\(messageIds\)/);
    assert.match(viewStateSync, /refreshSwipeButtons\(false\)/);
    assert.match(viewStateSync, /syncStylePinsOnProjectionEdge\(messageIds\)/);
    assert.match(viewStateSync, /syncLastInContextMessageMarker\(\)/);
    assert.match(viewStateSync, /updateEditArrowClasses\(\)/);
    assert.doesNotMatch(viewStateSync, /eventSource\.emit/);

    const deleteStateSync = script.slice(
        script.indexOf('function syncMountedDeleteState'),
        script.indexOf('function syncStylePinsOnProjectionEdge'),
    );
    assert.match(deleteStateSync, /for \(const messageId of messageIds\)/);
    assert.match(deleteStateSync, /messageId >= this_del_mes/);
    assert.doesNotMatch(deleteStateSync, /chat\.length/);

    const stylePinSync = script.slice(
        script.indexOf('function syncStylePinsOnProjectionEdge'),
        script.indexOf('function syncMountedChatViewState'),
    );
    assert.match(stylePinSync, /firstMessage: chat\[0\] \?\? null/);
    assert.match(stylePinSync, /firstMessageMounted: messageIds\.includes\(0\)/);
    assert.match(stylePinSync, /stylePinProjectionState[\s\S]*applyStylePins\(\)/);

    const deleteModeClick = script.slice(
        script.indexOf("$(document).on('click', '.mes'"),
        script.indexOf('/**\n     * Handles the deletion of a chat file', script.indexOf("$(document).on('click', '.mes'")),
    );
    assert.match(deleteModeClick, /this_del_mes = Number\([\s\S]*syncMountedDeleteState/);
    assert.doesNotMatch(deleteModeClick, /while \(.*chat\.length/);

    const finishProjection = bounded.slice(
        bounded.indexOf('function finishProjection'),
        bounded.indexOf('function commitCurrentGeometry'),
    );
    assert.ok(finishProjection.indexOf('onProjectionCommitted') < finishProjection.indexOf('virtual.measure'));

    assert.doesNotMatch(groups, /chatElement\.find\(['"]\.mes['"]\)\.remove\(\)/);
    assert.doesNotMatch(slash, /existingMessage\.(after|remove)\(/);
    assert.match(groups, /resetChatSurfaceView\(\)/);
    assert.match(slash, /rerenderChatMessage\(modifyAt\)/);

    const welcome = await readFile(path.join(REPO_ROOT, 'src/scripts/welcome-screen.js'), 'utf8');
    assert.doesNotMatch(welcome, /\$\(['"]#chat['"]\)\.empty\(\)/);
    assert.match(welcome, /resetChatSurfaceView\(\{ includeAuxiliary: true \}\)/);
});

test('absolute mesid, true tail and scroll writes have one owner seam', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const install = await readFile(
        path.join(REPO_ROOT, 'src/tauri/main/services/chat-surface/install.js'),
        'utf8',
    );
    const reasoning = await readFile(path.join(REPO_ROOT, 'src/scripts/reasoning.js'), 'utf8');
    const welcome = await readFile(path.join(REPO_ROOT, 'src/scripts/welcome-screen.js'), 'utf8');
    const ross = await readFile(path.join(REPO_ROOT, 'src/scripts/RossAscends-mods.js'), 'utf8');
    const slash = await readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands.js'), 'utf8');

    const refreshSwipes = script.slice(
        script.indexOf('export function refreshSwipeButtons'),
        script.indexOf('export function showSwipeButtons'),
    );
    assert.match(refreshSwipes, /Number\(div\.getAttribute\(['"]mesid['"]\)\)/);
    assert.doesNotMatch(refreshSwipes, /firstDisplayedMesId\s*\+\s*index/);

    const updateIds = script.slice(
        script.indexOf('export function updateViewMessageIds'),
        script.indexOf('export function getFirstDisplayedMessageId'),
    );
    assert.match(updateIds, /reconcileMountedChatSurface\(\)/);
    assert.match(updateIds, /chatSurface\.reconcileMounted\(options\)/);
    assert.doesNotMatch(updateIds, /\.attr\(['"]mesid['"]/);

    const rerender = script.slice(
        script.indexOf('export function rerenderChatMessage'),
        script.indexOf('export function isBoundedChatSurfaceView'),
    );
    assert.match(rerender, /return chatSurface\.rerenderMessage\(messageId\)/);
    assert.doesNotMatch(rerender, /jumpToMessage/);

    const directScrollWriter = /(?:chatElement|chatBlock|chatContainer)\.scrollTop\s*(?:\(|=|\+=|-=)|#chat['"]\)\.animate\(\{\s*scrollTop/;
    for (const source of [script, reasoning, welcome, ross, slash]) {
        assert.doesNotMatch(source, directScrollWriter);
    }
    assert.match(install, /createChatScrollAdapter/);
    assert.match(script, /setChatScrollTop\(/);
});

test('data mutations do not require DOM residency or implicit bounded navigation', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const powerUser = await readFile(path.join(REPO_ROOT, 'src/scripts/power-user.js'), 'utf8');
    const slash = await readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands.js'), 'utf8');

    const deleteSwipe = script.slice(
        script.indexOf('export async function deleteSwipe'),
        script.indexOf('export async function saveMetadata'),
    );
    const syncDeletedSwipe = deleteSwipe.indexOf('syncSwipeToMes(messageId, newSwipeId, message)');
    assert.ok(syncDeletedSwipe >= 0);
    assert.ok(syncDeletedSwipe < deleteSwipe.indexOf('MESSAGE_SWIPE_DELETED'));
    assert.match(deleteSwipe, /deletedCurrentSwipe && chatSurface\.getMessageElement\(messageId\)/);
    assert.match(deleteSwipe, /deletedCurrentSwipe[\s\S]*MESSAGE_SWIPED/);

    const swipe = script.slice(
        script.indexOf('export async function swipe('),
        script.indexOf('export async function swipe_left'),
    );
    assert.ok(swipe.indexOf("DOM element is not valid") < swipe.indexOf('swipeState = SWIPE_STATE.SWIPING'));
    assert.match(swipe, /finally \{\s*swipeState = SWIPE_STATE\.NONE;[\s\S]*delete document\.body\.dataset\.swiping;[\s\S]*showSwipeButtons\(\);/);

    const cut = powerUser.slice(
        powerUser.indexOf('async function doMesCut'),
        powerUser.indexOf('async function doDelMode'),
    );
    assert.match(cut, /await deleteMessage\(mesIDToCut, null, false\)/);
    assert.doesNotMatch(cut, /#chat|loadUntilMesId|showMoreMessages|setEditedMessageId/);
    assert.doesNotMatch(powerUser, /function loadUntilMesId/);

    const chatRender = slash.slice(
        slash.indexOf("name: 'chat-render'"),
        slash.indexOf("name: 'chat-reload'"),
    );
    assert.match(chatRender, /isBoundedChatSurfaceView\(\)[\s\S]*\/chat-render is unavailable/);

    const editCopy = script.slice(
        script.indexOf("$(document).on('click', '.mes_edit_copy'"),
        script.indexOf("$(document).on('click', '.mes_edit_delete'"),
    );
    assert.match(editCopy, /try \{[\s\S]*reconcileMountedChatSurface\([\s\S]*finally \{[\s\S]*showSwipeButtons\(\)/);
});

test('projection lifecycle does not import or emit SillyTavern business events', async () => {
    const controller = await readFile(
        path.join(REPO_ROOT, 'src/tauri/main/services/chat-surface/chat-surface-controller.js'),
        'utf8',
    );
    const lifecycle = await readFile(
        path.join(REPO_ROOT, 'src/tauri/main/services/chat-surface/participant-lifecycle.js'),
        'utf8',
    );
    assert.doesNotMatch(`${controller}\n${lifecycle}`, /eventSource|event_types|MESSAGE_UPDATED|MORE_MESSAGES_LOADED/);
});

test('regeneration animation keeps chat data and ChatSurface structure aligned', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const branchStart = script.indexOf('const removedMessageId = chat.length - 1;');
    const branch = script.slice(branchStart, script.indexOf('const isContinue', branchStart));
    const hideIndex = branch.indexOf('await hideMessageBeforeRemoval(removedMessageId);');
    const truncateIndex = branch.indexOf('chat.length = removedMessageId;');
    const reconcileIndex = branch.indexOf('reconcileMountedChatSurface();');
    const eventIndex = branch.indexOf('await eventSource.emit(event_types.MESSAGE_DELETED');

    assert.ok(branchStart >= 0);
    assert.ok(hideIndex < truncateIndex);
    assert.ok(truncateIndex < reconcileIndex);
    assert.ok(reconcileIndex < eventIndex);
});
