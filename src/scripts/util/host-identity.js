// @ts-check

/** @typedef {'windows' | 'macos' | 'linux' | 'android' | 'ios' | 'ohos'} HostPlatform */
/** @typedef {{ platform: HostPlatform, kind: 'desktop' | 'mobile' }} HostIdentity */

/** @returns {Readonly<HostIdentity>} */
function identity() {
    const value = globalThis.__TAURITAVERN_HOST__;
    if (!value) {
        throw new Error('TauriTavern host identity is missing');
    }
    return value;
}

export function hostPlatform() { return identity().platform; }
export function isMobileHost() { return identity().kind === 'mobile'; }
export function isDesktopHost() { return identity().kind === 'desktop'; }
