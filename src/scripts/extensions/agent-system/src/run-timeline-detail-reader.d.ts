import type {
    TimelineDetailReadInput,
    TimelineDetailSection,
} from './RunTimelineContract';

export function readTimelineDetailSections(input: TimelineDetailReadInput): Promise<TimelineDetailSection[]>;
