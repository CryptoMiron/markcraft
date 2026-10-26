import { test, expect } from 'bun:test'
import { EditorState } from '@codemirror/state'
import { syntaxTree } from '@codemirror/language'
import { markdown } from '@codemirror/lang-markdown'
import { codeBody } from './code-block.js'

// Тело блока ищется на настоящем дереве lezer: так проверяются и ограда,
// и отступ отступного кода, и незакрытый блок.
const body = (src, kind) => {
  const state = EditorState.create({ doc: src, extensions: [markdown()] })
  let found = null
  syntaxTree(state).iterate({
    enter: (n) => {
      if (!found && n.name === kind) found = codeBody(state.doc, n.node)
    },
  })
  return found
}

test('ограда с языком: тело без ограды', () => {
  expect(body('```js\na\nb\n```\n', 'FencedCode')).toEqual({
    text: 'a\nb',
    first: 2,
    last: 3,
  })
})

test('тильды: тело без ограды', () => {
  expect(body('~~~\nx\n~~~\n', 'FencedCode')).toEqual({ text: 'x', first: 2, last: 2 })
})

test('отступной код: отступ в тело не попадает', () => {
  expect(body('    a\n    b\n', 'CodeBlock')).toEqual({ text: 'a\nb', first: 1, last: 2 })
})

test('незакрытая ограда: тело до конца файла', () => {
  expect(body('```js\na\n', 'FencedCode')).toEqual({ text: 'a', first: 2, last: 2 })
})

test('пустой блок не даёт тела', () => {
  expect(body('```\n```\n', 'FencedCode')).toBeNull()
})

test('блок без текста, но с пустой строкой остаётся блоком', () => {
  expect(body('```\n\n```\n', 'FencedCode')).toEqual({ text: '', first: 2, last: 2 })
})
