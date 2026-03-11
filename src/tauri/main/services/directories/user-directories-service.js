// @ts-check

/**
 * @typedef {import('../../context/types.js').TauriInvokeFn} TauriInvokeFn
 * @typedef {import('../../context/types.js').UserDirectories} UserDirectories
 */

/**
 * @param {{ invoke: TauriInvokeFn }} deps
 */
export function createUserDirectoriesService({ invoke }) {
    /** @type {UserDirectories | null} */
    let userDirectories = null;

    async function initialize() {
        userDirectories = await invoke('get_default_user_directory');
    }

    function getUserDirectories() {
        return userDirectories;
    }

    return {
        initialize,
        getUserDirectories,
    };
}

