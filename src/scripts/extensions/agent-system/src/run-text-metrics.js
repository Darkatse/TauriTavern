import { translateAgentSystem as tr } from './i18n.js';

const SELECTED_METRIC_KEYS = Object.freeze({ chars: 'chars', words: 'words' });
const TOTAL_METRIC_KEYS = Object.freeze({ chars: 'totalChars', words: 'totalWords' });
const COPY_METRIC_KEYS = Object.freeze(['chars', 'words', 'totalChars', 'totalWords']);

export function textMetricsSummary(value) {
    return metricsSummary(value, SELECTED_METRIC_KEYS);
}

export function totalTextMetricsSummary(value) {
    return metricsSummary(value, TOTAL_METRIC_KEYS);
}

export function textMetricFields(value) {
    if (!value || typeof value !== 'object') {
        return {};
    }

    const fields = {};
    for (const key of COPY_METRIC_KEYS) {
        if (hasOwn(value, key)) {
            fields[key] = readMetric(value, key);
        }
    }
    return fields;
}

function metricsSummary(value, keys) {
    const chars = readMetric(value, keys.chars);
    const words = readMetric(value, keys.words);
    const hasChars = chars != null;
    const hasWords = words != null;
    if (hasChars && hasWords) {
        return tr('timelineTextMetrics', { chars, words });
    }
    if (hasChars) {
        return tr('timelineCharCount', { count: chars });
    }
    if (hasWords) {
        return tr('timelineWordCount', { count: words });
    }
    return '';
}

function readMetric(value, key) {
    if (!value || typeof value !== 'object' || !hasOwn(value, key)) {
        return null;
    }

    const metric = value[key];
    if (!Number.isInteger(metric) || metric < 0) {
        throw new Error(`agent.run_text_metrics_invalid: ${key}`);
    }
    return metric;
}

function hasOwn(value, key) {
    return Object.prototype.hasOwnProperty.call(value, key);
}
