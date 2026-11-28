import { splitMessageBlocks } from '../../../tauri/message/message-blocks.js';

/** Renders text by its top-level blocks; a block equal to the one in the same place of the previous text keeps its HTML. */
export function createMarkdownRenderer(convert: (text: string) => string): (text: string) => string {
    let blocks: string[] = [];
    let html: string[] = [];
    return text => {
        const next = splitMessageBlocks(text, []);
        html = next.map((block, index) => block === blocks[index] ? html[index]! : convert(block));
        blocks = next;
        return html.join('');
    };
}
