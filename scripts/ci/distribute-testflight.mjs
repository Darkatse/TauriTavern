#!/usr/bin/env node

import { createPrivateKey, sign } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const API_ROOT = 'https://api.appstoreconnect.apple.com';
const POLL_INTERVAL_MS = 30_000;

function requireEnvironment(name) {
    const value = process.env[name]?.trim();
    if (!value) {
        throw new Error(`Missing ${name} environment variable`);
    }
    return value;
}

function encodeJson(value) {
    return Buffer.from(JSON.stringify(value)).toString('base64url');
}

export function createAppStoreConnectToken({
    issuerId,
    keyId,
    privateKey,
    nowSeconds = Math.floor(Date.now() / 1000),
}) {
    const header = encodeJson({ alg: 'ES256', kid: keyId, typ: 'JWT' });
    const payload = encodeJson({
        iss: issuerId,
        iat: nowSeconds,
        exp: nowSeconds + 20 * 60,
        aud: 'appstoreconnect-v1',
    });
    const signingInput = `${header}.${payload}`;
    const signature = sign('sha256', Buffer.from(signingInput), {
        key: createPrivateKey(privateKey),
        dsaEncoding: 'ieee-p1363',
    }).toString('base64url');
    return `${signingInput}.${signature}`;
}

export function externalStateAction(state) {
    switch (state) {
        case 'PROCESSING':
        case 'IN_EXPORT_COMPLIANCE_REVIEW':
            return 'wait';
        case 'READY_FOR_BETA_SUBMISSION':
            return 'submit';
        case 'READY_FOR_BETA_TESTING':
        case 'IN_BETA_TESTING':
        case 'WAITING_FOR_BETA_REVIEW':
        case 'IN_BETA_REVIEW':
        case 'BETA_APPROVED':
            return 'complete';
        case 'PROCESSING_EXCEPTION':
        case 'MISSING_EXPORT_COMPLIANCE':
        case 'EXPIRED':
        case 'BETA_REJECTED':
        case 'NOT_APPLICABLE':
            return 'fail';
        default:
            throw new Error(`Unknown TestFlight external build state: ${state || '(missing)'}`);
    }
}

function sleep(milliseconds) {
    return new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
}

function apiError(method, path, response, responseText) {
    let detail = responseText.trim();
    try {
        const payload = JSON.parse(responseText);
        detail = payload.errors
            ?.map((error) => [error.code, error.title, error.detail].filter(Boolean).join(': '))
            .join('; ') || detail;
    } catch {
        // Keep the raw response when Apple does not return JSON.
    }
    return new Error(`${method} ${path} failed with ${response.status}${detail ? `: ${detail}` : ''}`);
}

class AppStoreConnectClient {
    constructor({ issuerId, keyId, privateKey }) {
        this.issuerId = issuerId;
        this.keyId = keyId;
        this.privateKey = privateKey;
    }

    async request(method, path, { body, expectedStatus = 200 } = {}) {
        const response = await fetch(`${API_ROOT}${path}`, {
            method,
            headers: {
                Authorization: `Bearer ${createAppStoreConnectToken(this)}`,
                Accept: 'application/json',
                ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
            },
            body: body === undefined ? undefined : JSON.stringify(body),
            signal: AbortSignal.timeout(60_000),
        });
        const responseText = await response.text();
        if (response.status !== expectedStatus) {
            throw apiError(method, path, response, responseText);
        }
        return responseText ? JSON.parse(responseText) : null;
    }

    get(path, parameters = {}) {
        const query = new URLSearchParams(parameters).toString();
        return this.request('GET', query ? `${path}?${query}` : path);
    }
}

async function findUploadedBuild(client, { appId, marketingVersion, buildVersion }) {
    const payload = await client.get('/v1/builds', {
        'filter[app]': appId,
        'filter[version]': buildVersion,
        'filter[preReleaseVersion.version]': marketingVersion,
        'filter[preReleaseVersion.platform]': 'IOS',
        'fields[builds]': 'version,processingState,uploadedDate',
        limit: '2',
    });
    if (!Array.isArray(payload?.data) || payload.data.length > 1) {
        throw new Error(
            `Expected at most one uploaded build, found ${payload?.data?.length ?? 'an invalid response'}`,
        );
    }
    return payload.data[0] ?? null;
}

