import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { translateAgentSystem as tr } from './i18n.js';

function createSkillFileViewerRoot({ file, requestClose }) {
    return {
        data() {
            return {
                file,
            };
        },
        computed: {
            rangeLabel() {
                return tr(this.file.truncated ? 'charRangeTruncated' : 'charRangeComplete', {
                    chars: this.file.chars,
                    totalChars: this.file.totalChars,
                });
            },
        },
        methods: {
            closeViewer() {
                requestClose();
            },
            tr(key, params) {
                return tr(key, params);
            },
        },
        template: `
            <div class="ttas-root ttas-file-viewer">
                <header class="ttas-titlebar ttas-file-viewer-titlebar">
                    <div>
                        <div class="ttas-eyebrow">{{ file.name }}</div>
                        <h3>{{ file.path }}</h3>
                    </div>
                    <div class="ttas-file-viewer-actions">
                        <span>{{ rangeLabel }}</span>
                        <button type="button" class="menu_button menu_button_icon ttas-close-button" :title="tr('close')" @click="closeViewer">
                            <i class="fa-solid fa-xmark"></i>
                        </button>
                    </div>
                </header>
                <pre class="ttas-file-content">{{ file.content }}</pre>
            </div>
        `,
    };
}

export function openSkillFileViewer(file) {
    if (typeof HTMLDialogElement === 'undefined') {
        throw new Error(tr('skillFileViewerElementUnsupported'));
    }

    const dialog = document.createElement('dialog');
    if (typeof dialog.showModal !== 'function') {
        throw new Error(tr('skillFileViewerDialogUnsupported'));
    }

    dialog.className = 'ttas-file-dialog';
    dialog.setAttribute('data-tt-mobile-surface', 'fullscreen-window');
    const mount = document.createElement('div');
    mount.className = 'ttas-file-viewer-mount';
    dialog.appendChild(mount);
    document.body.appendChild(dialog);

    let app = null;
    const requestClose = () => {
        dialog.close();
    };
    const cleanup = () => {
        app?.unmount();
        dialog.remove();
    };

    dialog.addEventListener('close', cleanup, { once: true });
    dialog.addEventListener('cancel', (event) => {
        event.preventDefault();
        dialog.close();
    });

    app = createApp(createSkillFileViewerRoot({ file, requestClose }));
    app.mount(mount);

    try {
        dialog.showModal();
    } catch (error) {
        cleanup();
        throw error;
    }
}
