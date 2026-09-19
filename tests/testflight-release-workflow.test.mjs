import assert from 'node:assert/strict';
import { generateKeyPairSync, verify } from 'node:crypto';
import test from 'node:test';

import {
    createAppStoreConnectToken,
    externalStateAction,
    verifyExternalGroup,
} from '../scripts/ci/distribute-testflight.mjs';

test('App Store Connect tokens use the required ES256 claims', () => {
    const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' });
    const token = createAppStoreConnectToken({
        issuerId: 'issuer',
        keyId: 'key',
        privateKey: privateKey.export({ type: 'pkcs8', format: 'pem' }),
        nowSeconds: 1_000,
    });
    const [headerPart, payloadPart, signaturePart] = token.split('.');

    assert.deepEqual(JSON.parse(Buffer.from(headerPart, 'base64url')), {
        alg: 'ES256',
        kid: 'key',
        typ: 'JWT',
    });
    assert.deepEqual(JSON.parse(Buffer.from(payloadPart, 'base64url')), {
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

test('TestFlight verifies that an external group belongs to the app', async () => {
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

test('TestFlight states fail fast when the remote state is unknown', () => {
    for (const state of ['PROCESSING', 'IN_EXPORT_COMPLIANCE_REVIEW']) {
        assert.equal(externalStateAction(state), 'wait');
    }
    assert.equal(externalStateAction('READY_FOR_BETA_SUBMISSION'), 'submit');
    for (const state of ['READY_FOR_BETA_TESTING', 'IN_BETA_TESTING', 'BETA_APPROVED']) {
        assert.equal(externalStateAction(state), 'complete');
    }
    for (const state of ['PROCESSING_EXCEPTION', 'EXPIRED', 'BETA_REJECTED']) {
        assert.equal(externalStateAction(state), 'fail');
    }
    assert.throws(() => externalStateAction('FUTURE_STATE'), /Unknown TestFlight external build state/);
});
