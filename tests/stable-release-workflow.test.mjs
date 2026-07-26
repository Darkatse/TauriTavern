import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import YAML from 'yaml';

const workflowPath = '.github/workflows/stable-release.yml';
const workflowSource = readFileSync(workflowPath, 'utf8');
const workflow = YAML.parse(workflowSource);

test('stable release workflow starts from a published release or an explicit tag', () => {
    assert.deepEqual(workflow.on.release.types, ['published']);
    assert.equal(workflow.on.workflow_dispatch.inputs.tag.required, true);
});

test('stable release workflow preserves manually written release notes', () => {
    assert.doesNotMatch(JSON.stringify(workflow.jobs['publish-release']), /codex|release edit|notes-file/i);
    assert.match(workflowSource, /Upload assets without changing release notes/);
});

test('stable release workflow publishes release assets before optional repositories', () => {
    assert.deepEqual(workflow.jobs['publish-release'].needs, ['prepare', 'desktop', 'mobile']);

    for (const jobName of ['publish-package-repositories', 'publish-nix-cache']) {
        const job = workflow.jobs[jobName];
        assert.deepEqual(job.needs, ['prepare', 'publish-release']);
        assert.equal(job['continue-on-error'], true);
    }
});

test('stable release workflow keeps repository credentials in GitHub secrets', () => {
    assert.match(workflowSource, /secrets\.R2_ACCESS_KEY_ID/);
    assert.match(workflowSource, /secrets\.R2_SECRET_ACCESS_KEY/);
    assert.match(workflowSource, /secrets\.LINUX_REPOSITORY_GPG_PRIVATE_KEY_BASE64/);
    assert.match(workflowSource, /secrets\.NIX_CACHE_PRIVATE_KEY_BASE64/);
    assert.doesNotMatch(workflowSource, /BEGIN (?:PGP|OPENSSH|PRIVATE) PRIVATE KEY/);
});

test('stable Nix publication includes reusable project dependencies', () => {
    assert.match(workflowSource, /tauritavern\.cargoDeps\.outPath/);
    assert.match(workflowSource, /tauritavern\.pnpmDeps\.outPath/);
    assert.match(workflowSource, /NIX_CACHE_URL: https:\/\/nix-cache\.tauritavern\.com/);
});
