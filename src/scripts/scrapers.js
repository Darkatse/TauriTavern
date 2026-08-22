import { getRequestHeaders } from '../script.js';
import { renderExtensionTemplateAsync } from './extensions.js';
import { POPUP_RESULT, POPUP_TYPE, callGenericPopup } from './popup.js';

/**
 * @typedef {Object} Scraper
 * @property {string} id
 * @property {string} name
 * @property {string} description
 * @property {string} iconClass
 * @property {boolean} iconAvailable
 * @property {() => Promise<void>} [init=null]
 * @property {() => Promise<boolean>} isAvailable
 * @property {() => Promise<File[]>} scrape
 */

/**
 * @typedef {Object} ScraperInfo
 * @property {string} id
 * @property {string} name
 * @property {string} description
 * @property {string} iconClass
 * @property {boolean} iconAvailable
 */

export class ScraperManager {
    /**
     * @type {Scraper[]}
     */
    static #scrapers = [];

    /**
     * Register a scraper to be used by the Data Bank.
     * @param {Scraper} scraper Instance of a scraper to register
     */
    static async registerDataBankScraper(scraper) {
        if (ScraperManager.#scrapers.some(s => s.id === scraper.id)) {
            console.warn(`Scraper with ID ${scraper.id} already registered`);
            return;
        }

        if (scraper.init) {
            await scraper.init();
        }

        ScraperManager.#scrapers.push(scraper);
    }

    /**
     * Gets a list of scrapers available for the Data Bank.
     * @returns {ScraperInfo[]} List of scrapers available for the Data Bank
     */
    static getDataBankScrapers() {
        return ScraperManager.#scrapers.map(s => ({ id: s.id, name: s.name, description: s.description, iconClass: s.iconClass, iconAvailable: s.iconAvailable }));
    }

    /**
     * Run a scraper to scrape data into the Data Bank.
     * @param {string} scraperId ID of the scraper to run
     * @returns {Promise<File[]>} List of files scraped by the scraper
     */
    static runDataBankScraper(scraperId) {
        const scraper = ScraperManager.#scrapers.find(s => s.id === scraperId);
        if (!scraper) {
            console.warn(`Scraper with ID ${scraperId} not found`);
            return;
        }
        return scraper.scrape();
    }

    /**
     * Check if a scraper is available.
     * @param {string} scraperId ID of the scraper to check
     * @returns {Promise<boolean>} Whether the scraper is available
     */
    static isScraperAvailable(scraperId) {
        const scraper = ScraperManager.#scrapers.find(s => s.id === scraperId);
        if (!scraper) {
            console.warn(`Scraper with ID ${scraperId} not found`);
            return;
        }
        return scraper.isAvailable();
    }
}

/**
 * Create a text file from a string.
 * @implements {Scraper}
 */
class Notepad {
    constructor() {
        this.id = 'text';
        this.name = 'Notepad';
        this.description = 'Create a text file from scratch.';
        this.iconClass = 'fa-solid fa-note-sticky';
        this.iconAvailable = true;
    }

    /**
     * Check if the scraper is available.
     * @returns {Promise<boolean>}
     */
    async isAvailable() {
        return true;
    }

    /**
     * Create a text file from a string.
     * @returns {Promise<File[]>} File attachments scraped from the text
     */
    async scrape() {
        const template = $(await renderExtensionTemplateAsync('attachments', 'notepad', {}));
        let fileName = `Untitled - ${new Date().toLocaleString()}`;
        let text = '';
        template.find('input[name="notepadFileName"]').val(fileName).on('input', function () {
            fileName = String($(this).val()).trim();
        });
        template.find('textarea[name="notepadFileContent"]').on('input', function () {
            text = String($(this).val());
        });

        const result = await callGenericPopup(template, POPUP_TYPE.CONFIRM, '', { wide: true, large: true, okButton: 'Save', cancelButton: 'Cancel' });

        if (!result || text === '') {
            return;
        }

        const file = new File([text], `Notepad - ${fileName}.txt`, { type: 'text/plain' });
        return [file];
    }
}

/**
 * Scrape data from a file selection.
 * @implements {Scraper}
 */
class FileScraper {
    constructor() {
        this.id = 'file';
        this.name = 'File';
        this.description = 'Upload a file from your computer.';
        this.iconClass = 'fa-solid fa-upload';
        this.iconAvailable = true;
    }

    /**
     * Check if the scraper is available.
     * @returns {Promise<boolean>}
     */
    async isAvailable() {
        return true;
    }

    /**
     * Scrape file attachments from a file.
     * @returns {Promise<File[]>} File attachments scraped from the files
     */
    async scrape() {
        return new Promise(resolve => {
            const fileInput = document.createElement('input');
            fileInput.type = 'file';
            fileInput.accept = '*/*';
            fileInput.multiple = true;
            fileInput.onchange = () => resolve(Array.from(fileInput.files));
            fileInput.click();
        });
    }
}

class MediaWikiScraper {
    constructor() {
        this.id = 'mediawiki';
        this.name = 'MediaWiki';
        this.description = 'Download a page from a MediaWiki wiki.';
        this.iconClass = 'fa-brands fa-wikipedia-w';
        this.iconAvailable = true;
    }

