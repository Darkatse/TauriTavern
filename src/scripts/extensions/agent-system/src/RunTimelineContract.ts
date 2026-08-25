import type { AgentSystemSettings } from './AgentSystemEntryController';
import type { AgentSystemTr } from './i18n.js';
import type { TimelineVirtualResult } from './run-timeline-virtual-list';

export type TimelineRun = {
    runId: string;
    generationType?: string;
};

export type TimelineReadInput = Parameters<TauriTavernAgentApi['readEvents']>[0] & { limit: number };

export type TimelineProjectionInvocation = Omit<
    TauriTavernAgentRunTimelineInvocation,
    'parentInvocationId' | 'kind' | 'status' | 'exitPolicy'
> & {
    parentInvocationId: string;
    kind: string;
    status: string;
    exitPolicy: string;
};

export type TimelineDelegationEdge = Omit<
    TauriTavernAgentRunTimelineDelegationEdge,
    'continuation' | 'status' | 'resultRef' | 'error'
> & {
    continuation: string;
    status: string;
    resultRef: string;
    error: string;
};

export type TimelineProjection = {
    foregroundInvocationIds: string[];
    invocations: TimelineProjectionInvocation[];
    delegationEdges: TimelineDelegationEdge[];
};

export type TimelineReadResult = Omit<
    Awaited<ReturnType<TauriTavernAgentApi['readEvents']>>,
    'timelineProjection'
> & {
    timelineProjection?: unknown;
};

export type TimelineItem = {
    id: string;
    seq: number;
    runId: string;
    type: string;
    level: string;
    timestamp: string;
    icon: string;
    tone: string;
    kind: string;
    titleKey: string;
    titleParams: Record<string, unknown>;
    summary: string;
    rawEvent: TauriTavernAgentRunEvent;
    rowSpan?: number;
    detailTargets?: TimelineDetailTarget[];
};

export type TimelineDetailTarget = {
    type: string;
    [key: string]: unknown;
};

type TimelineDetailActionBase = {
    labelKey: string;
    hintKey?: string;
    icon?: string;
};

export type TimelineDetailAction =
    | TimelineDetailActionBase & { kind: 'openSubAgent'; invocationId: string }
    | TimelineDetailActionBase & { kind: 'retry' };

export type TimelineDetailField = {
    label: string;
    value: string;
};

export type TimelineDiffRow = {
    type: string;
    oldLine: number | null;
    newLine: number | null;
    marker: string;
    text: string;
};

export type TimelineDetailBlock = {
    kind?: string;
    labelKey?: string;
    label?: string;
    text?: string;
    rows?: TimelineDiffRow[];
    meta?: string;
    truncated?: boolean;
    defaultOpen?: boolean;
};

export type TimelineDetailSection = {
    labelKey: string;
    path?: string;
    fields?: TimelineDetailField[];
    blocks?: TimelineDetailBlock[];
    actions?: TimelineDetailAction[];
};

export type TimelineDetailSnapshot = {
    loading: boolean;
    error: string;
    sections: readonly TimelineDetailSection[];
};

export type TimelineDetailReadInput = {
    runId: string;
    targets: readonly TimelineDetailTarget[];
    readOnly: boolean;
};

export type TimelineVirtualWindow = TimelineVirtualResult<TimelineItem>;

export type TimelineViewport = {
    scrollTop: number;
    viewportHeight: number;
    nearBottom: boolean;
};

export type SubAgentTask = {
    taskId: string;
    targetInvocationId: string;
    workspaceKey: string;
    status: string;
    color: string;
    displayName: string;
};

export type SubAgentTimelineSnapshot = {
    open: boolean;
    task: SubAgentTask | null;
    title: string;
    subtitle: string;
    displayItems: readonly TimelineItem[];
    virtualItems: TimelineVirtualWindow;
    selectedItem: TimelineItem | null;
    selectedSeq: number | null;
    navItems: readonly TimelineItem[];
    loading: boolean;
    loadingOlder: boolean;
    autoStick: boolean;
    detail: TimelineDetailSnapshot;
};

