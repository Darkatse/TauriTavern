// @ts-check

export {};

/**
 * @typedef {(command: string, args?: any) => Promise<any>} TauriInvokeFn
 */

/**
 * @typedef {(path: string, protocol?: string) => string} ConvertFileSrcFn
 */

/**
 * @typedef {{
 *   characters: string;
 *   avatars: string;
 *   backgrounds: string;
 *   [key: string]: any;
 * }} UserDirectories
 */

/**
 * @typedef {{
 *   filePath: string;
 *   error?: string;
 *   isTemporary?: boolean;
 *   cleanup?: (() => Promise<void>) | undefined;
 * }} MaterializedFileInfo
 */

/**
 * @typedef {{
 *   type: string;
 *   file: string;
 *   animated: boolean;
 *   cacheBust: string;
 * }} ThumbnailRouteSpec
 */

/**
 * @typedef {{
 *   animated?: boolean;
 *   useTimestamp?: boolean;
 * }} ThumbnailBlobOptions
 */

/**
 * @typedef {{
 *   savedTarget?: string;
 * }} AndroidExportResult
 */

/**
 * @typedef {{
 *   initialize: () => Promise<void>;
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 *   invalidateInvoke: (command: string, args?: any) => void;
 *   invalidateInvokeAll: (command: string) => void;
 *   flushInvokes: (command: string) => Promise<void>;
 *   flushAllInvokes: () => Promise<void>;
 *   invokeBroker: any;
 *   invokeTransport: (command: string, args?: any) => Promise<any>;
 *   normalizeCharacter: (character: any) => any;
 *   normalizeExtensions: (extensions: any) => any;
 *   getAllCharacters: (options?: { shallow?: boolean; forceRefresh?: boolean }) => Promise<any[]>;
 *   resolveCharacterId: (options?: { avatar?: any; fallbackName?: string }) => Promise<string | null>;
 *   getSingleCharacter: (body: any) => Promise<any | null>;
 *   ensureJsonl: (fileName: string) => string;
 *   stripJsonl: (fileName: string) => string;
 *   toFrontendChat: (chatDto: any) => any[];
 *   formatFileSize: (value: any) => string;
 *   parseTimestamp: (sendDate: any) => number;
 *   exportChatAsText: (frontendChat: any) => string;
 *   exportChatAsJsonl: (frontendChat: any[]) => string;
 *   findAvatarByCharacterId: (characterId: any) => string;
 *   uniqueCharacterName: (baseName: string) => Promise<string>;
 *   pickCharacterUpdateFields: (payload: any) => Record<string, any>;
 *   createCharacterFromForm: (formData: FormData, requestUrl: URL) => Promise<any>;
 *   editCharacterFromForm: (formData: FormData, requestUrl: URL) => Promise<void>;
 *   uploadAvatarFromForm: (formData: FormData, requestUrl: URL) => Promise<any>;
 *   materializeUploadFile: (file: Blob, options?: { preferredName?: string; preferredExtension?: string }) => Promise<MaterializedFileInfo | null>;
 *   materializeAndroidContentUriUpload: (contentUri: string) => Promise<MaterializedFileInfo>;
 *   pickAndroidImportArchive: () => Promise<string>;
 *   saveAndroidExportArchive: (sourcePath: string, preferredName?: string) => Promise<AndroidExportResult>;
 *   toAssetUrl: (path: string) => string | null;
 * }} TauriMainContext
 */

