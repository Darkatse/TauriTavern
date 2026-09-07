import { runScopeHandlers } from '@codemirror/view';
import { SearchQuery, getSearchQuery, setSearchQuery, findNext, findPrevious, replaceNext, replaceAll, closeSearchPanel } from '@codemirror/search';

export function createSearchPanel(view) {
    const dom = document.createElement('div');
    dom.className = 'cm-editor-search';
    dom.setAttribute('role', 'search');
    dom.setAttribute('aria-label', view.state.phrase('Find and replace'));
    let query = getSearchQuery(view.state);

    function button(label, text, action) {
        const button = document.createElement('button');
        button.type = 'button';
        button.title = view.state.phrase(label);
        button.setAttribute('aria-label', button.title);
        button.append(text);
        button.addEventListener('mousedown', event => event.preventDefault());
        button.addEventListener('click', action);
        return button;
    }

    function icon(path) {
        const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
        svg.setAttribute('viewBox', '0 0 16 16');
        svg.setAttribute('aria-hidden', 'true');
        svg.setAttribute('focusable', 'false');
        const shape = document.createElementNS(svg.namespaceURI, 'path');
        shape.setAttribute('d', path);
        svg.append(shape);
        return svg;
    }

    function field(name, label) {
        const input = document.createElement('input');
        input.type = 'text';
        input.name = name;
        input.placeholder = view.state.phrase(label);
        input.setAttribute('aria-label', input.placeholder);
        input.setAttribute('form', '');
        input.autocomplete = 'off';
        input.spellcheck = false;
        input.addEventListener('input', event => { if (!event.isComposing) commit(); });
        input.addEventListener('compositionend', commit);
        return input;
    }

    const searchField = field('search', 'Find');
    searchField.setAttribute('main-field', 'true');
    const replaceField = field('replace', 'Replace');
    const status = document.createElement('div');
    status.className = 'cm-search-status';
    status.setAttribute('role', 'status');

    function run(command) {
        if (!query.valid) return;
        status.textContent = command(view) ? '' : view.state.phrase('No matches');
    }

    const findBox = document.createElement('div');
    findBox.className = 'cm-search-field';
    findBox.append(searchField);
    const options = [
        ['caseSensitive', 'Case sensitive', 'Aa'],
        ['wholeWord', 'Whole word', 'ab'],
        ['regexp', 'Regular expression', '.*'],
    ].map(([key, label, text]) => {
        const toggle = button(label, text, () => {
            view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ ...query, [key]: !query[key] })) });
            searchField.focus();
        });
        toggle.className = `cm-search-option cm-search-${key}`;
        findBox.append(toggle);
        return [key, toggle];
    });
    const previous = button('Previous', icon('M8 13V3M3.5 7.5 8 3l4.5 4.5'), () => run(findPrevious));
    const next = button('Next', icon('M8 3v10M3.5 8.5 8 13l4.5-4.5'), () => run(findNext));
    const close = button('Close', icon('m4 4 8 8M12 4l-8 8'), () => closeSearchPanel(view));
    const replaceRow = document.createElement('div');
    replaceRow.className = 'cm-search-replace';
    replaceRow.hidden = true;
    const replace = button('Replace', view.state.phrase('Replace'), () => run(replaceNext));
    const all = button('Replace all', view.state.phrase('All'), () => run(replaceAll));
    replaceRow.append(replaceField, replace, all);
    const expand = button('Toggle replace', icon('m6 3.5 4.5 4.5L6 12.5'), () => {
        if (!replaceRow.hidden) searchField.focus();
        replaceRow.hidden = !replaceRow.hidden;
        expand.setAttribute('aria-expanded', String(!replaceRow.hidden));
        if (!replaceRow.hidden) replaceField.focus();
        view.requestMeasure();
    });
    expand.className = 'cm-search-expand';
    expand.setAttribute('aria-expanded', 'false');
    expand.hidden = view.state.readOnly;
    dom.append(expand, findBox, previous, next, close);
    if (!view.state.readOnly) dom.append(replaceRow);
    dom.append(status);

    function commit() {
        const next = new SearchQuery({ ...query, search: searchField.value, replace: replaceField.value });
        if (!next.eq(query)) view.dispatch({ effects: setSearchQuery.of(next) });
    }

    function sync() {
        query = getSearchQuery(view.state);
        searchField.value = query.search;
        replaceField.value = query.replace;
        for (const [key, toggle] of options) toggle.setAttribute('aria-pressed', String(query[key]));
        const invalid = Boolean(query.search && !query.valid);
        searchField.setAttribute('aria-invalid', String(invalid));
        status.textContent = invalid ? view.state.phrase('Invalid regular expression') : '';
        for (const action of [previous, next, replace, all]) action.disabled = !query.valid;
    }
    sync();
    dom.addEventListener('keydown', event => {
        if (event.isComposing) return;
        if (runScopeHandlers(view, event, 'search-panel')) event.preventDefault();
        else if (event.key === 'Enter' && (event.target === searchField || event.target === replaceField)) {
            event.preventDefault();
            run(event.target === replaceField ? replaceNext : event.shiftKey ? findPrevious : findNext);
        }
    });
    return {
        dom, top: true,
        mount() { searchField.focus(); searchField.select(); },
        update(update) {
            if (!getSearchQuery(update.state).eq(query)) sync();
            else if (update.docChanged && query.valid) status.textContent = '';
        },
    };
}