export type RunTimelineSnapshot = {
    mode: 'active' | 'history';
    rootId: string;
    visible: boolean;
    displayItems: readonly TimelineItem[];
    virtualItems: TimelineVirtualWindow;
    selectedItem: TimelineItem | null;
    selectedSeq: number | null;
    latestSeq: number | null;
    activeSeq: number | null;
    navItems: readonly TimelineItem[];
    loading: boolean;
    loadingOlder: boolean;
    detail: TimelineDetailSnapshot;
    collapsed: boolean;
    detailsOpen: boolean;
    autoStick: boolean;
    trayExpanded: boolean;
    panelHeightPx: number | null;
    resizing: boolean;
    isRunning: boolean;
    terminalType: string;
    panelStatus: TimelinePanelStatus;
    panelView: 'collapsed' | 'details' | 'events';
    headerTitle: string;
    headerSubtitle: string;
    detailTitle: string;
    selectedHasDetails: boolean;
    emptyText: string;
    subAgentTasks: readonly SubAgentTask[];
    subAgentTrayTitle: string;
    subAgent: SubAgentTimelineSnapshot;
};

export type TimelinePanelStatus =
    | 'failed'
    | 'cancelled'
    | 'partial'
    | 'completed'
    | 'running'
    | 'ready'
    | 'idle';

export type TimelineResizeBounds = { min: number; max: number };

export type RunTimelineController = {
    getSnapshot: () => RunTimelineSnapshot;
    subscribe: (listener: () => void) => () => void;
    init: () => Promise<void>;
    dispose: () => void;
    loadOlder: () => Promise<boolean>;
    selectItem: (seq: number) => void;
    toggleCollapsed: () => void;
    openDetails: () => void;
    showTimeline: () => void;
    toggleSubAgentTray: () => void;
    openSubAgent: (invocationId: string) => void;
    closeSubAgent: () => void;
    loadOlderSubAgent: () => Promise<boolean>;
    selectSubAgentItem: (seq: number) => void;
    invokeDetailAction: (action: TimelineDetailAction) => void;
    setTimelineViewport: (viewport: TimelineViewport) => void;
    setSubAgentViewport: (viewport: TimelineViewport) => void;
    startViewGesture: (event: PointerEvent) => void;
    trackViewGesture: (event: PointerEvent) => void;
    finishViewGesture: (event: PointerEvent) => void;
    cancelViewGesture: (pointerId: number) => void;
    startResize: (startY: number, startHeight: number, bounds: TimelineResizeBounds) => void;
    moveResize: (clientY: number) => void;
    finishResize: (save: boolean) => void;
    resizeByKey: (key: string, currentHeight: number, bounds: TimelineResizeBounds) => boolean;
    resetPanelHeight: () => void;
    requestClose: () => void;
};

type SharedTimelineDeps = {
    readEvents: (input: TimelineReadInput) => Promise<TimelineReadResult>;
    reportError: (error: unknown) => void;
    tr: AgentSystemTr;
};

export type ActiveTimelineOptions = {
    mode: 'active';
    deps: SharedTimelineDeps & {
        loadSettings: () => Promise<AgentSystemSettings>;
        patchSettings: (
            current: AgentSystemSettings,
            patch: Partial<AgentSystemSettings>,
        ) => Promise<AgentSystemSettings>;
        subscribeSettings: (listener: (settings: AgentSystemSettings) => void) => () => void;
        getActiveRun: () => TimelineRun | null;
        subscribeRunState: (listener: (state: {
            activeRun: TimelineRun | null;
            lastEvent: TauriTavernAgentRunEvent | null;
        }) => void) => () => void;
        subscribeRunEvents: (listener: (event: TauriTavernAgentRunEvent) => void) => () => void;
        retryFailure: (input: {
            run: TimelineRun | null;
            events: readonly TauriTavernAgentRunEvent[];
            terminalEvent: TauriTavernAgentRunEvent | null;
        }) => Promise<unknown>;
    };
};

export type HistoryTimelineOptions = {
    mode: 'history';
    rootId: string;
    run: TimelineRun;
    requestClose: () => void;
    deps: SharedTimelineDeps;
};

export type RunTimelineOptions = ActiveTimelineOptions | HistoryTimelineOptions;