    async isAvailable() {
        try {
            const result = await fetch('/api/plugins/fandom/probe-mediawiki', {
                method: 'POST',
                headers: getRequestHeaders({ omitContentType: true }),
            });

            return result.ok;
        } catch (error) {
            console.debug('Could not probe Fandom/MediaWiki plugin', error);
            return false;
        }
    }

    async scrape() {
        let url = '';
        let filter = '';
        let output = 'single';

        const template = $(await renderExtensionTemplateAsync('attachments', 'mediawiki-scrape', {}));
        template.find('input[name="scrapeInput"]').on('input', function () {
            url = String($(this).val()).trim();
        });
        template.find('input[name="scrapeFilter"]').on('input', function () {
            filter = String($(this).val());
        });
        template.find('input[name="scrapeOutput"]').on('input', function () {
            output = String($(this).val());
        });

        const confirm = await callGenericPopup(template, POPUP_TYPE.CONFIRM, '', { wide: false, large: false, okButton: 'Scrape', cancelButton: 'Cancel' });

        if (confirm !== POPUP_RESULT.AFFIRMATIVE) {
            return;
        }

        if (!url) {
            toastr.error('URL name is required');
            return;
        }

        const toast = toastr.info('Working, please wait...');

        const result = await fetch('/api/plugins/fandom/scrape-mediawiki', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify({ url, filter }),
        });

        if (!result.ok) {
            const error = await result.text();
            throw new Error(error);
        }

        const data = await result.json();
        toastr.clear(toast);

        if (output === 'multi') {
            const files = [];
            for (const attachment of data) {
                const file = new File([String(attachment.content).trim()], `${String(attachment.title).trim()}.txt`, { type: 'text/plain' });
                files.push(file);
            }
            return files;
        }

        if (output === 'single') {
            const combinedContent = data.map((a) => String(a.title).trim() + '\n\n' + String(a.content).trim()).join('\n\n\n\n');
            const file = new File([combinedContent], `${url}.txt`, { type: 'text/plain' });
            return [file];
        }

        return [];
    }
}

/**
 * Scrape data from a Fandom wiki.
 * @implements {Scraper}
 */
class FandomScraper {
    constructor() {
        this.id = 'fandom';
        this.name = 'Fandom';
        this.description = 'Download a page from the Fandom wiki.';
        this.iconClass = 'fa-solid fa-fire';
        this.iconAvailable = true;
    }

    /**
     * Check if the scraper is available.
     * @returns {Promise<boolean>}
     */
    async isAvailable() {
        try {
            const result = await fetch('/api/plugins/fandom/probe', {
                method: 'POST',
                headers: getRequestHeaders({ omitContentType: true }),
            });

            return result.ok;
        } catch (error) {
            console.debug('Could not probe Fandom plugin', error);
            return false;
        }
    }

    /**
     * Get the ID of a fandom from a URL or name.
     * @param {string} fandom URL or name of the fandom
     * @returns {string} ID of the fandom
     */
    getFandomId(fandom) {
        try {
            const url = new URL(fandom);
            return url.hostname.split('.')[0] || fandom;
        } catch {
            return fandom;
        }
    }

    async scrape() {
        let fandom = '';
        let filter = '';
        let output = 'single';

        const template = $(await renderExtensionTemplateAsync('attachments', 'fandom-scrape', {}));
        template.find('input[name="fandomScrapeInput"]').on('input', function () {
            fandom = String($(this).val()).trim();
        });
        template.find('input[name="fandomScrapeFilter"]').on('input', function () {
            filter = String($(this).val());
        });
        template.find('input[name="fandomScrapeOutput"]').on('input', function () {
            output = String($(this).val());
        });

        const confirm = await callGenericPopup(template, POPUP_TYPE.CONFIRM, '', { wide: false, large: false, okButton: 'Scrape', cancelButton: 'Cancel' });

        if (confirm !== POPUP_RESULT.AFFIRMATIVE) {
            return;
        }

        if (!fandom) {
            toastr.error('Fandom name is required');
            return;
        }

        const toast = toastr.info('Working, please wait...');

        const result = await fetch('/api/plugins/fandom/scrape', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify({ fandom, filter }),
        });

        if (!result.ok) {
            const error = await result.text();
            throw new Error(error);
        }

        const data = await result.json();
        toastr.clear(toast);

        if (output === 'multi') {
            const files = [];
            for (const attachment of data) {
                const file = new File([String(attachment.content).trim()], `${String(attachment.title).trim()}.txt`, { type: 'text/plain' });
                files.push(file);
            }
            return files;
        }

        if (output === 'single') {
            const combinedContent = data.map((a) => String(a.title).trim() + '\n\n' + String(a.content).trim()).join('\n\n\n\n');
            const file = new File([combinedContent], `${fandom}.txt`, { type: 'text/plain' });
            return [file];
        }

        return [];
    }
}

export async function initScrapers() {
    await ScraperManager.registerDataBankScraper(new FileScraper());
    await ScraperManager.registerDataBankScraper(new Notepad());
    await ScraperManager.registerDataBankScraper(new MediaWikiScraper());
    await ScraperManager.registerDataBankScraper(new FandomScraper());
}
