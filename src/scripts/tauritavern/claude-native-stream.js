export class ClaudeNativeStreamAccumulator {
    constructor() {
        this.content = [];
        this.openIndex = null;
        this.partialJson = null;
        this.stopReason = null;
        this.stopDetails = null;
        this.native = null;
    }

    consume(event) {
        switch (event?.type) {
            case 'content_block_start': {
                const { index, content_block: block } = event;
                if (this.openIndex !== null || index !== this.content.length || !block || typeof block !== 'object') {
                    throw new Error('Claude stream contains an invalid content_block_start');
                }
                this.content.push(structuredClone(block));
                this.openIndex = index;
                this.partialJson = null;
                break;
            }
            case 'content_block_delta': {
                const { index, delta } = event;
                if (index !== this.openIndex || !delta || typeof delta !== 'object') {
                    throw new Error('Claude stream delta does not match the open content block');
                }
                const block = this.content[index];
                switch (delta.type) {
                    case 'text_delta':
                        block.text += delta.text;
                        break;
                    case 'thinking_delta':
                        block.thinking += delta.thinking;
                        break;
                    case 'signature_delta':
                        block.signature += delta.signature;
                        break;
                    case 'input_json_delta':
                        this.partialJson ??= '';
                        this.partialJson += delta.partial_json;
                        break;
                    default:
                        throw new Error(`Unsupported Claude stream delta: ${delta.type}`);
                }
                break;
            }
            case 'content_block_stop': {
                if (event.index !== this.openIndex) {
                    throw new Error('Claude stream stopped a content block that is not open');
                }
                if (this.partialJson) {
                    try {
                        this.content[event.index].input = JSON.parse(this.partialJson);
                    } catch (cause) {
                        throw new Error('Claude tool_use block contains invalid JSON', { cause });
                    }
                }
                this.openIndex = null;
                this.partialJson = null;
                break;
            }
            case 'message_delta':
                this.stopReason = event.delta?.stop_reason ?? this.stopReason;
                this.stopDetails = event.delta?.stop_details ?? this.stopDetails;
                break;
            case 'message_stop': {
                if (this.openIndex !== null) {
                    throw new Error('Claude stream ended with an open content block');
                }
                const claude = { content: this.content };
                if (this.stopReason !== null) claude.stop_reason = this.stopReason;
                if (this.stopDetails !== null) claude.stop_details = this.stopDetails;
                this.native = { claude };
                return this.native;
            }
        }

        return null;
    }

    finish() {
        if (!this.native) {
            throw new Error('Claude stream ended before message_stop');
        }
        return this.native;
    }
}

export function hasClaudeToolUse(native) {
    return native?.claude?.content?.some(block => block?.type === 'tool_use') === true;
}

export function appendClaudeRefusalWarning(text, warning) {
    const suffix = `⚠️ ${warning}`;
    return text ? `${text}\n\n${suffix}` : suffix;
}

export function getClaudeStopStatus(stopReason, stopDetails = null) {
    if (stopReason === 'refusal') {
        const explanation = typeof stopDetails?.explanation === 'string'
            ? stopDetails.explanation.trim()
            : '';
        return {
            code: 'model.provider_refusal',
            message: explanation || 'Claude refused to complete the response.',
        };
    }

    if (stopReason === 'max_tokens' || stopReason === 'length') {
        return {
            code: 'model.output_truncated',
            message: 'Claude reached the output token limit. The response is incomplete.',
        };
    }

    if (stopReason === 'model_context_window_exceeded') {
        return {
            code: 'model.output_truncated',
            message: 'Claude reached the context window limit. The response is incomplete.',
        };
    }

    return null;
}
