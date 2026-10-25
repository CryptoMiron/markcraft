import { test, expect } from 'bun:test'
import { nextLinePrefix, toggleWrap, wordAt, insertLink, existingLink } from './editing.js'

// ─── Enter в списках ────────────────────────────────────────────────────────

test('маркированный список продолжается тем же маркером', () => {
  expect(nextLinePrefix('- пункт')).toEqual({ prefix: '- ' })
  expect(nextLinePrefix('* пункт')).toEqual({ prefix: '* ' })
  expect(nextLinePrefix('+ пункт')).toEqual({ prefix: '+ ' })
})

test('отступ вложенного пункта сохраняется', () => {
  expect(nextLinePrefix('  - вложенный')).toEqual({ prefix: '  - ' })
})

test('нумерованный список увеличивает номер', () => {
  expect(nextLinePrefix('1. первый')).toEqual({ prefix: '2. ' })
  expect(nextLinePrefix('9. девятый')).toEqual({ prefix: '10. ' })
  expect(nextLinePrefix('3) третий')).toEqual({ prefix: '4) ' })
})

test('задача продолжается невыполненной', () => {
  expect(nextLinePrefix('- [ ] задача')).toEqual({ prefix: '- [ ] ' })
  expect(nextLinePrefix('- [x] сделано')).toEqual({ prefix: '- [ ] ' })
  expect(nextLinePrefix('  - [X] вложенная')).toEqual({ prefix: '  - [ ] ' })
})

test('пустой пункт выходит из списка', () => {
  expect(nextLinePrefix('- ')).toEqual({ clear: true })
  expect(nextLinePrefix('1. ')).toEqual({ clear: true })
  expect(nextLinePrefix('> ')).toEqual({ clear: true })
  expect(nextLinePrefix('  - [ ] ')).toEqual({ clear: true })
})

test('цитата продолжается', () => {
  expect(nextLinePrefix('> цитата')).toEqual({ prefix: '> ' })
  expect(nextLinePrefix('>цитата')).toEqual({ prefix: '>' })
})

test('обычный абзац не получает префикса', () => {
  expect(nextLinePrefix('просто текст')).toBeNull()
  expect(nextLinePrefix('')).toBeNull()
  expect(nextLinePrefix('-не список')).toBeNull()
  expect(nextLinePrefix('1.5 не список')).toBeNull()
})

// ─── Оборачивание маркером ──────────────────────────────────────────────────

const doc = 'тут жирное слово и курсив'

test('жирный: выделение оборачивается', () => {
  const from = doc.indexOf('жирное')
  const to = from + 'жирное'.length
  expect(toggleWrap(doc, from, to, '**')).toEqual({
    from,
    to,
    insert: '**жирное**',
    anchor: from + 2,
    head: to + 2,
  })
})

test('жирный: повторное нажатие снимает обёртку', () => {
  const wrapped = 'тут **жирное** слово'
  const from = wrapped.indexOf('жирное')
  const to = from + 'жирное'.length
  expect(toggleWrap(wrapped, from, to, '**')).toEqual({
    from: from - 2,
    to: to + 2,
    insert: 'жирное',
    anchor: from - 2,
    head: to - 2,
  })
})

test('жирный: маркеры внутри выделения тоже снимаются', () => {
  const wrapped = 'тут **жирное** слово'
  const from = wrapped.indexOf('**')
  const to = from + '**жирное**'.length
  expect(toggleWrap(wrapped, from, to, '**').insert).toBe('жирное')
})

test('без выделения оборачивается слово под курсором', () => {
  const pos = doc.indexOf('курсив') + 2
  expect(toggleWrap(doc, pos, pos, '*')).toEqual({
    from: doc.indexOf('курсив'),
    to: doc.indexOf('курсив') + 'курсив'.length,
    insert: '*курсив*',
    anchor: doc.indexOf('курсив') + 1,
    head: doc.indexOf('курсив') + 1 + 'курсив'.length,
  })
})

test('без выделения и без слова вставляются два маркера', () => {
  const spaced = 'тут жирное слово и курсив '
  const pos = spaced.length
  expect(toggleWrap(spaced, pos, pos, '~~')).toEqual({
    from: pos,
    to: pos,
    insert: '~~~~',
    anchor: pos + 2,
    head: pos + 2,
  })
})

test('курсив по звёздочке не путается с жирным', () => {
  const wrapped = 'тут *курсив* слово'
  const from = wrapped.indexOf('курсив')
  const to = from + 'курсив'.length
  expect(toggleWrap(wrapped, from, to, '*').insert).toBe('курсив')
})

// ─── Слово под курсором ─────────────────────────────────────────────────────

test('слово под курсором', () => {
  expect(wordAt('привет мир', 3)).toEqual({ from: 0, to: 6 })
})

test('курсор между пробелами — слова нет', () => {
  expect(wordAt('привет  мир', 7)).toBeNull()
})

test('курсор в конце слова оборачивает это слово', () => {
  expect(wordAt('привет мир', 6)).toEqual({ from: 0, to: 6 })
})

test('маркер разметки границей слова считается', () => {
  expect(wordAt('**жирное**', 5)).toEqual({ from: 2, to: 8 })
})

// ─── Ссылка ─────────────────────────────────────────────────────────────────

test('Ctrl+K: выделение становится подписью, курсор на адресе', () => {
  const from = doc.indexOf('жирное')
  const to = from + 'жирное'.length
  expect(insertLink(doc, from, to)).toEqual({
    from,
    to,
    insert: '[жирное](https://)',
    anchor: from + 'жирное'.length + 3,
    head: from + 'жирное'.length + 3 + 'https://'.length,
  })
})

test('Ctrl+K без выделения вставляет подпись-заглушку', () => {
  const pos = doc.length
  expect(insertLink(doc, pos, pos).insert).toBe('[текст](https://)')
})

// ─── Уже существующая ссылка ────────────────────────────────────────────────

test('Ctrl+K внутри ссылки находит адрес', () => {
  const text = 'тут [подпись](https://a.b/c) хвост'
  const pos = text.indexOf('подпись') + 2
  expect(existingLink(text, pos)).toEqual({
    urlFrom: text.indexOf('https://a.b/c'),
    urlTo: text.indexOf('https://a.b/c') + 'https://a.b/c'.length,
  })
})

test('Ctrl+K на самом адресе тоже находит ссылку', () => {
  const text = 'тут [подпись](https://a.b/c) хвост'
  const pos = text.indexOf('a.b') + 1
  expect(existingLink(text, pos)).not.toBeNull()
})

test('вне ссылки ничего не находит', () => {
  const text = 'тут [подпись](https://a.b/c) хвост'
  expect(existingLink(text, text.indexOf('хвост'))).toBeNull()
  expect(existingLink('просто текст', 3)).toBeNull()
})

test('пустой адрес тоже распознаётся', () => {
  const text = '[текст](https://)'
  const link = existingLink(text, 3)
  expect(link).toEqual({ urlFrom: 8, urlTo: 16 })
})
