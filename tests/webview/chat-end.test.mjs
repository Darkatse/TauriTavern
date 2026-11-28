import { expect, test } from '@rstest/core';
import { captureBrowserErrors, createFrontend } from '../helpers/frontend.mjs';

const { window, fixture, load, startHost, waitUntilComplete } = await createFrontend();
window.frameElement.style.cssText = 'width: 420px; height: 800px';
const { errors, failed } = captureBrowserErrors(window);
await startHost();
const main = await load('script.js');
const { eventSource, event_types } = await load('scripts/events.js');
const started = [event_types.APP_READY, event_types.SETTINGS_UPDATED, event_types.EXTENSION_SETTINGS_LOADED]
    .map(event => new Promise(resolve => eventSource.once(event, resolve)));
window.jQuery.holdReady(false);
await Promise.race([Promise.all(started), failed]);

const chat = window.document.getElementById('chat');
const distanceFromEnd = () => chat.scrollHeight - chat.clientHeight - chat.scrollTop;

async function settle() {
    await waitUntilComplete();
    for (let i = 0; i < 3; i++) await new Promise(resolve => window.requestAnimationFrame(resolve));
}

/** Reopening leaves the replies above the end unrendered, as after opening a long chat. */
async function openChatOfTallReplies() {
    const reply = Array.from({ length: 120 }, (_, i) => `Paragraph ${i}.`).join('\n\n');
    const records = [{ user_name: 'User', character_name: 'Review', chat_metadata: { integrity: 'review-integrity' } }];
    for (let i = 0; i < 40; i++) {
        records.push({ name: 'User', is_user: true, is_system: false, mes: 'Go on.', send_date: '2026-01-01T12:00:00Z', extra: {} });
        records.push({ name: 'Review', is_user: false, is_system: false, mes: reply, send_date: '2026-01-01T12:00:00Z', extra: {} });
    }
    fixture.chatFiles.set('review-chat', records.map(record => JSON.stringify(record)).join('\n'));
    await Promise.race([main.reloadCurrentChat(), failed]);
    chat.scrollTop = chat.scrollHeight;
    await settle();
    expect(distanceFromEnd()).toBeLessThan(1);
}

test('sending after a tall reply keeps the reader at the end', async () => {
    await openChatOfTallReplies();
    await Promise.race([main.sendMessageAsUser('Next.', ''), failed]);
    await settle();
    expect(distanceFromEnd()).toBeLessThan(1);
    expect(errors).toEqual([]);
});

test('deleting a tall last reply keeps the reader at the end', async () => {
    await openChatOfTallReplies();
    await Promise.race([main.deleteLastMessage(), failed]);
    await settle();
    expect(distanceFromEnd()).toBeLessThan(1);
    expect(errors).toEqual([]);
});
