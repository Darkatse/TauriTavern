// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';
import { installJsSlashRunnerRuntimeAdapter } from '../../adapters/embedded-runtime/js-slash-runner-runtime-adapter.js';
import { createEmbeddedRuntimeService } from './embedded-runtime-service.js';

export function installEmbeddedRuntime() {
    const service = createEmbeddedRuntimeService();

    eventSource.on(event_types.APP_READY, () => {
        installJsSlashRunnerRuntimeAdapter({ manager: service.manager });
    });

    return service;
}
