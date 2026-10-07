import { t } from './i18n.js';
import { getSortableDelay } from './utils.js';

/** DOM order is the view's fact; the feature owns serialization and persistence. */
export function initSortableList(list, onReorder) {
    list.classList.add('tt-sortable-list');
    list.setAttribute('role', 'list');
    const status = document.createElement('span');
    status.className = 'sr-only';
    status.setAttribute('role', 'status');
    list.after(status);

    function announce(item) {
        const name = item.querySelector(':scope > span').textContent;
        const position = Array.from(list.children).indexOf(item) + 1;
        status.textContent = t`${name}: item ${position} of ${list.children.length}`;
    }

    $(list).sortable({
        delay: getSortableDelay(),
        stop(_event, ui) {
            onReorder();
            announce(ui.item[0]);
        },
    });
    for (const item of list.children) {
        item.setAttribute('role', 'listitem');
        const label = item.querySelector(':scope > span');
        label.id ||= `${list.id}_${item.dataset.id ?? item.dataset.name}_label`;
        for (const direction of ['up', 'down']) {
            const button = document.createElement('button');
            const text = direction === 'up' ? 'Move up' : 'Move down';
            button.type = 'button';
            button.id = `${label.id}-${direction}`;
            button.className = `right_menu_button fa-solid fa-chevron-${direction} tt-sortable-list-${direction}`;
            button.title = text;
            button.dataset.i18n = `[title]${text}`;
            button.setAttribute('aria-labelledby', `${button.id} ${label.id}`);
            button.addEventListener('click', () => {
                if (button.disabled) return;
                const sibling = direction === 'up' ? item.previousElementSibling : item.nextElementSibling;
                if (!sibling) {
                    status.textContent = direction === 'up' ? t`Already first` : t`Already last`;
                    return;
                }
                if (direction === 'up') sibling.before(item);
                else sibling.after(item);
                button.focus();
                onReorder();
                announce(item);
            });
            item.append(button);
        }
    }
}

export function setSortableListEnabled(list, enabled) {
    $(list).sortable(enabled ? 'enable' : 'disable');
    for (const button of list.querySelectorAll('button')) button.disabled = !enabled;
}
