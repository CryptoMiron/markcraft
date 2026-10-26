import { test, expect } from 'bun:test'
import { findFrontmatter } from './frontmatter.js'

/// Минимальная замена Text из CM6: то, что нужно findFrontmatter.
function docOf(text) {
  const lines = text.split('\n')
  const starts = []
  let pos = 0
  for (const line of lines) {
    starts.push(pos)
    pos += line.length + 1
  }
  return {
    length: text.length,
    lines: lines.length,
    line: (n) => ({ from: starts[n - 1], to: starts[n - 1] + lines[n - 1].length, text: lines[n - 1] }),
  }
}

const skill = ['---', 'name: good-readme', 'description: test', '---', '', '# Заголовок'].join('\n')

test('шапка в начале файла распознаётся', () => {
  const fm = findFrontmatter(docOf(skill))
  expect(fm).not.toBeNull()
  expect(fm.first).toBe(1)
  expect(fm.last).toBe(4)
  expect(fm.from).toBe(0)
  expect(fm.to).toBe('---\nname: good-readme\ndescription: test\n---'.length)
})

test('три точки закрывают шапку так же, как черта', () => {
  const fm = findFrontmatter(docOf(['---', 'name: x', '...', '', 'текст'].join('\n')))
  expect(fm.last).toBe(3)
})

test('пустые строки внутри шапки не мешают', () => {
  const fm = findFrontmatter(docOf(['---', 'name: x', '', '---', 'текст'].join('\n')))
  expect(fm.last).toBe(4)
})

test('нет закрывающей черты — это не шапка', () => {
  expect(findFrontmatter(docOf(['---', 'name: x', 'текст'].join('\n')))).toBeNull()
})

test('шапка не в начале файла не считается', () => {
  expect(findFrontmatter(docOf(['текст', '', '---', 'name: x', '---'].join('\n')))).toBeNull()
})

test('горизонтальная черта в начале без YAML — не шапка', () => {
  expect(findFrontmatter(docOf(['---', '', 'Обычный текст', '', '---'].join('\n')))).toBeNull()
})

test('одна строка — шапки быть не может', () => {
  expect(findFrontmatter(docOf('---'))).toBeNull()
})

test('пустой документ', () => {
  expect(findFrontmatter(docOf(''))).toBeNull()
})

test('файл без шапки', () => {
  expect(findFrontmatter(docOf('# Заголовок\n\nтекст'))).toBeNull()
})
