import native from 'libtauritavern_lib.so';
import { TauriPlugins } from '../tauri-plugins/TauriPlugins';
import { NativeModule } from '../tauri-plugins/NativePlugin';
import { RustAbility, WebHost } from '@ohos-rs/ability';
import Want from '@ohos.app.ability.Want';
import { AbilityConstant } from '@kit.AbilityKit';
import window from '@ohos.window';

class TauriNativeModule implements NativeModule {
  tauriOhosPluginInitialize(callback: (request: string) => void, files: string, cache: string, temp: string): void {
    native.tauriOhosPluginInitialize(callback, files, cache, temp);
  }
  tauriOhosPluginResponse(id: number, success: boolean, payload: string): void {
    native.tauriOhosPluginResponse(id, success, payload);
  }
  tauriOhosPluginClose(): void { native.tauriOhosPluginClose(); }
}

export default class EntryAbility extends RustAbility {
  private tauriPlugins?: TauriPlugins;
  // Must match the Cargo library name.
  public moduleName: string = 'tauritavern_lib';
  public defaultPage: boolean = false;
  public mode: 'xcomponent' | 'webview' = 'webview';

  async onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): Promise<void> {
    WebHost.configureBackNavigation(
      "(() => { const fn = window.__TAURITAVERN_HANDLE_BACK__; return Boolean(typeof fn === 'function' && fn()); })()",
      () => { this.context.moveAbilityToBackground(); },
    );
    this.tauriPlugins = new TauriPlugins(this.context, new TauriNativeModule());
    await super.onCreate(want, launchParam);
  }

  async onWindowStageCreate(windowStage: window.WindowStage): Promise<void> {
    const mainWindow = windowStage.getMainWindowSync();
    await mainWindow.setWindowLayoutFullScreen(false);
    await super.onWindowStageCreate(windowStage);
    await windowStage.loadContent('pages/Main');
  }

  onDestroy(): void {
    WebHost.configureBackNavigation("");
    this.tauriPlugins?.close();
    super.onDestroy();
  }
}
