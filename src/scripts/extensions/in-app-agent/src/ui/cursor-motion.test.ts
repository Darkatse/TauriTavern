import { expect, test } from '@rstest/core';
import { planCursorMove } from './cursor-motion';

test('acting on the same point again keeps the cursor still and unturned', () => {
    const point = { x: 100.1, y: 100.1 };
    const { samples } = planCursorMove({ ...point, rotation: 0 }, point);
    for (const { x, y, rotation } of samples) {
        expect(Math.hypot(x - point.x, y - point.y)).toBeLessThan(1e-6);
        expect(rotation).toBe(0);
    }
});
