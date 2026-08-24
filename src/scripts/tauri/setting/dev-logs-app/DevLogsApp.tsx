import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import {
    validateDevLogsBoundary,
    type DevLogsHandle,
    type DevLogsMountOptions,
} from './DevLogsContract';
import { LiveLogPanel } from './LiveLogPanel';
import { LlmApiLogsPanel } from './LlmApiLogsPanel';

function DevLogsApp({ options }: { options: DevLogsMountOptions }) {
    if (options.kind === 'live') {
        return <LiveLogPanel {...options} />;
    }
    return <LlmApiLogsPanel {...options} />;
}

export function mountTauriTavernDevLogsApp(mount: unknown, options: DevLogsMountOptions): DevLogsHandle {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern dev logs mount element is required');
    }
    validateDevLogsBoundary(options);

    const root = createRoot(mount);
    root.render(
        <StrictMode>
            <DevLogsApp options={options} />
        </StrictMode>,
    );

    return {
        unmount: () => root.unmount(),
    };
}
