import { EditorState, StateField, StateEffect, Transaction } from '@codemirror/state'
import { EditorView, Decoration, WidgetType, keymap, placeholder } from '@codemirror/view'
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
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
  undo,
  redo,
} from '@codemirror/commands'
import { search, searchKeymap, openSearchPanel } from '@codemirror/search'
import MarkdownIt from 'markdown-it'
import { linkHrefAt, opensLinkOn, anchorLine } from './links.js'
import { findFrontmatter } from './frontmatter.js'
import { codeBody } from './code-block.js'
import { actionFor } from './hotkeys.js'
import { nextLinePrefix, toggleWrap, insertLink, existingLink } from './editing.js'

// ─── Мост в Rust ────────────────────────────────────────────────────────────
const send = (type, extra = {}) =>
  window.webkit.messageHandlers.ipc.postMessage(JSON.stringify({ type, ...extra }))

// Словарь вставлен Rust до бандла: к созданию EditorView он уже на месте.
const t = (key) => window.mcI18n?.[key] ?? key

// ─── Статусбар ──────────────────────────────────────────────────────────────
const statusPath = document.getElementById('status-path')
const statusDir = document.getElementById('status-dir')
const statusFile = document.getElementById('status-file')
const statusStats = document.getElementById('status-stats')

const setPath = (path) => {
  const cut = path ? path.lastIndexOf('/') + 1 : 0
  statusDir.textContent = path ? path.slice(0, cut) : ''
  statusFile.textContent = path ? path.slice(cut) : t('file.untitled')
  statusPath.title = path || ''
}

/// Счётчики по всему тексту. shortcut: пересчёт на каждую правку; до сотен КБ
/// это дешевле дебаунса, при больших файлах — считать по таймеру.
function renderStats(state) {
  const text = state.doc.toString()
  const words = text.match(/\S+/g)?.length ?? 0
  const n = (value) => value.toLocaleString(document.documentElement.lang)
  statusStats.textContent =
    `${t('status.words')} ${n(words)} · ${t('status.chars')} ${n(state.doc.length)} · ${t('status.lines')} ${n(state.doc.lines)}`
}

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

/// Кнопка «копировать» в правом верхнем углу блока. Текст приходит готовым:
/// без ограды и без отступа отступного кода.
class CopyWidget extends WidgetType {
  constructor(text) {
    super()
    this.text = text
  }
  eq(other) {
    return other.text === this.text
  }
  toDOM() {
    const button = document.createElement('button')
    button.className = 'mc-copy'
    button.type = 'button'
    button.title = t('editor.copy_code')
    button.setAttribute('aria-label', t('editor.copy_code'))
    button.contentEditable = 'false'
    // Без этого клик по кнопке уводит фокус из редактора.
    button.onmousedown = (event) => event.preventDefault()
    button.innerHTML =
      '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true">' +
      '<rect x="5" y="5" width="9" height="10" rx="1.5"/>' +
      '<path d="M11 5V3.5A1.5 1.5 0 0 0 9.5 2h-6A1.5 1.5 0 0 0 2 3.5v8A1.5 1.5 0 0 0 3.5 13H5"/>' +
      '</svg>'
    const icon = button.innerHTML
    button.onclick = () => {
      send('copyText', { text: this.text })
      button.textContent = '✓'
      setTimeout(() => (button.innerHTML = icon), 1000)
    }
    return button
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
    // Только если это именно клик: конец перетаскивания выделения тоже даёт
    // click, и без проверки он перепрыгивал бы в начало таблицы.
    table.onclick = () => {
      if (!window.getSelection()?.isCollapsed) return
      view.dispatch({ selection: { anchor: this.pos }, scrollIntoView: true })
      view.focus()
    }
    return table
  }
  ignoreEvent() {
    return true
  }
}

