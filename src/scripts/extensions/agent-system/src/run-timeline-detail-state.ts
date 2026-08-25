import { errorText } from './host-api.js';
import { readTimelineDetailSections } from './run-timeline-detail-reader.js';
import type {
    TimelineDetailReadInput,
    TimelineDetailSection,
} from './RunTimelineContract';

export type TimelineDetailState = {
    loading: boolean;
    error: string;
    sections: TimelineDetailSection[];
    reset: () => void;
    load: (input: TimelineDetailReadInput) => Promise<boolean>;
};

export function createTimelineDetailState(options: {
    readSections?: (input: TimelineDetailReadInput) => Promise<TimelineDetailSection[]>;
} = {}): TimelineDetailState {
    const readSections = options.readSections ?? readTimelineDetailSections;
    let requestId = 0;

    const state: TimelineDetailState = {
        loading: false,
        error: '',
        sections: [],
        reset() {
            requestId += 1;
            state.loading = false;
            state.error = '';
            state.sections = [];
        },
        async load(input) {
            const runId = input.runId.trim();
            if (!runId) throw new Error('Agent run id is required.');
            const currentRequestId = ++requestId;
            state.loading = true;
            state.error = '';
            try {
                const sections = await readSections({
                    runId,
                    targets: input.targets,
                    readOnly: input.readOnly,
                });
                if (currentRequestId !== requestId) return false;
                state.sections = sections;
                return true;
            } catch (error) {
                if (currentRequestId === requestId) {
                    state.error = errorText(error);
                    state.sections = [];
                }
                return false;
            } finally {
                if (currentRequestId === requestId) state.loading = false;
            }
        },
    };
    return state;
}
