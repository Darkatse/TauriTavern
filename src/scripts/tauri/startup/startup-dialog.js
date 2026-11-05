const STYLE = `
    dialog {
        box-sizing: border-box;
        width: min(92vw, 560px);
        padding: 20px;
        border: 1px solid GrayText;
        border-radius: 12px;
        color-scheme: light dark;
        background: Canvas;
        color: CanvasText;
        font: 14px/1.5 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
    }
    dialog::backdrop { background: rgb(0 0 0 / 0.55); }
    h2 { margin: 0 0 8px; font-size: 18px; }
    p { margin: 0 0 12px; white-space: pre-line; overflow-wrap: anywhere; }
    pre {
        max-height: 40vh;
        margin: 0 0 16px;
        padding: 8px;
        overflow: auto;
        border-radius: 6px;
        background: rgb(127 127 127 / 0.15);
        font: 12px/1.4 ui-monospace, Menlo, Consolas, monospace;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        -webkit-user-select: text;
        user-select: text;
    }
    .actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }
    button { font: inherit; padding: 6px 14px; }
    button:last-child, #error { font-weight: 600; }
`;

/**
 * @typedef {{ name: string, label: string, run: (button: HTMLButtonElement) => unknown }} StartupDialogAction
 * @typedef {{ title: string, description: string, error?: string, details?: string, actions: StartupDialogAction[] }} StartupDialogContent
 * @typedef {{ show: (content: StartupDialogContent) => HTMLButtonElement[], remove: () => void }} StartupDialog
 */

/**
 * A modal dialog for startup that stays open until removed, since nothing usable remains behind
 * it. The last action is the default and holds focus; while an action runs, no other can start.
 * @param {string} id
 * @returns {StartupDialog}
 */
export function openStartupDialog(id) {
    const host = document.createElement('div');
    host.id = id;
    // Theme styles, inherited ones included, must not reach the dialog; keep this reset inline.
    host.style.all = 'initial';
    const root = host.attachShadow({ mode: 'open' });
    root.innerHTML = `<style>${STYLE}</style>
        <dialog role="alertdialog" tabindex="-1" aria-labelledby="title" aria-describedby="description">
            <h2 id="title"></h2>
            <p id="description"></p>
            <p id="error" role="alert" hidden></p>
            <pre hidden></pre>
            <div class="actions"></div>
        </dialog>`;
    const dialog = root.querySelector('dialog');
    const title = root.getElementById('title');
    const description = root.getElementById('description');
    const error = root.getElementById('error');
    const details = root.querySelector('pre');
    const actions = root.querySelector('.actions');
    const focusDefault = () => (actions.lastElementChild ?? dialog).focus();
    const reopening = new AbortController();
    dialog.addEventListener('close', () => {
        dialog.showModal();
        focusDefault();
    }, { signal: reopening.signal });
    document.body.append(host);

    return {
        show(content) {
            title.textContent = content.title;
            description.textContent = content.description;
            error.textContent = content.error ?? '';
            error.hidden = !content.error;
            details.textContent = content.details ?? '';
            details.hidden = !content.details;
            let running = false;
            const buttons = content.actions.map(({ name, label, run }) => {
                const button = document.createElement('button');
                button.type = 'button';
                button.name = name;
                button.textContent = label;
                button.addEventListener('click', async () => {
                    if (running) return;
                    running = true;
                    try {
                        await run(button);
                    } finally {
                        running = false;
                    }
                });
                return button;
            });
            actions.replaceChildren(...buttons);
            if (!dialog.open) dialog.showModal();
            focusDefault();
            return buttons;
        },
        remove() {
            reopening.abort();
            host.remove();
        },
    };
}
