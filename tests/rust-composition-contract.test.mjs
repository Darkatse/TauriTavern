import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

function count(source, needle) {
    return source.split(needle).length - 1;
}

test('Rust composition keeps repository sharing explicit', async () => {
    const source = await readRepoFile('src-tauri/src/app/composition/repositories.rs');

    assert.match(
        source,
        /let file_chat_repository = Arc::new\(FileChatRepository::with_chat_aliases\([\s\S]*?let chat_repository: Arc<dyn ChatRepository> = file_chat_repository\.clone\(\);[\s\S]*?let group_chat_repository: Arc<dyn GroupChatRepository> = file_chat_repository;/,
    );
    assert.match(
        source,
        /let agent_profile_file_repository = Arc::new\(FileAgentProfileRepository::new\([\s\S]*?let agent_profile_repository: Arc<dyn AgentProfileRepository> =\s*agent_profile_file_repository\.clone\(\);[\s\S]*?let agent_profile_storage_health_repository: Arc<dyn AgentProfileStorageHealthRepository> =\s*agent_profile_file_repository;/,
    );
    assert.match(
        source,
        /let file_agent_repository = Arc::new\(FileAgentRepository::new\([\s\S]*?let agent_run_repository: Arc<dyn AgentRunRepository> = file_agent_repository\.clone\(\);[\s\S]*?let agent_invocation_repository: Arc<dyn AgentInvocationRepository> =\s*file_agent_repository\.clone\(\);[\s\S]*?let workspace_repository: Arc<dyn WorkspaceRepository> = file_agent_repository\.clone\(\);[\s\S]*?let checkpoint_repository: Arc<dyn CheckpointRepository> = file_agent_repository\.clone\(\);[\s\S]*?let agent_workspace_lifecycle_repository: Arc<dyn AgentWorkspaceLifecycleRepository> =\s*file_agent_repository;/,
    );
});

test('Rust sync composition keeps one shared coordinator and pairing approval', async () => {
    const source = await readRepoFile('src-tauri/src/app/composition/services/sync.rs');

    assert.equal(count(source, 'let sync_job_coordinator ='), 1);
    assert.equal(count(source, 'SyncJobCoordinator::new('), 1);
    assert.equal(count(source, 'let pairing_approval ='), 1);

    assert.match(
        source,
        /let lan_inbound_service = Arc::new\(LanInboundService::new\([\s\S]*?sync_job_coordinator\.clone\(\),[\s\S]*?pairing_approval\.clone\(\),[\s\S]*?\)\);/,
    );
    assert.match(
        source,
        /let lan_sync_service = Arc::new\(LanSyncService::new\([\s\S]*?pairing_approval,[\s\S]*?sync_job_coordinator\.clone\(\),[\s\S]*?\)\);/,
    );
    assert.match(
        source,
        /let tt_sync_service = Arc::new\(TtSyncService::new\([\s\S]*?sync_job_coordinator\.clone\(\),[\s\S]*?\)\);/,
    );
    assert.match(
        source,
        /let sync_automation_service = Arc::new\(SyncAutomationService::new\([\s\S]*?sync_job_coordinator,[\s\S]*?\)\);/,
    );
});

test('Rust agent composition keeps Agent behind the chat completion service gateway', async () => {
    const source = await readRepoFile('src-tauri/src/app/composition/services/agent.rs');

    assert.match(source, /AgentRuntimeService::new_with_prompt_assembly_service\(/);
    assert.match(
        source,
        /Arc::new\(ChatCompletionAgentModelGateway::new\(\s*chat_completion_service,\s*\)\)/,
    );
    assert.doesNotMatch(source, /AgentRuntimeService::new\(/);
});
