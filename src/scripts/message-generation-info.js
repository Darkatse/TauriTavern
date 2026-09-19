import { moment } from '../lib.js';
import { t } from './i18n.js';

/** Format the existing generation timer and derive its local token rate once. */
export function formatGenerationTimer(genStarted, genFinished, tokenCount, reasoningDuration = null, timeToFirstToken = null) {
    if (!genStarted || !genFinished) return {};

    const dateFormat = 'HH:mm:ss D MMM YYYY';
    const start = moment(genStarted);
    const finish = moment(genFinished);
    const seconds = finish.diff(start, 'seconds', true);
    const tokenRate = tokenCount > 0 && seconds > 0 ? tokenCount / seconds : null;
    const timerTitle = [
        `Generation queued: ${start.format(dateFormat)}`,
        `Reply received: ${finish.format(dateFormat)}`,
        `Time to generate: ${seconds} seconds`,
        timeToFirstToken ? `Time to first token: ${timeToFirstToken / 1000} seconds` : '',
        reasoningDuration > 0 ? `Time to think: ${reasoningDuration / 1000} seconds` : '',
        tokenRate !== null ? `Token rate: ${tokenRate.toFixed(3)} t/s` : '',
    ].filter(Boolean).join('\n');

    if (isNaN(seconds) || seconds < 0) return { timerValue: '', timerTitle };
    return { timerValue: `${seconds.toFixed(1)}s`, timerTitle, tokenRate };
}

/** Render metadata in the existing message header, including clearing an absent swipe value. */
export function updateMessageGenerationInfo(messageElement, message, tokenRate) {
    const firstToken = message.extra?.time_to_first_token;
    const cache = message.extra?.prompt_cache;
    const show = !message.is_user && !message.is_system;
    messageElement.querySelector('.mes_generation_ttft').textContent =
        show && firstToken != null ? t`TTFT ${(firstToken / 1000).toFixed(1)}s` : '';
    messageElement.querySelector('.mes_generation_rate').textContent =
        show && tokenRate != null ? t`${tokenRate.toFixed(1)} token/s` : '';
    const cacheElement = messageElement.querySelector('.mes_generation_cache');
    cacheElement.textContent = show && cache?.input_tokens > 0
        ? t`Cache hit ${(cache.cached_tokens / cache.input_tokens * 100).toFixed(0)}%` : '';
    cacheElement.title = cache ? t`Share of input tokens read from cache in this request: ${cache.cached_tokens} / ${cache.input_tokens}` : '';
}
