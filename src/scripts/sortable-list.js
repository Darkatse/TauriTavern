import { setAriaRelation } from './dom-handlers.js';
import { t } from './i18n.js';
import { getSortableDelay } from './utils.js';

/** DOM order is the view's fact; the feature owns labels, serialization and persistence. */
export function initSortableList(list, onReorder, getLabel = item => item.querySelector(':scope > span')) {
    list.classList.add('tt-sortable-list');
    list.setAttribute('role', 'list');
    const status = document.createElement('span');
    status.className = 'sr-only';
    status.setAttribute('role', 'status');
    list.after(status);

    function announce(item) {
        const name = getLabel(item).textContent;
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
    /** Append and return the move buttons; callers may reposition them within the item. */
    function addItem(item) {
        item.setAttribute('role', 'listitem');
        const label = getLabel(item);
        const buttons = [];
        for (const direction of ['up', 'down']) {
            const button = document.createElement('button');
            const text = direction === 'up' ? 'Move up' : 'Move down';
            button.type = 'button';
            button.className = `right_menu_button fa-solid fa-chevron-${direction} tt-sortable-list-${direction}`;
            button.title = text;
            button.dataset.i18n = `[title]${text}`;
            setAriaRelation(button, 'labelledby', button, label);
            button.addEventListener('click', () => {
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
            buttons.push(button);
        }
        return buttons;
    }
    for (const item of list.children) addItem(item);
    return { addItem };
}

export function setSortableListEnabled(list, enabled) {
    $(list).sortable(enabled ? 'enable' : 'disable');
    for (const button of list.querySelectorAll('.tt-sortable-list-up, .tt-sortable-list-down')) button.disabled = !enabled;
}
