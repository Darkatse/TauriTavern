// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';
import { t } from '../../../../scripts/i18n.js';

/** @type {boolean} */
let installed = false;

/**
 * @returns {JQueryStatic}
 */
function mustGetJQuery() {
    const root = /** @type {any} */ (globalThis);
    const jq = root.jQuery ?? root.$;
    if (typeof jq !== 'function') {
        throw new Error('WorldInfoGlobalSelectorSelect2Enforcer: jQuery not found');
    }
    return /** @type {JQueryStatic} */ (jq);
}

/**
 * @returns {HTMLSelectElement}
 */
function mustGetWorldInfoGlobalSelect() {
    const el = document.getElementById('world_info');
    if (!(el instanceof HTMLSelectElement)) {
        throw new Error('WorldInfoGlobalSelectorSelect2Enforcer: #world_info <select> not found');
    }
    return el;
}

function enforceWorldInfoGlobalSelect2() {
    const $ = mustGetJQuery();
    const select = mustGetWorldInfoGlobalSelect();
    const control = $(select);

    if (!select.multiple) {
        throw new Error('WorldInfoGlobalSelectorSelect2Enforcer: #world_info is expected to be <select multiple>');
    }

    if (typeof $.fn.select2 !== 'function') {
        throw new Error('WorldInfoGlobalSelectorSelect2Enforcer: select2 plugin not found');
    }

    if (control.data('select2')) {
        return;
    }

    control.select2({
        width: '100%',
        placeholder: t`No Worlds active. Click here to select.`,
        allowClear: true,
        closeOnSelect: false,
    });
}

export function installWorldInfoGlobalSelectorSelect2Enforcer() {
    if (installed) {
        return;
    }
    installed = true;

    eventSource.on(event_types.APP_READY, enforceWorldInfoGlobalSelect2);
}
