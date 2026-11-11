import { appTasks, OhosAppContext, OhosPluginId } from '@ohos/hvigor-ohos-plugin';
import { HvigorNode, HvigorPlugin } from '@ohos/hvigor';
import { readFileSync } from 'fs';
import { resolve } from 'path';

function appVersionFromTauriConfig(): HvigorPlugin {
  return {
    pluginId: 'tauritavern-app-version',
    apply(node: HvigorNode) {
      // The CLI writes the merged config before Hvigor runs. Tauri has already
      // validated SemVer; use its Android versionCode formula for HAPs as well.
      const configPath = resolve(__dirname, 'assets/tauri.conf.json');
      const version = String(JSON.parse(readFileSync(configPath, 'utf8')).version ?? '');
      const [major, minor, patch] = version.split(/[-+]/, 1)[0].split('.').map(Number);
      const versionCode = major * 1_000_000 + minor * 1_000 + patch;
      if (!Number.isSafeInteger(versionCode) || minor >= 1000 || patch >= 1000 || versionCode > 2_147_483_647) {
        throw new Error(`Cannot encode HAP versionCode from Tauri version "${version}" in ${configPath}`);
      }

      const context = node.getContext(OhosPluginId.OHOS_APP_PLUGIN) as OhosAppContext;
      const app = context.getAppJsonOpt();
      app.app.versionName = version;
      app.app.versionCode = versionCode;
      context.setAppJsonOpt(app);
    },
  };
}

export default {
  system: appTasks,
  plugins: [appVersionFromTauriConfig()],
};
