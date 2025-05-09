import { DOMPurify, Handlebars } from '../lib.js';
import { applyLocale } from './i18n.js';
import { readTemplateFile } from './tauri/templates-api.js';

// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

/**
 * @type {Map<string, function>}
 * @description Cache for Handlebars templates.
 */
const TEMPLATE_CACHE = new Map();

/**
 * Loads a URL content using XMLHttpRequest synchronously or Tauri's file system API.
 * @param {string} url URL to load synchronously
 * @returns {string} Response text
 */
function getUrlSync(url) {
    console.debug('Loading URL synchronously', url);

    // If we're in a Tauri environment and the URL is a template path
    if (isTauri && url.startsWith('/scripts/templates/')) {
        // This is a synchronous function, but we need to use async in Tauri
        // Since we can't make this function async without breaking existing code,
        // we'll throw an error and recommend using the async version instead
        throw new Error('Synchronous template loading is not supported in Tauri. Use renderTemplateAsync instead.');
    }

    // Use XMLHttpRequest for non-Tauri environments
    const request = new XMLHttpRequest();
    request.open('GET', url, false); // `false` makes the request synchronous
    request.send();

    if (request.status >= 200 && request.status < 300) {
        return request.responseText;
    }

    throw new Error(`Error loading ${url}: ${request.status} ${request.statusText}`);
}

/**
 * Loads a URL content using XMLHttpRequest asynchronously or Tauri's file system API.
 * @param {string} url URL to load asynchronously
 * @returns {Promise<string>} Response text
 */
async function getUrlAsync(url) {
    console.debug('Loading URL asynchronously', url);

    // If we're in a Tauri environment and the URL is a template path
    if (isTauri && url.startsWith('/scripts/templates/')) {
        try {
            // Convert URL path to a relative path for Tauri resource
            // Remove the leading slash and convert to a relative path
            const templatePath = url.replace('/scripts/templates/', 'frontend-templates/');

            // Read the template file using Tauri's file system API
            return await readTemplateFile(templatePath);
        } catch (error) {
            console.error(`Error loading template in Tauri: ${url}`, error);
            throw error;
        }
    }

    // Use XMLHttpRequest for non-Tauri environments
    return new Promise((resolve, reject) => {
        const request = new XMLHttpRequest();
        request.open('GET', url, true);
        request.onload = () => {
            if (request.status >= 200 && request.status < 300) {
                resolve(request.responseText);
            } else {
                reject(new Error(`Error loading ${url}: ${request.status} ${request.statusText}`));
            }
        };
        request.onerror = () => {
            reject(new Error(`Error loading ${url}: ${request.status} ${request.statusText}`));
        };
        request.send();
    });
}

/**
 * Renders a Handlebars template asynchronously.
 * @param {string} templateId ID of the template to render
 * @param {Record<string, any>} templateData The data to pass to the template
 * @param {boolean} sanitize Should the template be sanitized with DOMPurify
 * @param {boolean} localize Should the template be localized
 * @param {boolean} fullPath Should the template ID be treated as a full path or a relative path
 * @returns {Promise<string>} Rendered template
 */
export async function renderTemplateAsync(templateId, templateData = {}, sanitize = true, localize = true, fullPath = false) {
    /**
     * Fetches and compiles a template from the given path
     * @param {string} pathToTemplate - Path to the template file
     * @returns {Promise<Function>} - Compiled Handlebars template
     */
    async function fetchTemplateAsync(pathToTemplate) {
        let template = TEMPLATE_CACHE.get(pathToTemplate);
        if (!template) {
            const templateContent = await getUrlAsync(pathToTemplate);
            template = Handlebars.compile(templateContent);
            TEMPLATE_CACHE.set(pathToTemplate, template);
        }
        return template;
    }

    try {
        const pathToTemplate = fullPath ? templateId : `/scripts/templates/${templateId}.html`;
        const template = await fetchTemplateAsync(pathToTemplate);
        let result = template(templateData);

        if (sanitize) {
            result = DOMPurify.sanitize(result);
        }

        if (localize) {
            result = applyLocale(result);
        }

        return result;
    } catch (err) {
        console.error('Error rendering template', templateId, templateData, err);
        // Use console.error instead of toastr to avoid dependency issues
        console.error('Error rendering template. Check the DevTools console for more information.');
        return `<div class="error">Error loading template: ${templateId}</div>`;
    }
}

/**
 * Renders a Handlebars template synchronously.
 * @param {string} templateId ID of the template to render
 * @param {Record<string, any>} templateData The data to pass to the template
 * @param {boolean} sanitize Should the template be sanitized with DOMPurify
 * @param {boolean} localize Should the template be localized
 * @param {boolean} fullPath Should the template ID be treated as a full path or a relative path
 * @returns {string} Rendered template
 *
 * @deprecated Use renderTemplateAsync instead.
 */
export function renderTemplate(templateId, templateData = {}, sanitize = true, localize = true, fullPath = false) {
    /**
     * Fetches and compiles a template from the given path
     * @param {string} pathToTemplate - Path to the template file
     * @returns {Function} - Compiled Handlebars template
     */
    function fetchTemplateSync(pathToTemplate) {
        let template = TEMPLATE_CACHE.get(pathToTemplate);
        if (!template) {
            const templateContent = getUrlSync(pathToTemplate);
            template = Handlebars.compile(templateContent);
            TEMPLATE_CACHE.set(pathToTemplate, template);
        }
        return template;
    }

    try {
        // In Tauri environment, recommend using the async version
        if (isTauri) {
            console.warn('Synchronous template rendering is not recommended in Tauri. Use renderTemplateAsync instead.');
            return `<div class="warning">Synchronous template rendering is not supported in Tauri. Use async version instead.</div>`;
        }

        const pathToTemplate = fullPath ? templateId : `/scripts/templates/${templateId}.html`;
        const template = fetchTemplateSync(pathToTemplate);
        let result = template(templateData);

        if (sanitize) {
            result = DOMPurify.sanitize(result);
        }

        if (localize) {
            result = applyLocale(result);
        }

        return result;
    } catch (err) {
        console.error('Error rendering template', templateId, templateData, err);
        // Use console.error instead of toastr to avoid dependency issues
        console.error('Error rendering template. Check the DevTools console for more information.');
        return `<div class="error">Error loading template: ${templateId}</div>`;
    }
}
