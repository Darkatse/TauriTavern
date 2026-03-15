// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';
import { installChatEmbeddedRuntimeAdapters } from '../../adapters/embedded-runtime/chat-embedded-runtime-adapter.js';
import { createEmbeddedRuntimeService } from './embedded-runtime-service.js';

export function installEmbeddedRuntime() {
    const service = createEmbeddedRuntimeService();

    eventSource.on(event_types.APP_READY, () => {
        installChatEmbeddedRuntimeAdapters({ manager: service.manager });
    });

    return service;
}
