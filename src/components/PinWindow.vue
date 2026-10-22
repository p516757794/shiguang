<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { PhysicalPosition } from '@tauri-apps/api/dpi';
import { Menu } from '@tauri-apps/api/menu';
import { native, imageUrl } from '../services/native';
import type { PinData } from '../types';
import Icon from './Icon.vue';
const data = ref<PinData | null>(null), folded = ref(false), text = ref<string | null>(null), textOpen = ref(false), recognizing = ref(false), opacity = ref(1), error = ref('');
const window = getCurrentWindow();
const picture = ref<HTMLImageElement | null>(null);
let currentMenu: Menu | null = null;
let imageWidth = 0, resizing = false, dragMoved = false;
let resizePointer: number | null = null;
let resizeBase = { x: 0, y: 0, width: 0, height: 0, px: 0, py: 0, scale: 1, direction: '' };
let resizeChain = Promise.resolve();
async function action(callback: () => Promise<unknown>) { try { error.value = ''; await callback(); } catch (e) { error.value = String(e); } }
async function size(width = imageWidth) { imageWidth = width; await native.resizePin(Math.round(width), textOpen.value ? 170 : 0); }
async function fold() { await action(async () => { await native.foldPin(!folded.value); folded.value = !folded.value; }); }
async function toggleText(open: boolean) { await action(async () => { textOpen.value = open; await nextTick(); await size(); }); }
async function recognize() {
  if (!data.value || recognizing.value) return; recognizing.value = true;
  await action(async () => { if (folded.value) { await native.foldPin(false); folded.value = false; } text.value = await native.ocrPin(); textOpen.value = true; await size(); }); recognizing.value = false;
}
async function menu() {
  if (!data.value) return;
  await action(async () => {
    if (currentMenu) await currentMenu.close();
    const popup = await Menu.new({ items: [
      { id: 'copy', text: '复制图片', action: () => { void action(() => native.copyPin()); } },
      { id: 'ocr', text: recognizing.value ? '正在提取文字…' : '提取图片文字', enabled: !recognizing.value, action: () => { void recognize(); } },
      { text: '缩放', enabled: !folded.value, items: [75, 100, 150, 200].map(percent => ({ id: `zoom-${percent}`, text: `${percent}%`, action: () => { void action(() => size(data.value!.width * percent / 100)); } })) },
      { text: '透明度', items: [100, 85, 65, 45].map(percent => ({ id: `opacity-${percent}`, text: `${percent}%`, action: () => { opacity.value = percent / 100; } })) },
      { item: 'Separator' },
      { id: 'delete', text: '删除贴图（保留记录）', action: () => { void action(() => native.closePin()); } },
    ] });
    currentMenu = popup; await popup.popup();
  });
}
async function imageDown(event: PointerEvent) { if (event.button !== 0 || resizing) return; event.preventDefault(); await action(() => window.startDragging()); }
function iconDown(event: PointerEvent) {
  if (event.button !== 0) return; dragMoved = false;
  const x = event.screenX, y = event.screenY;
  const element = event.currentTarget as HTMLElement; element.setPointerCapture(event.pointerId);
  const move = (next: PointerEvent) => { if (!dragMoved && Math.hypot(next.screenX - x, next.screenY - y) > 4) { dragMoved = true; element.removeEventListener('pointermove', move); void action(() => window.startDragging()); } };
  element.addEventListener('pointermove', move);
  element.addEventListener('pointerup', () => { element.removeEventListener('pointermove', move); if (!dragMoved) void fold(); }, { once: true });
}
async function resizeDown(event: PointerEvent, direction: string) {
  if (event.button !== 0 || !data.value) return; event.stopPropagation(); event.preventDefault();
  resizePointer = event.pointerId;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  await action(async () => {
    const [position, scale] = await Promise.all([window.outerPosition(), window.scaleFactor()]);
    if (resizePointer !== event.pointerId) return;
    resizeBase = { x: event.screenX, y: event.screenY, width: imageWidth, height: imageWidth * (data.value!.height / data.value!.width), px: position.x, py: position.y, scale, direction }; resizing = true;
  });
}
function resizeEnd() { resizePointer = null; resizing = false; }
function resizeMove(event: PointerEvent) {
  if (!resizing || !data.value) return;
  const b = resizeBase, ratio = data.value.height / data.value.width;
  const dx = (event.screenX - b.x) * b.scale, dy = (event.screenY - b.y) * b.scale;
  const delta = b.direction.includes('e') ? dx : b.direction.includes('w') ? -dx : b.direction.includes('s') ? dy / ratio : -dy / ratio;
  const width = Math.max(32, Math.min(12000, b.width + delta));
  const x = b.px + (b.direction.includes('w') ? b.width - width : 0), y = b.py + (b.direction.includes('n') ? b.height - width * ratio : 0);
  resizeChain = resizeChain.then(async () => { await size(width); await window.setPosition(new PhysicalPosition(Math.round(x), Math.round(y))); }).catch(e => { error.value = String(e); });
}
function wheel(event: WheelEvent) { if ((event.target as Element).closest('.sg-pin-ocr')) return; event.preventDefault(); if (folded.value || !data.value) return; if (event.ctrlKey) opacity.value = Math.max(.2, Math.min(1, opacity.value + (event.deltaY < 0 ? .05 : -.05))); else void action(() => size(Math.max(32, Math.min(12000, imageWidth * (event.deltaY < 0 ? 1.08 : 1 / 1.08))))); }
function key(event: KeyboardEvent) { if (event.key === 'Escape') error.value = ''; }
onMounted(async () => {
  document.addEventListener('keydown', key);
  await action(async () => {
    data.value = await native.pinData(); imageWidth = data.value.width;
    await nextTick();
    if (!picture.value) throw new Error('贴图图片未就绪');
    await picture.value.decode();
    await native.pinReady();
  });
});
onUnmounted(() => { document.removeEventListener('keydown', key); void currentMenu?.close().catch(() => {}); });
</script>
<template>
  <main class="sg-pin" @contextmenu.prevent="menu" @wheel="wheel">
    <button v-if="folded" class="sg-pin-folded" title="点击展开 · 拖动移动" @pointerdown="iconDown"><Icon name="Layers" /></button>
    <template v-else-if="data">
      <div class="sg-pin-picture" @pointerdown="imageDown">
        <img ref="picture" :src="imageUrl(data.imagePath)" alt="贴图" draggable="false" :style="{ opacity }" />
        <button class="sg-image-control sg-fold" title="折叠贴图" @pointerdown.stop @click.stop="fold"><Icon name="Minimize2" /></button>
        <button v-if="text !== null && !textOpen" class="sg-image-control sg-reopen" title="展开识别文字" @pointerdown.stop @click.stop="toggleText(true)"><Icon name="Text" /></button>
        <i v-for="direction in ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw']" :key="direction" class="sg-resize" :class="`sg-resize-${direction}`" @pointerdown="resizeDown($event, direction)" @pointermove="resizeMove" @pointerup="resizeEnd" @pointercancel="resizeEnd" @lostpointercapture="resizeEnd"></i>
      </div>
      <section v-if="textOpen" class="sg-pin-ocr"><div><small>{{ recognizing ? '正在识字…' : text ? '识别文字 · 保留换行' : '未识别到文字' }}</small><button title="收回文字框" @click="toggleText(false)"><Icon name="ChevronUp" /></button></div><textarea v-model="text" wrap="off" aria-label="识别文字"></textarea><button :disabled="!text" @click="action(() => native.copyText(text!))"><Icon name="Copy" />复制文字</button></section>
    </template>
    <div v-if="error" class="sg-pin-error" @click="error = ''">{{ error }}</div>
    <div v-if="recognizing" class="sg-pin-busy">识字中…</div>
  </main>
</template>
