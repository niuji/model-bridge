import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import ts from 'typescript'

const source = readFileSync(new URL('../src/utils/balancedColumns.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } })
const { balancedColumns } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
const sum = items => items.reduce((total, item) => total + item, 0)

function exhaustiveDifference(weights) {
  let best = Infinity
  for (let mask = 0; mask < 2 ** weights.length; mask++) {
    let left = 0
    for (let i = 0; i < weights.length; i++) if (mask & (2 ** i)) left += weights[i]
    best = Math.min(best, Math.abs(sum(weights) - 2 * left))
  }
  return best
}

test('finds an exact balance where tallest-first greedy leaves a gap', () => {
  const columns = balancedColumns([8, 7, 6, 5, 4], value => value)
  assert.deepEqual(columns.map(sum), [15, 15])
  assert.deepEqual(columns.flat().sort((a, b) => a - b), [4, 5, 6, 7, 8])
})

test('matches exhaustive optimum for estimated card heights and keeps every provider once', () => {
  for (const counts of [[], [7], [1, 3, 7], [2, 2, 2, 2], [2, 5, 1, 8, 3, 6], [1, 1, 20, 4, 3, 8, 6]]) {
    const cards = counts.map((count, index) => ({ id: index, weight: 74 + count * 48 }))
    const columns = balancedColumns(cards, card => card.weight)
    const totals = columns.map(column => sum(column.map(card => card.weight)))
    assert.equal(Math.abs(totals[0] - totals[1]), exhaustiveDifference(cards.map(card => card.weight)))
    assert.deepEqual(columns.flat().map(card => card.id).sort((a, b) => a - b), cards.map(card => card.id))
  }
})

test('handles larger provider lists without exponential enumeration', () => {
  const cards = Array.from({ length: 40 }, (_, id) => ({ id, weight: 122 }))
  const columns = balancedColumns(cards, card => card.weight)
  assert.deepEqual(columns.map(column => column.length), [20, 20])
  assert.equal(new Set(columns.flat()).size, 40)
})

test('places the taller column on the left when the difference exceeds one model row', () => {
  const columns = balancedColumns([150, 100, 100], value => value, 48)
  assert.deepEqual(columns.map(sum), [200, 150])
})

test('places fewer cards on the left within the close-height threshold', () => {
  for (const weights of [[95, 95, 150], [99, 99, 150], [75, 75, 150]]) {
    const columns = balancedColumns(weights, value => value, 48)
    assert.deepEqual(columns.map(column => column.length), [1, 2])
    assert.equal(sum(columns[0]), 150)
  }
})

test('uses height to break equal card counts and puts a single card on the left', () => {
  assert.deepEqual(balancedColumns([100, 120], value => value, 48).map(sum), [120, 100])
  assert.deepEqual(balancedColumns([122], value => value, 48), [[122], []])
})
