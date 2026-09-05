import { expect, test } from '@rstest/core';

import { formatPatchDiffDetail } from './run-detail-format';

test('patch detail preserves line numbers across the changed middle range', () => {
    const section = formatPatchDiffDetail({
        type: 'patchDiff',
        labelKey: 'timelinePatchDiff',
        path: 'output/main.md',
        argumentsRef: 'tool-arguments/call.json',
        replacements: 1,
        errorKey: '',
        errorParams: { path: 'output/main.md' },
    }, {
        path: 'tool-arguments/call.json',
        text: JSON.stringify({
            path: 'output/main.md',
            old_string: 'alpha\nbeta\ngamma\n',
            new_string: 'alpha\ndelta\ngamma\n',
        }),
        chars: 0,
        words: 0,
        sha256: 'sha256',
    });

    expect(section.blocks?.[0]).toMatchObject({
        rows: [
            { type: 'context', oldLine: 1, newLine: 1, marker: ' ', text: 'alpha' },
            { type: 'delete', oldLine: 2, newLine: null, marker: '-', text: 'beta' },
            { type: 'add', oldLine: null, newLine: 2, marker: '+', text: 'delta' },
            { type: 'context', oldLine: 3, newLine: 3, marker: ' ', text: 'gamma' },
        ],
    });
});
