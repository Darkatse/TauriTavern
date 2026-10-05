import window from '@ohos.window';
import image from '@ohos.multimedia.image';
import util from '@ohos.util';
import { WebHost } from '@ohos-rs/ability';
import { NativePlugin } from './tauri-plugins/NativePlugin';

interface WindowInsets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

interface WindowSnapshot {
  revision: number;
  width: number;
  height: number;
  scale: number;
  insets: WindowInsets;
}

interface BeginBackdropArgs {
  windowRevision: number;
  color: number[];
  replaceWallpaper: boolean;
}

interface RenderTarget {
  window: WindowSnapshot;
  token: number;
}

interface BackdropAcceptance {
  target: RenderTarget | null;
}

interface EncodedStrip {
  x: number;
  y: number;
  png: string;
}

interface ApplyBackdropArgs {
  windowRevision: number;
  wallpaperToken: number;
  strips: EncodedStrip[];
}

interface BackdropStrip {
  key: string;
  x: number;
  y: number;
  width: number;
  height: number;
  pixelMap: image.PixelMap;
}

const POLICY_AREAS: window.AvoidAreaType[] = [
  window.AvoidAreaType.TYPE_SYSTEM,
  window.AvoidAreaType.TYPE_CUTOUT,
  window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR,
];

/** Everything except the revision, which only counts geometry changes. */
function geometryKey(snapshot: WindowSnapshot): string {
  const insets = snapshot.insets;
  return `${snapshot.width}x${snapshot.height}@${snapshot.scale}:${insets.top},${insets.right},${insets.bottom},${insets.left}`;
}

/** Owns window snapshots and backdrops. ArkUI handles safe areas; ArkWeb handles IME. */
@Observed
export class WindowLayout implements NativePlugin {
  color: string = '#000000';
  strips: BackdropStrip[] = [];
  private mainWindow?: window.Window;
  private currentSnapshot?: WindowSnapshot;
  private wallpaperToken: number = 0;
  private pageReady: boolean = false;
  private windowChanged = (): void => this.refresh();
  private avoidAreaChanged = (options: window.AvoidAreaOptions): void => {
    if (POLICY_AREAS.includes(options.type)) this.refresh();
  };

  attach(mainWindow: window.Window): void {
    this.mainWindow = mainWindow;
    mainWindow.on('windowSizeChange', this.windowChanged);
    mainWindow.on('avoidAreaChange', this.avoidAreaChanged);
    this.refresh();
  }

  refresh(): void {
    const mainWindow = this.mainWindow;
    if (!mainWindow) return;
    const rect = mainWindow.getWindowProperties().windowRect;
    if (rect.width === 0 || rect.height === 0) return;
    const insets: WindowInsets = { top: 0, right: 0, bottom: 0, left: 0 };
    for (const type of POLICY_AREAS) {
      const area = mainWindow.getWindowAvoidArea(type);
      // Avoid rectangles are window-relative physical pixels. Reserve the distance from each edge.
      insets.top = Math.max(insets.top, area.topRect.top + area.topRect.height);
      insets.left = Math.max(insets.left, area.leftRect.left + area.leftRect.width);
      insets.right = Math.max(insets.right, area.rightRect.width > 0 ? rect.width - area.rightRect.left : 0);
      insets.bottom = Math.max(insets.bottom, area.bottomRect.height > 0 ? rect.height - area.bottomRect.top : 0);
    }
    const previous = this.currentSnapshot;
    const next: WindowSnapshot = {
      revision: (previous?.revision ?? 0) + 1,
      width: rect.width,
      height: rect.height,
      scale: mainWindow.getUIContext().vp2px(1),
      insets,
    };
    if (previous && geometryKey(previous) === geometryKey(next)) return;
    this.currentSnapshot = next;
    this.replaceStrips([]);
    if (this.pageReady) {
      WebHost.runJavaScript(`window.dispatchEvent(new CustomEvent('tt-window-changed',{detail:${JSON.stringify(next)}}));`)
        .catch((error: Error) => console.error(`Window layout publication: ${error.message}`));
    }
  }

