import { DefaultXComponent, WebHost } from '@ohos-rs/ability';
import { WindowBackdrop, WindowLayout } from '../WindowLayout';

@Entry
@Component
struct Main {
  // RustAbility stores the Cargo library name before loading this page.
  private moduleName: string = AppStorage.get<string>('moduleName')!;
  @State private windowLayout: WindowLayout = AppStorage.get<WindowLayout>('ttWindowLayout')!;

  onBackPress(): boolean {
    return WebHost.handleBackPress();
  }

  build() {
    Stack() {
      WindowBackdrop({ layout: this.windowLayout });
      DefaultXComponent({ moduleName: this.moduleName });
    }.width('100%').height('100%');
  }
}
