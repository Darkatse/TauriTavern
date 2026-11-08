// A front-end card's status bar: one HTML document of about 80 KB, mostly CSS and a script that renders tabs from the
// state it carries. It holds nothing that keeps a page from going quiet or reaches the network: no timers, endless
// animations, fonts or images.

const RARITIES = ['普通', '优良', '稀有', '史诗', '传说', '神话'];
const ELEMENTS = ['火', '水', '风', '土', '雷', '光', '暗'];
const PANELS = ['card', 'stat', 'item', 'quest', 'place', 'bond', 'log', 'chip', 'tag', 'meter', 'badge', 'note'];

function css() {
    const rules = [`:root {
  --bg: #14161c; --panel: #1d212b; --panel-2: #252a36; --line: #343b4b; --text: #e6e2d8; --muted: #9a9486;
  --accent: #d6a24a; --accent-2: #7fb0d8; --good: #6fbf73; --bad: #d8645a; --radius: 10px; --gap: 10px;
  --shadow: 0 2px 10px rgba(0, 0, 0, .35); --font: "PingFang SC", "Microsoft YaHei", system-ui, sans-serif;
}
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; background: transparent; color: var(--text); font: 14px/1.55 var(--font); }
.sb { margin: 6px auto; max-width: 860px; background: var(--bg); border: 1px solid var(--line); border-radius: var(--radius);
  box-shadow: var(--shadow); overflow: hidden; }
.sb-head { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--gap); padding: 12px 14px;
  background: linear-gradient(135deg, #222838, #191c25); border-bottom: 1px solid var(--line); }
.sb-head .cell { display: flex; flex-direction: column; min-width: 0; }
.sb-head .cell b { color: var(--accent); font-size: 12px; letter-spacing: .08em; }
.sb-head .cell span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.sb-tabs { display: flex; gap: 4px; padding: 8px 10px 0; background: var(--panel); border-bottom: 1px solid var(--line); }
.sb-tab { flex: 1 1 0; padding: 7px 4px; border: 0; border-radius: 8px 8px 0 0; background: transparent; color: var(--muted);
  font: inherit; cursor: pointer; transition: color .15s, background-color .15s; }
.sb-tab:hover { color: var(--text); background: var(--panel-2); }
.sb-tab.on { color: var(--accent); background: var(--bg); box-shadow: inset 0 2px 0 var(--accent); }
.sb-page { display: none; padding: 12px 14px; }
.sb-page.on { display: block; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: var(--gap); }
.row { display: flex; align-items: center; gap: 8px; }
.row > .grow { flex: 1 1 auto; min-width: 0; }
.bar { position: relative; height: 8px; border-radius: 4px; background: #0d0f14; overflow: hidden; }
.bar > i { position: absolute; inset: 0 auto 0 0; border-radius: 4px; background: var(--accent); }
.bar.hp > i { background: linear-gradient(90deg, #a83a32, var(--bad)); }
.bar.mp > i { background: linear-gradient(90deg, #2f5f8f, var(--accent-2)); }
.bar.sp > i { background: linear-gradient(90deg, #3d7f41, var(--good)); }
.bar.love > i { background: linear-gradient(90deg, #8f3f6f, #e07fb0); }
.search { width: 100%; margin: 0 0 10px; padding: 7px 10px; border: 1px solid var(--line); border-radius: 8px;
  background: var(--panel); color: var(--text); font: inherit; }
details.fold > summary { cursor: pointer; list-style: none; color: var(--muted); }
details.fold > summary::before { content: "▸ "; }
details.fold[open] > summary::before { content: "▾ "; }
table.attrs { width: 100%; border-collapse: collapse; font-size: 12px; }
table.attrs td { padding: 2px 4px; border-bottom: 1px dashed var(--line); }
table.attrs td:last-child { text-align: right; color: var(--accent); }
@media (max-width: 600px) {
  .sb-head { grid-template-columns: repeat(2, 1fr); }
  .grid { grid-template-columns: 1fr; }
  .sb-tab { font-size: 12px; }
}`];
    PANELS.forEach((panel, p) => {
        rules.push(`.${panel} { position: relative; padding: 10px 12px; border: 1px solid var(--line); border-radius: var(--radius);
  background: var(--panel); }
.${panel} > header { display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 6px; }
.${panel} > header h4 { margin: 0; font-size: 15px; font-weight: 600; }
.${panel} > header small { color: var(--muted); }
.${panel} p { margin: 4px 0; color: var(--muted); font-size: 12px; }
.${panel}:hover { border-color: #4a5266; }`);
        RARITIES.forEach((rarity, r) => {
            const hue = (r * 47 + p * 13) % 360;
            rules.push(`.${panel}.r${r} { border-left: 3px solid hsl(${hue} 55% 55%); }
.${panel}.r${r} > header h4 { color: hsl(${hue} 60% 72%); }
.${panel}.r${r} .tag { background: hsl(${hue} 35% 22%); color: hsl(${hue} 60% 80%); }
.${panel}.r${r}::after { content: "${rarity}"; position: absolute; top: 6px; right: 8px; font-size: 10px; color: hsl(${hue} 40% 60%); }`);
        });
        ELEMENTS.forEach((element, e) => {
            const hue = (e * 51 + p * 7) % 360;
            rules.push(`.${panel}.e${e} .el { color: hsl(${hue} 70% 65%); text-shadow: 0 0 4px hsl(${hue} 70% 30%); }
.${panel}.e${e} .bar > i { box-shadow: 0 0 6px hsl(${hue} 70% 40%); }`);
        });
    });
    return rules.join('\n');
}

