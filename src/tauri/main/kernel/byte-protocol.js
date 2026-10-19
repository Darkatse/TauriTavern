// @ts-check
// Shared with hosts implementing the existing file-read and byte-commit wire formats.
export const FILE_READ_COUNT_BYTES = 8;
export const BYTE_HEADERS = Object.freeze({ sessionId: 'session-id', offset: 'offset', encoding: 'chunk-encoding' });
