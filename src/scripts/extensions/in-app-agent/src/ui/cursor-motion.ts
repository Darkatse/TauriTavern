// Adapted from the signature_arc motion of Cua Driver's agent cursor:
// https://github.com/trycua/cua/tree/main/libs/typescript/cursor-motion
// Copyright (c) 2025 Cua AI, Inc. MIT License.
import type { InteractionPoint } from './geometry';

/** The tip, and the arrow's rotation in radians from its resting up-left pose. */
export type CursorPose = InteractionPoint & { rotation: number };
/** The speed glow: how far it trails the arrow in screen space, its scale and opacity. */
export type CursorGlow = InteractionPoint & { scale: number; opacity: number };
export type CursorSample = CursorPose & { t: number; glow: CursorGlow };
export type CursorMove = { samples: CursorSample[]; arrival: number; duration: number };

// Plans run on Cua's clock and only their output is sped up, so the turn keeps pace with the path.
const PLAYBACK_RATE = 1.5;
const STEP = 1000 / 120;
const SEGMENTS = 256;
const HANDLE = 0.3;
const ARC = 0.16;
const ARC_FLOW = 0.575;
const RESTING_TIP = -0.75 * Math.PI;
const RESTING_GLOW: CursorGlow = { x: 0, y: 0, scale: 1, opacity: 0 };

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);
const wrapAngle = (angle: number) => angle - 2 * Math.PI * Math.round(angle / (2 * Math.PI));
const minJerk = (t: number) => t * t * t * (10 - 15 * t + 6 * t * t);
const followThrough = (t: number) => t ** 8.2 * (1 - t) ** 1.8 / (0.82 ** 8.2 * 0.18 ** 1.8);
const decay = (ms: number) => Math.exp(-ms * 0.022);

/** Plan a move from the cursor's current pose; times are in milliseconds of playback. */
export function planCursorMove(from: CursorPose, to: InteractionPoint): CursorMove {
    const distance = Math.hypot(to.x - from.x, to.y - from.y);
    // Fitts' law for a 24 px target, the size Cua assumes when it cannot see the target.
    const duration = 1.1 * clamp(150 + 120 * Math.log2(distance / 24 + 1), 300, 1000);
    const steps = Math.ceil(duration / STEP);
    const step = duration / steps;
    const overshoot = Math.min(0.018, 8 / Math.max(distance, 1));
    const path = arcPath(from, to);
    const position = (t: number) => {
        const progress = clamp(t / duration, 0, 1);
        return path(minJerk(progress) + overshoot * followThrough(progress));
    };

    let rotation = wrapAngle(from.rotation);
    let travel = 0;
    const samples = Array.from({ length: steps + 1 }, (_, i): CursorSample => {
        const t = i * step;
        const before = Math.max(t - 2 * step, 0);
        const after = Math.min(t + 2 * step, duration);
        const a = position(before);
        const b = position(after);
        const vx = (b.x - a.x) / (after - before) * 1000;
        const vy = (b.y - a.y) / (after - before) * 1000;
        const speed = Math.hypot(vx, vy);
        const lead = clamp((speed - 40) / 260, 0, 1);
        // Keep the travel direction continuous; a wrapped one flips sign when the motion opposes the tip.
        travel += wrapAngle(Math.atan2(vy, vx) - RESTING_TIP - travel);
        if (i > 0) rotation += wrapAngle(travel * lead - rotation) * (1 - decay(step));
        return { t, ...position(t), rotation, glow: glow(vx, vy, speed) };
    });
    // Settle on the nearest full turn; unwinding to zero would spin the arrow back around.
    const rest = rotation - wrapAngle(rotation);
    let end = duration;
    while (Math.abs(rotation - rest) >= 0.002) {
        end += STEP;
        rotation = rest + (rotation - rest) * decay(STEP);
        samples.push({ t: end, ...to, rotation, glow: RESTING_GLOW });
    }
    const arrival = samples.find(sample => Math.hypot(sample.x - to.x, sample.y - to.y) <= 1)?.t ?? duration;

    return {
        samples: samples.map(sample => ({ ...sample, t: sample.t / PLAYBACK_RATE })),
        arrival: arrival / PLAYBACK_RATE,
        duration: end / PLAYBACK_RATE,
    };
}

/** A cubic arc parameterized by the fraction of its length; past the end it continues straight. */
function arcPath(from: InteractionPoint, to: InteractionPoint) {
    const dx = to.x - from.x;
    const dy = to.y - from.y;
    // Horizontal moves bow upward.
    const bend = dx >= 0 ? -ARC : ARC;
    const near = bend * (1 - ARC_FLOW / 2);
    const far = bend * (0.5 + ARC_FLOW / 2);
    const c1 = { x: from.x + dx * HANDLE - dy * near, y: from.y + dy * HANDLE + dx * near };
    const c2 = { x: to.x - dx * HANDLE - dy * far, y: to.y - dy * HANDLE + dx * far };
    const curve = (u: number): InteractionPoint => {
        const v = 1 - u;
        return {
            x: v * v * v * from.x + 3 * v * v * u * c1.x + 3 * v * u * u * c2.x + u * u * u * to.x,
            y: v * v * v * from.y + 3 * v * v * u * c1.y + 3 * v * u * u * c2.y + u * u * u * to.y,
        };
    };

    let length = 0;
    let previous = from;
    const segments = Array.from({ length: SEGMENTS }, (_, i) => {
        const point = curve((i + 1) / SEGMENTS);
        const start = length;
        length += Math.hypot(point.x - previous.x, point.y - previous.y);
        previous = point;
        return { i, start, end: length };
    });
    const before = curve(1 - 1e-3);
    // Coincident ends leave no end direction, so the path stays on the target past its end.
    const tangent = Math.hypot(to.x - before.x, to.y - before.y) || 1;

    return (fraction: number): InteractionPoint => {
        const target = fraction * length;
        const segment = segments.find(({ end }) => end >= target);
        if (!segment) {
            const beyond = (target - length) / tangent;
            return { x: to.x + (to.x - before.x) * beyond, y: to.y + (to.y - before.y) * beyond };
        }
        return curve((segment.i + (target - segment.start) / (segment.end - segment.start || 1)) / SEGMENTS);
    };
}

function glow(vx: number, vy: number, speed: number): CursorGlow {
    const opacity = Math.min(speed * 0.00014, 0.42);
    if (opacity <= 0.02) return RESTING_GLOW;
    const lag = Math.min(speed * 0.009, 18) / speed;
    return { x: -vx * lag, y: -vy * lag, scale: 1 + Math.min(speed * 0.00024, 0.44), opacity };
}

function effect<T>(duration: number, frame: (age: number) => T) {
    const steps = Math.ceil(duration / STEP);
    return {
        duration: duration / PLAYBACK_RATE,
        frames: Array.from({ length: steps + 1 }, (_, i) => frame(i / steps * duration)),
    };
}

/** The arrow's scale on a click: pressed in 50 ms, held until 90 ms, then released. */
export const press = effect(90 + 220 / 3, age => {
    const release = Math.max(age - 90, 0) / (220 / 3);
    return 1 - 0.12 * Math.min(age / 50, 1) * Math.cos(release * Math.PI / 2) * (1 - release / 3);
});

/** The click ring: its scale relative to full size, and its opacity. */
export const ripple = effect(520, age => {
    const progress = age / 520;
    return { scale: (8 + 44 * (1 - (1 - progress) ** 3)) / 52, opacity: 0.75 * (1 - progress) };
});
