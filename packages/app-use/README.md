# @tauritavern/app-use

Lets an agent read and operate a SillyTavern page: semantic DOM snapshots,
control interaction and an optional on-screen cursor.

The engine runs inside the page it observes. To observe another page, such as a
preview iframe, inject the bundle into that page instead of calling it from the
parent window.

## Use

```ts
import { createObservation, interact } from '@tauritavern/app-use';

const observation = createObservation();
observation.enterRun(runId);
const page = observation.snapshot({});
// Use a ref from page.tree, or a known selector that matches one element.
const result = interact({ action: 'click', selector: '#action' }, observation);
```

An observation belongs to one page and one Run. Call `enterRun` before using it.
Each snapshot page replaces the refs of the previous one. To look inside a
same-origin iframe, pass its ref as `root`; `snapshot({})` returns to the main
page. Refs and continuation cursors stop working once their document is removed
or navigated.

A host that shows refs itself, for example as numbered marks on a screenshot,
reads them with `observation.refs()`, the refs of the latest page in its order,
and finds each element with `observation.resolve({ ref })`. `resolve` applies
the same checks as `interact`, so it throws for a ref whose element has been
removed or hidden since the snapshot.

The snapshot counter is kept in the page's localStorage under
`tauritavern:in_app_agent_snapshot_sequence`. After a reload, new refs therefore
never collide with refs that still appear in the conversation.

Message text and reasoning (`.mes_text`, `.mes_reasoning`) appear as short
previews; controls inside them stay visible. Password values are never shown.
A host can hide more:

```ts
const observation = createObservation({
  sensitiveSelector: '[data-private]',
  omit: element => element.matches('.agent-history') ? 'agent conversation' : null,
});
```

These rules apply wherever the package reads the page: snapshots, selector
matching, accessible names, interaction results and error messages.

`interact` is synchronous. It checks the target, performs one DOM action (click,
fill, select, set_checked or scroll) and returns the target's state right after.
`dispatched: true` only means the action was performed, not that an asynchronous
save succeeded. The optional third argument lets the host adjust three things:

- `feedback(point)` receives the point that was hit, in main-page viewport
  coordinates.
- `surfaceCandidates(element)` returns extra elements that count as the visible
  surface of a native control, for example the wrapper around a visually hidden
  checkbox.
- `scrollTo(element, options)` replaces `element.scrollTo` for every scroll
  action. Route the regions your application scrolls itself, and scroll the rest
  normally.

TauriTavern uses `feedback` for its cursor and the other two for its control
shell and chat scrolling. The package imports no TT modules and does not check
which application it runs in.

## Tool definitions

`snapshotTool`, `interactTool`, `evaluateTool` and `readLogsTool` hold the tool
names, base descriptions and JSON schemas. A host registers them with its own
tool runtime and appends its own notes to the descriptions. The host implements
`app.evaluate` and `app.read_logs` itself. Call `observation.clearCursor()`
before running evaluate code, because the code can change the page even if it
then fails.

## Cursor

Import `@tauritavern/app-use/cursor.css` in the page where the cursor is
created. Create the cursor with `createInteractionCursor(getModalLayer)` and pass
`cursor.move` as `feedback`. The host decides its lifetime and calls
`cursor.clear()` when a Run ends or the page unloads. `getModalLayer` is only
used in WebViews without the Popover API. Motion never delays a tool call and
respects reduced-motion preferences.

The cursor uses the page's SmartTheme colors and falls back to built-in
defaults. To override them, set `--app-use-cursor-accent`,
`--app-use-cursor-fill`, `--app-use-cursor-stroke` or `--app-use-cursor-shadow`
on `.app-use-cursor`.

## Development and release

Inside the TauriTavern workspace the package is used as TypeScript source, so the
normal frontend watch picks up its changes. `pnpm --filter @tauritavern/app-use
build` writes ESM and type declarations to `dist`. Packing runs this build, and
`publishConfig.exports` points the published package at `dist`.

- Tests: `pnpm exec rstest packages/app-use`. `pnpm run check` runs them too.
- Real-browser iframe checks: run `node tests/browser/build-app-use.mjs` from the
  repository root.

The version is independent of TauriTavern. To release, bump `version` and push
the tag `app-use-v<VERSION>`. `.github/workflows/app-use-release.yml` then
publishes through npm trusted publishing and needs no npm token. On npm, the
trusted publisher is bound to `Darkatse/TauriTavern` and that workflow file;
renaming the file or moving the repository requires updating the npm setting.

AGPL-3.0-only. Cursor motion keeps its upstream MIT attribution in the source.
