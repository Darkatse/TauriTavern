import { translateAgentSystem as tr } from './i18n';

const SELECTED_METRIC_KEYS = Object.freeze({ chars: 'chars', words: 'words' });
const TOTAL_METRIC_KEYS = Object.freeze({ chars: 'totalChars', words: 'totalWords' });
const COPY_METRIC_KEYS = Object.freeze(['chars', 'words', 'totalChars', 'totalWords'] as const);

type TextMetricKey = 'chars' | 'words' | 'totalChars' | 'totalWords';
type MetricKeys = Readonly<{ chars: TextMetricKey; words: TextMetricKey }>;
export type TextMetricFields = Partial<Record<TextMetricKey, number>>;

export function textMetricsSummary(value: unknown): string {
    return metricsSummary(value, SELECTED_METRIC_KEYS);
}

export function totalTextMetricsSummary(value: unknown): string {
    return metricsSummary(value, TOTAL_METRIC_KEYS);
}

export function textMetricFields(value: unknown): TextMetricFields {
    const fields: TextMetricFields = {};
    for (const key of COPY_METRIC_KEYS) {
        const metric = readMetric(value, key);
        if (metric !== null) {
            fields[key] = metric;
        }
    }
    return fields;
}

function metricsSummary(value: unknown, keys: MetricKeys): string {
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

function readMetric(value: unknown, key: TextMetricKey): number | null {
    if (!plainObject(value) || !Object.prototype.hasOwnProperty.call(value, key)) {
        return null;
    }

    const metric = value[key];
    if (typeof metric !== 'number' || !Number.isInteger(metric) || metric < 0) {
        throw new Error(`agent.run_text_metrics_invalid: ${key}`);
    }
    return metric;
}

function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
