const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const vm = require('node:vm');
const ts = require('typescript');

function load(path, dependencies = {}) {
  const code = ts.transpileModule(readFileSync(path, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const context = { exports: {}, require: name => dependencies[name] };
  vm.runInNewContext(code, context);
  return context.exports;
}

const format = load('src/lib/format.ts');
const charts = load('src/lib/charts.ts', { './format': format });
const normalize = load('src/lib/normalize.ts', { './format': format });

test('usage labels truncate decimals, including values near zero and full usage', () => {
  for (const [input, expected] of [[0, '0%'], [0.9, '0%'], [20.7, '20%'], [99.9, '99%'], [100, '100%']]) {
    assert.equal(format.pct(input), expected);
  }
});

test('panel gauge and its accessible label use the same truncated percentage', () => {
  const html = charts.gauge(20.7, 'ok', null, 'Session');
  assert.ok(html.includes('aria-label="Session: 20%"'));
  assert.ok(html.includes('<b>20<small>%</small></b>'));
});

test('weekly all-model and Fable limits, including their shares, never round up', () => {
  for (const payload of [
    { seven_day: { utilization: 20.7 }, seven_day_fable: { utilization: 4.9 } },
    { limits: [
      { kind: 'weekly_all', percent: 20.7 },
      { kind: 'weekly_scoped', group: 'weekly', percent: 4.9, scope: { model: { display_name: 'Fable' } } },
    ], weekly_scoped_shares: [{ limit_index: 1, allowance_percent_of_weekly: 50.9, used_percent_of_weekly: 3.8 }] },
  ]) {
    const windows = normalize.limitWindows(payload);
    const weekly = windows.find(w => w.key === 'seven_day');
    const fable = windows.find(w => w.short === 'Fable');
    assert.equal(format.pct(weekly.utilization), '20%');
    assert.equal(format.pct(fable.utilization), '4%');
    // Keep raw precision for progress bars and calculations.
    assert.equal(weekly.utilization, 20.7);
    assert.equal(fable.utilization, 4.9);
    if (fable.note) {
      assert.ok(fable.note.includes('50%'));
      assert.ok(fable.note.includes('3%'));
    }
  }
});
