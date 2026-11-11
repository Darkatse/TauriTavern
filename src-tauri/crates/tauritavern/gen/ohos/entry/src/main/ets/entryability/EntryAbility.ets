import native from 'libtauritavern_lib.so';
import { TauriPlugins } from '../tauri-plugins/TauriPlugins';
import { NativeModule, NativePlugin } from '../tauri-plugins/NativePlugin';
import { WindowLayout } from '../WindowLayout';
import { RustAbility, WebHost } from '@ohos-rs/ability';
import Want from '@ohos.app.ability.Want';
import { AbilityConstant, Configuration } from '@kit.AbilityKit';
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
  private windowLayout: WindowLayout = new WindowLayout();
  // Must match the Cargo library name.
  public moduleName: string = 'tauritavern_lib';
  public defaultPage: boolean = false;
  public mode: 'xcomponent' | 'webview' = 'webview';

  async onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): Promise<void> {
    WebHost.configureBackNavigation(
      "(() => { const fn = window.__TAURITAVERN_HANDLE_BACK__; return Boolean(typeof fn === 'function' && fn()); })()",
      () => { this.context.moveAbilityToBackground(); },
    );
    const plugins = new Map<string, NativePlugin>();
    plugins.set('window-layout', this.windowLayout);
    this.tauriPlugins = new TauriPlugins(this.context, new TauriNativeModule(), plugins);
    await super.onCreate(want, launchParam);
  }

  async onWindowStageCreate(windowStage: window.WindowStage): Promise<void> {
    const mainWindow = windowStage.getMainWindowSync();
    await mainWindow.setWindowLayoutFullScreen(false);
    await super.onWindowStageCreate(windowStage);
    AppStorage.setOrCreate('ttWindowLayout', this.windowLayout);
    await windowStage.loadContent('pages/Main');
    // UIContext belongs to the loaded page; only then read its pixel scale and subscribe.
    this.windowLayout.attach(mainWindow);
  }

  onConfigurationUpdate(config: Configuration): void {
    super.onConfigurationUpdate(config);
    this.windowLayout.refresh();
  }

  onDestroy(): void {
    WebHost.configureBackNavigation("");
    this.tauriPlugins?.close();
    AppStorage.delete('ttWindowLayout');
    super.onDestroy();
  }
}
