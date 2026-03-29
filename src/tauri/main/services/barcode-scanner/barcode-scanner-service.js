// @ts-check

import { pushBackNavigationHandler } from '../back-navigation/back-handler-stack.js';

function getBarcodeScanner() {
    const barcodeScanner = window.__TAURI__?.barcodeScanner;
    if (!barcodeScanner?.requestPermissions || !barcodeScanner?.scan || !barcodeScanner?.cancel) {
        throw new Error('Tauri barcode scanner is unavailable');
    }

    if (!barcodeScanner?.Format?.QRCode) {
        throw new Error('Tauri barcode scanner QR format is unavailable');
    }

    return barcodeScanner;
}

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

/**
 * Starts a QR-code scan session that can be cancelled by the host back bridge.
 *
 * @returns {Promise<string | null>}
 */
export async function scanQrCodeWithBackCancellation() {
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
