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
 * Declining the rationale turns generation notifications off, so it is not shown again until they
 * are turned back on.
 *
 * @param {{
 *   safeInvoke: SafeInvokeFn;
 *   confirmPermissionRationale: () => Promise<boolean>;
 *   showNotificationsOffNotice: () => unknown;
 * }} deps
 */
export function createSystemNotificationService({
    safeInvoke,
    confirmPermissionRationale,
    showNotificationsOffNotice,
}) {
    /** @type {Promise<NotificationPermissionState> | null} */
    let permissionRequestPromise = null;
    /** @type {Promise<boolean> | null} */
    let permissionRationalePromise = null;

    async function isEnabled() {
        const state = await safeInvoke('get_generation_notification_state');
        if (typeof state?.enabled !== 'boolean') {
            throw new Error('Invalid generation notification state');
        }
        return state.enabled;
    }

    async function getPermissionState() {
        return normalizePermissionState(await safeInvoke('get_notification_permission_state'));
    }

    function requestPermission() {
        permissionRequestPromise ??= safeInvoke('request_notification_permission')
            .then(normalizePermissionState)
            .finally(() => {
                permissionRequestPromise = null;
            });

        return permissionRequestPromise;
    }

    async function turnOffNotifications() {
        await safeInvoke('update_tauritavern_settings', {
            dto: { generation_notifications_enabled: false },
        });
        await showNotificationsOffNotice();
    }

    function confirmPermissionRationaleOnce() {
        permissionRationalePromise ??= confirmPermissionRationale()
            .then(async (accepted) => {
                if (!accepted) {
                    await turnOffNotifications();
                }

                return accepted;
            })
            .finally(() => {
                permissionRationalePromise = null;
            });

        return permissionRationalePromise;
    }

    async function prepareForGeneration() {
        if (!await isEnabled()) {
            return false;
        }

        const currentState = await getPermissionState();
        if (currentState === 'granted') {
            return true;
        }
        if (currentState !== 'prompt') {
            return false;
        }

        return await confirmPermissionRationaleOnce()
            ? (await requestPermission()) === 'granted'
            : false;
    }

    /**
     * @param {{ title: string; body: string }} params
     */
    async function notifyIfAllowed({ title, body }) {
        if (!await isEnabled()) {
            return;
        }
        if (await getPermissionState() !== 'granted') {
            return;
        }

        await safeInvoke('show_system_notification', {
            dto: {
                title: String(title ?? '').trim(),
                body: String(body ?? '').trim(),
            },
        });
    }

    return {
        prepareForGeneration,
        notifyIfAllowed,
    };
}
