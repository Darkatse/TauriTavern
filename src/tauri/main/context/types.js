// @ts-check

export {};

/**
 * @typedef {import('../kernel/invokes/tauri-commands.js').TauriInvokeCommand} TauriInvokeCommand
 */

/**
 * @typedef {(command: TauriInvokeCommand, args?: any) => Promise<any>} TauriInvokeFn
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
 *   code: string;
 *   message: string;
 * }} CharacterCreateWarning
 */

/**
 * @typedef {{
 *   character: any;
 *   warnings: CharacterCreateWarning[];
 * }} CharacterCreateOutcome
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
 *   safeInvoke: (command: TauriInvokeCommand, args?: any) => Promise<any>;
 *   invalidateInvoke: (command: TauriInvokeCommand, args?: any) => void;
 *   invalidateInvokeAll: (command: TauriInvokeCommand) => void;
 *   flushInvokes: (command: TauriInvokeCommand) => Promise<void>;
 *   flushAllInvokes: () => Promise<void>;
 *   invokeBroker: any;
 *   invokeTransport: (command: TauriInvokeCommand, args?: any) => Promise<any>;
 *   normalizeCharacter: (character: any) => any;
 *   normalizeExtensions: (extensions: any) => any;
 *   getAllCharacters: (options?: { shallow?: boolean; forceRefresh?: boolean }) => Promise<any[]>;
 *   resolveCharacterId: (options?: { avatar?: any; fallbackName?: string }) => Promise<string | null>;
 *   resolveExistingCharacterId: (options?: { avatar?: any; fallbackName?: string }) => Promise<string | null>;
 *   getSingleCharacter: (body: any) => Promise<any | null>;
 *   ensureJsonl: (fileName: string) => string;
 *   stripJsonl: (fileName: string) => string;
 *   toFrontendChat: (chatDto: any) => any[];
 *   formatFileSize: (value: any) => string;
 *   parseTimestamp: (sendDate: any) => number;
 *   exportChatAsText: (frontendChat: any) => string;
 *   exportChatAsJsonl: (frontendChat: any[]) => string;
 *   findAvatarByCharacterId: (characterId: any) => string;
 *   createCharacterFromForm: (formData: FormData, requestUrl: URL) => Promise<CharacterCreateOutcome>;
 *   createCharacterFromPayload: (payload: Record<string, any>) => Promise<CharacterCreateOutcome>;
 *   editCharacterFromForm: (formData: FormData, requestUrl: URL) => Promise<void>;
 *   editCharacterAvatarFromForm: (formData: FormData, requestUrl: URL) => Promise<void>;
 *   uploadAvatarFromForm: (formData: FormData, requestUrl: URL) => Promise<any>;
 *   materializeUploadFile: (file: Blob, options?: { preferredName?: string; preferredExtension?: string }) => Promise<MaterializedFileInfo | null>;
 *   materializeAndroidContentUriUpload: (contentUri: string) => Promise<MaterializedFileInfo>;
 *   materializeAndroidSkillImportArchive: (contentUri: string) => Promise<MaterializedFileInfo>;
 *   pickAndroidImportArchive: () => Promise<string>;
 *   removeTemporaryFile: (filePath: string) => Promise<void>;
 *   createReadableFileStream: (filePath: string) => ReadableStream<Uint8Array> | Promise<ReadableStream<Uint8Array>>;
 *   saveAndroidExportArchive: (sourcePath: string, preferredName?: string) => Promise<AndroidExportResult>;
 *   toAssetUrl: (path: string) => string | null;
 * }} TauriMainContext
 */
