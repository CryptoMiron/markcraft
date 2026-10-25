import { EditorState, Compartment, StateField } from '@codemirror/state'
import {
  EditorView,
  Decoration,
  WidgetType,
  drawSelection,
  keymap,
  placeholder,
} from '@codemirror/view'
import { syntaxTree } from '@codemirror/language'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import {
  StreamLanguage,
  LanguageDescription,
  LanguageSupport,
  syntaxHighlighting,
  HighlightStyle,
} from '@codemirror/language'
import { rust } from '@codemirror/legacy-modes/mode/rust'
import { python } from '@codemirror/legacy-modes/mode/python'
import { javascript, json } from '@codemirror/legacy-modes/mode/javascript'
import { shell } from '@codemirror/legacy-modes/mode/shell'
import { yaml } from '@codemirror/legacy-modes/mode/yaml'
import { toml } from '@codemirror/legacy-modes/mode/toml'
import { sql } from '@codemirror/legacy-modes/mode/sql'
import { go } from '@codemirror/legacy-modes/mode/go'
import { c, cpp } from '@codemirror/legacy-modes/mode/clike'
import { css } from '@codemirror/legacy-modes/mode/css'
import { xml } from '@codemirror/legacy-modes/mode/xml'
import { tags } from '@lezer/highlight'

// support передаётся сразу: иначе markdown ставит парсер-заглушку и ждёт
// асинхронной загрузки, а в бандле грузить нечего — она уже здесь.
const describe = (name, aliases, mode) => {
  const support = new LanguageSupport(StreamLanguage.define(mode))
  return LanguageDescription.of({
    name,
    alias: aliases,
    support,
    load: async () => support.language,
  })
}

const codeLanguages = [
  describe('Rust', ['rust', 'rs'], rust),
  describe('Python', ['python', 'py'], python),
  describe('JavaScript', ['javascript', 'js', 'jsx', 'ts', 'tsx', 'typescript'], javascript),
  describe('JSON', ['json'], json),
  describe('Shell', ['sh', 'bash', 'shell', 'zsh'], shell),
  describe('YAML', ['yaml', 'yml'], yaml),
  describe('TOML', ['toml'], toml),
  describe('SQL', ['sql'], sql),
  describe('Go', ['go', 'golang'], go),
  describe('C', ['c', 'h'], c),
  describe('C++', ['cpp', 'c++', 'cc', 'hpp'], cpp),
  describe('CSS', ['css', 'scss', 'less'], css),
  describe('XML', ['xml', 'html', 'svg'], xml),
]

// Одна встроенная тема: цвета берутся из CSS-переменных, поэтому светлая
// и тёмная темы Omarchy красят код без пересборки.
const codeHighlight = syntaxHighlighting(
  HighlightStyle.define([
    { tag: tags.comment, class: 'tok-comment' },
    { tag: tags.lineComment, class: 'tok-comment' },
    { tag: tags.blockComment, class: 'tok-comment' },
    { tag: tags.string, class: 'tok-string' },
    { tag: tags.special(tags.string), class: 'tok-string' },
    { tag: tags.number, class: 'tok-number' },
    { tag: tags.bool, class: 'tok-number' },
    { tag: tags.null, class: 'tok-number' },
    { tag: tags.keyword, class: 'tok-keyword' },
    { tag: tags.controlKeyword, class: 'tok-keyword' },
    { tag: tags.definitionKeyword, class: 'tok-keyword' },
    { tag: tags.moduleKeyword, class: 'tok-keyword' },
    { tag: tags.typeName, class: 'tok-type' },
    { tag: tags.className, class: 'tok-type' },
    { tag: tags.function(tags.variableName), class: 'tok-func' },
    { tag: tags.definition(tags.variableName), class: 'tok-func' },
    { tag: tags.propertyName, class: 'tok-prop' },
    { tag: tags.operator, class: 'tok-op' },
    { tag: tags.punctuation, class: 'tok-punct' },
    { tag: tags.meta, class: 'tok-meta' },
  ]),
)
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { linkHrefAt } from './links.js'

// ─── Мост в Rust ────────────────────────────────────────────────────────────
const send = (type, extra = {}) =>
  window.webkit.messageHandlers.ipc.postMessage(JSON.stringify({ type, ...extra }))

// ─── Скрытие разметки ───────────────────────────────────────────────────────
// Правило живое-превью: маркер виден только там, где стоит курсор. Всё
// остальное отрисовано. Это дешёвая замена блочному WYSIWYG.

const hide = Decoration.replace({})

/// Активна ли строка: курсор или выделение её касается.
function lineActive(state, line) {
  for (const range of state.selection.ranges) {
    if (range.from <= line.to && range.to >= line.from) return true
  }
  return false
}

