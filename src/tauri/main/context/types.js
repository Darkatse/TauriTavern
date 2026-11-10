// @ts-check

export {};

/**
 * @typedef {import('../kernel/invokes/tauri-commands.js').TauriInvokeCommand} TauriInvokeCommand
 */

/**
 * @typedef {(command: TauriInvokeCommand, args?: any, options?: { headers?: HeadersInit }) => Promise<any>} TauriInvokeFn
 */

/**
 * A staged file its holder must release with `cleanup`.
 * @typedef {{
 *   filePath: string;
 *   cleanup: () => Promise<void>;
 * }} MaterializedFileInfo
 */

/**
 * @typedef {(file: Blob, options?: { preferredName?: string; preferredExtension?: string; kind?: string }) => Promise<MaterializedFileInfo>} MaterializeUploadFileFn
 * @typedef {(filePath: string) => MaterializedFileInfo} AdoptStagedFileFn
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
 *   safeInvoke: (command: TauriInvokeCommand, args?: any) => Promise<any>;
 *   invalidateInvoke: (command: TauriInvokeCommand, args?: any) => void;
 *   invalidateInvokeAll: (command: TauriInvokeCommand) => void;
 *   flushInvokes: (command: TauriInvokeCommand) => Promise<void>;
 *   flushAllInvokes: () => Promise<void>;
 *   invokeBroker: any;
 *   invokeTransport: TauriInvokeFn;
 *   normalizeCharacter: (character: any) => any;
 *   normalizeExtensions: (extensions: any) => any;
 *   getAllCharacters: () => Promise<any[]>;
 *   ensureCharacterIndex: () => Promise<void>;
 *   invalidateCharacterCache: () => void;
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
 *   materializeUploadFile: MaterializeUploadFileFn;
 *   adoptStagedFile: AdoptStagedFileFn;
 *   createChatBackupDownloadStream: (name: string) => Promise<ReadableStream<Uint8Array>>;
 *   createReadableFileStream: (filePath: string) => ReadableStream<Uint8Array> | Promise<ReadableStream<Uint8Array>>;
 * }} TauriMainContext
 */
