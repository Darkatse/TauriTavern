/** Install the native startup fact for a test and restore it during teardown. */
export function installHostIdentity(identity = HOSTS.windows, target = globalThis) {
    const previous = Object.getOwnPropertyDescriptor(target, '__TAURITAVERN_HOST__');
    Object.defineProperty(target, '__TAURITAVERN_HOST__', {
        configurable: true,
        value: Object.freeze(identity),
    });
    return () => {
        if (previous) {
            Object.defineProperty(target, '__TAURITAVERN_HOST__', previous);
        } else {
            delete target.__TAURITAVERN_HOST__;
        }
    };
}

export const HOSTS = {
    windows: { platform: 'windows', kind: 'desktop' },
    macos: { platform: 'macos', kind: 'desktop' },
    linux: { platform: 'linux', kind: 'desktop' },
    android: { platform: 'android', kind: 'mobile' },
    ios: { platform: 'ios', kind: 'mobile' },
    ohos: { platform: 'ohos', kind: 'mobile' },
};
