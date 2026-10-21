import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import ts from 'typescript'

const source = readFileSync(new URL('../src/utils/centeredBars.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } })
const { centeredBar } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)

test('centers a single active model regardless of its global series index', () => {
  const bar = centeredBar(4, [4], 160)
  assert.equal(bar.offset + bar.width / 2, 0)
})

test('centers only the active models and closes gaps after a model is hidden', () => {
  const left = centeredBar(0, [0, 2, 4], 160)
  const middle = centeredBar(2, [0, 2, 4], 160)
  const right = centeredBar(4, [0, 2, 4], 160)
  assert.equal(middle.offset + middle.width / 2, 0)
  assert.equal(left.offset + right.offset + right.width, 0)
  const afterHide = centeredBar(4, [0, 4], 160)
  assert.ok(afterHide.offset < right.offset)
  assert.equal(centeredBar(2, [0, 4], 160), null)
})

test('fits a large model group inside the client category without overlap', () => {
  const active = Array.from({ length: 20 }, (_, index) => index)
  const bars = active.map(index => centeredBar(index, active, 100))
  assert.ok(bars[0].offset >= -50)
  assert.ok(bars.at(-1).offset + bars.at(-1).width <= 50)
  for (let i = 1; i < bars.length; i++) assert.ok(bars[i].offset > bars[i - 1].offset + bars[i - 1].width)
})
