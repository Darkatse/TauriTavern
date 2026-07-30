import assert from 'node:assert/strict';
import { generateKeyPairSync, verify } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import YAML from 'yaml';

import {
    createAppStoreConnectToken,
    externalStateAction,
    verifyExternalGroup,
} from '../scripts/ci/distribute-testflight.mjs';

const canarySource = readFileSync('.github/workflows/canary-release.yml', 'utf8');
const stableSource = readFileSync('.github/workflows/stable-release.yml', 'utf8');
const testflightSource = readFileSync('.github/workflows/public-testflight.yml', 'utf8');
const mobileHttpScript = readFileSync('scripts/ci/configure-mobile-http.sh', 'utf8');
const testflightSkill = readFileSync('.github/codex/skills/tauritavern-testflight-notes/SKILL.md', 'utf8');
const humanizerSkill = readFileSync('.github/codex/skills/tauritavern-release-humanizer/SKILL.md', 'utf8');
const canary = YAML.parse(canarySource);
const stable = YAML.parse(stableSource);
const testflightWorkflow = YAML.parse(testflightSource);

test('Stable and Canary preserve the ordinary IPA and build a separate TestFlight variant', () => {
    for (const [name, workflow, source] of [
        ['Canary', canary, canarySource],
        ['Stable', stable, stableSource],
    ]) {
        const standardBuild = workflow.jobs.mobile.steps.find((step) => step.name === 'Build mobile bundle');
        const testflightBuild = workflow.jobs.mobile.steps.find((step) => step.name === 'Build public TestFlight IPA');

        assert.equal(workflow.jobs.mobile.concurrency.group.includes('tauritavern-public-testflight-build'), true, name);
        assert.equal(standardBuild.with.args, '${{ matrix.args }}', name);
        assert.equal(standardBuild.env.TAURITAVERN_IOS_POLICY_PROFILE, undefined, name);
        assert.equal(testflightBuild.if, "matrix.mobile == 'ios'", name);
        assert.equal(testflightBuild.env.TAURITAVERN_IOS_POLICY_PROFILE, 'ios_external_beta', name);
        assert.equal(testflightBuild.env.TAURITAVERN_SKIP_WEB_BUILD, '1', name);
        assert.match(testflightBuild.with.args, /--export-method app-store-connect --config/u, name);
        assert.match(testflightBuild.with.workflowArtifactNamePattern, /-TestFlight-\[bundle\]$/u, name);
        assert.match(source, /"bundleVersion":"%s"/u, name);
    }
});

test('standard mobile artifacts allow HTTP without relaxing the TestFlight IPA', () => {
    for (const [name, workflow] of [
        ['Canary', canary],
        ['Stable', stable],
    ]) {
        const steps = workflow.jobs.mobile.steps;
        const enableIndex = steps.findIndex((step) => step.name === 'Enable HTTP access for standard mobile bundle');
        const standardIndex = steps.findIndex((step) => step.name === 'Build mobile bundle');
        const restoreIndex = steps.findIndex((step) => step.name === 'Restore iOS transport security for TestFlight');
        const testflightIndex = steps.findIndex((step) => step.name === 'Build public TestFlight IPA');

        assert.equal(steps[enableIndex].run, './scripts/ci/configure-mobile-http.sh enable "${{ matrix.mobile }}"', name);
        assert.equal(steps[restoreIndex].if, "matrix.mobile == 'ios'", name);
        assert.equal(steps[restoreIndex].run, './scripts/ci/configure-mobile-http.sh disable ios', name);
        assert.equal(enableIndex < standardIndex, true, name);
        assert.equal(standardIndex < restoreIndex && restoreIndex < testflightIndex, true, name);
    }

    assert.match(mobileHttpScript, /usesCleartextTraffic/u);
    assert.match(mobileHttpScript, /MIXED_CONTENT_ALWAYS_ALLOW/u);
    assert.match(mobileHttpScript, /NSAllowsArbitraryLoadsInWebContent/u);
});

