import type {
    TimelineDelegationEdge,
    TimelineDetailTarget,
    TimelineItem,
} from './RunTimelineContract';

export const TERMINAL_EVENT_TYPES: readonly string[];
export function isDisplayableRunEvent(event: TauriTavernAgentRunEvent): boolean;
export function hasModelTurnNarration(event: TauriTavernAgentRunEvent): boolean;
export function timelineItemsFromEvents(
    events: readonly TauriTavernAgentRunEvent[],
    options?: {
        invocationId?: string;
        foregroundInvocationIds?: readonly string[];
        delegationEdges?: readonly TimelineDelegationEdge[];
    },
): TimelineItem[];
export function buildEventDetailTargets(
    item: TimelineItem,
    allEvents: readonly TauriTavernAgentRunEvent[],
): TimelineDetailTarget[];
