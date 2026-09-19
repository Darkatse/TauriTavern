import test from 'node:test';
import assert from 'node:assert/strict';
import { snapshotVariableMacroValues } from '../src/scripts/variables/values.js';

test('variable macro snapshots preserve key case and frontend value coercion', () => {
    const variables = {
        local: { Name: '007', name: '{{char}}', empty: '', whitespace: '  ', flag: false, missing: undefined },
        global: { Name: '008' },
    };
    const snapshot = snapshotVariableMacroValues(variables);
    variables.local.Name = 'changed';
    assert.deepEqual(snapshot, {
        local: { Name: '7', name: '{{char}}', empty: '', whitespace: '  ', flag: '0' },
        global: { Name: '8' },
    });
});
