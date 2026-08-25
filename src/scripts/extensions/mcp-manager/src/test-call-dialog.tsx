import {
    useEffect,
    useImperativeHandle,
    useRef,
    useState,
    type ReactNode,
    type Ref,
} from 'react';
import { createRoot } from 'react-dom/client';

import { createTextPopup, errorText, tr, type McpMessageKey, type McpTranslator } from './host';
import {
    buildArgumentFields,
    collectArgumentsJson,
    initialArgumentValues,
    type ArgumentField,
    type ArgumentFieldError,
} from './schema-form';

export type TestCallDialogDeps = {
    servers: TauriTavernMcpServer[];
    discover: TauriTavernMcpApi['servers']['discover'];
    refresh: TauriTavernMcpApi['servers']['refresh'];
    testCall: TauriTavernMcpApi['tools']['testCall'];
};

export type TestCallDialogHandle = {
    /** Stops the local wait for an in-flight call without closing anything. */
    abort: () => void;
};

type TestCallDialogProps = TestCallDialogDeps & {
    tr: McpTranslator;
    ref: Ref<TestCallDialogHandle>;
};

type CallPhase = 'idle' | 'waiting' | 'stopping';

const FIELD_ERROR_KEYS: Record<ArgumentFieldError, McpMessageKey> = {
    required: 'fieldRequired',
    number: 'invalidNumber',
    integer: 'invalidInteger',
    json: 'invalidJson',
};

const PERMISSION_KEYS: Record<TauriTavernMcpToolPermission, McpMessageKey> = {
    off: 'permissionOff',
    ask: 'permissionAsk',
    allow: 'permissionAllow',
};

function permissionLabel(tool: TauriTavernMcpTool, tr: McpTranslator): string {
    return tr(PERMISSION_KEYS[tool.permission]);
}

function ArgumentInput({
    field,
    index,
    value,
    disabled,
    describedBy,
    invalid,
    labelledBy,
    tr,
    onChange,
}: {
    field: ArgumentField;
    index: number;
    value: string;
    disabled: boolean;
    describedBy: string | undefined;
    invalid: boolean;
    labelledBy: string;
    tr: McpTranslator;
    onChange: (value: string) => void;
}): ReactNode {
    const id = `tt-mcp-arg-${index}`;
    const accessibility = {
        'aria-describedby': describedBy,
        'aria-invalid': invalid || undefined,
    } as const;
    switch (field.kind) {
        case 'text':
            return (
                <input
                    id={id}
                    className="text_pole"
                    type="text"
                    value={value}
                    autoComplete="off"
                    disabled={disabled}
                    required={field.required}
                    {...accessibility}
                    onChange={event => onChange(event.currentTarget.value)}
                />
            );
        case 'number':
        case 'integer':
            return (
                <input
                    id={id}
                    className="text_pole"
                    type="text"
                    inputMode={field.kind === 'integer' ? 'numeric' : 'decimal'}
                    value={value}
                    autoComplete="off"
                    disabled={disabled}
                    required={field.required}
                    {...accessibility}
                    onChange={event => onChange(event.currentTarget.value)}
                />
            );
        case 'boolean':
            return (
                <div
                    id={id}
                    className="tt-mcp-seg"
                    role="radiogroup"
                    aria-labelledby={labelledBy}
                    aria-required={field.required || undefined}
                    {...accessibility}
                >
                    {(['', 'true', 'false'] as const).map(option => (
                        <label key={option || 'unset'} className={value === option ? 'is-selected' : ''}>
                            <input
                                type="radio"
                                name={id}
                                value={option}
                                checked={value === option}
                                disabled={disabled}
                                onChange={() => onChange(option)}
                            />
                            <span>{option === '' ? tr('notSet') : option}</span>
                        </label>
                    ))}
                </div>
            );
        case 'enum':
            return (
                <select
                    id={id}
                    className="text_pole"
                    value={value}
                    disabled={disabled}
                    required={field.required}
                    {...accessibility}
                    onChange={event => onChange(event.currentTarget.value)}
                >
                    <option value="">{tr('notSet')}</option>
                    {field.options.map(option => (
                        <option key={option.token} value={option.token}>{option.label}</option>
                    ))}
                </select>
            );
        case 'lines':
            return (
                <textarea
                    id={id}
                    className="text_pole"
                    rows={3}
                    value={value}
                    spellCheck={false}
                    disabled={disabled}
                    required={field.required}
                    {...accessibility}
                    onChange={event => onChange(event.currentTarget.value)}
                />
            );
        case 'json':
            return (
                <textarea
                    id={id}
                    className="text_pole tt-mcp-json"
                    rows={4}
                    value={value}
                    spellCheck={false}
                    disabled={disabled}
                    required={field.required}
                    {...accessibility}
                    onChange={event => onChange(event.currentTarget.value)}
                />
            );
    }
}

