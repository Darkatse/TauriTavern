import { DOMPurify, Handlebars } from '../lib.js';
import { applyLocale } from './i18n.js';
import { isTauriEnv, invoke } from '../tauri-bridge.js';

/**
 * @type {Map<string, function>}
 * @description Cache for Handlebars templates.
 */
const TEMPLATE_CACHE = new Map();

/**
 * Loads a URL content using XMLHttpRequest synchronously.
 * @param {string} url URL to load synchronously
 * @returns {string} Response text
 */
function getUrlSync(url) {
    console.debug('Loading URL synchronously', url);
    const request = new XMLHttpRequest();
    request.open('GET', url, false); // `false` makes the request synchronous
    request.send();

    if (request.status >= 200 && request.status < 300) {
        return request.responseText;
    }

    throw new Error(`Error loading ${url}: ${request.status} ${request.statusText}`);
}

/**
 * Loads a URL content using fetch asynchronously.
 * @param {string} url URL to load asynchronously
 * @returns {Promise<string>} Response text
 */
async function getUrlAsync(url) {
    const response = await fetch(url);
    if (!response.ok) {
        throw new Error(`Error loading ${url}: ${response.status} ${response.statusText}`);
    }
    return response.text();
}

/**
 * Loads a template content, using Tauri resource reading on Tauri environments
 * and fetch on regular web environments.
 * @param {string} templateId Template ID (e.g., 'emptyBlock')
 * @param {boolean} fullPath Whether templateId is a full path
 * @returns {Promise<string>} Template HTML content
 */
async function getTemplateContent(templateId, fullPath) {
    // On Tauri, use invoke to read from bundled resources (handles Android asset:// paths)
    if (isTauriEnv && invoke && !fullPath) {
        const fileName = `${templateId}.html`;
        return invoke('read_frontend_template', { name: fileName });
    }

    // Fallback: use fetch for non-Tauri or full-path templates
    const pathToTemplate = fullPath ? templateId : `/scripts/templates/${templateId}.html`;
    return getUrlAsync(pathToTemplate);
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
    async function fetchTemplateAsync(pathOrId) {
        const cacheKey = fullPath ? pathOrId : `tauri:${pathOrId}`;
        let template = TEMPLATE_CACHE.get(cacheKey);
        if (!template) {
            const templateContent = await getTemplateContent(pathOrId, fullPath);
            template = Handlebars.compile(templateContent);
            TEMPLATE_CACHE.set(cacheKey, template);
        }
        return template;
    }

    try {
        const template = await fetchTemplateAsync(templateId);
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
        toastr.error('Check the DevTools console for more information.', 'Error rendering template');
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
        toastr.error('Check the DevTools console for more information.', 'Error rendering template');
    }
}
