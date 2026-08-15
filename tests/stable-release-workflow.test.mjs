import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import YAML from 'yaml';

const workflowPath = '.github/workflows/stable-release.yml';
const workflowSource = readFileSync(workflowPath, 'utf8');
const workflow = YAML.parse(workflowSource);
const canaryWorkflowSource = readFileSync('.github/workflows/canary-release.yml', 'utf8');
const qualityGateSource = readFileSync('.github/workflows/pr-quality-gate.yml', 'utf8');
const flatpakPublisherSource = readFileSync('distribution/flatpak/publish.sh', 'utf8');
const nixPackageSource = readFileSync('nix/package.nix', 'utf8');
const cargoLockSource = readFileSync('src-tauri/Cargo.lock', 'utf8');
const tauriConfig = JSON.parse(readFileSync(
    'src-tauri/crates/tauritavern/tauri.conf.json',
    'utf8',
));

test('stable release workflow starts from a published release or an explicit tag', () => {
    assert.deepEqual(workflow.on.release.types, ['published']);
    assert.equal(workflow.on.workflow_dispatch.inputs.tag.required, true);
});

test('stable release workflow preserves manually written release notes', () => {
    assert.doesNotMatch(JSON.stringify(workflow.jobs['publish-release']), /codex|release edit|notes-file/i);
    assert.match(workflowSource, /Upload assets without changing release notes/);
    assert.match(JSON.stringify(workflow.jobs['publish-release']), /--clobber/);
});

test('stable release builds Windows and macOS debug installers in parallel', () => {
    const debugBuilds = workflow.jobs.desktop.strategy.matrix.include
        .filter((entry) => entry.artifact_prefix === 'debug-')
        .map(({ platform, target_args: targetArgs, portable }) => ({ platform, targetArgs, portable }));

    assert.deepEqual(debugBuilds, [
        { platform: 'windows-latest', targetArgs: '--debug --bundles nsis', portable: false },
        {
            platform: 'macos-latest',
            targetArgs: '--target x86_64-apple-darwin --debug --bundles dmg',
            portable: false,
        },
        {
            platform: 'macos-latest',
            targetArgs: '--target aarch64-apple-darwin --debug --bundles dmg',
            portable: false,
        },
    ]);
});

test('stable release workflow publishes release assets before optional repositories', () => {
    assert.deepEqual(workflow.jobs['publish-release'].needs, ['prepare', 'desktop', 'mobile']);

    for (const jobName of ['publish-package-repositories', 'publish-nix-cache']) {
        const job = workflow.jobs[jobName];
        assert.deepEqual(job.needs, ['prepare', 'publish-release']);
        assert.equal(job['continue-on-error'], true);
    }

    const flatpak = workflow.jobs['publish-flatpak-repository'];
    assert.deepEqual(flatpak.needs, ['prepare', 'flatpak', 'publish-release']);
    assert.equal(flatpak['continue-on-error'], true);
});

test('stable release publishes WinGet only after the release assets are complete', () => {
    const winget = workflow.jobs['publish-winget'];

    assert.deepEqual(winget.needs, ['prepare', 'publish-release']);
    assert.equal(winget.uses, './.github/workflows/winget-release.yml');
    assert.equal(winget.with.release_tag, '${{ needs.prepare.outputs.tag }}');
    assert.equal(
        winget.secrets.WINGET_CREATE_GITHUB_TOKEN,
        '${{ secrets.WINGET_CREATE_GITHUB_TOKEN }}',
    );
});

test('stable release workflow keeps repository credentials in GitHub secrets', () => {
    assert.match(workflowSource, /secrets\.R2_ACCESS_KEY_ID/);
    assert.match(workflowSource, /secrets\.R2_SECRET_ACCESS_KEY/);
    assert.match(workflowSource, /secrets\.LINUX_REPOSITORY_GPG_PRIVATE_KEY_BASE64/);
    assert.match(workflowSource, /secrets\.NIX_CACHE_PRIVATE_KEY_BASE64/);
    assert.match(workflowSource, /secrets\.FLATPAK_R2_ACCESS_KEY_ID/);
    assert.match(workflowSource, /secrets\.FLATPAK_R2_SECRET_ACCESS_KEY/);
    assert.match(workflowSource, /secrets\.WINGET_CREATE_GITHUB_TOKEN/);
    assert.doesNotMatch(workflowSource, /BEGIN (?:PGP|OPENSSH|PRIVATE) PRIVATE KEY/);
});

test('stable Nix publication includes reusable project dependencies', () => {
    assert.match(workflowSource, /tauritavern\.cargoDeps\.outPath/);
    assert.match(workflowSource, /tauritavern\.pnpmDeps\.outPath/);
    assert.match(workflowSource, /NIX_CACHE_URL: https:\/\/nix-cache\.tauritavern\.com/);
});

test('Nix derives Rust dependencies directly from Cargo.lock', () => {
    assert.match(nixPackageSource, /cargoLock\s*=\s*\{\s*lockFile = \.\.\/src-tauri\/Cargo\.lock;/);
    assert.doesNotMatch(nixPackageSource, /\bcargoHash\s*=/);
    assert.doesNotMatch(cargoLockSource, /^source = "git\+/m);
});

test('Linux build and package contracts include the Tauri DBus dependency', () => {
    for (const source of [qualityGateSource, canaryWorkflowSource, workflowSource]) {
        assert.match(source, /libdbus-1-dev/u);
        assert.match(source, /pkg-config/u);
    }

    assert.match(nixPackageSource, /\bdbus\b/u);
    assert.ok(tauriConfig.bundle.linux.deb.depends.includes('libdbus-1-3'));
    assert.ok(tauriConfig.bundle.linux.rpm.depends.includes('libdbus-1.so.3()(64bit)'));
});

test('stable Flatpak build and publication keep signing isolated', () => {
    const build = workflow.jobs.flatpak;
    const publish = workflow.jobs['publish-flatpak-repository'];

    assert.equal(build['runs-on'], 'ubuntu-24.04');
    assert.equal(build.env.TAURITAVERN_FLATPAK_USER, '1');
    assert.match(JSON.stringify(build.steps), /actions\/cache@v4/);
    assert.doesNotMatch(JSON.stringify(build), /GPG_PRIVATE_KEY|GPG_PASSPHRASE/);

    assert.equal(publish.env.FLATPAK_R2_BUCKET, 'tauritavern-flatpak');
    assert.equal(publish.env.FLATPAK_REPOSITORY_URL, 'https://flatpak.tauritavern.com');
    assert.equal(publish.concurrency.group, 'tauritavern-stable-flatpak-repository');
    assert.equal(publish.concurrency['cancel-in-progress'], false);
    assert.match(workflowSource, /flatpak-public-verify/);
    assert.match(flatpakPublisherSource, /--gpg-sign/);
    assert.match(flatpakPublisherSource, /objects\/\*/);
    assert.match(flatpakPublisherSource, /summary\.sig/);
    assert.match(flatpakPublisherSource, /public, max-age=31536000, immutable/);
});

test('Flatpak publication restores OSTree ref layout lost by object storage', () => {
    const restoreRefs = flatpakPublisherSource.indexOf('mkdir -p "$repository_dir/refs/remotes"');
    const updateRepository = flatpakPublisherSource.indexOf('flatpak build-update-repo');

    assert.notEqual(restoreRefs, -1);
    assert.notEqual(updateRepository, -1);
    assert.ok(restoreRefs < updateRepository);
});
