// This file initializes the application
// It's used to load the libraries and start the application

// Define a global variable to indicate that we're running in Tauri
window.__TAURI_RUNNING__ = true;

// Log the initialization
console.log('TauriTavern: Initializing application');

// Create a function to initialize the application
async function initializeApplication() {
    try {
        // Import the library shim
        await import('./lib.js');
        console.log('TauriTavern: Library shim loaded');

        // Import the Tauri integration
        await import('./tauri-main.js');
        console.log('TauriTavern: Tauri integration loaded');

        // Import the main script
        await import('./script.js');
        console.log('TauriTavern: Main script loaded');

        console.log('TauriTavern: Application initialized successfully');
    } catch (error) {
        console.error('TauriTavern: Failed to initialize application:', error);
    }
}

// Initialize the application
initializeApplication();
