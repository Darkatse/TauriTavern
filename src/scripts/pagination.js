import { captureFocus, keepFocus } from './dom-handlers.js';
import { t } from './i18n.js';
import { makeKeyboardInteractable } from './legacy-controls.js';
import { accountStorage } from './util/AccountStorage.js';

/** @param {{ storageKey: string, defaultPageSize: number }} options */
function getPageSize({ storageKey, defaultPageSize }) {
    return Number(accountStorage.getItem(storageKey)) || defaultPageSize;
}

/** @param {number} index @param {{ storageKey: string, defaultPageSize: number }} options */
export function getPageForItem(index, options) {
    return Math.floor(index / getPageSize(options)) + 1;
}

/** Localize and initialize the plugin's newly rendered native and legacy controls. */
function preparePagination(container) {
    for (const [kind, label] of [['prev', t`Previous page`], ['next', t`Next page`], ['first', t`First page`], ['last', t`Last page`]]) {
        container.find(`.paginationjs-${kind}:not(.paginationjs-page)`).attr('title', label).find('a').attr('aria-label', label);
    }
    container.find('.paginationjs-page.active a').attr('aria-current', 'page');
    const controls = container.find('.paginationjs-pages li:not(.paginationjs-ellipsis) a');
    controls.filter('li.disabled a').attr('aria-disabled', 'true');
    const select = container.find('.paginationjs-size-changer select').attr('aria-label', t`Items per page`);
    select.find('option').each((_, option) => { option.textContent = `${option.value} ${t`/ page`}`; });
    makeKeyboardInteractable(...controls.toArray());
}

/**
 * Own shared pagination configuration, remembered page size, page retention and navigation focus.
 * Callers render their items and explicitly request a page when changing the data set.
 * @param {JQuery<HTMLElement>} container
 * @param {object} options
 * @param {string} options.storageKey
 * @param {number} options.defaultPageSize
 * @param {number[]} [options.sizeChangerOptions]
 * @param {any} options.dataSource
 * @param {number} [options.pageNumber]
 * @param {boolean} [options.showPageNumbers]
 * @param {boolean} [options.showSizeChanger]
 * @param {Function} options.callback
 * @param {Function} [options.beforeRender]
 * @param {Function} [options.afterRender]
 * @param {Function} [options.afterPaging]
 */
export function initPagination(container, { storageKey, defaultPageSize, beforeRender, afterRender, ...options }) {
    const pageNumber = options.pageNumber ?? (container.data('pagination')?.initialized ? container.pagination('getSelectedPageNum') : 1);
    const getItems = () => container.find('.paginationjs-pages li, .paginationjs-size-changer').toArray();
    const keyOf = item => item.classList.contains('paginationjs-page') ? `page-${item.dataset.num}` : item.classList[0];
    let restore;
    // Reinitialization removes the old navigation before any render hook runs.
    keepFocus(getItems, keyOf, () => container.pagination({
        position: 'top',
        pageRange: 1,
        showPageNumbers: false,
        showSizeChanger: true,
        showNavigator: true,
        prevText: '<',
        nextText: '>',
        formatNavigator: '<%= rangeStart %>-<%= rangeEnd %> .. <%= totalNumber %>',
        pageSize: getPageSize({ storageKey, defaultPageSize }),
        ...options,
        pageNumber,
        beforeRender(...args) {
            restore = captureFocus(getItems, keyOf);
            return beforeRender?.(...args);
        },
        afterRender(...args) {
            preparePagination(container);
            afterRender?.(...args);
            restore();
        },
        afterSizeSelectorChange(_event, size) {
            accountStorage.setItem(storageKey, String(size));
        },
    }));
}