  async invoke(command: string, payload: string): Promise<string> {
    switch (command) {
      case 'snapshot':
        // The page installs its listener before requesting the initial snapshot.
        this.pageReady = true;
        if (!this.currentSnapshot) throw new Error('Window layout is not ready');
        return JSON.stringify(this.currentSnapshot);
      case 'beginBackdrop': {
        const response: BackdropAcceptance = { target: this.beginBackdrop(JSON.parse(payload) as BeginBackdropArgs) };
        return JSON.stringify(response);
      }
      case 'applyBackdrop':
        await this.applyBackdrop(JSON.parse(payload) as ApplyBackdropArgs);
        return '{}';
      default:
        throw new Error(`Unknown window layout command: ${command}`);
    }
  }

  private beginBackdrop(args: BeginBackdropArgs): RenderTarget | null {
    const snapshot = this.currentSnapshot;
    if (!snapshot || snapshot.revision !== args.windowRevision) return null;
    this.color = '#' + args.color.map((channel: number): string => channel.toString(16).padStart(2, '0')).join('');
    if (args.replaceWallpaper) {
      this.wallpaperToken++;
      this.replaceStrips([]);
    }
    return { window: snapshot, token: this.wallpaperToken };
  }

  private async applyBackdrop(args: ApplyBackdropArgs): Promise<void> {
    const base64 = new util.Base64Helper();
    let decoded: BackdropStrip[] = [];
    try {
      for (let i = 0; i < args.strips.length; i++) {
        const strip = args.strips[i];
        const bytes = base64.decodeSync(strip.png);
        const source = image.createImageSource(bytes.buffer);
        try {
          const pixelMap = await source.createPixelMap();
          const size = pixelMap.getImageInfoSync().size;
          decoded.push({
            key: `${args.windowRevision}:${args.wallpaperToken}:${i}`,
            x: strip.x, y: strip.y, width: size.width, height: size.height, pixelMap,
          });
        } finally {
          await source.release();
        }
      }
      // Decoding yields to newer selections and window events. Only the current result may publish.
      if (this.currentSnapshot?.revision !== args.windowRevision || this.wallpaperToken !== args.wallpaperToken) return;
      this.replaceStrips(decoded);
      decoded = [];
    } finally {
      this.releaseStrips(decoded);
    }
  }

  private replaceStrips(next: BackdropStrip[]): void {
    const previous = this.strips;
    this.strips = next;
    this.releaseStrips(previous);
  }

  private releaseStrips(strips: BackdropStrip[]): void {
    for (const strip of strips) {
      strip.pixelMap.release().catch((error: Error) => console.error(`Release window backdrop: ${error.message}`));
    }
  }

  close(): void {
    this.mainWindow?.off('windowSizeChange', this.windowChanged);
    this.mainWindow?.off('avoidAreaChange', this.avoidAreaChanged);
    this.mainWindow = undefined;
    this.currentSnapshot = undefined;
    this.pageReady = false;
    this.replaceStrips([]);
  }
}

@Component
export struct WindowBackdrop {
  @ObjectLink layout: WindowLayout;

  build() {
    Stack({ alignContent: Alignment.TopStart }) {
      ForEach(this.layout.strips, (strip: BackdropStrip) => {
        // Rust already rendered physical pixels; do not apply Image's intrinsic density sizing.
        Image(strip.pixelMap)
          .width(`${strip.width}px`).height(`${strip.height}px`)
          .position({ x: `${strip.x}px`, y: `${strip.y}px` });
      }, (strip: BackdropStrip): string => strip.key);
    }
    .width(LayoutPolicy.matchParent).height(LayoutPolicy.matchParent)
    .ignoreLayoutSafeArea()
    .backgroundColor(this.layout.color)
    .hitTestBehavior(HitTestMode.None);
  }
}
