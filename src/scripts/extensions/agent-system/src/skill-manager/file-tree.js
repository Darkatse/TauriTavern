import { translateAgentSystem as tr } from '../i18n.js';

function skillFilePath(value) {
    const path = String(value || '').trim();
    if (!path) {
        throw new Error(tr('skillFilePathRequired'));
    }
    if (
        path.startsWith('/')
        || path.includes('\\')
        || path.split('/').some((segment) => segment === '' || segment === '.' || segment === '..')
    ) {
        throw new Error(tr('invalidSkillFilePath', { path }));
    }

    return path;
}

function compareNodes(left, right) {
    if (left.type !== right.type) {
        return left.type === 'folder' ? -1 : 1;
    }

    return left.name.localeCompare(right.name);
}

function sortTree(nodes) {
    nodes.sort(compareNodes);
    for (const node of nodes) {
        if (node.type === 'folder') {
            sortTree(node.children);
        }
    }
    return nodes;
}

export function buildSkillFileTree(files) {
    if (!Array.isArray(files)) {
        throw new Error(tr('skillFilesMustBeArray'));
    }

    const root = [];
    const folders = new Map();

    for (const file of files) {
        const path = skillFilePath(file?.path);
        const segments = path.split('/');
        let parentPath = '';
        let siblings = root;

        segments.slice(0, -1).forEach((segment) => {
            const folderPath = parentPath ? `${parentPath}/${segment}` : segment;
            let folder = folders.get(folderPath);
            if (!folder) {
                folder = {
                    type: 'folder',
                    name: segment,
                    path: folderPath,
                    children: [],
                };
                folders.set(folderPath, folder);
                siblings.push(folder);
            }
            parentPath = folderPath;
            siblings = folder.children;
        });

        siblings.push({
            type: 'file',
            name: segments[segments.length - 1],
            path,
            file,
        });
    }

    return sortTree(root);
}
