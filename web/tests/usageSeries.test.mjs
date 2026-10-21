import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import ts from 'typescript'

const source = readFileSync(new URL('../src/utils/usageSeries.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } })
const { usageSeries } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)

test('merges providers by client and model across dates into model bars for each client', () => {
  const rows = [
    { date: '2026-10-20', client: 'cli', model_id: 'shared', provider_id: 'a', request_count: 2, input_tokens: 100, output_tokens: 20 },
    { date: '2026-10-20', client: 'cli', model_id: 'shared', provider_id: 'b', request_count: 3, input_tokens: 200, output_tokens: 30 },
    { date: '2026-10-21', client: 'cli', model_id: 'shared', provider_id: 'b', request_count: 1, input_tokens: 10, output_tokens: 2 },
    { date: '2026-10-20', client: 'other', model_id: 'shared', provider_id: 'a', request_count: 4, input_tokens: 40, output_tokens: 4 },
    { date: '2026-10-20', client: 'cli', model_id: 'different', provider_id: 'a', request_count: 1, input_tokens: 50, output_tokens: 5 }
  ]
  const series = usageSeries(rows, ['2026-10-19', '2026-10-20', '2026-10-21'])
  assert.deepEqual(series.clients, ['cli', 'other'])
  assert.equal(series.series.length, 2)
  const merged = series.series.find(row => row.model_id === 'shared')
  assert.deepEqual(merged.requests, [6, 4])
  assert.deepEqual(merged.tokens, [362, 44])
  assert.equal(merged.request_count, 10)
  assert.deepEqual(series.series.find(row => row.model_id === 'different').requests, [1, 0])
  assert.equal(rows[0].provider_id, 'a')
  assert.equal(rows.length, 5)
})

test('keeps unknown clients and models with delimiter characters distinct', () => {
  const rows = [
    { date: 'd', client: '', model_id: 'a/b', request_count: 1, input_tokens: 1, output_tokens: 1 },
    { date: 'd', client: 'a/b', model_id: 'c', request_count: 2, input_tokens: 2, output_tokens: 2 },
    { date: 'd', client: 'a', model_id: 'b/c', request_count: 3, input_tokens: 3, output_tokens: 3 }
  ]
  const chart = usageSeries(rows, ['d'])
  assert.deepEqual(chart.clients, ['a', 'a/b', ''])
  assert.equal(chart.series.length, 3)
  assert.deepEqual(chart.series.find(row => row.model_id === 'a/b').requests, [0, 0, 1])
  assert.deepEqual(usageSeries([], ['d']), { clients: [], series: [] })
})

test('excludes records outside the selected date window', () => {
  const row = { date: 'old', client: 'cli', model_id: 'm', request_count: 100, input_tokens: 100, output_tokens: 20 }
  assert.deepEqual(usageSeries([row], ['today']), { clients: [], series: [] })
})
