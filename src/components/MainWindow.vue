<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { native, imageUrl } from '../services/native';
import { useDesktop } from '../services/desktop';
import type { ClipRecord, View, CaptureMode } from '../types';
import Icon from './Icon.vue';
import RecordPreview from './RecordPreview.vue';
import ConversionPane from './ConversionPane.vue';
import SettingsDialog from './SettingsDialog.vue';

const desktop = useDesktop();
const { records, settings, jobs, error, toast, ocrText, exitRequested, ready, active, completed, showProgress, storageBytes, clock } = desktop;
const view = ref<View>('clipboard'), query = ref(''), filter = ref('all'), day = ref(dayKey(Date.now()));
const preview = ref<ClipRecord | null>(null), newPhrase = ref(false), settingsOpen = ref(false), manage = ref(false);
const selected = ref<string[]>([]), context = ref<{ id: string; x: number; y: number } | null>(null);
const manageMenu = ref<{ x: number; y: number } | null>(null);
const topmostBusy = ref(false);
const deletion = ref<{ ids: string[]; view: View; message: string } | null>(null);
const dragging = ref(''); let suppressClickUntil = 0;
const failedImages = ref<Record<string, boolean>>({});
let pointerSort: { id: string; x: number; y: number; target: string } | null = null;
const window = getCurrentWindow();
const navigation: { id: View; title: string; icon: string }[] = [
  { id: 'clipboard', title: '剪贴板', icon: 'Clipboard' }, { id: 'favorites', title: '收藏', icon: 'Star' },
  { id: 'phrases', title: '常用短语', icon: 'MessageSquareText' }, { id: 'conversion', title: '文件转换', icon: 'Files' },
];
const conversionLabel = computed(() => active.value.length ? active.value.every(j => j.status === 'paused') ? '转换已暂停' : `转换中 · 余 ${active.value.length} 个` : jobs.value.some(j => j.status === 'failed' || j.status === 'cancelled') ? '本轮处理结束' : '转换完成');
function dayKey(time: number) { const d = new Date(time); return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`; }
function dayLabel(time: number, index: number) { return index === 0 ? '今天' : index === 1 ? '昨天' : new Date(time).toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric' }); }
const today = computed(() => dayKey(clock.value));
const days = computed(() => { today.value; return Array.from({ length: 7 }, (_, i) => { const date = new Date(); date.setHours(12, 0, 0, 0); date.setDate(date.getDate() - i); return { key: dayKey(+date), label: dayLabel(+date, i), short: i < 2 ? dayLabel(+date, i) : `${date.getMonth() + 1}/${date.getDate()}`, timestamp: +date }; }); });
watch(today, (next, previous) => { if (day.value === previous) day.value = next; });
const base = computed(() => records.value.filter(r => view.value === 'favorites' ? r.favorite : view.value === 'phrases' ? r.kind === 'phrase' && !r.historyDeleted : r.kind !== 'phrase' && !r.historyDeleted));
const visible = computed(() => base.value.filter(r => {
  const q = query.value.trim().toLocaleLowerCase();
  return (!q || `${r.name || ''} ${r.text} ${r.source}`.toLocaleLowerCase().includes(q)) &&
    (view.value !== 'clipboard' || q || dayKey(r.createdAt) === day.value) &&
    (filter.value === 'all' || (filter.value === 'image' ? r.kind === 'image' : r.kind !== 'image'));
}));
const title = computed(() => query.value.trim() ? '搜索结果' : view.value === 'clipboard' ? days.value.find(d => d.key === day.value)?.label || '历史记录' : navigation.find(n => n.id === view.value)?.title);
watch(visible, next => { selected.value = selected.value.filter(id => next.some(r => r.id === id)); });
const dateSubtitle = computed(() => {
  const record = base.value.find(r => dayKey(r.createdAt) === day.value);
  const timestamp = record?.createdAt ?? days.value.find(d => d.key === day.value)?.timestamp ?? Date.now();
  return new Date(timestamp).toLocaleDateString('zh-CN', { month: 'long', day: 'numeric', weekday: 'long' });
});
function switchView(next: View) { view.value = next; manage.value = false; manageMenu.value = null; selected.value = []; context.value = null; query.value = ''; filter.value = 'all'; }
function time(record: ClipRecord) { return new Date(record.createdAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' }); }
function headline(record: ClipRecord) { return record.kind === 'image' ? `图片 · ${record.width} × ${record.height}` : record.name || record.text.split(/\r?\n/).find(s => s.trim()) || '文字记录'; }
async function paste(record: ClipRecord) {
  if (Date.now() < suppressClickUntil) return;
  if (manage.value) { toggleSelected(record.id); return; }
  await desktop.perform(async () => { await native.paste(record.id, settings.value?.pasteFormatted || false); });
}
function toggleSelected(id: string) { selected.value = selected.value.includes(id) ? selected.value.filter(s => s !== id) : [...selected.value, id]; }
function contextMenu(event: MouseEvent, record: ClipRecord) { event.preventDefault(); context.value = { id: record.id, x: Math.min(event.clientX, innerWidth - 140), y: Math.min(event.clientY, innerHeight - 55) }; }
async function remove(ids: string[]) { if (!ids.length) return; await desktop.perform(async () => { await native.remove(ids, view.value); selected.value = []; context.value = null; }); }
function openManage(event: MouseEvent) { if (manage.value) { manage.value = false; selected.value = []; return; } const rect = (event.currentTarget as HTMLElement).getBoundingClientRect(); manageMenu.value = manageMenu.value ? null : { x: Math.max(8, Math.min(rect.right - 168, innerWidth - 180)), y: rect.bottom + 5 }; }
function clearRange(all: boolean) { const ids = base.value.filter(r => all || dayKey(r.createdAt) === day.value).map(r => r.id); deletion.value = { ids, view: view.value, message: all ? `清空七天内的 ${ids.length} 条历史记录？收藏与常用短语保留。` : `清空当天的 ${ids.length} 条历史记录？收藏内容保留。` }; manageMenu.value = null; }
async function confirmDeletion() { const pending = deletion.value; if (!pending) return; await desktop.perform(async () => { await native.remove(pending.ids, pending.view); deletion.value = null; selected.value = []; }); }
// Windows 的原生文件拖放与 HTML5 拖放共用 WebView2 处理器，记录排序改用指针事件。
function sortDown(event: PointerEvent, id: string) {
  if (manage.value || event.button !== 0 || !(event.target as Element).closest('.cn-item-main')) return;
  pointerSort = { id, x: event.clientX, y: event.clientY, target: id };
}
function sortMove(event: PointerEvent) {
  if (!pointerSort) return;
  if (!dragging.value && Math.hypot(event.clientX - pointerSort.x, event.clientY - pointerSort.y) < 7) return;
  if (!dragging.value) (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  dragging.value = pointerSort.id; suppressClickUntil = Date.now() + 1000; event.preventDefault();
  const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('[data-record-id]');
  if (target?.dataset.recordId) pointerSort.target = target.dataset.recordId;
  const list = target?.closest('.cn-items');
  if (list) { const bounds = list.getBoundingClientRect(); if (event.clientY < bounds.top + 24) list.scrollTop -= 12; else if (event.clientY > bounds.bottom - 24) list.scrollTop += 12; }
}
function sortUp() {
  const current = pointerSort; pointerSort = null;
  if (!current || !dragging.value) return;
  const target = visible.value.find(r => r.id === current.target); if (target) void drop(target); else dragging.value = '';
  suppressClickUntil = Date.now() + 350;
}
async function drop(target: ClipRecord) {
  const id = dragging.value; dragging.value = ''; suppressClickUntil = Date.now() + 300;
  if (!id || id === target.id) return;
  const source = visible.value.find(r => r.id === id);
  if (!source || (view.value === 'clipboard' && dayKey(source.createdAt) !== dayKey(target.createdAt))) return;
  const ids = visible.value.map(r => r.id).filter(i => i !== id); ids.splice(ids.indexOf(target.id), 0, id);
  await desktop.perform(() => native.reorder(ids));
}
async function capture(mode: CaptureMode) { await desktop.perform(() => native.capture(mode)); }
async function toggleTopmost() {
  if (!settings.value || topmostBusy.value) return;
  topmostBusy.value = true;
  try { await desktop.updateSettings({ alwaysOnTop: !settings.value.alwaysOnTop }); } finally { topmostBusy.value = false; }
}
function outside(event: PointerEvent) { const target = event.target as Element; if (!target.closest('.cn-delete-bubble')) context.value = null; if (!target.closest('.cn-manage-popover, .cn-manage-trigger')) manageMenu.value = null; }
function escape(event: KeyboardEvent) { if (event.key === 'Escape') { preview.value = null; newPhrase.value = false; settingsOpen.value = false; context.value = null; manageMenu.value = null; deletion.value = null; } }
async function requestExit() { if (active.value.length) exitRequested.value = true; else await desktop.perform(() => native.exit()); }
onMounted(() => { void desktop.initialize(); document.addEventListener('pointerdown', outside); document.addEventListener('keydown', escape); });
onUnmounted(() => { desktop.dispose(); document.removeEventListener('pointerdown', outside); document.removeEventListener('keydown', escape); });
</script>

<template>
  <main id="clipnest-preview" class="cn-desktop">
    <header class="cn-titlebar" data-tauri-drag-region>
      <div class="cn-brand" data-tauri-drag-region><img class="cn-logo" src="/assets/brand.png" alt="拾光" /><strong data-tauri-drag-region>拾光</strong><span class="cn-subtitle" data-tauri-drag-region>桌面工具</span></div>
      <div class="cn-capture-tools">
        <button class="cn-capture-top" title="截图：框选后复制图片" @click="capture('copy')"><Icon name="ScanLine" /><span>截图</span></button>
        <button class="cn-capture-top" title="贴图：框选后置顶，不复制" @click="capture('stick')"><Icon name="PictureInPicture2" /><span>贴图</span></button>
        <button class="cn-capture-top cn-ocr-top" title="框选识字" @click="capture('text')"><Icon name="ScanText" /><span>识字</span></button>
      </div>
      <div class="cn-window-actions">
        <button class="cn-pin" :class="{ 'is-active': settings?.alwaysOnTop }" :aria-pressed="!!settings?.alwaysOnTop" :title="settings?.alwaysOnTop ? '取消主窗口置顶' : '主窗口置顶'" :disabled="!settings || topmostBusy" @click="toggleTopmost"><Icon name="Pin" /></button>
        <button title="最小化" @click="desktop.perform(() => window.minimize())"><Icon name="Minus" /></button>
        <button title="收起到托盘，后台任务继续" @click="desktop.perform(() => window.hide())"><Icon name="X" /></button>
      </div>
    </header>
    <nav class="cn-tabs"><button v-for="item in navigation" :key="item.id" :class="{ 'is-active': view === item.id }" @click="switchView(item.id)"><Icon :name="item.icon" /><span>{{ item.title }}</span></button></nav>
    <section v-if="view !== 'conversion'" class="cn-clipboard" :class="{ 'cn-without-dates': view !== 'clipboard' }">
      <aside v-if="view === 'clipboard'" class="cn-sidebar"><div class="cn-label">最近七天</div><div class="cn-dates"><button v-for="item in days" :key="item.key" :class="{ 'is-active': day === item.key && !query }" :title="item.label" @click="day = item.key; query = ''; selected = []"><span class="cn-date-long">{{ item.label }}</span><span class="cn-date-short">{{ item.short }}</span></button></div><p class="cn-retention"><Icon name="Clock3" /><span>普通记录保留 7 天<small>收藏与短语长期保留</small></span></p></aside>
      <section class="cn-history">
        <div class="cn-toolbar"><label class="cn-search"><Icon name="Search" /><input v-model="query" :placeholder="view === 'clipboard' ? '搜索最近七天的记录' : '搜索内容'" /><button v-if="query" title="清空搜索" @click="query = ''"><Icon name="X" /></button></label><button v-if="view === 'phrases'" class="cn-secondary" @click="newPhrase = true"><Icon name="Plus" />新增</button><button class="cn-secondary cn-manage-trigger" :class="{ 'is-active': manage }" @click="openManage"><Icon :name="manage ? 'Check' : 'Ellipsis'" /><span>{{ manage ? '完成' : '管理' }}</span></button></div>
        <div class="cn-history-heading"><div><h2>{{ title }}</h2><span class="cn-day-sub">{{ query ? '跨日期搜索' : view === 'clipboard' ? dateSubtitle : '点击内容，插入目标软件' }}</span></div><span class="cn-total">{{ visible.length }} 条</span></div>
        <div class="cn-filters"><button v-for="item in [{ id: 'all', label: '全部' }, { id: 'text', label: '文字' }, { id: 'image', label: '图片' }]" :key="item.id" :class="{ 'is-active': filter === item.id }" @click="filter = item.id; selected = []">{{ item.label }}</button><label class="cn-format-check"><input type="checkbox" :checked="settings?.pasteFormatted" @change="desktop.updateSettings({ pasteFormatted: ($event.target as HTMLInputElement).checked })" />带格式粘贴</label></div>
        <div v-if="manage" class="cn-manage-bar"><label><input type="checkbox" :checked="visible.length > 0 && selected.length === visible.length" @change="selected = ($event.target as HTMLInputElement).checked ? visible.map(r => r.id) : []" />全选</label><span>已选 {{ selected.length }} 条</span><button :disabled="!selected.length" @click="remove(selected)"><Icon name="Trash2" />{{ view === 'favorites' ? '取消收藏' : '删除记录' }}</button></div>
        <div class="cn-items">
          <article v-for="record in visible" :key="record.id" class="cn-item" :data-record-id="record.id" :class="{ 'is-image': record.kind === 'image', 'is-selected': selected.includes(record.id), 'is-dragging': dragging === record.id }" @pointerdown="sortDown($event, record.id)" @pointermove="sortMove" @pointerup="sortUp" @pointercancel="pointerSort = null; dragging = ''" @contextmenu="contextMenu($event, record)">
            <button class="cn-item-main" @click="paste(record)"><span class="cn-row-visual"><template v-if="record.kind === 'image'"><img v-if="record.imagePath && !failedImages[record.id]" :key="record.imagePath" :src="imageUrl(record.imagePath)" alt="图片缩略图" draggable="false" loading="lazy" @error="failedImages[record.id] = true" /><Icon v-else name="Image" /><input v-if="manage" class="cn-thumbnail-check" type="checkbox" tabindex="-1" :checked="selected.includes(record.id)" :aria-label="`选择图片 ${record.width} × ${record.height}`" /></template><input v-else-if="manage" type="checkbox" tabindex="-1" :checked="selected.includes(record.id)" /><Icon v-else :name="record.html ? 'FileType2' : 'Type'" /></span><span class="cn-row-body"><strong class="cn-item-title">{{ headline(record) }}</strong><span class="cn-item-note">{{ record.kind === 'image' ? (record.source.includes('贴图') ? '拾光贴图 · 点击插入图片' : record.source.includes('截图') ? '截图记录 · 点击插入图片' : '图片记录 · 点击插入图片') : record.text.replace(/\s+/g, ' ').slice(0, 200) }}</span></span><span class="cn-row-time">{{ time(record) }}</span></button>
            <span class="cn-item-footer">{{ record.source || '剪贴板' }}<template v-if="record.mergeCount > 1"> · 合并 {{ record.mergeCount }} 次</template></span>
            <button class="cn-favorite" :class="{ 'is-saved': record.favorite }" :title="record.favorite ? '取消收藏' : '收藏'" @click.stop="desktop.perform(() => native.favorite(record.id, !record.favorite))"><Icon name="Star" /></button>
            <button class="cn-item-preview" title="内容预览" @click.stop="preview = record"><Icon name="Eye" /></button>
          </article>
          <div v-if="!visible.length" class="cn-empty"><Icon :name="query ? 'Search' : 'Clipboard'" /><p>{{ !ready ? '正在读取本地记录…' : query ? '没有找到匹配内容' : view === 'phrases' ? '把常用内容添加为短语，随时插入' : '这里还没有记录，复制文字或图片后会显示在这里' }}</p></div>
        </div>
      </section>
    </section>
    <ConversionPane v-else :jobs="jobs" :configured="!!settings?.componentPath" :show-progress="showProgress" @error="error = $event" @settings="settingsOpen = true" />
    <footer class="cn-footer"><button class="cn-record" @click="desktop.updateSettings({ paused: !settings?.paused })"><span class="cn-status-dot" :class="{ paused: settings?.paused }"></span>{{ settings?.paused ? '已暂停记录' : '正在记录' }}</button><div v-if="showProgress" class="cn-background-progress"><Icon name="Files" /><span>{{ conversionLabel }}</span><progress :value="completed.length" :max="Math.max(1, jobs.length)"></progress></div><span v-else class="cn-footer-note">本地保存 · 七天历史</span><button class="cn-settings-button" title="设置" @click="settingsOpen = true"><Icon name="Settings2" /></button></footer>
    <RecordPreview v-if="preview || newPhrase" :record="preview" :new-phrase="newPhrase" @close="preview = null; newPhrase = false" @error="error = $event" />
    <SettingsDialog v-if="settingsOpen && settings" :settings="settings" :storage-bytes="storageBytes" @close="settingsOpen = false" @saved="settingsOpen = false" @error="error = $event" @exit="requestExit" />
    <div v-if="context" class="cn-contextmenu cn-delete-bubble" :style="{ left: `${context.x}px`, top: `${context.y}px` }"><button @click="remove([context!.id])"><Icon name="Trash2" />{{ view === 'favorites' ? '取消收藏' : '删除记录' }}</button></div>
    <div v-if="manageMenu" class="cn-contextmenu cn-manage-popover" :style="{ left: `${manageMenu.x}px`, top: `${manageMenu.y}px` }"><button @click="manage = true; selected = []; manageMenu = null"><Icon name="ListChecks" />批量选择</button><template v-if="view === 'clipboard'"><button @click="clearRange(false)"><Icon name="CalendarX" />清空当天记录</button><button @click="clearRange(true)"><Icon name="Trash2" />清空七天记录</button><small>收藏与常用短语会保留</small></template></div>
    <div v-if="deletion" class="cn-overlay" @pointerdown.self="deletion = null"><section class="cn-dialog"><h3>清空记录？</h3><p>{{ deletion.message }}</p><div class="cn-dialog-footer"><button class="cn-secondary" @click="deletion = null">取消</button><button class="cn-primary" @click="confirmDeletion">确认清空</button></div></section></div>
    <div v-if="toast" class="cn-toast" role="status">{{ toast }}</div>
    <div v-if="error" class="cn-error" role="alert"><Icon name="CircleAlert" /><span>{{ error }}</span><button title="关闭提示" @click="error = ''"><Icon name="X" /></button></div>
    <div v-if="ocrText !== null" class="cn-overlay" @pointerdown.self="ocrText = null"><section class="cn-dialog"><div class="cn-dialog-heading"><div><h3>提取文字</h3><p>{{ ocrText ? '按原行显示，可编辑后手动复制' : '没有识别到文字，请重新框选' }}</p></div><button title="关闭" @click="ocrText = null"><Icon name="X" /></button></div><textarea v-model="ocrText" wrap="off" aria-label="识别文字" class="cn-ocr-text"></textarea><div class="cn-dialog-footer"><small>识别内容不会自动复制</small><button class="cn-primary" :disabled="!ocrText" @click="desktop.perform(async () => { await native.copyText(ocrText!); desktop.message('文字已复制'); })">复制文字</button></div></section></div>
    <div v-if="exitRequested" class="cn-overlay"><section class="cn-dialog"><h3>还有转换任务</h3><p>退出将取消未完成任务。关闭窗口可继续后台转换。</p><div class="cn-dialog-footer"><button class="cn-secondary" @click="exitRequested = false; desktop.perform(() => window.hide())">继续后台转换</button><button class="cn-primary" @click="desktop.perform(() => native.exit())">取消任务并退出</button></div></section></div>
  </main>
</template>
