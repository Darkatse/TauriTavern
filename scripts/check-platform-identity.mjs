import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const HOST = 'src-tauri/crates/tauritavern/src/';
const IDENTITY = 'src/scripts/util/host-identity.js';
const FRONTEND_DOC = 'docs/FrontendGuide.md §2.1（宿主身份）';
const BACKEND_DOC = 'docs/BackendStructure.md §6.4（平台事实）';
// These own browser compatibility or diagnostics, not native host identity.
const BROWSER_READERS = new Set([
    'src/scripts/RossAscends-mods.js',
    'src/scripts/browser-fixes.js',
    'src/tauri/main/perf/perf-hud.js',
]);
const CATEGORIES = { mobile: ['android', 'ios', 'ohos'], desktop: ['windows', 'macos', 'linux'] };
const contract = rustTokens(readFileSync(path.join(ROOT, 'src-tauri/crates/tt-contracts/src/host.rs'), 'utf8'));
const hostEnums = new Map(['HostPlatform', 'HostKind'].map(name => {
    const start = contract.findIndex((token, index) => token.value === 'enum' && contract[index + 1]?.value === name);
    const variants = contract.slice(start + 3, closing(contract, start + 2))
        .filter(token => /^[A-Z]\w*$/u.test(token.value)).map(token => token.value);
    return [name, new Set(variants)];
}));
const platforms = [...hostEnums.get('HostPlatform')].map(name => name.toLowerCase());

function category(values) {
    const set = new Set(values);
    return Object.keys(CATEGORIES).find(kind => set.size === CATEGORIES[kind].length
        && CATEGORIES[kind].every(value => set.has(value)));
}

// Keep the observed pre-OHOS mobile shortcut explicit. Other platform subsets
// can share a real native API (for example Android + OHOS Content URI).
function legacyMobilePair(values) {
    const set = new Set(values);
    return set.size === 2 && set.has('android') && set.has('ios');
}

function reporter(file, source) {
    const errors = [];
    return {
        errors,
        add(index, rule, message) {
            const before = source.slice(0, index);
            errors.push({ file, line: before.split('\n').length, column: index - before.lastIndexOf('\n'), rule, message });
        },
    };
}

