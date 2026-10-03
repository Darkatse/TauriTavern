import { hapTasks } from '@ohos/hvigor-ohos-plugin';
import { HvigorPlugin, HvigorNode } from '@ohos/hvigor';
import { execFileSync } from 'child_process';
import { resolve } from 'path';
import { readFileSync } from 'fs';

export default {
  system: hapTasks,  /* Built-in plugin of Hvigor. It cannot be modified. */
  plugins:[tauriPlugin()]         /* Custom plugin to extend the functionality of Hvigor. */
}

function tauriPlugin(): HvigorPlugin {
  return {
    pluginId: 'tauri',
    apply(node: HvigorNode) {
      const buildRustCode = () => {
        // Tauri filters the environment before starting Hvigor. Use the same
        // persisted ABI as native packaging instead of silently defaulting to ARM.
        const profile = JSON.parse(readFileSync(resolve(__dirname, "build-profile.json5"), "utf8"));
        const abis: string[] = profile.buildOption.externalNativeOptions.abiFilters;
        if (abis.length !== 1 || !["arm64-v8a", "x86_64"].includes(abis[0])) {
          throw new Error("Select exactly one HAP ABI with scripts/ohos/select-target.py before building");
        }
        const target = abis[0] === "x86_64" ? "x86_64" : "aarch64";
        console.info(`Building Rust for HAP ABI ${abis[0]} (${target})`);
        execFileSync(`cargo`,
          ["tauri", "ohos", "dev-eco-studio-script", "--target", target.toString(), "--release"], {
            cwd: resolve(__dirname, "../../.."),
            stdio: "inherit",
          });
      }

      node.getTaskByName('default@ConfigureCmake')!.afterRun(buildRustCode);
    }
  }
}
