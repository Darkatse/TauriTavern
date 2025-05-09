// Tauri Backgrounds Adapter for SillyTavern
// This module adapts the SillyTavern backgrounds API to use Tauri backend

import * as BackgroundsAPI from '../scripts/tauri/backgrounds-api.js';
import { isTauri } from '../tauri-bridge.js';

/**
 * Initialize the backgrounds adapter
 */
export function initializeBackgroundsAdapter() {
    if (!isTauri) {
        console.log('Not in Tauri environment, skipping backgrounds adapter initialization');
        return;
    }

    console.log('Initializing Tauri backgrounds adapter');

    try {
        // Patch the global functions in backgrounds.js
        if (window.getBackgrounds) {
            const originalGetBackgrounds = window.getBackgrounds;
            window.getBackgrounds = async function() {
                try {
                    console.log('Using Tauri backend for getBackgrounds');
                    const backgrounds = await BackgroundsAPI.getAllBackgrounds();

                    // Process the backgrounds data as in the original function
                    $('#bg_menu_content').children('div').remove();
                    for (const bg of backgrounds) {
                        // bg is now just a filename string, not an object
                        const template = window.getBackgroundFromTemplate(bg, false);
                        $('#bg_menu_content').append(template);
                    }

                    return backgrounds;
                } catch (error) {
                    console.error('Error in Tauri getBackgrounds, falling back to original:', error);
                    return originalGetBackgrounds.apply(this, arguments);
                }
            };
        }

        if (window.delBackground) {
            const originalDelBackground = window.delBackground;
            window.delBackground = async function(bg) {
                try {
                    console.log(`Using Tauri backend for delBackground: ${bg}`);
                    return await BackgroundsAPI.deleteBackground(bg);
                } catch (error) {
                    console.error('Error in Tauri delBackground, falling back to original:', error);
                    return originalDelBackground.apply(this, arguments);
                }
            };
        }

        // Patch the uploadBackground function
        if (window.uploadBackground) {
            const originalUploadBackground = window.uploadBackground;
            window.uploadBackground = async function(formData) {
                try {
                    console.log('Using Tauri backend for uploadBackground');

                    // Extract file from FormData
                    const file = formData.get('avatar');
                    if (!file || !(file instanceof File)) {
                        throw new Error('No file found in FormData');
                    }

                    // Read file as ArrayBuffer
                    const arrayBuffer = await file.arrayBuffer();
                    const uint8Array = new Uint8Array(arrayBuffer);

                    // Upload using Tauri API
                    const bg = await BackgroundsAPI.uploadBackground(file.name, Array.from(uint8Array));

                    // Update UI as in the original function
                    window.setBackground(bg, window.generateUrlParameter(bg, false));
                    await window.getBackgrounds();
                    window.highlightNewBackground(bg);

                    return bg;
                } catch (error) {
                    console.error('Error in Tauri uploadBackground, falling back to original:', error);
                    return originalUploadBackground.apply(this, arguments);
                }
            };
        }

        // Patch the onRenameBackgroundClick function
        const originalOnRenameBackgroundClick = window.onRenameBackgroundClick;
        if (originalOnRenameBackgroundClick) {
            window.onRenameBackgroundClick = async function(e) {
                e.stopPropagation();

                const bgNames = await window.getNewBackgroundName(this);

                if (!bgNames) {
                    return;
                }

                try {
                    console.log(`Using Tauri backend for renameBackground: ${bgNames.oldBg} to ${bgNames.newBg}`);
                    await BackgroundsAPI.renameBackground(bgNames.oldBg, bgNames.newBg);
                    await window.getBackgrounds();
                    window.highlightNewBackground(bgNames.newBg);
                } catch (error) {
                    console.error('Error in Tauri renameBackground, falling back to original:', error);
                    if (originalOnRenameBackgroundClick) {
                        return originalOnRenameBackgroundClick.apply(this, arguments);
                    }
                }
            };
        }

        console.log('Tauri backgrounds adapter initialized successfully');
    } catch (error) {
        console.error('Error initializing Tauri backgrounds adapter:', error);
    }
}
