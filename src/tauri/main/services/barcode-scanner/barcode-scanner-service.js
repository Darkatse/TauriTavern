// @ts-check

import { getBarcodeScanner } from '../../../../tauri-bridge.js';
import { pushBackNavigationHandler } from '../back-navigation/back-handler-stack.js';

/**
 * @param {unknown} error
 * @returns {boolean}
 */
function isBarcodeScannerCancellationError(error) {
    const message = error instanceof Error
        ? error.message
        : String(error ?? '').trim();
    return message.trim().toLowerCase() === 'cancelled';
}

export function canScanQrCode() {
    const scanner = getBarcodeScanner();
    return typeof scanner?.requestPermissions === 'function'
        && typeof scanner?.scan === 'function'
        && typeof scanner?.cancel === 'function'
        && Boolean(scanner?.Format?.QRCode);
}

/**
 * Starts a QR-code scan session that can be cancelled by the host back bridge.
 *
 * @returns {Promise<string | null>}
 */
export async function scanQrCodeWithBackCancellation() {
    if (!canScanQrCode()) {
        throw new Error('Tauri barcode scanner is unavailable');
    }
    const barcodeScanner = getBarcodeScanner();

    const granted = await barcodeScanner.requestPermissions();
    if (!granted) {
        throw new Error('Camera permission is required to scan QR codes');
    }

    let cancelledByBack = false;
    const disposeBackHandler = pushBackNavigationHandler(() => {
        cancelledByBack = true;
        void barcodeScanner.cancel();
        return true;
    });

    try {
        const result = await barcodeScanner.scan({ formats: [barcodeScanner.Format.QRCode] });
        const content = String(result?.content ?? '').trim();
        if (!content) {
            throw new Error('Scanned Pair URI is empty');
        }

        return content;
    } catch (error) {
        if (cancelledByBack && isBarcodeScannerCancellationError(error)) {
            return null;
        }

        throw error;
    } finally {
        disposeBackHandler();
    }
}
