// @ts-check

/**
 * @typedef {'granted' | 'denied' | 'prompt'} NotificationPermissionState
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 */

const NOTIFICATION_PERMISSION_STATES = new Set(['granted', 'denied', 'prompt']);

/**
 * @param {unknown} value
 * @returns {NotificationPermissionState}
 */
function normalizePermissionState(value) {
    const normalized = String(value || '').trim().toLowerCase();
    if (NOTIFICATION_PERMISSION_STATES.has(normalized)) {
        return /** @type {NotificationPermissionState} */ (normalized);
    }

    throw new Error(`Unsupported notification permission state: ${String(value || '')}`);
}

/**
 * @param {{
 *   safeInvoke: SafeInvokeFn;
 * }} deps
 */
export function createSystemNotificationService({ safeInvoke }) {
    /** @type {Promise<NotificationPermissionState> | null} */
    let permissionRequestPromise = null;

    async function getPermissionState() {
        return normalizePermissionState(await safeInvoke('get_notification_permission_state'));
    }

    async function requestPermission() {
        if (!permissionRequestPromise) {
            permissionRequestPromise = safeInvoke('request_notification_permission')
                .then(normalizePermissionState)
                .finally(() => {
                    permissionRequestPromise = null;
                });
        }

        return permissionRequestPromise;
    }

    async function preparePermission() {
        const currentState = await getPermissionState();
        if (currentState !== 'prompt') {
            return currentState;
        }

        return requestPermission();
    }

    /**
     * @param {{ title: string; body: string }} params
     */
    async function show({ title, body }) {
        await safeInvoke('show_system_notification', {
            dto: {
                title: String(title ?? '').trim(),
                body: String(body ?? '').trim(),
            },
        });
    }

    return {
        getPermissionState,
        requestPermission,
        preparePermission,
        show,
    };
}