async function waitForProcessedBuild(client, identity) {
    const deadline = Date.now() + 60 * 60_000;
    let lastStatus;

    while (Date.now() < deadline) {
        const build = await findUploadedBuild(client, identity);
        if (build) {
            const state = build.attributes?.processingState;
            if (state === 'VALID') {
                return build;
            }
            if (state === 'FAILED' || state === 'INVALID') {
                throw new Error(
                    `App Store Connect rejected build ${identity.marketingVersion} (${identity.buildVersion}): ${state}`,
                );
            }
            if (state !== 'PROCESSING') {
                throw new Error(`Unknown App Store Connect processing state: ${state || '(missing)'}`);
            }
            if (lastStatus !== state) {
                console.log(`Waiting for App Store Connect processing: ${state}`);
                lastStatus = state;
            }
        } else if (lastStatus !== 'NOT_FOUND') {
            console.log(`Waiting for uploaded build ${identity.marketingVersion} (${identity.buildVersion}) to appear`);
            lastStatus = 'NOT_FOUND';
        }
        await sleep(POLL_INTERVAL_MS);
    }

    throw new Error(
        `Timed out waiting for build ${identity.marketingVersion} (${identity.buildVersion}) to finish processing`,
    );
}

export async function verifyExternalGroup(client, { appId, groupId, groupName }) {
    const group = (await client.get(`/v1/betaGroups/${groupId}`, {
        'fields[betaGroups]': 'name,isInternalGroup,publicLinkEnabled',
    })).data;
    if (!group || group.attributes?.name !== groupName) {
        throw new Error(`TestFlight group ${groupId} is not ${groupName}`);
    }
    if (group.attributes.isInternalGroup !== false || group.attributes.publicLinkEnabled !== true) {
        throw new Error(`TestFlight group ${groupName} is not a public external group`);
    }
    const app = (await client.get(`/v1/betaGroups/${groupId}/relationships/app`)).data;
    if (app?.id !== appId) {
        throw new Error(`TestFlight group ${groupName} does not belong to app ${appId}`);
    }
}

async function upsertWhatsNew(client, buildId, whatsNew) {
    const payload = await client.get('/v1/betaBuildLocalizations', {
        'filter[build]': buildId,
        'filter[locale]': 'en-US',
        'fields[betaBuildLocalizations]': 'locale,whatsNew',
        limit: '2',
    });
    if (!Array.isArray(payload?.data) || payload.data.length > 1) {
        throw new Error(
            `Expected at most one en-US build localization, found ${payload?.data?.length ?? 'an invalid response'}`,
        );
    }

    if (payload.data.length === 1) {
        const localizationId = payload.data[0].id;
        await client.request('PATCH', `/v1/betaBuildLocalizations/${localizationId}`, {
            body: {
                data: {
                    type: 'betaBuildLocalizations',
                    id: localizationId,
                    attributes: { whatsNew },
                },
            },
        });
        return;
    }

    await client.request('POST', '/v1/betaBuildLocalizations', {
        expectedStatus: 201,
        body: {
            data: {
                type: 'betaBuildLocalizations',
                attributes: { locale: 'en-US', whatsNew },
                relationships: {
                    build: { data: { type: 'builds', id: buildId } },
                },
            },
        },
    });
}

async function enableAutomaticNotifications(client, buildId) {
    const detail = (await client.get(`/v1/builds/${buildId}/buildBetaDetail`, {
        'fields[buildBetaDetails]': 'autoNotifyEnabled',
    })).data;
    if (!detail?.id) {
        throw new Error(`Build ${buildId} has no TestFlight beta detail`);
    }
    if (detail.attributes?.autoNotifyEnabled !== true) {
        await client.request('PATCH', `/v1/buildBetaDetails/${detail.id}`, {
            body: {
                data: {
                    type: 'buildBetaDetails',
                    id: detail.id,
                    attributes: { autoNotifyEnabled: true },
                },
            },
        });
    }
}

async function associateGroup(client, buildId, groupId) {
    const existing = await client.get('/v1/betaGroups', {
        'filter[builds]': buildId,
        'fields[betaGroups]': 'name,isInternalGroup',
        limit: '200',
    });
    if (!Array.isArray(existing?.data)) {
        throw new Error('Invalid beta group relationship response');
    }
    const unexpectedExternalGroups = existing.data.filter(
        (group) => group.id !== groupId && group.attributes?.isInternalGroup === false,
    );
    if (unexpectedExternalGroups.length > 0) {
        const groupNames = unexpectedExternalGroups
            .map((group) => group.attributes?.name || group.id)
            .join(', ');
        throw new Error(
            `Build ${buildId} is already assigned to another external group: ${groupNames}`,
        );
    }
    if (!existing.data.some((group) => group.id === groupId)) {
        await client.request('POST', `/v1/betaGroups/${groupId}/relationships/builds`, {
            expectedStatus: 204,
            body: { data: [{ type: 'builds', id: buildId }] },
        });
    }
}

