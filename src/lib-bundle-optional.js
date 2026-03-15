// Optional library bundle for TauriTavern.
//
// This bundle should contain libraries that are not required for Shell/Core
// readiness, especially heavy deps that are primarily used for specific UI
// affordances (syntax highlighting) or rarely used utilities (scraping).

import hljs from 'highlight.js';
import { isProbablyReaderable, Readability } from '@mozilla/readability';

const optionalBundle = {
    hljs,
    isProbablyReaderable,
    Readability,
    initialized: true,
};

export {
    hljs,
    isProbablyReaderable,
    Readability,
};

export default optionalBundle;

