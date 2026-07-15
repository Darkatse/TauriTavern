const DEFAULT_PREPEND_BATCH_SIZE = 5;

export function getMessageRenderBatches(messageCount, batchSize = DEFAULT_PREPEND_BATCH_SIZE) {
    const count = Math.max(0, Math.trunc(Number(messageCount) || 0));
    const size = Math.max(1, Math.trunc(Number(batchSize) || DEFAULT_PREPEND_BATCH_SIZE));
    const batches = [];

    for (let start = 0; start < count; start += size) {
        batches.push({ start, end: Math.min(start + size, count) });
    }

    return batches;
}
