// @ts-check

import { createUserDirectoriesService } from '../services/directories/user-directories-service.js';
import { createAssetService } from '../services/assets/asset-service.js';
import { createThumbnailService } from '../services/thumbnails/thumbnail-service.js';
import { createInvokeService } from '../services/invokes/invoke-service.js';
import { createCharacterService } from '../services/characters/character-service.js';
import { createCharacterFormService } from '../services/characters/character-form-service.js';
import { createCharacterCreateService } from '../services/characters/character-create-service.js';
import { createUploadService } from '../services/uploads/upload-service.js';
import { createAndroidArchiveService } from '../services/android/android-archive-service.js';
import { createHostInvokePolicies } from '../kernel/invokes/invoke-policies.js';
import { installAssetPathHelpers } from './asset-path-helpers.js';
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
    const THUMBNAIL_BLOB_CACHE_LIMIT = 300;

    const userDirectoriesService = createUserDirectoriesService({ invoke });
    const assetService = createAssetService({
        convertFileSrc,
        getUserDirectories: userDirectoriesService.getUserDirectories,
        thumbnailRouteTypes: THUMBNAIL_ROUTE_TYPES,
    });

    const thumbnailService = createThumbnailService({
        buildThumbnailRouteUrl: assetService.buildThumbnailRouteUrl,
        thumbnailRouteTypes: THUMBNAIL_ROUTE_TYPES,
        cacheLimit: THUMBNAIL_BLOB_CACHE_LIMIT,
    });

    const invokeService = createInvokeService({
        invoke,
        policies: createHostInvokePolicies({ thumbnailBlobCacheLimit: THUMBNAIL_BLOB_CACHE_LIMIT }),
    });

    const characterService = createCharacterService({ safeInvoke: invokeService.safeInvoke });
    const uploadService = createUploadService();
    const characterCreateService = createCharacterCreateService({
        safeInvoke: invokeService.safeInvoke,
        materializeUploadFile: uploadService.materializeUploadFile,
    });
    const androidArchiveService = createAndroidArchiveService({
        safeInvoke: invokeService.safeInvoke,
        removeTempUploadFile: uploadService.removeTempUploadFile,
        bridgeName: ANDROID_IMPORT_ARCHIVE_BRIDGE_NAME,
    });
    const characterFormService = createCharacterFormService({
        safeInvoke: invokeService.safeInvoke,
        invalidateInvokeAll: invokeService.invalidateInvokeAll,
        resolveCharacterId: characterService.resolveCharacterId,
        resolveExistingCharacterId: characterService.resolveExistingCharacterId,
        materializeUploadFile: uploadService.materializeUploadFile,
    });

    async function initialize() {
        await userDirectoriesService.initialize();
    }

    /** @param {string} filePath */
    async function removeTemporaryFile(filePath) {
        const invokeApi = window.__TAURI__?.core?.invoke;
        if (typeof invokeApi !== 'function') {
            throw new Error('Tauri invoke API is unavailable');
        }

        await uploadService.removeTempUploadFile(filePath, invokeApi);
    }

    installAssetPathHelpers({
        assetService,
        thumbnailService,
        thumbnailRouteTypes: THUMBNAIL_ROUTE_TYPES,
    });
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
        resolveExistingCharacterId: characterService.resolveExistingCharacterId,
        getSingleCharacter: characterService.getSingleCharacter,
        ensureJsonl,
        stripJsonl,
        toFrontendChat,
        formatFileSize,
        parseTimestamp,
        exportChatAsText,
        exportChatAsJsonl,
        findAvatarByCharacterId: characterService.findAvatarByCharacterId,
        createCharacterFromForm: characterCreateService.createCharacterFromForm,
        createCharacterFromPayload: characterCreateService.createCharacterFromPayload,
        editCharacterFromForm: characterFormService.editCharacterFromForm,
        editCharacterAvatarFromForm: characterFormService.editCharacterAvatarFromForm,
        uploadAvatarFromForm: characterFormService.uploadAvatarFromForm,
        materializeUploadFile: uploadService.materializeUploadFile,
        materializeAndroidContentUriUpload: androidArchiveService.materializeAndroidContentUriUpload,
        materializeAndroidSkillImportArchive: androidArchiveService.materializeAndroidSkillImportArchive,
        pickAndroidImportArchive: androidArchiveService.pickAndroidImportArchive,
        removeTemporaryFile,
        saveAndroidExportArchive: androidArchiveService.saveAndroidExportArchive,
        toAssetUrl: assetService.toAssetUrl,
    };
}
