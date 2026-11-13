const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const vm = require('node:vm');
const ts = require('typescript');

const source = ts.transpileModule(readFileSync('src/views/widget.ts', 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;

function element() {
  const classes = new Set();
  return {
    dataset: { level: 'ok' }, style: {}, textContent: '', innerHTML: '', offsetWidth: 72,
    classList: {
      add(...items) { items.forEach(i => classes.add(i)); },
      remove(...items) { items.forEach(i => classes.delete(i)); },
      contains(i) { return classes.has(i); },
      toggle(i, on = !classes.has(i)) { if (on) classes.add(i); else classes.delete(i); },
    },
    setAttribute() {}, addEventListener() {},
  };
}

async function mount(claude = 40, codex = 47) {
  const events = new Map(), timers = new Map(), frames = new Map(), elements = new Map();
  let id = 0, now = 0;
  const usage = (percent, hours) => [{ key: 'five_hour', label: 'Session', utilization: percent,
    resetsAt: new Date(Date.now() + hours * 3600000) }];
  const snapshot = { usage: usage(claude, 2), codex: { usage: usage(codex, 5) } };
  const settings = { provider: 'claude', widgetMetric: 'five_hour' };
  const root = { innerHTML: '', querySelector(sel) {
    if (!elements.has(sel)) elements.set(sel, element());
    return elements.get(sel);
  } };
  const mocks = {
    '../lib/provider': { providerSnapshot: (s, p) => p === 'codex' ? s.codex : s },
    '../lib/ipc': { invoke: async cmd => cmd === 'get_settings' ? settings : snapshot,
      listen: (name, fn) => events.set(name, fn) },
    '../lib/normalize': { limitWindows: u => u ?? [], serviceIssue: () => null },
    '../lib/format': { when: () => '', pct: n => `${n}%` },
    '../lib/brand': { claudeMark: () => '', providerMark: p => p },
  };
  const context = { exports: {}, require: name => mocks[name], Date, Math,
    performance: { now: () => now },
    requestAnimationFrame(fn) { frames.set(++id, fn); return id; },
    cancelAnimationFrame: key => frames.delete(key), clearTimeout: key => timers.delete(key),
    window: { addEventListener() {}, setTimeout(fn, delay) { timers.set(++id, { fn, delay }); return id; } },
  };
  vm.runInNewContext(source, context);
  context.exports.mountWidget(root);
  await new Promise(resolve => setImmediate(resolve));
  return {
    snapshot, settings, elements,
    number: () => elements.get('.w-n-main .v').textContent,
    switchTo(provider, metric = 'five_hour') {
      events.get('settings')({ ...settings, provider, widgetMetric: metric });
    },
    refresh() { events.get('snapshot')(snapshot); },
    finishFrames() {
      now += 1000;
      const pending = [...frames.values()]; frames.clear(); pending.forEach(fn => fn(now));
    },
    nextTimer() {
      assert.equal(timers.size, 1);
      const [key, timer] = [...timers][0]; timers.delete(key); timer.fn();
    },
  };
}

test('switching back after either provider resets synchronizes number and ring to zero', async () => {
  for (const provider of ['claude', 'codex']) {
    const h = await mount();
    const other = provider === 'claude' ? 'codex' : 'claude';
    h.switchTo(other); h.finishFrames();
    assert.notEqual(h.number(), '0');
    (provider === 'claude' ? h.snapshot : h.snapshot.codex).usage[0].utilization = 0;
    h.refresh(); h.switchTo(provider);
    assert.equal(h.number(), '0');
    assert.equal(Number(h.elements.get('.w-arc').style.strokeDashoffset), 2 * Math.PI * 33);
    h.finishFrames();
    assert.equal(h.number(), '0');
  }
});

test('zero is displayed on startup, after missing data, and on a metric switch', async () => {
  const h = await mount(0, 0);
  assert.equal(h.number(), '0');
  const saved = h.snapshot.usage;
  h.snapshot.usage = null; h.refresh();
  h.snapshot.usage = saved; h.refresh();
  assert.equal(h.number(), '0');
  saved[0].utilization = 40; h.refresh(); h.finishFrames();
  saved.push({ ...saved[0], key: 'seven_day', utilization: 0 });
  h.switchTo('claude', 'seven_day');
  assert.equal(h.number(), '0');
});

test('a reset cancels a pending percentage animation', async () => {
  const h = await mount();
  h.snapshot.usage[0].utilization = 80; h.refresh();
  h.snapshot.usage[0].utilization = 0; h.refresh();
  assert.equal(h.number(), '0');
  h.finishFrames();
  assert.equal(h.number(), '0');
});

test('fractional usage is displayed without rounding up in both widget metrics', async () => {
  const h = await mount(20.7, 99.9);
  assert.equal(h.number(), '20');
  h.snapshot.usage.push({ ...h.snapshot.usage[0], key: 'seven_day', utilization: 4.9 });
  h.refresh();
  assert.equal(h.elements.get('.w-n-alt .v').textContent, '4');
  h.switchTo('codex');
  assert.equal(h.number(), '99');
});

test('switching during the countdown restarts rotation with the new provider', async () => {
  const h = await mount();
  h.nextTimer();
  assert.equal(h.elements.get('.w-n-time').textContent, '2h00');
  h.switchTo('codex');
  assert.equal(h.elements.get('.w').classList.contains('time'), false);
  h.nextTimer();
  assert.equal(h.elements.get('.w-n-time').textContent, '5h00');
});
