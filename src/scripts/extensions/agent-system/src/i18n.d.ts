export type AgentSystemTr = (key: string, params?: Record<string, unknown>) => string;

export const translateAgentSystem: AgentSystemTr;
export function translateSkillInstallAction(action: string): string;
