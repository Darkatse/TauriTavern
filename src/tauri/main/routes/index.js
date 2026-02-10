import { registerSystemRoutes } from './system-routes.js';
import { registerSettingsRoutes } from './settings-routes.js';
import { registerExtensionRoutes } from './extensions-routes.js';
import { registerResourceRoutes } from './resource-routes.js';
import { registerCharacterRoutes } from './character-routes.js';
import { registerChatRoutes } from './chat-routes.js';

export function registerRoutes(router, context, responses) {
    registerSystemRoutes(router, context, responses);
    registerSettingsRoutes(router, context, responses);
    registerExtensionRoutes(router, context, responses);
    registerResourceRoutes(router, context, responses);
    registerCharacterRoutes(router, context, responses);
    registerChatRoutes(router, context, responses);
}
