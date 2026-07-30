// Optional library bundle for TauriTavern.
//
// This bundle contains heavy libraries exposed through an intentionally async API.

import hljs from 'highlight.js';

const optionalBundle = {
    hljs,
    initialized: true,
};

export {
    hljs,
};

export default optionalBundle;
