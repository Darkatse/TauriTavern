// @ts-check

/**
 * @param {{ bridgeName: string }} deps
 */
export function createAndroidGenerationBridge({ bridgeName }) {
    const name = String(bridgeName || '').trim();
    if (!name) {
        throw new Error('AndroidGenerationBridge requires bridgeName');
    }

    const hostWindow = /** @type {any} */ (window);

    /**
     * @param {string} methodName
     * @returns {any}
     */
    function getMethod(methodName) {
        const bridge = hostWindow?.[name];
        if (!bridge) {
            return null;
        }

        // @ts-ignore - dynamic bridge lookup by name.
        return bridge?.[methodName] ?? null;
    }

    return {
        /**
         * @param {string} methodName
         * @param {...any} args
         * @returns {boolean}
         */
        call(methodName, ...args) {
            const bridge = hostWindow?.[name];
            // @ts-ignore - dynamic bridge method lookup by contract.
            const method = bridge?.[methodName];
            if (typeof method !== 'function') {
                return false;
            }

            try {
                method.apply(bridge, args);
                return true;
            } catch (error) {
                console.debug(`Failed to call ${name}.${methodName}:`, error);
                return false;
            }
        },

        /**
         * @param {string} methodName
         * @param {...any} args
         * @returns {any}
         */
        get(methodName, ...args) {
            const bridge = hostWindow?.[name];
            // @ts-ignore - dynamic bridge method lookup by contract.
            const method = bridge?.[methodName];
            if (typeof method !== 'function') {
                return null;
            }

            try {
                return method.apply(bridge, args);
            } catch (error) {
                console.debug(`Failed to call ${name}.${methodName}:`, error);
                return null;
            }
        },

        /** @param {string} methodName */
        has(methodName) {
            const method = getMethod(methodName);
            return typeof method === 'function';
        },
    };
}
