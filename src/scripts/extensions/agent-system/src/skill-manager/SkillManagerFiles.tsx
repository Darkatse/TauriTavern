import { useState } from 'react';

import type { SkillManagerTr } from './SkillManagerContract';

export type SkillFileTreeNode =
    | { type: 'folder'; name: string; path: string; children: SkillFileTreeNode[] }
    | { type: 'file'; name: string; path: string; file: TauriTavernSkillFileRef };

function skillFilePath(value: string, tr: SkillManagerTr): string {
    const path = value.trim();
    if (!path) throw new Error(tr('skillFilePathRequired'));
    if (path.startsWith('/') || path.includes('\\') || path.split('/').some(segment => !segment || segment === '.' || segment === '..')) {
        throw new Error(tr('invalidSkillFilePath', { path }));
    }
    return path;
}

function compareNodes(left: SkillFileTreeNode, right: SkillFileTreeNode): number {
    if (left.type !== right.type) return left.type === 'folder' ? -1 : 1;
    return left.name.localeCompare(right.name);
}

function sortTree(nodes: SkillFileTreeNode[]): SkillFileTreeNode[] {
    nodes.sort(compareNodes);
    nodes.forEach(node => {
        if (node.type === 'folder') sortTree(node.children);
    });
    return nodes;
}

export function buildSkillFileTree(
    files: readonly TauriTavernSkillFileRef[],
    tr: SkillManagerTr,
): SkillFileTreeNode[] {
    const root: SkillFileTreeNode[] = [];
    const folders = new Map<string, Extract<SkillFileTreeNode, { type: 'folder' }>>();
    for (const file of files) {
        const path = skillFilePath(file.path, tr);
        const segments = path.split('/');
        let parentPath = '';
        let siblings = root;
        for (const segment of segments.slice(0, -1)) {
            const folderPath = parentPath ? `${parentPath}/${segment}` : segment;
            let folder = folders.get(folderPath);
            if (!folder) {
                folder = { type: 'folder', name: segment, path: folderPath, children: [] };
                folders.set(folderPath, folder);
                siblings.push(folder);
            }
            parentPath = folderPath;
            siblings = folder.children;
        }
        siblings.push({ type: 'file', name: path.slice(path.lastIndexOf('/') + 1), path, file });
    }
    return sortTree(root);
}

type SkillFileTreeProps = {
    nodes: readonly SkillFileTreeNode[];
    isFolderOpen: (path: string) => boolean;
    onToggleFolder: (path: string) => void;
    onOpenFile: (file: TauriTavernSkillFileRef) => void;
    tr: SkillManagerTr;
};

function FileNode(props: SkillFileTreeProps & { node: SkillFileTreeNode; depth: number }) {
    const { node, depth, isFolderOpen, onToggleFolder, onOpenFile, tr } = props;
    const paddingLeft = `${8 + depth * 16}px`;
    if (node.type === 'file') {
        return (
            <li className="ttas-file-tree-item ttas-file-tree-file">
                <button type="button" className="ttas-file-row" style={{ paddingLeft }} onClick={() => onOpenFile(node.file)}>
                    <i className={`fa-solid ${node.file.kind === 'binary' ? 'fa-file' : 'fa-file-lines'}`}></i>
                    <span>{node.name}</span>
                    <small>{tr(node.file.kind === 'binary' ? 'skillFileKindBinary' : 'skillFileKindText')}</small>
                </button>
            </li>
        );
    }
    const open = isFolderOpen(node.path);
    return (
        <li className="ttas-file-tree-item ttas-file-tree-folder">
            <button
                type="button"
                className="ttas-file-row"
                style={{ paddingLeft }}
                aria-expanded={open}
                onClick={() => onToggleFolder(node.path)}
            >
                <i className={`fa-solid ${open ? 'fa-folder-open' : 'fa-folder'}`}></i>
                <span>{node.name}</span>
                <small>{node.children.length}</small>
            </button>
            {open && (
                <ul className="ttas-file-tree">
                    {node.children.map(child => (
                        <FileNode key={child.path} {...props} node={child} depth={depth + 1} />
                    ))}
                </ul>
            )}
        </li>
    );
}

export function SkillFileTree(props: SkillFileTreeProps) {
    return (
        <ul className="ttas-file-tree ttas-file-tree-root">
            {props.nodes.map(node => <FileNode key={node.path} {...props} node={node} depth={0} />)}
        </ul>
    );
}

export function SkillFileViewer(props: {
    file: TauriTavernSkillReadResult;
    onSave: (content: string) => Promise<TauriTavernSkillReadResult>;
    onClose: () => void;
    tr: SkillManagerTr;
}) {
    const { file, onSave, onClose, tr } = props;
    const [editing, setEditing] = useState(false);
    const [draft, setDraft] = useState(file.content);
    const [saving, setSaving] = useState(false);
    const rangeLabel = tr(file.truncated ? 'charRangeTruncated' : 'charRangeComplete', {
        chars: file.chars,
        totalChars: file.totalChars,
    });

    const save = async () => {
        setSaving(true);
        try {
            const saved = await onSave(draft);
            setDraft(saved.content);
            setEditing(false);
        } finally {
            setSaving(false);
        }
    };

    return (
        <div className="ttas-root ttas-file-viewer">
            <header className="ttas-titlebar ttas-file-viewer-titlebar">
                <div>
                    <div className="ttas-eyebrow">{file.name}</div>
                    <h3>{file.path}</h3>
                </div>
                <div className="ttas-file-viewer-actions">
                    <span>{rangeLabel}</span>
                    {!file.truncated && !editing && (
                        <button type="button" className="menu_button menu_button_icon" title={tr('edit')} onClick={() => { setDraft(file.content); setEditing(true); }}>
                            <i className="fa-solid fa-pen-to-square"></i><span>{tr('edit')}</span>
                        </button>
                    )}
                    {editing && (
                        <>
                            <button type="button" className="menu_button menu_button_icon ttas-primary-button" disabled={saving || draft === file.content} title={tr('save')} onClick={() => void save()}>
                                <i className="fa-solid fa-floppy-disk"></i><span>{tr('save')}</span>
                            </button>
                            <button type="button" className="menu_button menu_button_icon" disabled={saving} title={tr('cancel')} onClick={() => { setDraft(file.content); setEditing(false); }}>
                                <i className="fa-solid fa-rotate-left"></i><span>{tr('cancel')}</span>
                            </button>
                        </>
                    )}
                    <button type="button" className="menu_button menu_button_icon ttas-close-button" title={tr('close')} aria-label={tr('close')} onClick={onClose}>
                        <i className="fa-solid fa-xmark"></i>
                    </button>
                </div>
            </header>
            {editing ? (
                <textarea className="text_pole textarea_compact ttas-file-content ttas-file-editor" spellCheck={false} value={draft} onChange={event => setDraft(event.target.value)} />
            ) : (
                <pre className="ttas-file-content">{file.content}</pre>
            )}
        </div>
    );
}