async function submitForExternalTesting(client, buildId) {
    const deadline = Date.now() + 10 * 60_000;
    let lastState;

    while (Date.now() < deadline) {
        const detail = (await client.get(`/v1/builds/${buildId}/buildBetaDetail`, {
            'fields[buildBetaDetails]': 'externalBuildState',
        })).data;
        const state = detail?.attributes?.externalBuildState;
        const action = externalStateAction(state);

        if (action === 'complete') {
            console.log(`TestFlight external state: ${state}`);
            return;
        }
        if (action === 'fail') {
            throw new Error(`Build ${buildId} cannot enter external testing: ${state}`);
        }
        if (action === 'submit') {
            await client.request('POST', '/v1/betaAppReviewSubmissions', {
                expectedStatus: 201,
                body: {
                    data: {
                        type: 'betaAppReviewSubmissions',
                        relationships: {
                            build: { data: { type: 'builds', id: buildId } },
                        },
                    },
                },
            });
            console.log('Submitted build for TestFlight beta review');
            return;
        }
        if (lastState !== state) {
            console.log(`Waiting for TestFlight external state: ${state}`);
            lastState = state;
        }
        await sleep(POLL_INTERVAL_MS);
    }

    throw new Error(`Timed out waiting for build ${buildId} to become ready for external testing`);
}

async function main() {
    const issuerId = requireEnvironment('APPLE_API_ISSUER');
    const keyId = requireEnvironment('APPLE_API_KEY');
    const privateKey = await readFile(requireEnvironment('APPLE_API_KEY_PATH'), 'utf8');
    const appId = requireEnvironment('TESTFLIGHT_APP_ID');
    const groupId = requireEnvironment('TESTFLIGHT_GROUP_ID');
    const groupName = requireEnvironment('TESTFLIGHT_GROUP_NAME');
    const marketingVersion = requireEnvironment('TESTFLIGHT_MARKETING_VERSION');
    const buildVersion = requireEnvironment('TESTFLIGHT_BUILD_VERSION');
    const whatsNew = (await readFile(requireEnvironment('TESTFLIGHT_NOTES_FILE'), 'utf8')).trim();
    if (!whatsNew || whatsNew.length > 4000 || /^#{1,6}\s/mu.test(whatsNew) || whatsNew.includes('```')) {
        throw new Error('TestFlight What to Test text must be plain text between 1 and 4000 characters');
    }

    const client = new AppStoreConnectClient({ issuerId, keyId, privateKey });
    await verifyExternalGroup(client, { appId, groupId, groupName });
    const build = await waitForProcessedBuild(client, { appId, marketingVersion, buildVersion });
    await upsertWhatsNew(client, build.id, whatsNew);
    await enableAutomaticNotifications(client, build.id);
    await associateGroup(client, build.id, groupId);
    await submitForExternalTesting(client, build.id);
    console.log(`Distributed ${marketingVersion} (${buildVersion}) to ${groupName}`);
}

async function printBuildExists() {
    const issuerId = requireEnvironment('APPLE_API_ISSUER');
    const keyId = requireEnvironment('APPLE_API_KEY');
    const privateKey = await readFile(requireEnvironment('APPLE_API_KEY_PATH'), 'utf8');
    const client = new AppStoreConnectClient({ issuerId, keyId, privateKey });
    const build = await findUploadedBuild(client, {
        appId: requireEnvironment('TESTFLIGHT_APP_ID'),
        marketingVersion: requireEnvironment('TESTFLIGHT_MARKETING_VERSION'),
        buildVersion: requireEnvironment('TESTFLIGHT_BUILD_VERSION'),
    });
    process.stdout.write(`${build !== null}\n`);
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
    if (process.argv[2] === undefined) {
        await main();
    } else if (process.argv[2] === 'build-exists') {
        await printBuildExists();
    } else {
        throw new Error('Usage: distribute-testflight.mjs [build-exists]');
    }
}
