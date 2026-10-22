import { appToolAliases } from './names.ts';

// Keep equal to tt_ports::tool_images::MAX_TOOL_IMAGE_EDGE so stored images preserve annotation coordinates.
const CAPTURE_MAX_EDGE = 1280;
const MASK_COLOR = '#808080';
const MARK_COLOR = '#FF3B30';
const LABEL_COLOR = '#FFFFFF';
const LABEL_FONT = '600 12px system-ui';
const LABEL_HEIGHT = 20;
const LABEL_PADDING = 4;
const MARK_WIDTH = 2;
const { snapshot: SNAPSHOT } = appToolAliases;

export type Rect = { x: number; y: number; width: number; height: number };
export type CaptureLayout = {
    viewport: { width: number; height: number };
    crop: Rect | null;
    cropClipped: boolean;
    masks: Rect[];
    marks: { label: string; rect: Rect }[];
    markPage: string | null;
    markTotal: number;
};

export async function renderCapture(image: ImageBitmap, layout: CaptureLayout): Promise<{ blob: Blob; width: number; height: number }> {
    const ratio = image.width / layout.viewport.width;
    const crop = layout.crop ?? { x: 0, y: 0, ...layout.viewport };
    const sourceX = Math.max(0, crop.x * ratio);
    const sourceY = Math.max(0, crop.y * ratio);
    const sourceWidth = Math.min(image.width, (crop.x + crop.width) * ratio) - sourceX;
    const sourceHeight = Math.min(image.height, (crop.y + crop.height) * ratio) - sourceY;
    const scale = Math.min(1, CAPTURE_MAX_EDGE / Math.max(sourceWidth, sourceHeight));
    const canvas = document.createElement('canvas');
    canvas.width = Math.round(sourceWidth * scale);
    canvas.height = Math.round(sourceHeight * scale);
    const context = canvas.getContext('2d');
    if (!context) throw new Error('A canvas 2D context is required to render the screenshot.');
    context.drawImage(image, sourceX, sourceY, sourceWidth, sourceHeight, 0, 0, canvas.width, canvas.height);
    const outputRect = (rect: Rect): Rect => ({
        x: (rect.x * ratio - sourceX) * canvas.width / sourceWidth,
        y: (rect.y * ratio - sourceY) * canvas.height / sourceHeight,
        width: rect.width * ratio * canvas.width / sourceWidth,
        height: rect.height * ratio * canvas.height / sourceHeight,
    });
    context.fillStyle = MASK_COLOR;
    for (const mask of layout.masks) {
        const rect = outputRect(mask);
        context.fillRect(rect.x, rect.y, rect.width, rect.height);
    }
    context.font = LABEL_FONT;
    context.textBaseline = 'middle';
    context.lineWidth = MARK_WIDTH;
    context.strokeStyle = MARK_COLOR;
    for (const mark of layout.marks) {
        const rect = outputRect(mark.rect);
        context.strokeRect(rect.x, rect.y, rect.width, rect.height);
        const labelWidth = context.measureText(mark.label).width + LABEL_PADDING * 2;
        const x = Math.max(0, Math.min(rect.x + MARK_WIDTH, canvas.width - labelWidth));
        const y = Math.max(0, Math.min(rect.y + MARK_WIDTH, canvas.height - LABEL_HEIGHT));
        context.fillStyle = MARK_COLOR;
        context.fillRect(x, y, labelWidth, LABEL_HEIGHT);
        context.fillStyle = LABEL_COLOR;
        context.fillText(mark.label, x + LABEL_PADDING, y + LABEL_HEIGHT / 2);
    }
    const blob = await new Promise<Blob>((resolve, reject) => {
        canvas.toBlob(blob => {
            if (blob) resolve(blob);
            else reject(new Error('The screenshot could not be encoded as JPEG.'));
        }, 'image/jpeg', 0.85);
    });
    return { blob, width: canvas.width, height: canvas.height };
}

export function captureSummary(layout: CaptureLayout, size: { width: number; height: number }): string {
    let summary = `Screenshot of the ${layout.crop ? 'element' : 'visible page'}, ${size.width}×${size.height}.`;
    if (layout.cropClipped) summary += ' Only the part inside the visible area is shown.';
    if (layout.masks.length > 0) summary += ' Gray boxes cover areas you cannot observe.';
    if (layout.markPage !== null) summary += ` Red boxes mark ${layout.marks.length} of the ${layout.markTotal} refs on your latest ${SNAPSHOT} page; each label is the part after "${layout.markPage}:".`;
    return summary;
}