function scanFrontend(file, source) {
    const report = reporter(file, source);
    const identityUse = /hostPlatform|__TAURITAVERN_HOST__|plugin-os/u.test(source);
    const browserUse = !BROWSER_READERS.has(file) && (/getParsedUA/u.test(source)
        || source.includes('navigator') && /\b(?:userAgent|platform|maxTouchPoints|userAgentData)\b/u.test(source));
    if (!identityUse && !browserUse) return [];
    const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
    for (const diagnostic of ast.parseDiagnostics) {
        report.add(diagnostic.start ?? 0, 'FE-parse', `无法可靠扫描，请先修复语法：${ts.flattenDiagnosticMessageText(diagnostic.messageText, ' ')}`);
    }
    const platformFunctions = new Set(['hostPlatform']);
    const browserFunctions = new Set(['getParsedUA']);
    const bindings = new Map();
    const isScope = node => ts.isSourceFile(node) || ts.isBlock(node) || ts.isFunctionLike(node)
        || ts.isForStatement(node) || ts.isForOfStatement(node) || ts.isForInStatement(node) || ts.isCatchClause(node);
    const bind = (node, name, value) => {
        let scope = node.parent;
        while (scope && !isScope(scope)) scope = scope.parent;
        if (!bindings.has(scope)) bindings.set(scope, new Map());
        bindings.get(scope).set(name, value);
    };
    const binding = node => {
        for (let scope = node.parent; scope; scope = scope.parent) {
            if (bindings.get(scope)?.has(node.text)) return { value: bindings.get(scope).get(node.text) };
        }
        return undefined;
    };
    const visit = (node, action) => { action(node); ts.forEachChild(node, child => visit(child, action)); };
    visit(ast, node => {
        if (ts.isImportSpecifier(node) && (node.propertyName ?? node.name).text === 'hostPlatform') platformFunctions.add(node.name.text);
        if (ts.isImportSpecifier(node) && (node.propertyName ?? node.name).text === 'getParsedUA') browserFunctions.add(node.name.text);
        if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name)) {
            // Mutable bindings need control-flow analysis; only follow const aliases.
            const constant = ts.isVariableDeclarationList(node.parent) && node.parent.flags & ts.NodeFlags.Const;
            bind(node, node.name.text, constant ? node.initializer : undefined);
        }
        if (ts.isParameter(node) && ts.isIdentifier(node.name)) bind(node, node.name.text, undefined);
    });
    const unwrap = node => ts.isParenthesizedExpression(node) ? unwrap(node.expression) : node;
    const isPlatform = (node, seen = new Set()) => {
        node = unwrap(node);
        if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)) return platformFunctions.has(node.expression.text) && !binding(node.expression);
        if (!ts.isIdentifier(node) || seen.has(node)) return false;
        const initializer = binding(node)?.value;
        return initializer ? isPlatform(initializer, new Set([...seen, node])) : false;
    };
    const property = node => ts.isPropertyAccessExpression(node) ? node.name.text
        : ts.isElementAccessExpression(node) && ts.isStringLiteral(node.argumentExpression) ? node.argumentExpression.text : null;
    const isNavigator = (node, seen = new Set()) => {
        if (!node || seen.has(node)) return false;
        node = unwrap(node);
        if (ts.isIdentifier(node)) {
            const alias = binding(node);
            return alias ? isNavigator(alias.value, new Set([...seen, node])) : node.text === 'navigator';
        }
        return property(node) === 'navigator' && ['window', 'globalThis', 'self'].includes(node.expression.text);
    };
    const values = (node, seen = new Set()) => {
        node = unwrap(node);
        if (ts.isIdentifier(node) && binding(node)?.value && !seen.has(node)) {
            return values(binding(node).value, new Set([...seen, node]));
        }
        if (!ts.isBinaryExpression(node)) return [];
        if ([ts.SyntaxKind.BarBarToken, ts.SyntaxKind.AmpersandAmpersandToken].includes(node.operatorToken.kind)) {
            return [...values(node.left, seen), ...values(node.right, seen)];
        }
        if (![ts.SyntaxKind.EqualsEqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsEqualsToken,
            ts.SyntaxKind.EqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsToken].includes(node.operatorToken.kind)) return [];
        if (isPlatform(node.left) && ts.isStringLiteral(node.right)) return [node.right.text];
        if (isPlatform(node.right) && ts.isStringLiteral(node.left)) return [node.left.text];
        return [];
    };
    const rejectCategory = (node, literals) => {
        const kind = category(literals) ?? (legacyMobilePair(literals) ? 'mobile' : null);
        if (kind) report.add(node.getStart(ast), 'FE-category', `不要用平台列表推断 ${kind}；改用 ${kind === 'mobile' ? 'isMobileHost()' : 'isDesktopHost()'}。阶段性平台排除单独写明。参见 ${FRONTEND_DOC}、${IDENTITY}。`);
    };
    visit(ast, node => {
        const member = property(node);
        if (file !== IDENTITY && ((ts.isIdentifier(node) && node.text === '__TAURITAVERN_HOST__') || member === '__TAURITAVERN_HOST__')) {
            // Property-access identifiers are visited separately.
            if (!ts.isPropertyAccessExpression(node)) report.add(node.getStart(ast), 'FE-owner', `不要直接读写 __TAURITAVERN_HOST__；从 ${IDENTITY} 导入 hostPlatform()/isMobileHost()/isDesktopHost()。参见 ${FRONTEND_DOC}。`);
        }
        if (file !== IDENTITY && ts.isCallExpression(node) && property(node.expression) === 'defineProperty'
            && node.arguments[1] && ts.isStringLiteral(node.arguments[1]) && node.arguments[1].text === '__TAURITAVERN_HOST__') {
            report.add(node.getStart(ast), 'FE-owner', `宿主身份由 Rust 注入；删除本地 defineProperty，读取时使用 ${IDENTITY}。参见 ${FRONTEND_DOC}。`);
        }
        if (!BROWSER_READERS.has(file)) {
            if (member && ['userAgent', 'platform', 'maxTouchPoints', 'userAgentData'].includes(member)
                && isNavigator(node.expression)) {
                report.add(node.getStart(ast), 'FE-browser', `不要用 navigator.${member} 推断宿主；宿主类别用 isMobileHost()/isDesktopHost()，具体平台用 hostPlatform()。浏览器兼容/诊断留在已有浏览器边界，参见 ${FRONTEND_DOC}。`);
            }
            if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && browserFunctions.has(node.expression.text) && !binding(node.expression)) {
                report.add(node.getStart(ast), 'FE-browser', `不要用 getParsedUA() 推断宿主；改用 ${IDENTITY}，上游布局的 isMobile() 可保留。参见 ${FRONTEND_DOC}。`);
            }
            if (ts.isStringLiteral(node) && node.text === '@tauri-apps/plugin-os') {
                report.add(node.getStart(ast), 'FE-owner', `宿主平台只从 ${IDENTITY} 读取，不引入第二个 OS 来源。参见 ${FRONTEND_DOC}。`);
            }
            if (ts.isVariableDeclaration(node) && ts.isObjectBindingPattern(node.name)
                && isNavigator(node.initializer)) {
                for (const element of node.name.elements) {
                    const name = (element.propertyName ?? element.name).text;
                    if (['userAgent', 'platform', 'maxTouchPoints', 'userAgentData'].includes(name)) {
                        report.add(element.getStart(ast), 'FE-browser', `不要从 navigator 解构 ${name} 推断宿主；改用 ${IDENTITY}。参见 ${FRONTEND_DOC}。`);
                    }
                }
            }
        }
        if (ts.isBinaryExpression(node)) {
            // Report a combined condition once, at its outermost boolean expression.
            if (!ts.isBinaryExpression(node.parent)) rejectCategory(node, values(node));
        }
        if (ts.isCallExpression(node) && property(node.expression) === 'includes'
            && node.arguments.length === 1 && isPlatform(node.arguments[0])) {
            const receiver = unwrap(node.expression.expression);
            const array = ts.isIdentifier(receiver) ? binding(receiver)?.value : receiver;
            if (array && ts.isArrayLiteralExpression(array) && array.elements.every(ts.isStringLiteral)) {
                rejectCategory(node, array.elements.map(element => element.text));
            }
        }
        if (ts.isSwitchStatement(node) && isPlatform(node.expression)) {
            const cases = node.caseBlock.clauses.filter(ts.isCaseClause).map(clause => clause.expression.text);
            const fallback = node.caseBlock.clauses.find(ts.isDefaultClause);
            const last = fallback?.statements.at(-1);
            const throws = last && (ts.isThrowStatement(last) || (ts.isBlock(last) && last.statements.length && ts.isThrowStatement(last.statements.at(-1))));
            const missing = platforms.filter(platform => !cases.includes(platform));
            if (missing.length || !throws) report.add(node.getStart(ast), 'FE-switch', `平台分派必须列全 HostPlatform（缺少：${missing.join(', ') || '无'}），default 直接抛错；单个平台特例用正向判断。参见 ${FRONTEND_DOC}。`);
        }
    });
    return report.errors;
}

