import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import YAML from 'yaml';

const workflowPath = '.github/workflows/winget-release.yml';
const workflowSource = readFileSync(workflowPath, 'utf8');
const workflow = YAML.parse(workflowSource);
const publish = workflow.jobs.publish;

test('WinGet publication is reusable and manually retryable by stable tag', () => {
    for (const trigger of ['workflow_call', 'workflow_dispatch']) {
        assert.equal(workflow.on[trigger].inputs.release_tag.required, true);
        assert.equal(workflow.on[trigger].inputs.release_tag.type, 'string');
    }

    assert.equal(workflow.on.workflow_call.secrets.WINGET_CREATE_GITHUB_TOKEN.required, true);
    assert.equal(workflow.concurrency['cancel-in-progress'], false);
});

test('WinGet publication is limited to the x64 user-scope NSIS release asset', () => {
    assert.equal(workflow.env.PACKAGE_ID, 'TauriTavern.TauriTavern');

    const resolveRelease = publish.steps.find((step) => step.id === 'release');
    const generate = publish.steps.find((step) => step.id === 'manifest');
    assert.match(resolveRelease.run, /windows-x64-setup\.exe/);
    assert.match(generate.run, /\$env:INSTALLER_URL\|x64\|user/);
    assert.doesNotMatch(workflowSource, /windows-x64-portable|\.msi|canary/i);
});

test('WinGetCreate is pinned, verified, and never receives the token as an argument', () => {
    assert.equal(
        workflow.env.WINGETCREATE_URL,
        'https://github.com/microsoft/winget-create/releases/download/v1.12.13.0/wingetcreate.exe',
    );
    assert.equal(
        workflow.env.WINGETCREATE_SHA256,
        '24042bd37915805615e6cf969ac57c6439124c3fe85823327f5f3fb24bd9ffea',
    );
    assert.match(workflowSource, /Get-FileHash/);
    assert.match(workflowSource, /WINGET_CREATE_GITHUB_TOKEN/);
    assert.doesNotMatch(workflowSource, /--token/);
});

test('WinGet publication is idempotent and preserves an inspectable manifest artifact', () => {
    const preflight = publish.steps.find((step) => step.id === 'preflight');
    assert.match(preflight.run, /microsoft\/winget-pkgs/);
    assert.match(preflight.run, /is:pr is:open/);
    assert.match(preflight.run, /seed manifest/);

    const generateIndex = publish.steps.findIndex((step) => step.id === 'manifest');
    const archiveIndex = publish.steps.findIndex((step) => step.uses === 'actions/upload-artifact@v4');
    const submitIndex = publish.steps.findIndex((step) => step.name === 'Submit manifest update');
    assert.ok(generateIndex < archiveIndex);
    assert.ok(archiveIndex < submitIndex);
});