/// Строки блока кода: фон на каждой, рамка — на первой и последней,
/// иначе граница рисуется между строками. Тело блока получает номер строки.
function addCodeLines(ranges, doc, node, active) {
  const range = blockRange({ doc }, node)
  const first = doc.lineAt(range.from).number
  const last = doc.lineAt(Math.max(range.from, range.to - 1)).number
  const body = codeBody(doc, node.node)
  // Закрывающая ограда вне активной строки прячется вместе с переводом
  // строки, и её пустая строка сливается с пустой строкой после блока.
  // Красить её нечем: рамку блока рисует последняя строка тела.
  const bareLast =
    node.name === 'FencedCode' && !active && body && body.last < last ? last : -1
  const lastStyled = bareLast < 0 ? last : body.last
  for (let n = first; n <= last; n++) {
    if (n === bareLast) continue
    const classes = ['mc-code', 'mc-code-block']
    const attributes = {}
    if (active) classes.push('mc-code-active')
    if (n === first) classes.push('mc-code-first')
    if (n === lastStyled) classes.push('mc-code-last')
    if (body && n >= body.first && n <= body.last)
      attributes['data-num'] = String(n - body.first + 1)
    // Ограда прячется вместе с переводом строки, и CM сливает её строку
    // с первой строкой тела, теряя декорацию тела. Номер 1 ставим на обе:
    // на слитой строке уцелеет одна из них, дубликата не будет.
    else if (body && !active && node.name === 'FencedCode' && n === first)
      attributes['data-num'] = '1'
    ranges.push({
      from: doc.line(n).from,
      to: doc.line(n).from,
      deco: Decoration.line({ class: classes.join(' '), attributes }),
    })
  }
  return { first, last, body }
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
  const composing = state.field(composingField, false)
  const ranges = []

  // Фон выделения рисуем сами: WebKitGTK прогоняет нативный ::selection
  // через чужой цветовой профиль, и тёмный цвет темы становится почти
  // чёрным. Метка ложится поверх фона строки, но под текст.
  for (const range of state.selection.ranges) {
    if (range.empty) continue
    ranges.push({
      from: range.from,
      to: range.to,
      deco: Decoration.mark({ class: 'mc-selected' }),
    })
  }

  const add = (from, to, deco) => {
    if (to > from) ranges.push({ from, to, deco })
  }

  /// replace-декорации не могут пересекаться, mark — могут (курсив лежит
  /// поверх своих же звёздочек).
  const addReplace = (from, to, deco) => {
    if (to > from) ranges.push({ from, to, deco, replace: true })
  }

  /// Блочная замена: во время IME её не ставим вовсе.
  const addBlock = (from, to, deco) => {
    if (!composing) addReplace(from, to, deco)
  }

  const tree = syntaxTree(state)

  // Шапка файла (frontmatter) — это YAML, а не Markdown. Без отдельной
  // проверки парсер делает из неё заголовок setext и прячет закрывающий
  // `---`, поэтому блок распознаём сами и рисуем как код.
  const fm = findFrontmatter(doc)
  if (fm) {
    for (let n = fm.first; n <= fm.last; n++) {
      const line = doc.line(n)
      const classes = ['mc-code']
      if (n === fm.first) classes.push('mc-code-first')
      if (n === fm.last) classes.push('mc-code-last')
      ranges.push({
        from: line.from,
        to: line.from,
        deco: Decoration.line({ class: classes.join(' ') }),
      })
    }
  }

  {
    tree.iterate({
      from: 0,
      to: doc.length,
      enter: (node) => {
        const name = node.name
        const line = doc.lineAt(node.from)
        const active = lineActive(state, line)

        // Внутри шапки разметку не разбираем: там YAML, а не Markdown.
        if (fm && node.from >= fm.from && node.to <= fm.to) return

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
          case 'CodeBlock':
          case 'FencedCode': {
            const range = blockRange(state, node)
            const active = inRange(state, range.from, range.to)
            const { first: firstLine, last: lastLine, body } = addCodeLines(ranges, doc, node, active)
            // Пока идёт IME, кнопка не пересобирается: замена DOM под курсором
            // рвёт composition.
            if (body && !composing) {
              const line = doc.line(firstLine)
              ranges.push({
                from: line.from,
                to: line.from,
                deco: Decoration.widget({ widget: new CopyWidget(body.text), side: -1 }),
              })
            }
            if (name === 'FencedCode' && !active) {
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
            addBlock(
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
            addBlock(
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

// Пока идёт набор через IME, блочные виджеты не пересобираются: замена DOM
// под курсором рвёт composition и съедает первый слог.
const setComposing = StateEffect.define()

const composingField = StateField.define({
  create: () => false,
  update: (value, tr) => {
    for (const effect of tr.effects) if (effect.is(setComposing)) value = effect.value
    return value
  },
})

// Блочные виджеты (таблицы, hr) разрешены только из StateField, не из ViewPlugin.
const livePreview = StateField.define({
  create: (state) => buildDecorations({ state }),
  update: (deco, tr) => {
    if (
      tr.docChanged ||
      tr.selection ||
      tr.effects.length ||
      syntaxTree(tr.state) !== syntaxTree(tr.startState)
    ) {
      return buildDecorations({ state: tr.state })
    }
    return deco
  },
  provide: (field) => EditorView.decorations.from(field),
})

// ─── Поиск и служебные команды ───────────────────────────────────────────────

// Для Ctrl+Shift+C: тот же markdown, что видит пользователь, но в HTML.
const md = new MarkdownIt({ html: false, linkify: true })

const copyAsHtml = (view) => {
  const text = view.state.doc.toString()
  send('copyHtml', { html: md.render(text), text })
  return true
}

// Панель поиска: свои строки из словаря вместо английских по умолчанию.
const phrases = EditorState.phrases.of({
  Find: t('search.find'),
  Replace: t('search.replace'),
  next: t('search.next'),
  previous: t('search.previous'),
  all: t('search.all'),
  'match case': t('search.match_case'),
  regexp: t('search.regexp'),
  'by word': t('search.by_word'),
  replace: t('search.replace_one'),
  'replace all': t('search.replace_all'),
  close: t('search.close'),
})

const serviceKeys = [
  // Ctrl+H открывает ту же панель: замена живёт в ней.
  { key: 'Mod-h', run: openSearchPanel, scope: 'editor search-panel' },
  { key: 'Mod-Shift-c', run: copyAsHtml },
  { key: 'Mod-n', run: () => (send('newWindow'), true) },
  { key: 'Mod-/', run: () => (send('help'), true) },
  { key: 'F1', run: () => (send('help'), true) },
]

// ─── Хоткеи независимо от раскладки ─────────────────────────────────────────
// На русской раскладке Ctrl+S даёт `event.key === 'ы'`, и совпадения с 's' нет.
// Поэтому служебные сочетания разбираем по `event.code` — это физическая
// клавиша, она одинакова при любой раскладке.

const runAction = (action, view) => {
  switch (action) {
    case 'save':
    case 'saveAs':
      send(action, { text: view.state.doc.toString() })
      return true
    case 'open':
      send('open')
      return true
    case 'newWindow':
      send('newWindow')
      return true
    case 'paste':
      send('paste')
      return true
    case 'bold':
      return wrap('**')(view)
    case 'italic':
      return wrap('*')(view)
    case 'strike':
      return wrap('~~')(view)
    case 'link':
      return makeLink(view)
    case 'undo':
      return undo(view)
    case 'redo':
      return redo(view)
    case 'find':
    case 'replace':
      return openSearchPanel(view)
    case 'copyHtml':
      return copyAsHtml(view)
    case 'help':
      send('help')
      return true
    case 'zoomIn':
      send('zoom', { step: 1 })
      return true
    case 'zoomOut':
      send('zoom', { step: -1 })
      return true
    case 'zoomReset':
      send('zoom', { step: 0 })
      return true
    default:
      return false
  }
}

// ─── Правка ─────────────────────────────────────────────────────────────────
// Правила лежат в editing.js и проверяются тестами; здесь только их
// применение к документу.

/// Enter в списке продолжает список, в пустом пункте — выходит из него.
const continueList = (view) => {
  const { state } = view
  const range = state.selection.main
  if (!range.empty) return false
  const line = state.doc.lineAt(range.head)
  const rule = nextLinePrefix(line.text)
  if (!rule) return false

  if (rule.clear) {
    // Пустой пункт: убираем маркер и оставляем пустую строку.
    view.dispatch({
      changes: { from: line.from, to: line.to, insert: '' },
      selection: { anchor: line.from },
      userEvent: 'delete',
    })
    return true
  }

  view.dispatch({
    changes: { from: range.head, insert: '\n' + rule.prefix },
    selection: { anchor: range.head + 1 + rule.prefix.length },
    scrollIntoView: true,
    userEvent: 'input',
  })
  return true
}

/// Ctrl+B / Ctrl+I / Ctrl+Shift+X / Ctrl+K.
const wrap = (marker) => (view) => {
  const { state } = view
  const range = state.selection.main
  const text = state.doc.toString()
  const edit = toggleWrap(text, range.from, range.to, marker)
  view.dispatch({
    changes: { from: edit.from, to: edit.to, insert: edit.insert },
    selection: { anchor: edit.anchor, head: edit.head },
    scrollIntoView: true,
    userEvent: 'input',
  })
  return true
}

const makeLink = (view) => {
  const { state } = view
  const range = state.selection.main
  const text = state.doc.toString()

  // Уже внутри ссылки: выделяем адрес, чтобы набрать новый поверх.
  const existing = existingLink(text, range.from)
  if (existing) {
    view.dispatch({
      selection: { anchor: existing.urlFrom, head: existing.urlTo },
      scrollIntoView: true,
    })
    return true
  }

  const edit = insertLink(text, range.from, range.to, t('editor.link_label'))
  view.dispatch({
    changes: { from: edit.from, to: edit.to, insert: edit.insert },
    selection: { anchor: edit.anchor, head: edit.head },
    scrollIntoView: true,
    userEvent: 'input',
  })
  return true
}

const editingKeys = [
  { key: 'Enter', run: continueList },
  { key: 'Mod-b', run: wrap('**') },
  { key: 'Mod-i', run: wrap('*') },
  { key: 'Mod-Shift-x', run: wrap('~~') },
  { key: 'Mod-k', run: makeLink },
]

// ─── Редактор ───────────────────────────────────────────────────────────────

const view = new EditorView({
  parent: document.getElementById('editor'),
  state: EditorState.create({
    doc: '',
    extensions: [
      history(),
      EditorView.lineWrapping,
      markdown({ base: markdownLanguage, codeLanguages }),
      codeHighlight,
      composingField,
      livePreview,
      placeholder(t('editor.placeholder')),
      phrases,
      search({ top: true }),
      keymap.of([
        ...serviceKeys,
        ...searchKeymap,
        ...editingKeys,
        ...defaultKeymap,
        ...historyKeymap,
        indentWithTab,
      ]),
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return
        renderStats(update.state)
        if (window.mc.quiet) return
        send('dirty', { text: update.state.doc.toString() })
      }),
      EditorView.domEventHandlers({
        // Ctrl+колесо меняет размер текста, как в браузере.
        wheel(event) {
          if (!event.ctrlKey) return false
          event.preventDefault()
          send('zoom', { step: event.deltaY < 0 ? 1 : -1 })
          return true
        },
        compositionstart(event, view) {
          view.dispatch({ effects: setComposing.of(true) })
          return false
        },
        compositionend(event, view) {
          view.dispatch({ effects: setComposing.of(false) })
          return false
        },
        mousedown(event, view) {
          // Ссылка открывается только по Ctrl/Cmd+клик, как в редакторах кода.
          // Обычный клик обязан ставить курсор и начинать выделение, иначе
          // в текст ссылки нельзя попасть и нельзя потянуть выделение с неё.
          const link = event.target.closest?.('.mc-link')
          if (!link || !opensLinkOn(event)) return false
          const pos = view.posAtDOM(link)
          const href = linkHrefAt(view.state.doc.toString(), pos)
          if (!href) return false
          event.preventDefault()
          if (href.startsWith('#')) {
            const line = anchorLine(view.state.doc.toString(), href)
            if (line !== null) {
              view.dispatch({
                selection: { anchor: view.state.doc.line(line).from },
                scrollIntoView: true,
              })
            }
            view.focus()
            return true
          }
          send('openUrl', { url: href })
          return true
        },
        keydown(event) {
          // Поля панели поиска не перехватываем: там Ctrl+V и Ctrl+A свои.
          if (event.target.closest?.('.cm-panels')) return false
          const action = actionFor(event)
          if (!action || !runAction(action, view)) return false
          event.preventDefault()
          return true
        },
      }),
    ],
  }),
})

// Вызовы из Rust.
window.mc = {
  quiet: false,
  loaded({ text, path }) {
    setPath(path)
    this.quiet = true
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
      selection: { anchor: 0 },
      scrollIntoView: true,
      // Загрузка файла — не правка: иначе первый Ctrl+Z стирает документ,
      // а автосейв записывает пустоту.
      annotations: Transaction.addToHistory.of(false),
    })
    this.quiet = false
    view.focus()
  },
  insert({ text }) {
    view.dispatch(view.state.replaceSelection(text), { scrollIntoView: true, userEvent: 'input' })
  },
  /// Только путь: Save As и выбор имени не должны трогать текст и курсор.
  path(path) {
    setPath(path)
  },
  /// Палитра и размер текста приходят из Omarchy одной строкой CSS.
  theme(css) {
    let style = document.getElementById('mc-theme')
    if (!style) {
      style = document.createElement('style')
      style.id = 'mc-theme'
      document.head.append(style)
    }
    style.textContent = css
  },
}

view.focus()
renderStats(view.state)

// Одно активное выделение на приложение: при потере фокуса окна снимаем
// выделение, чтобы старое не мешало выделять в другом окне.
window.addEventListener('blur', () => {
  const main = view.state.selection.main
  if (main.empty) return
  view.dispatch({ selection: { anchor: main.head } })
})
