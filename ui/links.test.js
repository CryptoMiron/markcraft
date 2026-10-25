import { test, expect } from 'bun:test'
import { linkHrefAt } from './links.js'

const doc = [
  'Абзац [ссылка](https://example.com/a "t") и ещё.',
  '',
  'Автолинк <https://auto.example/x> в тексте.',
  '',
  'Голый https://bare.example/y?q=1 хвост.',
  '',
  'Обычный [текст] без адреса.',
].join('\n')

test('ссылка в скобках', () => {
  const pos = doc.indexOf('ссылка') + 2
  expect(linkHrefAt(doc, pos)).toBe('https://example.com/a')
})

test('клик по адресу внутри скобок тоже даёт адрес', () => {
  const pos = doc.indexOf('example.com/a') + 3
  expect(linkHrefAt(doc, pos)).toBe('https://example.com/a')
})

test('автолинк в угловых скобках', () => {
  const pos = doc.indexOf('auto.example') + 2
  expect(linkHrefAt(doc, pos)).toBe('https://auto.example/x')
})

test('голый адрес', () => {
  const pos = doc.indexOf('bare.example') + 2
  expect(linkHrefAt(doc, pos)).toBe('https://bare.example/y?q=1')
})

test('обычный текст — не ссылка', () => {
  expect(linkHrefAt(doc, doc.indexOf('Обычный') + 2)).toBeNull()
})

test('ссылка без адреса не притворяется ссылкой', () => {
  expect(linkHrefAt(doc, doc.indexOf('[текст]') + 2)).toBeNull()
})
