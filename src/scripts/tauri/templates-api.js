/**
 * Templates API for Tauri
 * This module provides functions for reading template files in a Tauri environment
 */

// Import Tauri API
const { resolveResource } = window.__TAURI__.path;
const { readTextFile } = window.__TAURI__.fs;

/**
 * Reads a template file from the resources directory
 * @param {string} templatePath - Path to the template file relative to the resources directory
 * @returns {Promise<string>} - The content of the template file
 */
export async function readTemplateFile(templatePath) {
    try {
        // Resolve the resource path
        const resourcePath = await resolveResource(templatePath);
        
        // Read the file content
        const content = await readTextFile(resourcePath);

        console.debug('Read template file', templatePath, 'from', resourcePath);
        
        return content;
    } catch (error) {
        console.error(`Error reading template file ${templatePath}:`, error);
        throw error;
    }
}

export default {
    readTemplateFile
};