/// Внутри ли диапазона (для блоков вроде таблиц и кода).
function inRange(state, from, to) {
  for (const range of state.selection.ranges) {
    if (range.from < to && range.to > from) return true
    if (range.empty && range.from >= from && range.from <= to) return true
  }
  return false
}

class CheckboxWidget extends WidgetType {
  constructor(checked) {
    super()
    this.checked = checked
  }
  eq(other) {
    return other.checked === this.checked
  }
  toDOM() {
    const box = document.createElement('span')
    box.className = 'mc-task' + (this.checked ? ' mc-task-done' : '')
    box.textContent = this.checked ? '✓' : ''
    return box
  }
  ignoreEvent() {
    return false
  }
}

class ImageWidget extends WidgetType {
  constructor(src, alt) {
    super()
    this.src = src
    this.alt = alt
  }
  eq(other) {
    return other.src === this.src && other.alt === this.alt
  }
  toDOM() {
    const img = document.createElement('img')
    img.className = 'mc-img'
    img.alt = this.alt
    img.src = this.src
    img.title = this.alt
    img.onerror = () => {
      console.error('img failed: ' + img.src)
      const box = document.createElement('span')
      box.className = 'mc-img-missing'
      box.textContent = this.alt || this.src
      img.replaceWith(box)
    }
    return img
  }
  ignoreEvent() {
    return false
  }
}

class RuleWidget extends WidgetType {
  toDOM() {
    const hr = document.createElement('hr')
    hr.className = 'mc-hr'
    return hr
  }
}

class BulletWidget extends WidgetType {
  constructor(ordered, text) {
    super()
    this.ordered = ordered
    this.text = text
  }
  eq(other) {
    return other.ordered === this.ordered && other.text === this.text
  }
  toDOM() {
    const span = document.createElement('span')
    span.className = 'mc-bullet'
    span.textContent = this.text
    return span
  }
}

/// Разбор таблицы: только то, что нужно для отрисовки.
function parseTable(text) {
  const lines = text.split('\n').filter((l) => l.trim() !== '')
  if (lines.length < 2) return null
  const cells = (line) =>
    line
      .replace(/^\s*\|/, '')
      .replace(/\|\s*$/, '')
      .split('|')
      .map((c) => c.trim())
  const aligns = cells(lines[1]).map((c) => {
    const left = c.startsWith(':')
    const right = c.endsWith(':')
    if (left && right) return 'center'
    if (right) return 'right'
    if (left) return 'left'
    return ''
  })
  return { head: cells(lines[0]), aligns, rows: lines.slice(2).map(cells) }
}

class TableWidget extends WidgetType {
  constructor(text, pos) {
    super()
    this.text = text
    this.pos = pos
  }
  eq(other) {
    return other.text === this.text
  }
  toDOM(view) {
    const table = document.createElement('table')
    table.className = 'mc-table'
    const parsed = parseTable(this.text)
    if (!parsed) return table
    const head = table.createTHead().insertRow()
    parsed.head.forEach((cell, i) => {
      const th = document.createElement('th')
      th.textContent = cell
      if (parsed.aligns[i]) th.style.textAlign = parsed.aligns[i]
      head.append(th)
    })
    const body = table.createTBody()
    for (const row of parsed.rows) {
      const tr = body.insertRow()
      parsed.aligns.forEach((align, i) => {
        const td = tr.insertCell()
        td.textContent = row[i] ?? ''
        if (align) td.style.textAlign = align
      })
    }
    // Клик по таблице ставит курсор в её начало: иначе в неё не попасть.
    table.onclick = () => {
      view.dispatch({ selection: { anchor: this.pos }, scrollIntoView: true })
      view.focus()
    }
    return table
  }
  ignoreEvent() {
    return true
  }
}

/// Ищет границы блока по дереву: от начала строки первого узла до конца строки последнего.
function blockRange(state, node) {
  const doc = state.doc
  const first = doc.lineAt(node.from)
  const last = doc.lineAt(Math.max(node.from, node.to - 1))
  return { from: first.from, to: Math.min(last.to + 1, doc.length) }
}

