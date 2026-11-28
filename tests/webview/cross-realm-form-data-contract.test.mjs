import { expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { readRequestBody } = await importFrontend('tauri/main/http-utils.js');

test('request body decoding preserves multipart data across realms', async () => {
    const iframe = document.createElement('iframe');
    document.body.append(iframe);
    const frame = iframe.contentWindow;

    const body = new frame.FormData();
    body.append('tag', 'first');
    body.append('avatar', new frame.File(['avatar-bytes'], 'avatar.png', { type: 'image/png' }));
    body.append('tag', 'second');
    expect(body).not.toBeInstanceOf(FormData);

    const decoded = await readRequestBody(null, { body });
    expect(decoded).toBeInstanceOf(FormData);
    expect([...decoded.entries()].map(([name, value]) => [name, typeof value === 'string' ? value : value.name])).toEqual([
        ['tag', 'first'],
        ['avatar', 'avatar.png'],
        ['tag', 'second'],
    ]);

    const avatar = decoded.get('avatar');
    expect(avatar).toBeInstanceOf(File);
    expect(avatar.name).toBe('avatar.png');
    expect(avatar.type).toBe('image/png');
    expect(await avatar.text()).toBe('avatar-bytes');
});
