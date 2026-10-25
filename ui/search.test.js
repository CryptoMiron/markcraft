import { test, expect } from 'bun:test'
import { EditorState } from '@codemirror/state'
import { search, setSearchQuery, SearchQuery, replaceAll } from '@codemirror/search'

// Замена проверяется на настоящем состоянии CM: так ловится случай, когда
// наши расширения (декорации, история) мешают команде поиска.
const make = (doc) =>
  EditorState.create({ doc, extensions: [search({ top: true })] })

// Команды поиска отдают в dispatch спеку транзакции, а не готовую транзакцию.
const view = (getState, setState) => ({
  get state() {
    return getState()
  },
  dispatch: (spec) => setState(getState().update(spec).state),
})

const withQuery = (state, search, replace) =>
  state.update({
    effects: setSearchQuery.of(new SearchQuery({ search, replace })),
  }).state

test('заменить всё меняет все совпадения', () => {
  let state = make('цитата тут и цитата там\n')
  state = withQuery(state, 'цитата', 'ЗАМЕНА')
  expect(replaceAll(view(() => state, (s) => (state = s)))).toBe(true)
  expect(state.doc.toString()).toBe('ЗАМЕНА тут и ЗАМЕНА там\n')
})

// replaceNext не тестируется: ему нужен настоящий EditorView с панелью,
// а в bun без DOM его нет. Проверяется в живом окне.

test('без совпадений замена ничего не портит', () => {
  let state = make('ничего похожего\n')
  state = withQuery(state, 'цитата', 'ЗАМЕНА')
  replaceAll(view(() => state, (s) => (state = s)))
  expect(state.doc.toString()).toBe('ничего похожего\n')
})

test('пустой запрос не стирает документ', () => {
  let state = make('текст\n')
  state = withQuery(state, '', 'мусор')
  replaceAll(view(() => state, (s) => (state = s)))
  expect(state.doc.toString()).toBe('текст\n')
})

test('поиск с учётом регистра', () => {
  let state = make('Цитата и цитата\n')
  state = withQuery(state, 'цитата', 'X')
  state = state.update({
    effects: setSearchQuery.of(
      new SearchQuery({ search: 'цитата', replace: 'X', caseSensitive: true }),
    ),
  }).state
  replaceAll(view(() => state, (s) => (state = s)))
  expect(state.doc.toString()).toBe('Цитата и X\n')
})
