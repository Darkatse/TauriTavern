import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';

const installerPath = resolve('scripts/install-linux.sh');
const installerSource = readFileSync(installerPath, 'utf8');

function commandExists(command) {
    const result = spawnSync(command, ['-c', 'exit 0'], { stdio: 'ignore' });
    return !result.error && result.status === 0;
}

function osReleaseFile(contents) {
    const directory = mkdtempSync(join(tmpdir(), 'tauritavern-installer-test-'));
    const path = join(directory, 'os-release');
    writeFileSync(path, contents);
    return path;
}

function shellPath(path, platform = process.platform) {
    if (platform !== 'win32') {
        return path;
    }

    const normalized = path.replaceAll('\\', '/');
    return normalized.replace(/^([A-Za-z]):\//, (_, drive) => `/${drive.toLowerCase()}/`);
}

function runDryRun({
    shell = 'sh',
    osRelease,
    osReleasePath,
    architecture,
    method,
    channel,
    expectFailure = false,
}) {
    const args = [installerPath];
    if (method) {
        args.push('--method', method);
    }
    if (channel) {
        args.push('--channel', channel);
    }
    args.push('--dry-run', '--no-color');

    const result = spawnSync(
        shell,
        args,
        {
            encoding: 'utf8',
            env: {
                ...process.env,
                TAURITAVERN_TEST_ARCHITECTURE: architecture,
                TAURITAVERN_TEST_KERNEL: 'Linux',
                TAURITAVERN_TEST_OS_RELEASE: shellPath(osReleasePath ?? osReleaseFile(osRelease)),
            },
        },
    );

    if (!expectFailure && result.error) {
        throw result.error;
    }

    return {
        status: result.status,
        output: `${result.stdout}${result.stderr}`,
    };
}

test('Linux installer keeps all side effects behind the final main call', () => {
    const lastLine = installerSource.trimEnd().split('\n').at(-1);
    assert.equal(lastLine, 'main "$@"');
});

test('Linux installer test paths use the shell filesystem namespace', () => {
    assert.equal(
        shellPath('C:\\Users\\runner\\AppData\\Local\\Temp\\os-release', 'win32'),
        '/c/Users/runner/AppData/Local/Temp/os-release',
    );
    assert.equal(shellPath('/tmp/os-release', 'linux'), '/tmp/os-release');
});

test('Linux installer is syntactically valid in available common shells', () => {
    const shells = ['sh', 'dash', 'bash', 'zsh'];
    const availableShells = shells.filter(commandExists);

    assert.ok(availableShells.includes('sh'));
    for (const shell of availableShells) {
        execFileSync(shell, ['-n', installerPath]);
    }
});

test('Linux installer supports piped execution through common shells', () => {
    const release = osReleaseFile(
        'ID=debian\nVERSION_ID="12"\nPRETTY_NAME="Debian GNU/Linux 12 (bookworm)"\n',
    );
    const shells = ['sh', 'dash', 'bash', 'zsh'].filter(commandExists);

    for (const shell of shells) {
        const result = spawnSync(
            shell,
            ['-s', '--', '--dry-run', '--no-color'],
            {
                encoding: 'utf8',
                input: installerSource,
                env: {
                    ...process.env,
                    TAURITAVERN_TEST_ARCHITECTURE: 'amd64',
                    TAURITAVERN_TEST_KERNEL: 'Linux',
                    TAURITAVERN_TEST_OS_RELEASE: shellPath(release),
                },
            },
        );

        assert.equal(result.status, 0, `${shell}: ${result.stdout}${result.stderr}`);
        assert.match(result.stdout, /Dry run complete/);
    }
});

test('Linux installer detects every documented package distribution', () => {
    const cases = [
        {
            name: 'Debian GNU/Linux 12 (bookworm)',
            release: 'ID=debian\nVERSION_ID="12"\nPRETTY_NAME="Debian GNU/Linux 12 (bookworm)"\n',
            architecture: 'amd64',
            packageSystem: 'apt',
        },
        {
            name: 'Ubuntu 22.04.5 LTS',
            release: 'ID=ubuntu\nVERSION_ID="22.04"\nPRETTY_NAME="Ubuntu 22.04.5 LTS"\n',
            architecture: 'arm64',
            packageSystem: 'apt',
        },
        {
            name: 'Fedora Linux 42',
            release: 'ID=fedora\nVERSION_ID="42"\nPRETTY_NAME="Fedora Linux 42"\n',
            architecture: 'x86_64',
            packageSystem: 'dnf',
        },
        {
            name: 'openSUSE Leap 16.0',
            release: 'ID=opensuse-leap\nVERSION_ID="16.0"\nPRETTY_NAME="openSUSE Leap 16.0"\n',
            architecture: 'aarch64',
            packageSystem: 'zypper',
        },
    ];

    for (const item of cases) {
        const result = runDryRun({
            osRelease: item.release,
            architecture: item.architecture,
        });

        assert.equal(result.status, 0, result.output);
        assert.match(result.output, new RegExp(`Detected ${item.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`));
        assert.match(result.output, new RegExp(`Install method\\s+${item.packageSystem}`));
        assert.match(result.output, /no system changes were made/);
    }
});

test('Linux installer rejects versions outside the documented support range', () => {
    const unsupportedCases = [
        {
            release: 'ID=debian\nVERSION_ID="11"\nPRETTY_NAME="Debian GNU/Linux 11"\n',
            architecture: 'amd64',
            expected: 'Debian 12 or later is required',
        },
        {
            release: 'ID=ubuntu\nVERSION_ID="20.04"\nPRETTY_NAME="Ubuntu 20.04 LTS"\n',
            architecture: 'amd64',
            expected: 'Ubuntu 22.04 LTS or later is required',
        },
        {
            release: 'ID=opensuse-leap\nVERSION_ID="15.6"\nPRETTY_NAME="openSUSE Leap 15.6"\n',
            architecture: 'x86_64',
            expected: 'Leap 16.0 is required',
        },
    ];

    for (const item of unsupportedCases) {
        const result = runDryRun({
            osRelease: item.release,
            architecture: item.architecture,
            expectFailure: true,
        });

        assert.equal(result.status, 1, result.output);
        assert.match(result.output, new RegExp(item.expected.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
        assert.doesNotMatch(result.output, /no system changes were made/);
    }
});

test('Linux installer automatically selects the flake on NixOS', () => {
    const result = runDryRun({
        osRelease: 'ID=nixos\nVERSION_ID="26.05"\nPRETTY_NAME="NixOS 26.05"\n',
        architecture: 'x86_64',
    });

    assert.equal(result.status, 0, result.output);
    assert.match(result.output, /Install method\s+nix/);
    assert.match(result.output, /Architecture\s+x86_64-linux/);
    assert.match(result.output, /github:Darkatse\/TauriTavern#tauritavern/);
    assert.match(result.output, /nix-cache\.tauritavern\.com/);
});

test('Linux installer can explicitly select Nix on another distribution', () => {
    const result = runDryRun({
        osRelease: 'ID=arch\nPRETTY_NAME="Arch Linux"\n',
        architecture: 'aarch64',
        method: 'nix',
    });

    assert.equal(result.status, 0, result.output);
    assert.match(result.output, /Detected Arch Linux \(aarch64-linux\)/);
    assert.match(result.output, /Install method\s+nix/);
});

test('Linux installer maps Canary to every supported package system', () => {
    const cases = [
        {
            release: 'ID=debian\nVERSION_ID="12"\nPRETTY_NAME="Debian 12"\n',
            architecture: 'amd64',
            expected: /\/apt \(suite: canary\)/,
        },
        {
            release: 'ID=fedora\nVERSION_ID="42"\nPRETTY_NAME="Fedora 42"\n',
            architecture: 'x86_64',
            expected: /rpm\/fedora\/canary/,
        },
        {
            release: 'ID=opensuse-leap\nVERSION_ID="16.0"\nPRETTY_NAME="openSUSE Leap 16.0"\n',
            architecture: 'aarch64',
            expected: /rpm\/opensuse\/16\.0\/canary/,
        },
        {
            release: 'ID=nixos\nVERSION_ID="26.05"\nPRETTY_NAME="NixOS 26.05"\n',
            architecture: 'x86_64',
            expected: /github:Darkatse\/TauriTavern\/Canary#canary/,
        },
    ];

    for (const item of cases) {
        const result = runDryRun({
            osRelease: item.release,
            architecture: item.architecture,
            channel: 'canary',
        });

        assert.equal(result.status, 0, result.output);
        assert.match(result.output, /Channel\s+canary/);
        assert.match(result.output, item.expected);
    }
});

test('explicit Nix installation does not require os-release', () => {
    const result = runDryRun({
        osReleasePath: '/path/that/does/not/exist',
        architecture: 'x86_64',
        method: 'nix',
    });

    assert.equal(result.status, 0, result.output);
    assert.match(result.output, /Detected Linux \(x86_64-linux\)/);
    assert.match(result.output, /Install method\s+nix/);
});

test('Linux installer rejects a native package request on NixOS', () => {
    const result = runDryRun({
        osRelease: 'ID=nixos\nVERSION_ID="26.05"\nPRETTY_NAME="NixOS 26.05"\n',
        architecture: 'x86_64',
        method: 'native',
        expectFailure: true,
    });

    assert.equal(result.status, 1, result.output);
    assert.match(result.output, /NixOS does not use the APT\/RPM repository/);
});
