import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { validateDevLogsBoundary, type DevLogsHandle } from './DevLogsContract';
import { LiveLogPanel } from './LiveLogPanel';

export function mountTauriTavernDevLogsApp(mount: unknown, options: unknown): DevLogsHandle {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern dev logs mount element is required');
    }
    validateDevLogsBoundary(options);

    const root = createRoot(mount);
    root.render(
        <StrictMode>
            <LiveLogPanel {...options} />
        </StrictMode>,
    );

    return {
        unmount: () => root.unmount(),
    };
}
