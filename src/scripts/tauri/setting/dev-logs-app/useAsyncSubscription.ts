import { useEffect, useEffectEvent } from 'react';

export type SubscriptionTeardown = () => void | Promise<void>;

async function disposeSubscription(teardown: SubscriptionTeardown): Promise<void> {
    try {
        await teardown();
    } catch (error) {
        console.error('TauriTavern dev log subscription teardown failed', error);
    }
}

/**
 * Owns the lifecycle of one host push subscription for a mounted panel.
 *
 * - Entry and error handlers are effect events, so local state changes never
 *   resubscribe; `subscribe` itself must be a stable reference (client
 *   methods qualify).
 * - Each effect setup registers a fresh wrapper as the subscriber identity:
 *   the stable effect-event function would collide in identity-based host
 *   registries, letting the StrictMode-discarded first subscription's late
 *   teardown remove the live one.
 * - A teardown that resolves only after cleanup is disposed immediately, so
 *   no host listener outlives the panel.
 * - Teardown failures remain visible in the console after the panel is gone.
 * - Late entries and late setup failures after cleanup are ignored.
 * - Sync and Promise teardowns are both accepted.
 *
 * Under StrictMode this settles into exactly one active subscription after
 * mount, and zero after the real unmount.
 */
export function useAsyncSubscription<TEntry>(
    subscribe: (handler: (entry: TEntry) => void) => Promise<SubscriptionTeardown>,
    onEntry: (entry: TEntry) => void,
    onError: (error: unknown) => void,
): void {
    const handleEntry = useEffectEvent(onEntry);
    const handleError = useEffectEvent(onError);

    useEffect(() => {
        let cleanedUp = false;
        let teardown: SubscriptionTeardown | null = null;
        const subscriber = (entry: TEntry) => handleEntry(entry);

        const run = async () => {
            let result: SubscriptionTeardown;
            try {
                result = await subscribe(subscriber);
            } catch (error) {
                if (!cleanedUp) {
                    handleError(error);
                }
                return;
            }
            if (cleanedUp) {
                await disposeSubscription(result);
                return;
            }
            teardown = result;
        };
        void run();

        return () => {
            cleanedUp = true;
            if (teardown) {
                void disposeSubscription(teardown);
            }
        };
    }, [subscribe]);
}
