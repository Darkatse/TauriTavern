import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import YAML from 'yaml';

const workflowSource = readFileSync('.github/workflows/canary-release.yml', 'utf8');
const workflow = YAML.parse(workflowSource);
const publisherSource = readFileSync('distribution/apt-rpm/publish.sh', 'utf8');
const nixPublisherSource = readFileSync('distribution/nix-cache/publish.sh', 'utf8');
const flakeSource = readFileSync('flake.nix', 'utf8');

test('Canary artifact names use the China-local calendar date without a time', () => {
    assert.match(workflowSource, /asset_time=.*date \+'%Y%m%d'/);
    assert.doesNotMatch(workflowSource, /asset_time=.*%H%M/);
});

test('Canary Linux package versions derive from the latest stable release', () => {
    assert.equal(workflow.env.NEXT_STABLE_VERSION, undefined);
    assert.match(workflowSource, /releases\/latest/);
    assert.match(workflowSource, /\+canary\.\$\{GITHUB_RUN_NUMBER\}\.g/);
    assert.match(workflowSource, /2\.canary\.\$\{GITHUB_RUN_NUMBER\}\.g/);
    assert.match(workflowSource, /bundle: \{linux: \{rpm: \{release: \$release\}\}\}/);
    assert.match(workflowSource, /rewrite-deb-version\.sh/);
});

test('Canary repositories run only after the GitHub release is published', () => {
    for (const jobName of ['publish-package-repositories', 'publish-nix-cache']) {
        const job = workflow.jobs[jobName];
        assert.deepEqual(job.needs, ['prepare', 'publish']);
        assert.equal(job['continue-on-error'], true);
    }
});

test('Canary APT and RPM use isolated repository paths', () => {
    const job = workflow.jobs['publish-package-repositories'];
    assert.equal(job.env.REPOSITORY_CHANNEL, 'canary');
    assert.match(workflowSource, /apt\/dists\/canary\/InRelease/);
    assert.match(workflowSource, /rpm\/fedora\/canary\/x86_64/);
    assert.match(workflowSource, /rpm\/opensuse\/16\.0\/canary\/x86_64/);
    assert.match(publisherSource, /pool\/canary\/main\/t\/tauri-tavern/);
    assert.match(publisherSource, /repository-manifest-canary\.json/);
});

test('Canary Nix builds an explicit output and shares the content-addressed cache', () => {
    const job = workflow.jobs['publish-nix-cache'];
    assert.equal(job.env.NIX_R2_BUCKET, 'tauritavern-nix-cache');
    assert.match(workflowSource, /build\s+\.#canary/);
    assert.match(workflowSource, /secrets\.NIX_CACHE_PRIVATE_KEY_BASE64/);
    assert.match(workflowSource, /canary\.cargoDeps\.outPath/);
    assert.match(workflowSource, /canary\.pnpmDeps\.outPath/);
    assert.match(nixPublisherSource, /\.narinfo/);
    assert.match(nixPublisherSource, /Already cached/);
    assert.match(flakeSource, /canary = packageFor "dev"/);
});

test('Canary repository credentials remain in GitHub secrets', () => {
    assert.match(workflowSource, /secrets\.R2_ACCESS_KEY_ID/);
    assert.match(workflowSource, /secrets\.R2_SECRET_ACCESS_KEY/);
    assert.match(workflowSource, /secrets\.LINUX_REPOSITORY_GPG_PRIVATE_KEY_BASE64/);
    assert.doesNotMatch(workflowSource, /BEGIN (?:PGP|OPENSSH|PRIVATE) PRIVATE KEY/);
});
