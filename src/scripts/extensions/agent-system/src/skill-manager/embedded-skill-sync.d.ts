export function syncSkillInstallPortability(result: TauriTavernSkillInstallResult): Promise<void>;
export function syncSkillMovePortability(
    request: Parameters<TauriTavernSkillApi['move']>[0],
    result: TauriTavernSkillInstallResult,
): Promise<void>;
export function syncSkillWritePortability(request: { scope: TauriTavernSkillScope; name: string }): Promise<void>;
export function syncSkillDeletePortability(request: { scope: TauriTavernSkillScope; name: string }): Promise<void>;
