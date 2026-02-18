import { registerSystemRoutes } from './system-routes.js';
import { registerSettingsRoutes } from './settings-routes.js';
import { registerExtensionRoutes } from './extensions-routes.js';
import { registerQuickReplyRoutes } from './quick-replies-routes.js';
import { registerResourceRoutes } from './resource-routes.js';
import { registerCharacterRoutes } from './character-routes.js';
import { registerChatRoutes } from './chat-routes.js';
import { registerAiRoutes } from './ai-routes.js';
import { registerStatsRoutes } from './stats-routes.js';
import { registerWorldInfoRoutes } from './worldinfo-routes.js';
import { registerContentRoutes } from './content-routes.js';

export function registerRoutes(router, context, responses) {
    registerSystemRoutes(router, context, responses);
    registerSettingsRoutes(router, context, responses);
    registerQuickReplyRoutes(router, context, responses);
    registerExtensionRoutes(router, context, responses);
    registerResourceRoutes(router, context, responses);
    registerCharacterRoutes(router, context, responses);
    registerChatRoutes(router, context, responses);
    registerContentRoutes(router, context, responses);
    registerWorldInfoRoutes(router, context, responses);
    registerAiRoutes(router, context, responses);
    registerStatsRoutes(router, context, responses);
}