function OutcomeView({ outcome, tr }: { outcome: TauriTavernMcpTestCallOutcome; tr: McpTranslator }): ReactNode {
    if (outcome.outcome === 'not_sent') {
        return (
            <>
                <h4 role="status"><i className="fa-solid fa-ban" aria-hidden="true" />{tr('notSent')}</h4>
                <p>{outcome.message}</p>
                <code className="tt-mcp-outcome-code">{outcome.code}</code>
            </>
        );
    }
    if (outcome.outcome === 'outcome_unknown') {
        return (
            <>
                <h4 className="is-danger" role="status"><i className="fa-solid fa-circle-question" aria-hidden="true" />{tr('outcomeUnknown')}</h4>
                <p>{outcome.message}</p>
                <p className="tt-mcp-note">{tr('outcomeUnknownHint')}</p>
                <code className="tt-mcp-outcome-code">{outcome.code}</code>
            </>
        );
    }

    const response = outcome.response;
    if (response.kind === 'server_error') {
        return (
            <>
                <h4 className="is-danger" role="status"><i className="fa-solid fa-circle-exclamation" aria-hidden="true" />{tr('serverError')}</h4>
                <p>{response.message}</p>
                <code className="tt-mcp-outcome-code">{response.code}</code>
                {response.dataJson !== undefined && <pre>{response.dataJson}</pre>}
            </>
        );
    }
    if (response.kind === 'unsupported_response') {
        return (
            <>
                <h4 role="status"><i className="fa-solid fa-circle-info" aria-hidden="true" />{tr('unsupportedResponse')}</h4>
                <p>{response.message}</p>
                <code className="tt-mcp-outcome-code">{response.responseType}</code>
            </>
        );
    }

    return (
        <>
            <h4 className={response.isError ? 'is-danger' : 'is-ok'} role="status">
                <i className={`fa-solid ${response.isError ? 'fa-circle-exclamation' : 'fa-circle-check'}`} aria-hidden="true" />
                {tr(response.isError ? 'toolError' : 'serverResponded')}
            </h4>
            {response.textBlocks.map(block => <pre key={block.index}>{block.text}</pre>)}
            {response.structuredJson !== undefined && (
                <>
                    <b>{tr('structuredResult')}</b>
                    <pre className="tt-mcp-json-output">{response.structuredJson}</pre>
                </>
            )}
            {response.diagnostics.length > 0 && (
                <details className="tt-mcp-details">
                    <summary>{tr('diagnostics')} · {response.diagnostics.length}</summary>
                    <ul>
                        {response.diagnostics.map(diagnostic => (
                            <li key={`${diagnostic.code}:${diagnostic.contentIndex ?? ''}:${diagnostic.message}`}>
                                <code>
                                    {diagnostic.contentIndex === undefined
                                        ? diagnostic.code
                                        : `${diagnostic.code} [${diagnostic.contentIndex}]`}
                                </code>
                                <span>{diagnostic.message}</span>
                            </li>
                        ))}
                    </ul>
                </details>
            )}
            {response.textBlocks.length === 0 && response.structuredJson === undefined && (
                <p className="tt-mcp-note">{tr('noDisplayableContent')}</p>
            )}
        </>
    );
}

