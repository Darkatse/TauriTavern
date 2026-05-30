import { translateAgentSystem as tr } from '../i18n.js';

export const SkillFileTreeNode = {
    name: 'SkillFileTreeNode',
    props: {
        depth: {
            type: Number,
            required: true,
        },
        isFolderOpen: {
            type: Function,
            required: true,
        },
        node: {
            type: Object,
            required: true,
        },
    },
    emits: ['toggle-folder', 'open-file'],
    methods: {
        rowPadding(depth) {
            return `${8 + depth * 16}px`;
        },
        tr(key, params) {
            return tr(key, params);
        },
        fileKindLabel(kind) {
            return tr(kind === 'binary' ? 'skillFileKindBinary' : 'skillFileKindText');
        },
    },
    template: `
        <li class="ttas-file-tree-item" :class="'ttas-file-tree-' + node.type">
            <button
                v-if="node.type === 'folder'"
                type="button"
                class="ttas-file-row"
                :style="{ paddingLeft: rowPadding(depth) }"
                :aria-expanded="isFolderOpen(node)"
                @click="$emit('toggle-folder', node)"
            >
                <i class="fa-solid" :class="isFolderOpen(node) ? 'fa-folder-open' : 'fa-folder'"></i>
                <span>{{ node.name }}</span>
                <small>{{ node.children.length }}</small>
            </button>
            <button
                v-else
                type="button"
                class="ttas-file-row"
                :style="{ paddingLeft: rowPadding(depth) }"
                @click="$emit('open-file', node)"
            >
                <i class="fa-solid" :class="node.file.kind === 'binary' ? 'fa-file' : 'fa-file-lines'"></i>
                <span>{{ node.name }}</span>
                <small>{{ fileKindLabel(node.file.kind) }}</small>
            </button>
            <ul v-if="node.type === 'folder' && isFolderOpen(node)" class="ttas-file-tree">
                <SkillFileTreeNode
                    v-for="child in node.children"
                    :key="child.path"
                    :node="child"
                    :depth="depth + 1"
                    :is-folder-open="isFolderOpen"
                    @toggle-folder="$emit('toggle-folder', $event)"
                    @open-file="$emit('open-file', $event)"
                />
            </ul>
        </li>
    `,
};