test('Stable and Canary target only their existing public TestFlight groups', () => {
    const canaryJob = canary.jobs.testflight;
    assert.deepEqual(canaryJob.needs, ['prepare', 'publish']);
    assert.equal(canaryJob.permissions.contents, 'write');
    assert.equal(canaryJob.uses, './.github/workflows/public-testflight.yml');
    assert.equal(canaryJob.with.channel, 'canary');
    assert.match(canaryJob.with.ipa_artifact, /-ios-arm64-TestFlight-ipa$/u);
    assert.match(canaryJob.with.release_asset, /-ios-arm64-TestFlight\.ipa$/u);
    assert.equal(canaryJob.with.release_tag, 'Canary');
    assert.equal(canaryJob.with.group_id, 'd379dde8-9206-4533-ba2c-c9f7569e57a7');
    assert.equal(canaryJob.with.group_name, 'TauriTavern Canary Test');

    const stableJob = stable.jobs.testflight;
    assert.deepEqual(stableJob.needs, ['prepare', 'publish-release']);
    assert.equal(stableJob.permissions.contents, 'write');
    assert.equal(stableJob.uses, './.github/workflows/public-testflight.yml');
    assert.equal(stableJob.with.channel, 'stable');
    assert.match(stableJob.with.ipa_artifact, /-ios-arm64-TestFlight-ipa$/u);
    assert.match(stableJob.with.release_asset, /-ios-arm64-TestFlight\.ipa$/u);
    assert.equal(stableJob.with.group_id, 'fde21316-1511-4a66-b48f-7cbadc1be3f7');
    assert.equal(stableJob.with.group_name, 'TauriTavern Beta Test');
});

test('TestFlight notes keep Codex isolated, read-only, and non-blocking', () => {
    const notes = testflightWorkflow.jobs.notes;
    const codexStep = notes.steps.find((step) => step.id === 'codex');
    assert.equal(codexStep['continue-on-error'], true);
    assert.equal(codexStep.with['permission-profile'], ':read-only');
    assert.match(testflightSource, /cp -R \.github\/codex\/skills\/\. "\$codex_home\/skills\/"/u);
    assert.match(testflightSource, /using the deterministic testing prompt/u);
    assert.doesNotMatch(testflightSource, /\.agents\/skills/u);
    assert.equal(testflightWorkflow.jobs.publish.needs, 'notes');
});

