import { DefaultXComponent, WebHost } from '@ohos-rs/ability';

@Entry
@Component
struct Main {
  // RustAbility stores the Cargo library name before loading this page.
  private moduleName: string = AppStorage.get<string>('moduleName')!;

  onBackPress(): boolean {
    return WebHost.handleBackPress();
  }

  build() {
    Stack() {
      DefaultXComponent({ moduleName: this.moduleName });
    }.width('100%').height('100%');
  }
}
