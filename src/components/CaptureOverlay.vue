<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { native, imageUrl } from '../services/native';
import type { CaptureSession } from '../types';
const session = ref<CaptureSession | null>(null), error = ref(''), busy = ref(false);
const screenImage = ref<HTMLImageElement | null>(null), ready = ref(false);
let revision = 0, alive = true;
const unlisten: UnlistenFn[] = [];
function clear() { revision++; session.value = null; start.value = null; ready.value = false; busy.value = false; error.value = ''; }
async function prepare(value: CaptureSession) {
  if (!alive || session.value?.imagePath === value.imagePath) return;
  clear(); const current = revision; session.value = value;
  try {
    await nextTick();
    const image = screenImage.value;
    if (!image) throw new Error('无法加载框选画面');
    await image.decode();
    if (!alive || current !== revision) return;
    // 隐藏窗口的动画帧会被系统暂停，不能等待 requestAnimationFrame 后才显示。
    await native.captureReady(value.imagePath);
    if (current === revision) ready.value = true;
  } catch (e) {
    if (current !== revision || !alive) return;
    error.value = String(e);
    // 后端超时保护会恢复主窗口，避免图片加载失败后停留在隐藏框选状态。
  }
}
const start = ref<{ x: number; y: number } | null>(null), end = ref({ x: 0, y: 0 });
const box = computed(() => start.value ? { x: Math.min(start.value.x, end.value.x), y: Math.min(start.value.y, end.value.y), width: Math.abs(start.value.x - end.value.x), height: Math.abs(start.value.y - end.value.y) } : null);
const pixelSize = computed(() => ({ width: Math.round((box.value?.width || 0) * (session.value?.width || innerWidth) / innerWidth), height: Math.round((box.value?.height || 0) * (session.value?.height || innerHeight) / innerHeight) }));
function point(event: PointerEvent) { return { x: Math.max(0, Math.min(innerWidth, event.clientX)), y: Math.max(0, Math.min(innerHeight, event.clientY)) }; }
function down(event: PointerEvent) { if (event.button !== 0 || busy.value || !ready.value) return; (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId); start.value = point(event); end.value = point(event); }
function move(event: PointerEvent) { if (start.value && !busy.value) end.value = point(event); }
async function up(event: PointerEvent) {
  if (event.button !== 0) return;
  if (!session.value || !box.value || busy.value) return;
  const sx = session.value.width / innerWidth, sy = session.value.height / innerHeight;
  const x = Math.round(box.value.x * sx), y = Math.round(box.value.y * sy);
  const width = Math.min(session.value.width - x, Math.round(box.value.width * sx));
  const height = Math.min(session.value.height - y, Math.round(box.value.height * sy));
  if (width < 2 || height < 2) { start.value = null; return; }
  busy.value = true;
  const current = revision;
  try { await native.finishCapture({ x, y, width, height }); } catch (e) { if (current === revision) { error.value = String(e); busy.value = false; } }
}
async function cancel() { try { await native.cancelCapture(); } catch (e) { error.value = String(e); } }
function key(event: KeyboardEvent) { if (event.key === 'Escape') void cancel(); }
onMounted(async () => {
  document.addEventListener('keydown', key);
  for (const [name, callback] of [
    ['capture-prepare', (payload: CaptureSession) => { void prepare(payload); }],
    ['capture-clear', () => clear()],
  ] as const) {
    const stop = await listen<CaptureSession>(name, event => callback(event.payload));
    if (!alive) { stop(); return; } unlisten.push(stop);
  }
  // 新建窗口可能错过首次事件；预热窗口没有会话是正常状态。
  const current = revision;
  try { const value = await native.captureSession(); if (alive && current === revision) await prepare(value); } catch { /* 等待下一次框选 */ }
});
onUnmounted(() => { alive = false; revision++; document.removeEventListener('keydown', key); unlisten.forEach(stop => stop()); });
</script>
<template>
  <main class="sg-capture" @pointerdown="down" @pointermove="move" @pointerup="up" @contextmenu.prevent="cancel">
    <img v-if="session" ref="screenImage" :src="imageUrl(session.imagePath)" draggable="false" alt="当前屏幕" />
    <div v-if="box" class="sg-capture-box" :style="{ left: `${box.x}px`, top: `${box.y}px`, width: `${box.width}px`, height: `${box.height}px` }"><span>{{ pixelSize.width }} × {{ pixelSize.height }}</span></div>
    <div v-if="session" class="sg-capture-help">{{ error || (busy ? '处理中…' : session.mode === 'stick' ? '拖动框选 · 松开后原位贴图 · Esc 取消' : session.mode === 'text' ? '拖动框选识字 · Esc 取消' : '拖动框选 · 松开后复制图片 · Esc 取消') }}</div>
  </main>
</template>