test('Stable TestFlight notes use maintainer release notes as verified priorities', () => {
    const contextStep = testflightWorkflow.jobs.notes.steps.find(
        (step) => step.name === 'Prepare TestFlight change context',
    );

    assert.equal(contextStep.env.GH_TOKEN, '${{ github.token }}');
    assert.equal(contextStep.env.RELEASE_TAG, '${{ inputs.release_tag }}');
    assert.match(contextStep.run, /if \[\[ "\$CHANNEL" == stable \]\]/u);
    assert.match(contextStep.run, /gh release view "\$RELEASE_TAG"/u);
    assert.match(contextStep.run, /## Maintainer-written release notes/u);
    assert.match(testflightSkill, /use them to prioritize testing but verify every claim/u);
});

test('TestFlight notes keep the app voice lightly playful without weakening factual rules', () => {
    assert.match(testflightSkill, /warm, lightly playful voice/u);
    assert.match(testflightSkill, /never let it replace a concrete change or testing request/u);
    assert.match(humanizerSkill, /For TestFlight copy, preserve or lightly refine one restrained playful touch/u);
    assert.match(testflightSource, /If anything wobbles, send it our way through TestFlight\./u);
});

test('TestFlight publishing uploads, localizes, and distributes the exact IPA', () => {
    const publishSteps = testflightWorkflow.jobs.publish.steps;
    const existingBuildIndex = publishSteps.findIndex(
        (step) => step.name === 'Check for an existing App Store Connect build',
    );
    const uploadIndex = publishSteps.findIndex((step) => step.name === 'Upload IPA to App Store Connect');
    const distributeIndex = publishSteps.findIndex((step) => step.name === 'Publish to the public TestFlight group');
    const removeIndex = publishSteps.findIndex(
        (step) => step.name === 'Remove the temporary TestFlight IPA from the GitHub Release',
    );

    assert.match(testflightSource, /xcrun altool/u);
    assert.match(testflightSource, /--upload-app/u);
    assert.match(testflightSource, /TESTFLIGHT_APP_ID: "6760324223"/u);
    assert.match(testflightSource, /node scripts\/ci\/distribute-testflight\.mjs/u);
    assert.match(publishSteps[existingBuildIndex].run, /distribute-testflight\.mjs build-exists/u);
    assert.equal(publishSteps[uploadIndex].if, "steps.app-store-build.outputs.exists != 'true'");
    assert.equal(existingBuildIndex < uploadIndex && uploadIndex < distributeIndex, true);
    assert.match(testflightSource, /CFBundleShortVersionString/u);
    assert.match(testflightSource, /CFBundleVersion/u);
    assert.equal(testflightWorkflow.jobs.publish.permissions.contents, 'write');
    assert.equal(removeIndex > distributeIndex, true);
    assert.match(publishSteps[removeIndex].run, /gh release delete-asset/u);
});

test('App Store Connect JWTs use ES256 and a twenty-minute lifetime', () => {
    const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' });
    const token = createAppStoreConnectToken({
        issuerId: 'issuer',
        keyId: 'key',
        privateKey: privateKey.export({ type: 'pkcs8', format: 'pem' }),
        nowSeconds: 1_000,
    });
    const [headerPart, payloadPart, signaturePart] = token.split('.');
    const header = JSON.parse(Buffer.from(headerPart, 'base64url'));
    const payload = JSON.parse(Buffer.from(payloadPart, 'base64url'));

    assert.deepEqual(header, { alg: 'ES256', kid: 'key', typ: 'JWT' });
    assert.deepEqual(payload, {
        iss: 'issuer',
        iat: 1_000,
        exp: 2_200,
        aud: 'appstoreconnect-v1',
    });
    assert.equal(Buffer.from(signaturePart, 'base64url').length, 64);
    assert.equal(verify(
        'sha256',
        Buffer.from(`${headerPart}.${payloadPart}`),
        { key: publicKey, dsaEncoding: 'ieee-p1363' },
        Buffer.from(signaturePart, 'base64url'),
    ), true);
});

test('TestFlight group ownership uses the explicit App Store Connect linkage', async () => {
    const paths = [];
    const client = {
        async get(path) {
            paths.push(path);
            if (path.endsWith('/relationships/app')) {
                return { data: { id: 'app-id' } };
            }
            return {
                data: {
                    attributes: {
                        name: 'Public Group',
                        isInternalGroup: false,
                        publicLinkEnabled: true,
                    },
                },
            };
        },
    };

    await verifyExternalGroup(client, {
        appId: 'app-id',
        groupId: 'group-id',
        groupName: 'Public Group',
    });
    assert.deepEqual(paths, [
        '/v1/betaGroups/group-id',
        '/v1/betaGroups/group-id/relationships/app',
    ]);
});

test('TestFlight external states submit, wait, complete, or fail explicitly', () => {
    for (const state of ['PROCESSING', 'IN_EXPORT_COMPLIANCE_REVIEW']) {
        assert.equal(externalStateAction(state), 'wait');
    }
    assert.equal(externalStateAction('READY_FOR_BETA_SUBMISSION'), 'submit');
    for (const state of [
        'READY_FOR_BETA_TESTING',
        'IN_BETA_TESTING',
        'WAITING_FOR_BETA_REVIEW',
        'IN_BETA_REVIEW',
        'BETA_APPROVED',
    ]) {
        assert.equal(externalStateAction(state), 'complete');
    }
    for (const state of [
        'PROCESSING_EXCEPTION',
        'MISSING_EXPORT_COMPLIANCE',
        'EXPIRED',
        'BETA_REJECTED',
        'NOT_APPLICABLE',
    ]) {
        assert.equal(externalStateAction(state), 'fail');
    }
    assert.throws(() => externalStateAction('FUTURE_STATE'), /Unknown TestFlight external build state/u);
});
