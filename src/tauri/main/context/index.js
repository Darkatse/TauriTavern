// @ts-check

import { createUserDirectoriesService } from '../services/directories/user-directories-service.js';
import { createAssetService } from '../services/assets/asset-service.js';
import { createThumbnailService } from '../services/thumbnails/thumbnail-service.js';
import { createInvokeService } from '../services/invokes/invoke-service.js';
import { createCharacterService } from '../services/characters/character-service.js';
import { createCharacterFormService } from '../services/characters/character-form-service.js';
import { createUploadService } from '../services/uploads/upload-service.js';
import { createAndroidArchiveService } from '../services/android/android-archive-service.js';
import { createHostInvokePolicies } from '../kernel/invokes/invoke-policies.js';
import {
    ensureJsonl,
    stripJsonl,
    toFrontendChat,
    formatFileSize,
    parseTimestamp,
    exportChatAsText,
    exportChatAsJsonl,
} from '../kernel/chat-utils.js';

/**
 * @typedef {import('./types.js').TauriInvokeFn} TauriInvokeFn
 * @typedef {import('./types.js').ConvertFileSrcFn} ConvertFileSrcFn
 * @typedef {import('./types.js').TauriMainContext} TauriMainContext
 */

/**
 * @param {{ invoke: TauriInvokeFn; convertFileSrc: ConvertFileSrcFn }} deps
 * @returns {TauriMainContext}
 */
