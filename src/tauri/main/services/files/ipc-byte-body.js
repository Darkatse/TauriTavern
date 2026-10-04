// @ts-check
import { encodeBytesToBase64 } from '../../binary-utils.js';
import { hostPlatform } from '../../../../scripts/util/host-identity.js';

/** Encode the byte body shared by upload and commit commands. @param {Uint8Array} bytes */
export function encodeIpcByteBody(bytes) {
    switch (hostPlatform()) {
        case 'android':
            return { body: { data: encodeBytesToBase64(bytes) }, headers: { 'chunk-encoding': 'base64' } };
        case 'windows':
        case 'macos':
        case 'linux':
        case 'ios':
        case 'ohos':
            return { body: bytes, headers: {} };
        default:
            throw new Error(`Unsupported byte transport platform: ${hostPlatform()}`);
    }
}