const pick = (list, i) => list[i % list.length];

function state() {
    const names = ['艾琳', '洛岚', '维克托', '赛菲娅', '诺德', '米拉', '卡修斯', '伊芙'];
    const places = ['北境哨所', '灰石镇', '雾港', '黑松林', '旧王陵', '赤砂集市', '星落湖', '断桥关', '鸦巢', '银矿坑'];
    const characters = names.slice(0, 5).map((name, i) => ({
        name, title: pick(['佣兵', '游侠', '学者', '祭司', '盗贼'], i), level: 12 + i * 3, element: i % ELEMENTS.length,
        hp: [320 - i * 20, 400], mp: [80 + i * 15, 160], sp: [55 + i * 5, 100],
        attrs: Object.fromEntries(['力量', '敏捷', '体质', '智力', '感知', '魅力', '幸运'].map((attr, a) => [attr, 8 + ((i * 7 + a * 3) % 11)])),
        status: Array.from({ length: 3 }, (_, s) => pick(['轻伤', '疲惫', '鼓舞', '中毒', '护盾', '专注', '饥饿'], i + s)),
        equipment: Array.from({ length: 6 }, (_, s) => `${pick(['铁', '银', '秘银', '龙鳞', '星辉'], i + s)}${pick(['长剑', '短弓', '法杖', '圆盾', '皮甲', '戒指'], s)}`),
        note: `${name}在${pick(places, i)}接下委托，正在追查失踪的商队。她的判断一向冷静，但这次线索断在了雾里。`,
    }));
    const items = Array.from({ length: 100 }, (_, i) => ({
        name: `${pick(['回复', '解毒', '照明', '烟雾', '火焰', '冰霜', '迷踪'], i)}${pick(['药水', '卷轴', '符石', '粉末', '晶片'], i * 3)}`,
        rarity: (i * 5) % RARITIES.length, count: 1 + (i * 7) % 12, element: i % ELEMENTS.length,
        text: `${pick(places, i)}的${pick(['工匠', '药师', '商人', '旧友'], i)}留下的物品，使用后${pick(['恢复体力', '驱散黑暗', '短暂隐身', '抵御寒冷', '标记敌人'], i)}。`,
    }));
    const quests = Array.from({ length: 12 }, (_, i) => ({
        name: `${pick(['失踪的商队', '旧王陵的钟声', '雾港密信', '黑松林狼群', '银矿坑塌方', '星落湖祭典'], i)}${i >= 6 ? '（续）' : ''}`,
        rarity: i % RARITIES.length, done: i % 3 === 0,
        steps: Array.from({ length: 5 }, (_, s) => ({ text: `在${pick(places, i + s)}${pick(['打听消息', '寻找证物', '护送证人', '清理魔物', '交付报酬'], s)}`, done: s < i % 5 })),
    }));
    const map = places.flatMap((place, i) => Array.from({ length: 3 }, (_, d) => ({
        name: `${place}${pick(['', '东区', '地下'], d)}`, found: (i + d) % 4 !== 0, danger: (i * 3 + d) % 5, element: (i + d) % ELEMENTS.length,
    })));
    const bonds = names.map((name, i) => ({ name, love: 20 + (i * 11) % 70, trust: 30 + (i * 17) % 60, mood: pick(['平静', '好奇', '警惕', '愉快', '忧虑'], i) }));
    const log = Array.from({ length: 72 }, (_, i) => ({
        day: 1 + Math.floor(i / 4), text: `${pick(names, i)}在${pick(places, i * 3)}${pick(['发现了新的线索', '与守卫起了争执', '买下了一张旧地图', '击退了夜袭的狼群', '收到一封没有署名的信'], i)}。`,
    }));
    return { world: { date: '霜月第十二日', time: '黄昏', place: '雾港', weather: '小雨' }, characters, items, quests, map, bonds, log };
}

