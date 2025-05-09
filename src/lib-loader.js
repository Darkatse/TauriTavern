/**
 * Library Loader for TauriTavern
 *
 * This file is a non-module script that loads the bundled libraries.
 * It's used to fix the "Failed to resolve module specifier" error by:
 * 1. Loading the bundled libraries from dist/lib.bundle.js
 * 2. Exposing the libraries to the global scope (window object)
 * 3. Verifying that all libraries are available
 */

// Log initialization
console.log('TauriTavern Library Loader: Initializing...');

// Define the libraries we expect to be loaded
const EXPECTED_LIBRARIES = [
    'lodash', 'Fuse', 'DOMPurify', 'hljs', 'localforage', 'Handlebars',
    'css', 'Bowser', 'DiffMatchPatch', 'isProbablyReaderable', 'Readability',
    'SVGInject', 'showdown', 'moment', 'seedrandom', 'Popper', 'droll',
    'morphdom', 'slideToggle', 'chalk'
];

// Set a global flag to indicate that the library loader is running
window.__TAURITAVERN_LIB_LOADER_RUNNING__ = true;

/**
 * Load the library bundle from one of several possible paths
 * @returns {Promise<boolean>} True if the bundle was loaded successfully
 */
async function loadLibraryBundle() {
    // Try multiple possible paths for the bundle
    const possiblePaths = [
        './dist/lib.bundle.js',
        '../dist/lib.bundle.js',
        '/dist/lib.bundle.js',
        'dist/lib.bundle.js'
    ];

    // Try each path until one works
    for (const path of possiblePaths) {
        try {
            console.log(`Trying to load library bundle from ${path}...`);

            // Create a script element
            const script = document.createElement('script');
            script.type = 'module'; // Use module type to support ES modules
            script.src = path;

            // Create a promise that resolves when the script loads
            const loadPromise = new Promise((resolve, reject) => {
                script.onload = () => {
                    console.log(`Library bundle loaded successfully from ${path}`);
                    resolve(true);
                };
                script.onerror = (error) => {
                    console.error(`Failed to load library bundle from ${path}:`, error);
                    reject(error);
                };
            });

            // Add the script to the page
            document.head.appendChild(script);
            console.log(`Added script for library bundle from ${path}`);

            // Wait for the script to load
            await loadPromise;

            // Wait a moment for the libraries to be initialized
            await new Promise(resolve => setTimeout(resolve, 500));

            // Check if libraries are available in global scope
            const librariesLoaded = checkLibraries();

            if (librariesLoaded) {
                window.__TAURITAVERN_LIB_BUNDLE_LOADED__ = true;
                return true;
            }
        } catch (error) {
            console.error(`Failed to load library bundle from ${path}:`, error);
        }
    }

    // If we get here, none of the paths worked
    console.error('Failed to load library bundle from any path');
    window.__TAURITAVERN_LIB_BUNDLE_LOADED__ = false;
    return false;
}

/**
 * Check if all expected libraries are available in the global scope
 * @returns {boolean} True if all libraries are available
 */
function checkLibraries() {
    const missingLibraries = EXPECTED_LIBRARIES.filter(lib => !(lib in window));

    if (missingLibraries.length > 0) {
        console.warn('Some libraries are not available in the global scope:', missingLibraries);
        return false;
    } else {
        console.log('All libraries are available in the global scope');
        return true;
    }
}

// Immediately-invoked async function to load the bundle
(async function() {
    try {
        const success = await loadLibraryBundle();

        if (success) {
            console.log('TauriTavern Library Loader: Libraries loaded successfully');
        } else {
            console.error('TauriTavern Library Loader: Failed to load libraries');
        }
    } catch (error) {
        console.error('TauriTavern Library Loader: Error during initialization', error);
    } finally {
        // Set a global flag to indicate that the library loader has completed
        window.__TAURITAVERN_LIB_LOADER_RUNNING__ = false;
        window.__TAURITAVERN_LIB_LOADER_COMPLETED__ = true;

        // Log completion
        console.log('TauriTavern Library Loader: Initialization complete');

        // Dispatch an event to notify that libraries are loaded
        window.dispatchEvent(new CustomEvent('tauritavern:libraries-loaded', {
            detail: {
                success: window.__TAURITAVERN_LIB_BUNDLE_LOADED__
            }
        }));
    }
})();
