import { t } from '../../i18n.js';
import { ensureCompactRowsStyle } from '../../tauri/compact-rows/overflow-menu.js';

/**
 * A second view of the connection selector at the top of the AI Response
 * Configuration panel, as one compact row above the preset row. The API panel's
 * `#connection_profiles` stays the only owner: this mirror copies its options and
 * forwards a choice to it, so applying, events and slash commands keep one code path.
 * Managing models stays in the API panel; only "Reapply" is offered here, while the
 * live settings differ from the selected item.
 * @param {HTMLSelectElement} source The Connection Manager selector
 * @param {object} options
 * @param {() => void} options.onReapply Reapplies the selected item
 * @returns {{ sync: () => void, setDirty: (dirty: boolean) => void }} `sync` copies the
 *   source's options and selection into the mirror; `setDirty` shows or hides "Reapply"
 */
export function installSidebarSelect(source, { onReapply }) {
    const container = document.getElementById('respective-presets-block');
    if (!container) {
        throw new Error('Connection Manager: #respective-presets-block not found for the sidebar selector');
    }
    ensureCompactRowsStyle();

    const row = document.createElement('div');
    row.className = 'tt-sel-row tt-cm-sidebar';
    const label = document.createElement('label');
    label.className = 'tt-sel-label';
    label.htmlFor = 'tt_sidebar_connection_profiles';
    label.textContent = t`Model`;
    const mirror = document.createElement('select');
    mirror.className = 'text_pole';
    mirror.id = 'tt_sidebar_connection_profiles';
    const spinner = document.createElement('i');
    spinner.id = 'tt_sidebar_connection_spinner';
    spinner.className = 'fa-solid fa-spinner fa-spin hidden';
    const reapply = document.createElement('button');
    reapply.type = 'button';
    reapply.className = 'menu_button menu_button_icon tt-row-button tt-cm-dirty tt-hidden';
    reapply.title = reapply.ariaLabel = t`Reapply`;
    reapply.innerHTML = '<i class="fa-fw fa-solid fa-recycle" aria-hidden="true"></i>';
    row.append(label, mirror, spinner, reapply);
    container.prepend(row);

    // Long names are cut off in the compact row.
    const showSelectedName = () => {
        mirror.title = mirror.selectedOptions[0]?.textContent ?? '';
    };

    mirror.addEventListener('change', () => {
        showSelectedName();
        if (source.value === mirror.value) return;
        source.value = mirror.value;
        source.dispatchEvent(new Event('change'));
    });
    reapply.addEventListener('click', onReapply);

    return {
        sync() {
            const options = [...source.children].map(node => node.cloneNode(true));
            // Only "(Current API settings)": say where models come from.
            if (!source.querySelector('optgroup')) {
                const guidance = document.createElement('option');
                guidance.disabled = true;
                guidance.textContent = t`Save a model in the API Connections panel`;
                options.push(guidance);
            }
            mirror.replaceChildren(...options);
            mirror.value = source.value;
            showSelectedName();
        },
        setDirty(dirty) {
            reapply.classList.toggle('tt-hidden', !dirty);
        },
    };
}
