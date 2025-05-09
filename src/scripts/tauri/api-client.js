// Tauri API Client Factory
// This module provides a factory for creating API clients for different domains

import { invoke } from '../../tauri-bridge.js';

// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

/**
 * Creates an API client for a specific domain
 * @param {string} domain - The domain of the API (e.g., 'secrets', 'characters')
 * @returns {Object} - An object with methods for interacting with the API
 */
export function createApiClient(domain) {
    // Base URL for fallback HTTP requests
    const baseUrl = `/api/${domain}`;

    /**
     * Generic function to call a Tauri command or fallback to HTTP
     * @param {string} method - The method name
     * @param {Object} params - The parameters to pass to the method
     * @param {Object} options - Additional options for HTTP requests
     * @returns {Promise<any>} - The result of the API call
     */
    async function callApi(method, params = {}, options = {}) {
        // If not in Tauri environment or invoke is not available, fallback to HTTP
        if (!isTauri || !invoke) {
            const { httpMethod = 'POST', path = method } = options;

            const url = `${baseUrl}/${path}`;
            const fetchOptions = {
                method: httpMethod,
                headers: {
                    'Content-Type': 'application/json',
                },
            };

            // Add body for POST requests
            if (httpMethod === 'POST' && Object.keys(params).length > 0) {
                fetchOptions.body = JSON.stringify(params);
            }

            try {
                const response = await fetch(url, fetchOptions);

                if (!response.ok) {
                    if (response.status === 403) {
                        throw new Error('Permission denied');
                    }
                    throw new Error(`HTTP error ${response.status}`);
                }

                // Check if the response is JSON
                const contentType = response.headers.get('content-type');
                if (contentType && contentType.includes('application/json')) {
                    return await response.json();
                }

                return await response.text();
            } catch (error) {
                console.error(`Error calling ${domain}/${method} via HTTP:`, error);
                throw error;
            }
        }

        // Use Tauri invoke
        const commandName = options.commandName || `${domain.replace(/-/g, '_')}_${method.replace(/-/g, '_')}`;
        try {
            return await invoke(commandName, params);
        } catch (error) {
            console.error(`Error calling ${commandName} via Tauri:`, error);
            throw error;
        }
    }

    return {
        /**
         * Call a specific API method
         * @param {string} method - The method name
         * @param {Object} params - The parameters to pass to the method
         * @param {Object} options - Additional options for HTTP requests
         * @returns {Promise<any>} - The result of the API call
         */
        call: callApi,

        /**
         * Create a method on the client
         * @param {string} methodName - The name of the method
         * @param {Object} options - Options for the method
         * @returns {Function} - A function that calls the API method
         */
        createMethod(methodName, options = {}) {
            return async (params = {}) => {
                return await callApi(methodName, params, options);
            };
        }
    };
}
