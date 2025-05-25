// Tauri initialization script for SillyTavern
import * as TauriBridge from './tauri-bridge.js';

// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

// Initialize Tauri integration
async function initializeTauri() {
    if (!isTauri) {
        console.log('Not running in Tauri environment. Using standard SillyTavern backend.');
        return;
    }
    
    console.log('Initializing Tauri integration...');
    
    try {
        // Initialize the bridge
        const initialized = await TauriBridge.initializeBridge();
        
        if (initialized) {
            console.log('Tauri bridge initialized successfully.');
            
            // Expose the bridge to the global scope for debugging
            window.TauriBridge = TauriBridge;
            
            TauriBridge.addEventListener('Error', (data) => {
                console.error('Tauri backend error:', data);
                // Show error to user
                if (typeof toastr !== 'undefined') {
                    toastr.error(data.message || 'Unknown error occurred');
                }
            });
            
            // Get version information
            const version = await TauriBridge.getVersion();
            console.log('Tauri backend version:', version);
            
        } else {
            console.error('Failed to initialize Tauri bridge.');
        }
    } catch (error) {
        console.error('Error initializing Tauri integration:', error);
    }
}

// Initialize when the document is ready
document.addEventListener('DOMContentLoaded', initializeTauri);

// Export the bridge for use in other scripts
export { TauriBridge };
