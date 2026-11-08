import { afterEach } from '@rstest/core';
import { cleanup } from '@testing-library/react';

afterEach(() => {
    cleanup();
    document.body.replaceChildren();
});
