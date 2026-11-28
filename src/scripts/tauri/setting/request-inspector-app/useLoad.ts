import { useEffect, useEffectEvent, useRef, useState } from 'react';

export type LoadState<V> =
    | { status: 'loading' }
    | { status: 'ready'; value: V }
    | { status: 'error'; error: string };

const LOADING: LoadState<never> = { status: 'loading' };

export function mapLoad<V, R>(state: LoadState<V>, map: (value: V) => R): LoadState<R> {
    return state.status === 'ready' ? { status: 'ready', value: map(state.value) } : state;
}

/**
 * Loads the value for `key` and keeps the last one loaded.
 *
 * - A `null` key loads nothing and keeps what is there, so the value is still
 *   present when the key comes back.
 * - Host reads cannot be cancelled: a result that arrives after a newer load
 *   started is dropped.
 */
export function useLoad<K extends string | number, V>(
    key: K | null,
    load: (key: K) => Promise<V>,
    errorText: (error: unknown) => string,
): LoadState<V> | null {
    const [slot, setSlot] = useState<{ key: K; state: LoadState<V> } | null>(null);
    const epochRef = useRef(0);

    const run = useEffectEvent(async (target: K) => {
        const epoch = ++epochRef.current;
        let state: LoadState<V>;
        try {
            state = { status: 'ready', value: await load(target) };
        } catch (error) {
            state = { status: 'error', error: errorText(error) };
        }
        if (epoch === epochRef.current) {
            setSlot({ key: target, state });
        }
    });

    useEffect(() => {
        if (key !== null && slot?.key !== key) {
            void run(key);
        }
    }, [key, slot]);

    return key === null ? null : slot?.key === key ? slot.state : LOADING;
}
