import { translateAgentSystem as tr } from '../i18n.js';

export const SkillFileViewer = {
    name: 'SkillFileViewer',
    props: {
        file: {
            type: Object,
            required: true,
        },
    },
    emits: ['close'],
    data() {
        return {
            currentFile: this.file,
            editing: false,
            draftContent: this.file.content,
            saving: false,
        };
    },
    computed: {
        rangeLabel() {
            return tr(this.currentFile.truncated ? 'charRangeTruncated' : 'charRangeComplete', {
                chars: this.currentFile.chars,
                totalChars: this.currentFile.totalChars,
            });
        },
        canEdit() {
            return typeof this.currentFile.onSave === 'function' && !this.currentFile.truncated;
        },
        isDirty() {
            return this.draftContent !== this.currentFile.content;
        },
    },
    methods: {
        closeViewer() {
            this.$emit('close');
        },
        startEdit() {
            if (!this.canEdit) {
                throw new Error(tr(this.currentFile.truncated ? 'cannotEditTruncatedSkillFile' : 'hostSkillWriteApiUnavailable'));
            }
            this.draftContent = this.currentFile.content;
            this.editing = true;
        },
        cancelEdit() {
            this.draftContent = this.currentFile.content;
            this.editing = false;
        },
        async saveEdit() {
            if (!this.canEdit) {
                throw new Error(tr(this.currentFile.truncated ? 'cannotEditTruncatedSkillFile' : 'hostSkillWriteApiUnavailable'));
            }
            this.saving = true;
            try {
                const saved = await this.currentFile.onSave({
                    ...this.currentFile,
                    content: this.draftContent,
                });
                this.currentFile = {
                    ...this.currentFile,
                    ...(saved || {}),
                    content: saved?.content ?? this.draftContent,
                    chars: saved?.chars ?? this.draftContent.length,
                    totalChars: saved?.totalChars ?? this.draftContent.length,
                    truncated: saved?.truncated ?? false,
                };
                this.draftContent = this.currentFile.content;
                this.editing = false;
                window.toastr?.success?.(tr('savedSkillFile', { path: this.currentFile.path }));
            } catch (error) {
                console.error('[AgentSystem:SkillFileViewer]', error);
                window.toastr?.error?.(String(error?.message || error || tr('unknownError')));
                throw error;
            } finally {
                this.saving = false;
            }
        },
        tr(key, params) {
            return tr(key, params);
        },
    },
    template: `
        <div class="ttas-root ttas-file-viewer">
            <header class="ttas-titlebar ttas-file-viewer-titlebar">
                <div>
                    <div class="ttas-eyebrow">{{ currentFile.name }}</div>
                    <h3>{{ currentFile.path }}</h3>
                </div>
                <div class="ttas-file-viewer-actions">
                    <span>{{ rangeLabel }}</span>
                    <button v-if="canEdit && !editing" type="button" class="menu_button menu_button_icon" :title="tr('edit')" @click="startEdit">
                        <i class="fa-solid fa-pen-to-square"></i>
                        <span>{{ tr('edit') }}</span>
                    </button>
                    <button v-if="editing" type="button" class="menu_button menu_button_icon ttas-primary-button" :disabled="saving || !isDirty" :title="tr('save')" @click="saveEdit">
                        <i class="fa-solid fa-floppy-disk"></i>
                        <span>{{ tr('save') }}</span>
                    </button>
                    <button v-if="editing" type="button" class="menu_button menu_button_icon" :disabled="saving" :title="tr('cancel')" @click="cancelEdit">
                        <i class="fa-solid fa-rotate-left"></i>
                        <span>{{ tr('cancel') }}</span>
                    </button>
                    <button type="button" class="menu_button menu_button_icon ttas-close-button" :title="tr('close')" :aria-label="tr('close')" @click="closeViewer">
                        <i class="fa-solid fa-xmark"></i>
                    </button>
                </div>
            </header>
            <textarea
                v-if="editing"
                v-model="draftContent"
                class="text_pole textarea_compact ttas-file-content ttas-file-editor"
                :spellcheck="false"
            ></textarea>
            <pre v-else class="ttas-file-content">{{ currentFile.content }}</pre>
        </div>
    `,
};
