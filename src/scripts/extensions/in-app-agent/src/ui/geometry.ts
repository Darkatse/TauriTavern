import { documentWindow } from './document';
import type { DocumentScope } from './document';
import { isSensitive, preview } from './semantics';
import type { Semantics } from './semantics';

export type InteractionPoint = { x: number; y: number };
type Bounds = { left: number; right: number; top: number; bottom: number };
type FrameMapping = { frame: HTMLIFrameElement; x: number; y: number; scaleX: number; scaleY: number };
type SurfaceCheck = { point: InteractionPoint } | { reason: string };
type MissedHit = { point: InteractionPoint; document: Document; hit: Element | null };

/** Find one point that hits the target inside its document and every host frame above it. */
export function inspectInputSurface(element: HTMLElement, scope: DocumentScope, semantics: Semantics): SurfaceCheck {
    if (!semantics.isVisible(element)) {
        return { reason: `${identify(element, semantics)} is hidden or has no visible size. Open or restore its panel, then observe again.` };
    }
    const mappings = [...scope.frames].reverse().map(frameMapping);
    let outsideDocument = scope.document;
    let missed: MissedHit | null = null;

    // ponytail: one point per client rect; add richer geometry only for observed partial-overlay failures.
    for (const rect of Array.from(element.getClientRects())) {
        if (rect.width <= 0 || rect.height <= 0) continue;
        let visible = clipToViewport(rect, scope.document);
        if (!visible) outsideDocument = scope.document;
        for (const mapping of mappings) {
            if (!visible) break;
            visible = clipToViewport({
                left: mapping.x + visible.left * mapping.scaleX,
                right: mapping.x + visible.right * mapping.scaleX,
                top: mapping.y + visible.top * mapping.scaleY,
                bottom: mapping.y + visible.bottom * mapping.scaleY,
            }, mapping.frame.ownerDocument);
            if (!visible) outsideDocument = mapping.frame.ownerDocument;
        }
        if (!visible) continue;

        const point = { x: (visible.left + visible.right) / 2, y: (visible.top + visible.bottom) / 2 };
        const obstruction = findMissedHit(point, mappings, element);
        if (!obstruction) return { point };
        missed ??= obstruction;
    }
    if (missed) return { reason: describeMissedHit(missed, semantics) };
    return { reason: `${identify(element, semantics)} is outside the visible area of ${identifyPage(outsideDocument, semantics)}. Scroll its containing region, then observe again.` };
}

function clipToViewport(bounds: Bounds, doc: Document): Bounds | null {
    const view = documentWindow(doc);
    const left = Math.max(0, bounds.left);
    const right = Math.min(view.innerWidth, bounds.right);
    const top = Math.max(0, bounds.top);
    const bottom = Math.min(view.innerHeight, bounds.bottom);
    return right > left && bottom > top ? { left, right, top, bottom } : null;
}

function findMissedHit(point: InteractionPoint, mappings: FrameMapping[], target: Element): MissedHit | null {
    let local = point;
    for (const mapping of [...mappings].reverse()) {
        const hit = mapping.frame.ownerDocument.elementFromPoint(local.x, local.y);
        if (hit !== mapping.frame) return { point: local, document: mapping.frame.ownerDocument, hit };
        local = { x: (local.x - mapping.x) / mapping.scaleX, y: (local.y - mapping.y) / mapping.scaleY };
    }
    const hit = target.ownerDocument.elementFromPoint(local.x, local.y);
    return hit && target.contains(hit) ? null : { point: local, document: target.ownerDocument, hit };
}

function identify(element: Element, semantics: Semantics): string {
    // A hit may belong to an omitted region; diagnostics must not disclose its contents.
    if (!semantics.isWithinObservedContent(element) || isSensitive(element)) return `${element.localName} (content omitted)`;
    const { name } = semantics.describeElement(element);
    const id = element.id ? ` id=${JSON.stringify(preview(element.id).value)}` : '';
    return `${element.localName}${id}${name ? ` ${JSON.stringify(name)}` : ''}`;
}

function identifyPage(doc: Document, semantics: Semantics): string {
    if (doc === document) return 'the main page';
    const frame = documentWindow(doc).frameElement;
    return frame ? `the page inside ${identify(frame, semantics)}` : 'the target page';
}

function describeMissedHit({ point, document: doc, hit }: MissedHit, semantics: Semantics): string {
    let hitDescription = 'no element';
    if (hit) {
        const rect = hit.getBoundingClientRect();
        const bounds = [rect.x, rect.y, rect.width, rect.height].map(Math.round).join(', ');
        const zIndex = documentWindow(doc).getComputedStyle(hit).zIndex;
        hitDescription = `${identify(hit, semantics)} (rect x,y,width,height: ${bounds}; z-index: ${zIndex})`;
    }
    return `The checked point (${Math.round(point.x)}, ${Math.round(point.y)}) in ${identifyPage(doc, semantics)} hit ${hitDescription}, not the target. Inspect the covering panel or the target's pointer handling, then observe again. Coordinates are in that page's viewport.`;
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