export function TestCallDialog({ servers, discover, refresh, testCall, tr, ref }: TestCallDialogProps) {
    const activeServers = servers.filter(server => server.state === 'active');
    const [serverId, setServerId] = useState(activeServers.length === 1 ? (activeServers[0]?.id ?? '') : '');
    const [tools, setTools] = useState<TauriTavernMcpTool[] | null>(null);
    const [diagnostics, setDiagnostics] = useState<TauriTavernMcpDiscoveryResult['diagnostics']>([]);
    const [toolsBusy, setToolsBusy] = useState(serverId !== '');
    const [toolsError, setToolsError] = useState('');
    const [toolName, setToolName] = useState('');
    const [fields, setFields] = useState<ArgumentField[]>([]);
    const [values, setValues] = useState<Record<string, string>>({});
    const [fieldErrors, setFieldErrors] = useState<Record<string, ArgumentFieldError>>({});
    const [phase, setPhase] = useState<CallPhase>('idle');
    const [outcome, setOutcome] = useState<TauriTavernMcpTestCallOutcome | null>(null);
    const [callError, setCallError] = useState('');
    const [refreshNonce, setRefreshNonce] = useState(0);
    const abortRef = useRef<AbortController | null>(null);
    const discoverSeq = useRef(0);

    const selectedServer = activeServers.find(server => server.id === serverId);
    const selectedTool = tools?.find(tool => tool.nativeName === toolName);
    const busy = phase !== 'idle';

    useImperativeHandle(ref, () => ({
        abort: () => {
            abortRef.current?.abort();
        },
    }));

    useEffect(() => {
        // Bumping first invalidates any previous in-flight discover, including on unset.
        const seq = ++discoverSeq.current;
        if (!serverId) {
            return;
        }
        const load = refreshNonce === 0 ? discover : refresh;
        load(serverId)
            .then(result => {
                if (discoverSeq.current === seq) {
                    setTools(result.tools);
                    setDiagnostics(result.diagnostics);
                    setToolsError('');
                }
            })
            .catch((error: unknown) => {
                if (discoverSeq.current === seq) {
                    setToolsError(errorText(error, tr('unknownError')));
                }
            })
            .finally(() => {
                if (discoverSeq.current === seq) {
                    setToolsBusy(false);
                }
            });
    }, [serverId, refreshNonce, discover, refresh, tr]);

    function selectServer(id: string): void {
        if (id === serverId) {
            return;
        }
        setServerId(id);
        setTools(null);
        setDiagnostics([]);
        setToolsError('');
        setRefreshNonce(0);
        setToolName('');
        setFields([]);
        setOutcome(null);
        setCallError('');
        setToolsBusy(id !== '');
    }

    function selectTool(nativeName: string): void {
        const tool = tools?.find(item => item.nativeName === nativeName);
        const nextFields = tool ? buildArgumentFields(tool.inputSchema) : [];
        setToolName(nativeName);
        setFields(nextFields);
        setValues(initialArgumentValues(nextFields));
        setFieldErrors({});
        setOutcome(null);
        setCallError('');
    }

    function changeValue(name: string, value: string): void {
        setValues(current => ({ ...current, [name]: value }));
        setFieldErrors(current => {
            if (!(name in current)) {
                return current;
            }
            const next = { ...current };
            delete next[name];
            return next;
        });
    }

    async function run(): Promise<void> {
        if (!selectedServer || !selectedTool || busy) {
            return;
        }
        const collected = collectArgumentsJson(fields, values);
        // Note: narrow with `in`, not `!collected.ok` — the lint program's module
        // resolution degrades union-discriminant narrowing to an error type here.
        if ('errors' in collected) {
            setFieldErrors(collected.errors);
            return;
        }

        setFieldErrors({});
        setOutcome(null);
        setCallError('');
        const controller = new AbortController();
        abortRef.current = controller;
        setPhase('waiting');
        try {
            setOutcome(await testCall({
                registrationId: selectedServer.id,
                nativeName: selectedTool.nativeName,
                argumentsJson: collected.json,
            }, { signal: controller.signal }));
        } catch (error) {
            setCallError(errorText(error, tr('unknownError')));
        } finally {
            abortRef.current = null;
            setPhase('idle');
        }
    }

    return (
        <div className="tt-mcp-test">
            <h3>{tr('testCallTitle')}</h3>

            {activeServers.length === 0 ? (
                <p className="tt-mcp-note">
                    <i className="fa-solid fa-circle-info" aria-hidden="true" />
                    {tr('noActiveServers')}
                </p>
            ) : (
                <>
                    <div className="tt-mcp-field">
                        <label htmlFor="tt-mcp-test-server">{tr('selectServer')}</label>
                        <select
                            id="tt-mcp-test-server"
                            className="text_pole"
                            value={serverId}
                            disabled={busy}
                            onChange={event => selectServer(event.currentTarget.value)}
                        >
                            <option value="">{tr('selectServerPlaceholder')}</option>
                            {activeServers.map(server => (
                                <option key={server.id} value={server.id}>{server.displayName}</option>
                            ))}
                        </select>
                        {selectedServer && <code className="tt-mcp-test-endpoint">{selectedServer.endpoint}</code>}
                    </div>

                    {toolsError && (
                        <div className="tt-mcp-test-tools-error">
                            <p className="tt-mcp-error" role="alert">{toolsError}</p>
                            <button
                                type="button"
                                className="menu_button menu_button_icon"
                                disabled={busy}
                                onClick={() => {
                                    setToolsError('');
                                    setToolsBusy(true);
                                    setRefreshNonce(nonce => nonce + 1);
                                }}
                            >
                                <i className="fa-solid fa-arrows-rotate" aria-hidden="true" />
                                <span>{tr('retry')}</span>
                            </button>
                        </div>
                    )}

                    {diagnostics.length > 0 && (
                        <details className="tt-mcp-details">
                            <summary>{tr('diagnostics')} · {diagnostics.length}</summary>
                            <ul>
                                {diagnostics.map(diagnostic => (
                                    <li key={`${diagnostic.code}:${diagnostic.nativeName ?? ''}:${diagnostic.message}`}>
                                        <b>{diagnostic.nativeName ?? diagnostic.code}</b>
                                        <span>{diagnostic.message}</span>
                                    </li>
                                ))}
                            </ul>
                        </details>
                    )}

                    <div className="tt-mcp-field">
                        <label htmlFor="tt-mcp-test-tool">{tr('selectTool')}</label>
                        <select
                            id="tt-mcp-test-tool"
                            className="text_pole"
                            value={toolName}
                            disabled={busy || !tools}
                            onChange={event => selectTool(event.currentTarget.value)}
                        >
                            <option value="">
                                {toolsBusy ? tr('loadingTools') : tr('selectToolPlaceholder')}
                            </option>
                            {(tools ?? []).map(tool => (
                                <option key={tool.nativeName} value={tool.nativeName}>
                                    {tool.title ?? tool.nativeName}
                                </option>
                            ))}
                        </select>
                    </div>

                    {selectedTool && (
                        <div className="tt-mcp-test-args">
                            <div className="tt-mcp-test-tool-head">
                                <code>{selectedTool.nativeName}</code>
                                {selectedTool.description && <p>{selectedTool.description}</p>}
                            </div>

                            {fields.length === 0 && <p className="tt-mcp-note">{tr('noArguments')}</p>}

                            {fields.map((field, index) => {
                                const fieldError = fieldErrors[field.name];
                                const id = `tt-mcp-arg-${index}`;
                                const labelId = `${id}-label`;
                                const hintId = field.hint ? `${id}-hint` : undefined;
                                const linesHintId = field.kind === 'lines' ? `${id}-lines-hint` : undefined;
                                const errorId = fieldError ? `${id}-error` : undefined;
                                const describedBy = [hintId, linesHintId, errorId].filter(Boolean).join(' ') || undefined;
                                return (
                                    <div className="tt-mcp-field" key={field.name}>
                                        <label
                                            id={labelId}
                                            htmlFor={field.kind === 'boolean' ? undefined : id}
                                        >
                                            <code>{field.name}</code>
                                            {field.required && <span className="tt-mcp-req" aria-hidden="true">*</span>}
                                        </label>
                                        <ArgumentInput
                                            field={field}
                                            index={index}
                                            value={values[field.name] ?? ''}
                                            disabled={busy}
                                            describedBy={describedBy}
                                            invalid={fieldError !== undefined}
                                            labelledBy={labelId}
                                            tr={tr}
                                            onChange={value => changeValue(field.name, value)}
                                        />
                                        {field.hint && <small id={hintId}>{field.hint}</small>}
                                        {field.kind === 'lines' && <small id={linesHintId}>{tr('onePerLine')}</small>}
                                        {fieldError && (
                                            <p id={errorId} className="tt-mcp-error" role="alert">{tr(FIELD_ERROR_KEYS[fieldError])}</p>
                                        )}
                                    </div>
                                );
                            })}

                            <p className="tt-mcp-note">
                                <i className="fa-solid fa-bolt" aria-hidden="true" />
                                {tr('testCallWarning')}
                            </p>
                            <p className="tt-mcp-note">
                                {tr('testCallPermission', { permission: permissionLabel(selectedTool, tr) })}
                            </p>

                            <div className="tt-mcp-test-actions">
                                <button
                                    type="button"
                                    className="menu_button menu_button_icon"
                                    disabled={busy}
                                    onClick={() => void run()}
                                >
                                    <i className={`fa-solid ${phase === 'waiting' ? 'fa-circle-notch fa-spin' : 'fa-play'}`} aria-hidden="true" />
                                    <span>{tr('runTest')}</span>
                                </button>
                                {busy && (
                                    <button
                                        type="button"
                                        className="menu_button"
                                        disabled={phase === 'stopping'}
                                        onClick={() => {
                                            abortRef.current?.abort();
                                            setPhase('stopping');
                                        }}
                                    >
                                        {tr('stopWaiting')}
                                    </button>
                                )}
                            </div>
                        </div>
                    )}
                </>
            )}

            <section className="tt-mcp-test-result">
                {phase === 'stopping' && (
                    <p className="tt-mcp-note" role="status">
                        <i className="fa-solid fa-circle-notch fa-spin" aria-hidden="true" />
                        {tr('stopping')}
                    </p>
                )}
                {phase === 'waiting' && (
                    <>
                        <p className="tt-mcp-note" role="status">
                            <i className="fa-solid fa-circle-notch fa-spin" aria-hidden="true" />
                            {tr('waitingForServer')}
                        </p>
                        <p className="tt-mcp-note">{tr('waitingForServerHint')}</p>
                    </>
                )}
                {callError && <p className="tt-mcp-error" role="alert">{callError}</p>}
                {outcome && <OutcomeView outcome={outcome} tr={tr} />}
            </section>
        </div>
    );
}

/**
 * Opens the unified test-call console in a vanilla popup. The dialog owns its
 * discovery fetches and the in-flight call; closing the popup aborts the local wait.
 */
export async function openTestCallDialog(deps: TestCallDialogDeps): Promise<void> {
    const mount = document.createElement('div');
    let dialog: TestCallDialogHandle | null = null;

    const root = createRoot(mount);
    try {
        root.render(
            <TestCallDialog
                {...deps}
                tr={tr}
                ref={handle => {
                    dialog = handle;
                }}
            />,
        );

        const popup = createTextPopup(mount, {
            okButton: tr('close'),
            allowVerticalScrolling: true,
            wide: true,
            leftAlign: true,
            onOpen: () => mount.querySelector<HTMLSelectElement>('#tt-mcp-test-server')?.focus(),
            onClosing: () => {
                dialog?.abort();
                return true;
            },
        });
        await popup.show();
    } finally {
        root.unmount();
    }
}