// Only tokenize Rust source: cfg token trees need no compiler or dependency build.
// Comments (including nested blocks), strings and raw strings cannot become code.
function rustTokens(source) {
    const tokens = [];
    const pattern = /\s+|\/\/[^\n]*|\/\*|(?:br|cr|r)(#*)"|"(?:\\[\s\S]|[^"\\])*"|'(?:\\.|[^'\\])'|[A-Za-z_]\w*|=>|::|[^\s]/gy;
    let match;
    while ((match = pattern.exec(source))) {
        const value = match[0];
        if (/^\s|^\/\//u.test(value)) continue;
        if (value === '/*') {
            let depth = 1;
            let index = pattern.lastIndex;
            while (depth && index < source.length) {
                if (source.startsWith('/*', index)) { depth++; index += 2; }
                else if (source.startsWith('*/', index)) { depth--; index += 2; }
                else index++;
            }
            pattern.lastIndex = index;
            continue;
        }
        if (match[1] !== undefined) {
            const end = source.indexOf(`"${match[1]}`, pattern.lastIndex);
            pattern.lastIndex = end < 0 ? source.length : end + 1 + match[1].length;
            continue;
        }
        tokens.push({ value, index: match.index });
    }
    return tokens;
}

function closing(tokens, start) {
    const close = { '(': ')', '[': ']', '{': '}' }[tokens[start].value];
    for (let i = start + 1; i < tokens.length; i++) {
        if (tokens[i].value === close) return i;
        if (['(', '[', '{'].includes(tokens[i].value)) i = closing(tokens, i);
    }
    throw new Error('Unclosed Rust token tree');
}

function rustIdentityScopes(tokens) {
    const blocks = [{ start: 0, end: tokens.length }];
    const contexts = [];
    for (let i = 0; i < tokens.length; i++) {
        while (i > blocks.at(-1).end) blocks.pop();
        if (tokens[i].value === '{') blocks.push({ start: i, end: closing(tokens, i) });
        if (tokens[i].value === 'impl') {
            let start = i + 1;
            while (start < tokens.length && tokens[start].value !== '{') start++;
            if (start < tokens.length) {
                // Concrete inherent/trait impls identify Self; unrelated impls also
                // enter the scope list so they shadow any enclosing host impl.
                contexts.push({ kind: 'self', type: tokens[start - 1].value, start, end: closing(tokens, start) });
            }
        }
        if (tokens[i].value === 'use') {
            for (let j = i + 1; j < tokens.length && tokens[j].value !== ';'; j++) {
                if (hostEnums.has(tokens[j].value) && tokens[j + 1]?.value === '::' && tokens[j + 2]?.value === '*') {
                    contexts.push({ kind: 'glob', type: tokens[j].value, ...blocks.at(-1) });
                }
            }
        }
    }
    return contexts;
}

function scanRust(file, source) {
    if (!/target_os|target_env|\bdesktop\b|\bmobile\b|HostPlatform|HostKind|consts|CARGO_CFG_TARGET/u.test(source)) return [];
    const report = reporter(file, source);
    const tokens = rustTokens(source);
    const identityScopes = rustIdentityScopes(tokens);
    const host = file.startsWith(HOST);
    for (let i = 0; i < tokens.length; i++) {
        const token = tokens[i];
        if (['cfg', 'cfg_attr'].includes(token.value)) {
            const runtime = tokens[i + 1]?.value === '!';
            const start = i + (runtime ? 2 : 1);
            if (tokens[start]?.value !== '(') continue;
            const body = tokens.slice(start + 1, closing(tokens, start));
            const words = body.map(item => item.value);
            const os = body.flatMap((item, j) => item.value === 'target_os' || item.value === 'target_env' ? [body[j + 2]?.value?.slice(1, -1)] : []);
            if (!host && words.some(word => ['desktop', 'mobile'].includes(word))) {
                report.add(token.index, 'RS-host-only', `非 host crate 中 desktop/mobile 恒为假；由 composition root 注入 HostKind 或所需数值，系统 API 可继续用 target_os/target_env。参见 ${BACKEND_DOC}。`);
            }
            if (category(os) || legacyMobilePair(os)) report.add(token.index, 'RS-category', `不要组合 Android/iOS 或桌面 OS 推断外壳类别；host 中改用 cfg(mobile)/cfg(desktop)，OHOS 排除需显式写 target_env = "ohos"；非 host 注入 HostKind。参见 ${BACKEND_DOC}。`);
            if (host && body.some((item, j) => item.value === 'not' && ['mobile', 'desktop'].includes(body[j + 2]?.value))) {
                report.add(token.index, 'RS-category', `外壳类别用正向 cfg(desktop)/cfg(mobile)，不要取另一类别的反值。参见 ${BACKEND_DOC}。`);
            }
            if (runtime && words.some(word => /^(?:target_os|target_env)$/u.test(word))) {
                report.add(token.index, 'RS-runtime', `运行时平台事实从 platform/identity.rs 的 HOST_IDENTITY 读取，非 host 由 composition root 注入；系统 API 的编译门控用 #[cfg(...)]。参见 ${BACKEND_DOC}。`);
            }
            if (body.some((item, j) => item.value === 'target_os' && body[j + 2]?.value === '"ohos"')) {
                report.add(token.index, 'RS-ohos', `OpenHarmony 的 target_os 是 linux；改用 target_env = "ohos"。参见 ${BACKEND_DOC}。`);
            }
        }
        if ((token.value === 'OS' && tokens[i - 2]?.value === 'consts')
            || (/^"CARGO_CFG_TARGET_(OS|ENV)"$/u.test(token.value) && tokens[i - 1]?.value === '('
                && (['var', 'var_os'].includes(tokens[i - 2]?.value) || tokens[i - 2]?.value === '!'))) {
            report.add(token.index, 'RS-owner', `不要另读 OS 构造宿主身份；使用 platform/identity.rs 的 HOST_IDENTITY，非 host 通过参数接收事实。参见 ${BACKEND_DOC}。`);
        }
        if (token.value === 'match') {
            let start = i + 1;
            while (start < tokens.length && tokens[start].value !== '{') start++;
            if (start === tokens.length) continue;
            const end = closing(tokens, start);
            let arm = [];
            const patterns = [];
            for (let j = start + 1; j < end; j++) {
                if (['(', '[', '{'].includes(tokens[j].value)) { j = closing(tokens, j); continue; }
                if (tokens[j].value === ',') arm = [];
                else if (tokens[j].value === '=>') { patterns.push(arm); arm = []; }
                else arm.push(tokens[j]);
            }
            const contexts = identityScopes.filter(scope => scope.start < i && i < scope.end);
            const selfType = contexts.findLast(scope => scope.kind === 'self')?.type;
            const globTypes = contexts.filter(scope => scope.kind === 'glob').map(scope => scope.type);
            const matchesHost = patterns.some(pattern => pattern.some((item, index) =>
                hostEnums.has(item.value)
                || item.value === 'Self' && hostEnums.has(selfType)
                || pattern[index - 1]?.value !== '::' && pattern[index + 1]?.value !== '::'
                    && globTypes.some(type => hostEnums.get(type).has(item.value))));
            if (matchesHost || hostEnums.has(selfType) && tokens[i + 1]?.value === 'self') {
                for (const pattern of patterns) {
                    const wildcard = pattern.find(item => item.value === '_');
                    if (wildcard) report.add(wildcard.index, 'RS-match', `HostPlatform/HostKind 分派不要写 _ 通配分支；列全枚举，让编译器发现新增平台遗漏。参见 ${BACKEND_DOC}。`);
                }
            }
        }
    }
    return report.errors;
}

function filesIn(directory, ignored = new Set()) {
    return readdirSync(path.join(ROOT, directory), { withFileTypes: true }).flatMap(entry => {
        if (ignored.has(entry.name)) return [];
        const file = `${directory}/${entry.name}`;
        return entry.isDirectory() ? filesIn(file, ignored) : entry.isFile() ? [file] : [];
    });
}

function main() {
    const frontend = filesIn('src', new Set(['lib', 'dist', 'node_modules', 'third-party']))
        .filter(file => /\.(?:[cm]?js|jsx|tsx?)$/u.test(file) && !/\.d\.ts$|\.(?:test|spec)\.[^.]+$|\.min\.js$/u.test(file));
    const backend = readdirSync(path.join(ROOT, 'src-tauri/crates'), { withFileTypes: true })
        .filter(entry => entry.isDirectory())
        .flatMap(entry => filesIn(`src-tauri/crates/${entry.name}/src`))
        .filter(file => file.endsWith('.rs'));
    const errors = [
        ...frontend.flatMap(file => scanFrontend(file, readFileSync(path.join(ROOT, file), 'utf8'))),
        ...backend.flatMap(file => scanRust(file, readFileSync(path.join(ROOT, file), 'utf8'))),
    ];
    for (const error of errors) console.error(`${error.file}:${error.line}:${error.column} [${error.rule}] ${error.message}`);
    console[errors.length ? 'error' : 'log'](`[platform-identity] ${errors.length ? `FAILED: ${errors.length} violation(s)` : 'clean'} (${frontend.length} frontend + ${backend.length} Rust files; ${performance.now().toFixed(0)} ms)`);
    process.exitCode = errors.length ? 1 : 0;
}

try { main(); } catch (error) {
    console.error('[platform-identity] scan failed:', error);
    process.exitCode = 1;
}