export function createTauriMainContext({ invoke, convertFileSrc }) {
    const ANDROID_IMPORT_ARCHIVE_BRIDGE_NAME = 'TauriTavernAndroidImportArchiveBridge';
    const THUMBNAIL_ROUTE_TYPES = new Set(['bg', 'avatar', 'persona']);
    const IMAGE_THUMBNAIL_ROUTE_TYPES = new Set(['avatar', 'persona']);
    const THUMBNAIL_BLOB_CACHE_LIMIT = 300;

    const userDirectoriesService = createUserDirectoriesService({ invoke });
    const assetService = createAssetService({
        convertFileSrc,
        getUserDirectories: userDirectoriesService.getUserDirectories,
        thumbnailRouteTypes: THUMBNAIL_ROUTE_TYPES,
    });

    const thumbnailService = createThumbnailService({
        buildThumbnailRouteUrl: assetService.buildThumbnailRouteUrl,
        parseThumbnailRouteUrl: assetService.parseThumbnailRouteUrl,
        thumbnailRouteTypes: THUMBNAIL_ROUTE_TYPES,
        imageThumbnailRouteTypes: IMAGE_THUMBNAIL_ROUTE_TYPES,
        cacheLimit: THUMBNAIL_BLOB_CACHE_LIMIT,
    });

    const invokeService = createInvokeService({
        invoke,
        policies: createHostInvokePolicies({ thumbnailBlobCacheLimit: THUMBNAIL_BLOB_CACHE_LIMIT }),
    });

    const characterService = createCharacterService({ safeInvoke: invokeService.safeInvoke });
    const uploadService = createUploadService();
    const androidArchiveService = createAndroidArchiveService({
        safeInvoke: invokeService.safeInvoke,
        removeTempUploadFile: uploadService.removeTempUploadFile,
        bridgeName: ANDROID_IMPORT_ARCHIVE_BRIDGE_NAME,
    });
    const characterFormService = createCharacterFormService({
        safeInvoke: invokeService.safeInvoke,
        invalidateInvokeAll: invokeService.invalidateInvokeAll,
        resolveCharacterId: characterService.resolveCharacterId,
        materializeUploadFile: uploadService.materializeUploadFile,
    });

    async function initialize() {
        await userDirectoriesService.initialize();
    }

    /**
     * @param {string} type
     * @param {string} file
     * @param {boolean} [useTimestamp]
     */
    function buildThumbnailUrl(type, file, useTimestamp = false) {
            const normalizedType = String(type || '').trim().toLowerCase();

            if (THUMBNAIL_ROUTE_TYPES.has(normalizedType)) {
                return assetService.buildThumbnailRouteUrl(normalizedType, file, {
                    cacheBust: useTimestamp ? Date.now() : null,
                });
            }

            const filePath = assetService.resolveAssetPath(normalizedType, file);

            if (filePath) {
                const assetUrl = assetService.toAssetUrl(filePath);
                if (assetUrl) {
                    return `${assetUrl}${useTimestamp ? `?t=${Date.now()}` : ''}`;
                }
            }

            return assetService.buildThumbnailRouteUrl(normalizedType, file);
    }

    /** @param {string} file */
    function buildBackgroundPath(file) {
            const filePath = assetService.resolveAssetPath('bg', file);
            const assetUrl = filePath ? assetService.toAssetUrl(filePath) : null;
            return assetUrl || `backgrounds/${encodeURIComponent(file)}`;
    }

    /** @param {string} file */
    function buildAvatarPath(file) {
            const filePath = assetService.resolveAssetPath('avatar', file);
            const assetUrl = filePath ? assetService.toAssetUrl(filePath) : null;
            return assetUrl || null;
    }

    /** @param {string} file */
    function buildPersonaPath(file) {
            const filePath = assetService.resolveAssetPath('persona', file);
            const assetUrl = filePath ? assetService.toAssetUrl(filePath) : null;
            return assetUrl || `User Avatars/${file}`;
    }

    /**
     * @param {string} type
     * @param {string} file
     * @param {import('./types.js').ThumbnailBlobOptions} [options]
     */
    function resolveThumbnailBlobUrl(type, file, options = {}) {
        return thumbnailService.resolveThumbnailBlobUrl(type, file, options);
    }

    function installAssetPathHelpers() {
        window.__TAURITAVERN_THUMBNAIL__ = buildThumbnailUrl;
        window.__TAURITAVERN_BACKGROUND_PATH__ = buildBackgroundPath;
        window.__TAURITAVERN_AVATAR_PATH__ = buildAvatarPath;
        window.__TAURITAVERN_PERSONA_PATH__ = buildPersonaPath;
        window.__TAURITAVERN_THUMBNAIL_BLOB_URL__ = resolveThumbnailBlobUrl;

        thumbnailService.installThumbnailImageBridge();
    }

    installAssetPathHelpers();
    invokeService.installFlushOnHide();

    return {
        initialize,
        safeInvoke: invokeService.safeInvoke,
        invalidateInvoke: invokeService.invalidateInvoke,
        invalidateInvokeAll: invokeService.invalidateInvokeAll,
        flushInvokes: invokeService.flushInvokes,
        flushAllInvokes: invokeService.flushAllInvokes,
        get invokeTransport() {
            return invokeService.invokeTransport;
        },
        set invokeTransport(next) {
            invokeService.invokeTransport = next;
        },
        invokeBroker: invokeService.invokeBroker,
        normalizeCharacter: characterService.normalizeCharacter,
        normalizeExtensions: characterService.normalizeExtensions,
        getAllCharacters: characterService.getAllCharacters,
        resolveCharacterId: characterService.resolveCharacterId,
        getSingleCharacter: characterService.getSingleCharacter,
        ensureJsonl,
        stripJsonl,
        toFrontendChat,
        formatFileSize,
        parseTimestamp,
        exportChatAsText,
        exportChatAsJsonl,
        findAvatarByCharacterId: characterService.findAvatarByCharacterId,
        uniqueCharacterName: characterService.uniqueCharacterName,
        pickCharacterUpdateFields: characterFormService.pickCharacterUpdateFields,
        createCharacterFromForm: characterFormService.createCharacterFromForm,
        editCharacterFromForm: characterFormService.editCharacterFromForm,
        uploadAvatarFromForm: characterFormService.uploadAvatarFromForm,
        materializeUploadFile: uploadService.materializeUploadFile,
        materializeAndroidContentUriUpload: androidArchiveService.materializeAndroidContentUriUpload,
        pickAndroidImportArchive: androidArchiveService.pickAndroidImportArchive,
        saveAndroidExportArchive: androidArchiveService.saveAndroidExportArchive,
        toAssetUrl: assetService.toAssetUrl,
    };
}
