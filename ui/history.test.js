import { test, expect } from 'bun:test'
import { EditorState, Transaction } from '@codemirror/state'
import { history, undoDepth } from '@codemirror/commands'

const make = (doc) => EditorState.create({ doc, extensions: [history()] })

const load = (state, text) =>
  state.update({
    changes: { from: 0, to: state.doc.length, insert: text },
    annotations: Transaction.addToHistory.of(false),
  }).state

const type = (state, text) =>
  state.update({ changes: { from: state.doc.length, insert: text } }).state

test('загрузка файла не попадает в историю', () => {
  let state = make('')
  state = load(state, '# Документ\n\nтекст\n')
  expect(undoDepth(state)).toBe(0)
})

test('первый Ctrl+Z после загрузки не стирает документ', () => {
  let state = make('')
  state = load(state, '# Документ\n')
  // Истории нет — откатывать нечего, документ остаётся на месте.
  expect(undoDepth(state)).toBe(0)
  expect(state.doc.toString()).toBe('# Документ\n')
})

test('набор после загрузки откатывается, загрузка — нет', () => {
  let state = make('')
  state = load(state, 'начало\n')
  state = type(state, 'ещё')
  expect(undoDepth(state)).toBe(1)
  expect(state.doc.toString()).toBe('начало\nещё')
})
