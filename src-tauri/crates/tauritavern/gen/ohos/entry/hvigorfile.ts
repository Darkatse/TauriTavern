import { hapTasks } from '@ohos/hvigor-ohos-plugin';
import { hvigor, HvigorPlugin, HvigorNode, HvigorTask } from '@ohos/hvigor';
import { execFileSync } from 'child_process';
import { resolve } from 'path';

export default {
  system: hapTasks,  /* Built-in plugin of Hvigor. It cannot be modified. */
  plugins:[tauriPlugin()]         /* Custom plugin to extend the functionality of Hvigor. */
}

function tauriPlugin(): HvigorPlugin {
  return {
    pluginId: 'tauri',
    apply(node: HvigorNode) {
      const buildRustCode = () => {
        const properties = hvigor.getParameter().getProperties();
        const target = properties.target || process.env.TARGET_TRIPLE?.split("-")[0] || "aarch64";
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
