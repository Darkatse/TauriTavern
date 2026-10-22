// @ts-check
// Shared with hosts implementing the existing file-read and byte-commit wire formats.
export const FILE_READ_COUNT_BYTES = 8;
export const BYTE_HEADERS = Object.freeze({ sessionId: 'session-id', offset: 'offset', encoding: 'chunk-encoding' });

// Native Tauri delivers a byte response as an ArrayBuffer; the Web Host delivers a Uint8Array.
/** @param {unknown} data */
export function readByteResponse(data) {
    if (data instanceof Uint8Array) return data;
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    throw new Error(`Unexpected IPC byte response: expected ArrayBuffer or Uint8Array, got ${Object.prototype.toString.call(data)}`);
}
