import { skillScopeKey } from '../skill-scope';
import { buildSkillScopeSections } from './scope';
import type {
    SkillManagerDeps,
    SkillManagerSnapshot,
    SkillSection,
    SkillSectionId,
} from './SkillManagerContract';
import { sortSkillEntries } from './SkillManagerContract';

export function createSkillManagerSections(context: {
    deps: SkillManagerDeps;
    getSnapshot: () => SkillManagerSnapshot;
    commit: (patch: Partial<SkillManagerSnapshot>) => void;
    isDisposed: () => boolean;
    closePreview: () => void;
}) {
    const { deps } = context;
    const epochs = new Map<SkillSectionId, number>();

    function section(id: SkillSectionId | ''): SkillSection | null {
        return context.getSnapshot().sections.find(item => item.id === id) ?? null;
    }

    function availableSection(id: SkillSectionId | ''): SkillSection & { scope: TauriTavernSkillScope } {
        const value = section(id);
        if (!value?.available || !value.scope) throw new Error(deps.tr('skillScopeNotFound', { id }));
        return { ...value, scope: value.scope };
    }

    function sectionKey(value: SkillSection | undefined): string {
        return value?.available ? skillScopeKey(value.scope) : '';
    }

    function rebuildSections(): SkillSectionId[] {
        const snapshot = context.getSnapshot();
        const previous = new Map(snapshot.sections.map(item => [item.id, item]));
        const changed: SkillSectionId[] = [];
        const sections = buildSkillScopeSections({
            context: deps.getHostContext(),
            selectedProfileId: snapshot.selectedProfileId,
            profiles: snapshot.profiles,
            tr: deps.tr,
        }).map((next): SkillSection => {
            const current = previous.get(next.id);
            const unchanged = Boolean(current) && sectionKey(current) === skillScopeKey(next.scope);
            if (!unchanged) changed.push(next.id);
            return { ...next, skills: unchanged ? current?.skills ?? [] : [], loading: unchanged ? current?.loading ?? false : false };
        });
        context.commit({ sections });
        return changed;
    }

    function patchSection(id: SkillSectionId, patch: Partial<Pick<SkillSection, 'skills' | 'loading'>>): void {
        context.commit({ sections: context.getSnapshot().sections.map(item => item.id === id ? { ...item, ...patch } : item) });
    }

    async function refreshSection(id: SkillSectionId): Promise<void> {
        const current = section(id);
        if (!current) throw new Error(deps.tr('skillScopeNotFound', { id }));
        const epoch = (epochs.get(id) ?? 0) + 1;
        epochs.set(id, epoch);
        if (!current.available || !current.scope) {
            patchSection(id, { skills: [], loading: false });
            return;
        }
        const scopeKey = skillScopeKey(current.scope);
        const requestCurrent = () => !context.isDisposed()
            && epochs.get(id) === epoch
            && sectionKey(section(id) ?? undefined) === scopeKey;
        patchSection(id, { loading: true });
        try {
            const skills = sortSkillEntries(await deps.getSkillApi().list({ scope: current.scope }));
            if (!requestCurrent()) return;
            patchSection(id, { skills });
            const preview = context.getSnapshot().preview;
            if (preview?.sectionId === id && !skills.some(skill => skill.name === preview.skill.name)) context.closePreview();
        } catch (error) {
            if (requestCurrent()) throw error;
        } finally {
            if (requestCurrent()) patchSection(id, { loading: false });
        }
    }

    return { availableSection, rebuildSections, refreshSection };
}
