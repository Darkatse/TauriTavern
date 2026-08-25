import type {
    SubAgentTask,
    TimelineProjection,
} from './RunTimelineContract';

export function projectSubAgentTasks(projection: TimelineProjection): SubAgentTask[];
export function eventBelongsToInvocation(event: TauriTavernAgentRunEvent, invocationId: string): boolean;
export function normalizeInvocationId(value: unknown): string;
export function isActiveTaskStatus(status: string): boolean;