function schema() {
    const sentence = (subject, i) => `${subject}${pick(['影响战斗中的命中与闪避', '决定旅途中能携带的负重', '关系到交涉时对方的态度', '在夜间与雾中尤为重要', '会随休息与饮食缓慢恢复'], i)}，`
        + `${pick(['数值越高，相关检定越容易成功', '低于阈值时会触发负面状态', '与装备和祝福叠加计算', '在剧情关键处会被单独检定'], i * 3)}。`;
    const field = (label, i) => ({ label, base: 8 + i % 5, max: 20, desc: sentence(label, i) + sentence(label, i + 2) });
    return {
        attrs: Object.fromEntries(['力量', '敏捷', '体质', '智力', '感知', '魅力', '幸运'].map((label, i) => [label, field(label, i)])),
        status: Object.fromEntries(['轻伤', '疲惫', '鼓舞', '中毒', '护盾', '专注', '饥饿', '流血', '恐惧', '祝福', '隐匿', '冻伤', '灼烧', '眩晕', '迟缓', '亢奋', '虚弱', '清醒', '迷惑', '守护']
            .map((label, i) => [label, { kind: i % 3 ? 'debuff' : 'buff', turns: 1 + i % 4, desc: sentence(label, i) }])),
        elements: ELEMENTS.map((label, i) => ({ label, strong: ELEMENTS[(i + 1) % ELEMENTS.length], weak: ELEMENTS[(i + 3) % ELEMENTS.length], desc: sentence(`${label}属性`, i) + sentence(`${label}属性`, i + 1) })),
        rarities: RARITIES.map((label, i) => ({ label, weight: 60 >> i, desc: sentence(`${label}品质`, i) })),
        help: Array.from({ length: 48 }, (_, i) => ({ id: `tip-${i}`, text: sentence(pick(['标签页', '数值条', '装备栏', '任务步骤', '地图标记', '好感度', '日志'], i), i) + sentence('提示', i + 1) })),
    };
}

