import { afterEach } from '@rstest/core';
import { cleanup, configure } from '@testing-library/react';

// Test files run in parallel; under that load the default 1 s wait is too short.
configure({ asyncUtilTimeout: 3000 });

afterEach(() => {
    cleanup();
    document.body.replaceChildren();
});
