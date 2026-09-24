import { documentWindow } from './document';
import type { DocumentScope } from './document';
import type { Semantics } from './semantics';

export type InteractionPoint = { x: number; y: number };
type Bounds = { left: number; right: number; top: number; bottom: number };
type FrameMapping = { frame: HTMLIFrameElement; x: number; y: number; scaleX: number; scaleY: number };

/** Find one point that hits the target inside its document and every host frame above it. */
export function inputPoint(element: HTMLElement, scope: DocumentScope, semantics: Semantics): InteractionPoint | null {
    if (!semantics.isVisible(element) || scope.frames.some(frame => !semantics.isVisible(frame))) return null;
    const mappings = [...scope.frames].reverse().map(frameMapping);

    // ponytail: one point per client rect; add richer geometry only for observed partial-overlay failures.
    for (const rect of Array.from(element.getClientRects())) {
        let visible = clipToViewport(rect, scope.document);
        for (const mapping of mappings) {
            if (!visible) break;
            visible = clipToViewport({
                left: mapping.x + visible.left * mapping.scaleX,
                right: mapping.x + visible.right * mapping.scaleX,
                top: mapping.y + visible.top * mapping.scaleY,
                bottom: mapping.y + visible.bottom * mapping.scaleY,
            }, mapping.frame.ownerDocument);
        }
        if (!visible) continue;

        const point = { x: (visible.left + visible.right) / 2, y: (visible.top + visible.bottom) / 2 };
        const local = descendToTarget(point, mappings);
        if (!local) continue;
        const hit = scope.document.elementFromPoint(local.x, local.y);
        if (hit && element.contains(hit)) return point;
    }
    return null;
}

function clipToViewport(bounds: Bounds, doc: Document): Bounds | null {
    const view = documentWindow(doc);
    const left = Math.max(0, bounds.left);
    const right = Math.min(view.innerWidth, bounds.right);
    const top = Math.max(0, bounds.top);
    const bottom = Math.min(view.innerHeight, bounds.bottom);
    return right > left && bottom > top ? { left, right, top, bottom } : null;
}

function descendToTarget(point: InteractionPoint, mappings: FrameMapping[]): InteractionPoint | null {
    let local = point;
    for (const mapping of [...mappings].reverse()) {
        const hit = mapping.frame.ownerDocument.elementFromPoint(local.x, local.y);
        if (hit !== mapping.frame) return null;
        local = { x: (local.x - mapping.x) / mapping.scaleX, y: (local.y - mapping.y) / mapping.scaleY };
    }
    return local;
}

function frameMapping(frame: HTMLIFrameElement): FrameMapping {
    const view = documentWindow(frame.ownerDocument);
    // Bounding rectangles provide a reliable mapping for translation and positive axis-aligned scale.
    // Reject other transforms instead of using an approximate point to bypass an outer obstruction.
    for (let ancestor: Element | null = frame; ancestor; ancestor = ancestor.parentElement) {
        const style = view.getComputedStyle(ancestor);
        const matrix = new view.DOMMatrixReadOnly(style.transform === 'none' ? undefined : style.transform);
        const rotated = style.rotate && style.rotate !== 'none' && Number.parseFloat(style.rotate) !== 0;
        const flipped = style.scale && style.scale !== 'none' && style.scale.split(/\s+/).some(value => Number(value) <= 0);
        const perspective = style.perspective && style.perspective !== 'none';
        if (!matrix.is2D || matrix.b !== 0 || matrix.c !== 0 || matrix.a <= 0 || matrix.d <= 0 || rotated || flipped || perspective) {
            throw new Error('No action was performed: this embedded page has a rotation, skew, or 3D transform that app.interact cannot map reliably. Use app.evaluate with a known API for this control.');
        }
    }

    const rect = frame.getBoundingClientRect();
    const style = view.getComputedStyle(frame);
    const scaleX = rect.width / frame.offsetWidth;
    const scaleY = rect.height / frame.offsetHeight;
    return {
        frame, scaleX, scaleY,
        x: rect.left + (frame.clientLeft + Number.parseFloat(style.paddingLeft)) * scaleX,
        y: rect.top + (frame.clientTop + Number.parseFloat(style.paddingTop)) * scaleY,
    };
}
