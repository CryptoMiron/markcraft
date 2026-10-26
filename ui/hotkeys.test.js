import { test, expect } from 'bun:test'
import { actionFor } from './hotkeys.js'

const key = (code, extra = {}) => actionFor({ code, key: '', ...extra })

test('сохранение по физической клавише', () => {
  expect(key('KeyS', { ctrlKey: true })).toBe('save')
  expect(key('KeyS', { ctrlKey: true, shiftKey: true })).toBe('saveAs')
})

test('русская раскладка: клавиша та же, символ другой', () => {
  // Ctrl+S на русской раскладке даёт event.key === 'ы'.
  expect(actionFor({ code: 'KeyS', key: 'ы', ctrlKey: true })).toBe('save')
  expect(actionFor({ code: 'KeyO', key: 'щ', ctrlKey: true })).toBe('open')
  expect(actionFor({ code: 'KeyB', key: 'и', ctrlKey: true })).toBe('bold')
  expect(actionFor({ code: 'KeyZ', key: 'я', ctrlKey: true })).toBe('undo')
  expect(actionFor({ code: 'KeyF', key: 'а', ctrlKey: true })).toBe('find')
})

test('жирный, курсив, ссылка, зачёркнутый', () => {
  expect(key('KeyB', { ctrlKey: true })).toBe('bold')
  expect(key('KeyI', { ctrlKey: true })).toBe('italic')
  expect(key('KeyK', { ctrlKey: true })).toBe('link')
  expect(key('KeyX', { ctrlKey: true, shiftKey: true })).toBe('strike')
  expect(key('KeyX', { ctrlKey: true })).toBeNull()
})

test('отмена и возврат', () => {
  expect(key('KeyZ', { ctrlKey: true })).toBe('undo')
  expect(key('KeyZ', { ctrlKey: true, shiftKey: true })).toBe('redo')
  expect(key('KeyY', { ctrlKey: true })).toBe('redo')
})

test('поиск и замена', () => {
  expect(key('KeyF', { ctrlKey: true })).toBe('find')
  expect(key('KeyH', { ctrlKey: true })).toBe('replace')
})

test('копировать как HTML только с Shift', () => {
  expect(key('KeyC', { ctrlKey: true, shiftKey: true })).toBe('copyHtml')
  // Обычный Ctrl+C остаётся браузеру.
  expect(key('KeyC', { ctrlKey: true })).toBeNull()
})

test('масштаб: клавиши и цифровая часть', () => {
  expect(key('Equal', { ctrlKey: true })).toBe('zoomIn')
  expect(key('Minus', { ctrlKey: true })).toBe('zoomOut')
  expect(key('Digit0', { ctrlKey: true })).toBe('zoomReset')
  expect(key('NumpadAdd', { ctrlKey: true })).toBe('zoomIn')
  expect(key('NumpadSubtract', { ctrlKey: true })).toBe('zoomOut')
  expect(key('Numpad0', { ctrlKey: true })).toBe('zoomReset')
})

test('справка', () => {
  expect(key('Slash', { ctrlKey: true })).toBe('help')
})

test('без Ctrl или Meta ничего не срабатывает', () => {
  expect(actionFor({ code: 'KeyS', key: 's' })).toBeNull()
  expect(actionFor({ code: 'KeyB', key: 'b', shiftKey: true })).toBeNull()
})

test('Meta работает как Ctrl', () => {
  expect(actionFor({ code: 'KeyS', key: 's', metaKey: true })).toBe('save')
})

test('неизвестная клавиша — null', () => {
  expect(key('KeyQ', { ctrlKey: true })).toBeNull()
  expect(key('ArrowDown', { ctrlKey: true })).toBeNull()
})

test('если code пустой, берётся латинский key', () => {
  expect(actionFor({ code: '', key: 's', ctrlKey: true })).toBe('save')
  expect(actionFor({ code: '', key: 'z', ctrlKey: true })).toBe('undo')
  // Кириллица без code не угадывается: лучше не сработать, чем сработать не то.
  expect(actionFor({ code: '', key: 'ы', ctrlKey: true })).toBeNull()
})