function buildDecorations({ state }) {
  const doc = state.doc
  const ranges = []

  const add = (from, to, deco) => {
    if (to > from) ranges.push({ from, to, deco })
  }

  /// replace-декорации не могут пересекаться, mark — могут (курсив лежит
  /// поверх своих же звёздочек).
  const addReplace = (from, to, deco) => {
    if (to > from) ranges.push({ from, to, deco, replace: true })
  }

  const tree = syntaxTree(state)
  {
    tree.iterate({
      from: 0,
      to: doc.length,
      enter: (node) => {
        const name = node.name
        const line = doc.lineAt(node.from)
        const active = lineActive(state, line)

        switch (name) {
          case 'ATXHeading1':
          case 'ATXHeading2':
          case 'ATXHeading3':
          case 'ATXHeading4':
          case 'ATXHeading5':
          case 'ATXHeading6':
          case 'SetextHeading1':
          case 'SetextHeading2': {
            const level = name.slice(-1)
            ranges.push({
              from: line.from,
              to: line.from,
              deco: Decoration.line({ class: `mc-h${level}` }),
            })
            return
          }
          case 'HeaderMark': {
            if (active) return
            // Setext: маркер занимает отдельную строку «====» или «----».
            const own = doc.lineAt(node.from)
            if (node.to - node.from === own.length && /^[=-]+$/.test(doc.sliceString(node.from, node.to))) {
              addReplace(own.from, Math.min(own.to + 1, doc.length), hide)
              return
            }
            // Маркер + пробел после него, чтобы строка не начиналась с отступа.
            const after = doc.sliceString(node.to, node.to + 1)
            addReplace(node.from, after === ' ' ? node.to + 1 : node.to, hide)
            return
          }
          case 'StrongEmphasis': {
            ranges.push({ from: node.from, to: node.to, deco: Decoration.mark({ class: 'mc-strong' }) })
            return
          }
          case 'Emphasis': {
            ranges.push({ from: node.from, to: node.to, deco: Decoration.mark({ class: 'mc-em' }) })
            return
          }
          case 'Strikethrough': {
            ranges.push({ from: node.from, to: node.to, deco: Decoration.mark({ class: 'mc-strike' }) })
            return
          }
          case 'InlineCode': {
            ranges.push({ from: node.from, to: node.to, deco: Decoration.mark({ class: 'mc-code-inline' }) })
            return
          }
          case 'EmphasisMark':
          case 'StrikethroughMark': {
            if (active) return
            add(node.from, node.to, hide)
            return
          }
          case 'Link': {
            // Сноска `[^1]` — не ссылка, её маркеры не трогаем.
            if (doc.sliceString(node.from, node.from + 2) === '[^') return
            ranges.push({
              from: node.from,
              to: node.to,
              deco: Decoration.mark({ class: 'mc-link' }),
            })
            if (active) return
            // `[текст](url "title")` → виден только текст.
            const slice = doc.sliceString(node.from, node.to)
            if (doc.sliceString(node.from, node.from + 1) === '[')
              addReplace(node.from, node.from + 1, hide)
            const close = slice.lastIndexOf(']')
            if (close > 0) addReplace(node.from + close, node.to, hide)
            return
          }
          case 'LinkReference':
          case 'LinkLabel':
          case 'FootnoteReference':
          case 'FootnoteDefinition':
          case 'Footnote': {
            return
          }
          case 'Autolink': {
            ranges.push({
              from: node.from,
              to: node.to,
              deco: Decoration.mark({ class: 'mc-link' }),
            })
            return
          }
          case 'LinkMark':
          case 'URL':
          case 'LinkTitle': {
            return
          }
          case 'QuoteMark': {
            if (active) return
            const after = doc.sliceString(node.to, node.to + 1)
            addReplace(node.from, after === ' ' ? node.to + 1 : node.to, hide)
            return
          }
          case 'Blockquote': {
            const range = blockRange(state, node)
            for (let n = doc.lineAt(range.from).number; n <= doc.lineAt(range.to - 1).number; n++) {
              ranges.push({
                from: doc.line(n).from,
                to: doc.line(n).from,
                deco: Decoration.line({ class: 'mc-quote' }),
              })
            }
            return
          }
          case 'ListMark': {
            const text = doc.sliceString(node.from, node.to)
            if (!/^[-*+]$/.test(text)) return
            // У задачи на месте пули чекбокс: сам дефис тоже убираем.
            if (/^\s*\[[ xX]\]/.test(doc.sliceString(node.to, node.to + 8))) {
              addReplace(node.from, node.to + 1, hide)
              return
            }
            addReplace(node.from, node.to, Decoration.replace({ widget: new BulletWidget(false, '•') }))
            return
          }
          case 'TaskMarker': {
            const text = doc.sliceString(node.from, node.to)
            addReplace(node.from, node.to, Decoration.replace({ widget: new CheckboxWidget(/x/i.test(text)) }))
            return
          }
          case 'CodeMark': {
            // Ограду блока не трогаем — её убирает ветка FencedCode целиком.
            if (node.node.parent?.name !== 'InlineCode') return
            if (active) return
            addReplace(node.from, node.to, hide)
            return
          }
          case 'CodeInfo': {
            return
          }
          case 'CodeBlock': {
            const range = blockRange(state, node)
            const active = inRange(state, range.from, range.to)
            const firstLine = doc.lineAt(range.from).number
            const lastLine = doc.lineAt(Math.max(range.from, range.to - 1)).number
            for (let n = firstLine; n <= lastLine; n++) {
              ranges.push({
                from: doc.line(n).from,
                to: doc.line(n).from,
                deco: Decoration.line({ class: active ? 'mc-code mc-code-active' : 'mc-code' }),
              })
            }
            return
          }
          case 'FencedCode': {
            const range = blockRange(state, node)
            const active = inRange(state, range.from, range.to)
            const firstLine = doc.lineAt(range.from).number
            const lastLine = doc.lineAt(Math.max(range.from, range.to - 1)).number
            for (let n = firstLine; n <= lastLine; n++) {
              ranges.push({
                from: doc.line(n).from,
                to: doc.line(n).from,
                deco: Decoration.line({ class: active ? 'mc-code mc-code-active' : 'mc-code' }),
              })
            }
            if (!active && name === 'FencedCode') {
              // Ограда — служебная строка: убираем её целиком вместе с \n.
              const first = doc.line(firstLine)
              const last = doc.line(lastLine)
              addReplace(first.from, first.to + 1, hide)
              if (last.from > first.to)
                addReplace(last.from, Math.min(last.to + 1, doc.length), hide)
            }
            return
          }
          case 'Table': {
            const range = blockRange(state, node)
            if (inRange(state, range.from, range.to)) return
            addReplace(
              range.from,
              range.to,
              Decoration.replace({
                widget: new TableWidget(doc.sliceString(range.from, range.to), range.from),
                block: true,
              }),
            )
            return
          }
          case 'HorizontalRule': {
            const range = blockRange(state, node)
            if (inRange(state, range.from, range.to)) return
            addReplace(
              range.from,
              range.to,
              Decoration.replace({ widget: new RuleWidget(), block: true }),
            )
            return
          }
          case 'Image': {
            if (inRange(state, node.from, node.to)) return
            const text = doc.sliceString(node.from, node.to)
            const match = /^!\[([^\]]*)\]\(([^)\s]+)/.exec(text)
            if (!match) return
            addReplace(
              node.from,
              node.to,
              Decoration.replace({ widget: new ImageWidget(match[2], match[1]) }),
            )
            return
          }
        }
      },
    })
  }

  ranges.sort((a, b) => a.from - b.from || a.to - b.to)
  const builder = []
  let replacedUntil = -1
  for (const r of ranges) {
    if (r.replace) {
      if (r.from < replacedUntil) continue
      replacedUntil = r.to
    }
    builder.push(r.deco.range(r.from, r.to))
  }
  return Decoration.set(builder, true)
}