const SCRIPT = `
const S = JSON.parse(document.getElementById('state').textContent);
const SCHEMA = ${JSON.stringify(schema(), null, 1)};
for (const c of S.characters) for (const [name, field] of Object.entries(SCHEMA.attrs)) c.attrs[name] = Math.min(field.max, c.attrs[name] ?? field.base);
const RARITIES = ${JSON.stringify(RARITIES)};
const ELEMENTS = ${JSON.stringify(ELEMENTS)};
const esc = text => String(text).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const pct = ([value, max]) => Math.max(0, Math.min(100, Math.round(value / max * 100)));
const bar = (kind, pair) => \`<div class="row"><small>\${kind.toUpperCase()}</small><div class="bar \${kind} grow"><i style="width:\${pct(pair)}%"></i></div><small>\${pair[0]}/\${pair[1]}</small></div>\`;
const tags = list => list.map(text => \`<span class="tag" title="\${esc(SCHEMA.status[text]?.desc ?? '')}">\${esc(text)}</span>\`).join(' ');
const pages = {
  角色: () => \`<div class="grid">\${S.characters.map((c, i) => \`
    <section class="card r\${i % RARITIES.length} e\${c.element}">
      <header><h4>\${esc(c.name)} <span class="el">\${ELEMENTS[c.element]}</span></h4><small>Lv.\${c.level} \${esc(c.title)}</small></header>
      \${bar('hp', c.hp)}\${bar('mp', c.mp)}\${bar('sp', c.sp)}
      <table class="attrs">\${Object.entries(c.attrs).map(([k, v]) => \`<tr title="\${esc(SCHEMA.attrs[k].desc)}"><td>\${k}</td><td>\${v}</td></tr>\`).join('')}</table>
      <p>\${tags(c.status)}</p>
      <details class="fold"><summary>装备</summary><ul>\${c.equipment.map(e => \`<li>\${esc(e)}</li>\`).join('')}</ul></details>
      <p>\${esc(c.note)}</p>
    </section>\`).join('')}</div>\`,
  背包: () => \`<input class="search" placeholder="搜索物品"><div class="grid items">\${S.items.map(item => \`
    <section class="item r\${item.rarity} e\${item.element}" data-name="\${esc(item.name)}">
      <header><h4>\${esc(item.name)}</h4><small>×\${item.count}</small></header>
      <p><span class="el">\${ELEMENTS[item.element]}</span> · \${RARITIES[item.rarity]}</p><p>\${esc(item.text)}</p>
    </section>\`).join('')}</div>\`,
  任务: () => S.quests.map(q => \`
    <section class="quest r\${q.rarity}">
      <header><h4>\${esc(q.name)}</h4><small>\${q.done ? '已完成' : '进行中'}</small></header>
      <ol>\${q.steps.map(s => \`<li\${s.done ? ' class="done"' : ''}>\${esc(s.text)}</li>\`).join('')}</ol>
    </section>\`).join(''),
  地图: () => \`<div class="grid">\${S.map.map((m, i) => \`
    <section class="place r\${m.danger} e\${m.element}"><header><h4>\${m.found ? esc(m.name) : '？？？'}</h4><small>危险 \${'★'.repeat(m.danger + 1)}</small></header>
      <p>\${m.found ? '已探索' : '尚未发现'} · <span class="el">\${ELEMENTS[m.element]}</span></p></section>\`).join('')}</div>\`,
  关系: () => \`<div class="grid">\${S.bonds.map((b, i) => \`
    <section class="bond r\${i % RARITIES.length}"><header><h4>\${esc(b.name)}</h4><small>\${esc(b.mood)}</small></header>
      \${bar('love', [b.love, 100])}\${bar('sp', [b.trust, 100])}</section>\`).join('')}</div>\`,
  日志: () => S.log.map(e => \`<section class="log"><header><h4>第\${e.day}天</h4></header><p>\${esc(e.text)}</p></section>\`).join(''),
};
const app = document.getElementById('app');
const w = S.world;
app.innerHTML = \`<div class="sb">
  <div class="sb-head">\${[['日期', w.date], ['时间', w.time], ['地点', w.place], ['天气', w.weather]].map(([k, v]) => \`<div class="cell"><b>\${k}</b><span>\${esc(v)}</span></div>\`).join('')}</div>
  <nav class="sb-tabs">\${Object.keys(pages).map((name, i) => \`<button class="sb-tab\${i ? '' : ' on'}" data-page="\${name}">\${name}</button>\`).join('')}</nav>
  \${Object.entries(pages).map(([name, render], i) => \`<div class="sb-page\${i ? '' : ' on'}" data-page="\${name}">\${render()}</div>\`).join('')}
</div>\`;
app.querySelector('.sb-tabs').addEventListener('click', event => {
  const tab = event.target.closest('.sb-tab');
  if (!tab) return;
  for (const node of app.querySelectorAll('.sb-tab, .sb-page')) node.classList.toggle('on', node.dataset.page === tab.dataset.page);
});
app.querySelector('.search').addEventListener('input', event => {
  const query = event.target.value.trim();
  for (const item of app.querySelectorAll('.items > .item')) item.hidden = Boolean(query) && !item.dataset.name.includes(query);
});
`;

/** The status bar document, fenced as the regex that replaces the placeholder writes it. */
export function statusBarDocument() {
    return `\`\`\`html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<style>
${css()}
</style>
</head>
<body>
<div id="app"></div>
<script type="application/json" id="state">${JSON.stringify(state(), null, 1)}</script>
<script>${SCRIPT}</script>
</body>
</html>
\`\`\``;
}
