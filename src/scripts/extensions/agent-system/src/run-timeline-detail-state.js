import { errorText } from './host-api.js';
import { readTimelineDetailSections } from './run-timeline-detail-reader.js';

export function createTimelineDetailState(options = {}) {
    const readSections = Object.hasOwn(options, 'readSections')
        ? options.readSections
        : readTimelineDetailSections;
    if (typeof readSections !== 'function') {
        throw new Error('Agent timeline detail readSections dependency must be a function.');
    }

    return {
        loading: false,
        error: '',
        sections: [],
        requestId: 0,

        reset() {
            this.requestId += 1;
            this.loading = false;
            this.error = '';
            this.sections = [];
        },

        async load({ runId, targets, readOnly = false } = {}) {
            const normalizedRunId = String(runId || '').trim();
            if (!normalizedRunId) {
                this.reset();
                return false;
            }

            const requestId = ++this.requestId;
            this.loading = true;
            this.error = '';
            try {
                const sections = await readSections({
                    runId: normalizedRunId,
                    targets,
                    readOnly,
                });
                if (requestId !== this.requestId) {
                    return false;
                }
                this.sections = sections;
                return true;
            } catch (error) {
                if (requestId === this.requestId) {
                    this.error = errorText(error);
                    this.sections = [];
                }
                return false;
            } finally {
                if (requestId === this.requestId) {
                    this.loading = false;
                }
            }
        },
    };
}