// Блочные виджеты (таблицы, hr) разрешены только из StateField, не из ViewPlugin.
const livePreview = StateField.define({
  create: (state) => buildDecorations({ state }),
  update: (deco, tr) => {
    if (tr.docChanged || tr.selection || syntaxTree(tr.state) !== syntaxTree(tr.startState)) {
      return buildDecorations({ state: tr.state })
    }
    return deco
  },
  provide: (field) => EditorView.decorations.from(field),
})

// ─── Редактор ───────────────────────────────────────────────────────────────

const themeCompartment = new Compartment()

const view = new EditorView({
  parent: document.getElementById('editor'),
  state: EditorState.create({
    doc: '',
    extensions: [
      history(),
      drawSelection(),
      EditorView.lineWrapping,
      markdown({ base: markdownLanguage, codeLanguages }),
      codeHighlight,
      livePreview,
      placeholder('Начните писать…'),
      keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return
        if (window.mc.quiet) return
        send('dirty', { text: update.state.doc.toString() })
      }),
      EditorView.domEventHandlers({
        mousedown(event, view) {
          const link = event.target.closest?.('.mc-link')
          if (!link || event.ctrlKey || event.metaKey || event.button !== 0) return false
          const pos = view.posAtDOM(link)
          const href = linkHrefAt(view.state.doc.toString(), pos)
          if (!href) return false
          event.preventDefault()
          send('openUrl', { url: href })
          return true
        },
        keydown(event) {
          if (!event.ctrlKey) return false
          const key = event.key.toLowerCase()
          if (key === 's') {
            event.preventDefault()
            send(event.shiftKey ? 'saveAs' : 'save', { text: view.state.doc.toString() })
            return true
          }
          if (key === 'o') {
            event.preventDefault()
            send('open')
            return true
          }
          return false
        },
      }),
      themeCompartment.of(EditorView.theme({})),
    ],
  }),
})

// Вызовы из Rust.
window.mc = {
  quiet: false,
  loaded({ text }) {
    this.quiet = true
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
      selection: { anchor: 0 },
      scrollIntoView: true,
    })
    this.quiet = false
    view.focus()
  },
  theme(css) {
    view.dispatch({ effects: themeCompartment.reconfigure(EditorView.theme(css)) })
  },
}

view.focus()
