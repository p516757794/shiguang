<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type { Settings, ComponentDownload } from '../types';
import { native } from '../services/native';
import Icon from './Icon.vue';
const props = defineProps<{ settings: Settings; storageBytes: number }>();
const emit = defineEmits<{ close: []; saved: []; error: [message: string]; exit: [] }>();
const draft = ref<Settings>(JSON.parse(JSON.stringify(props.settings)));
const ignored = ref(draft.value.ignoredApps.join('\n')), saving = ref(false);
const pane = ref('general'), shortcutError = ref(''), confirmImages = ref(false);
const component = ref<ComponentDownload | null>(null), startingDownload = ref(false), cancelling = ref(false), advanced = ref(false);
const componentBusy = computed(() => startingDownload.value || ['downloading', 'installing'].includes(component.value?.status || ''));
const downloadPercent = computed(() => component.value?.status === 'ready' ? 100 : component.value?.status === 'installing' ? 95 : Math.min(85, Math.floor((component.value?.downloaded || 0) / Math.max(1, component.value?.total || 1) * 85)));
let stopComponents: UnlistenFn | undefined, disposed = false, componentVersion = 0;
function applyComponent(status: ComponentDownload) { component.value = status; if (!['downloading', 'installing'].includes(status.status)) cancelling.value = false; }
watch(() => props.settings.componentPath, path => { draft.value.componentPath = path; });
async function download() { if (componentBusy.value) return; startingDownload.value = true; try { await native.downloadComponents(); const version = componentVersion; const status = await native.componentStatus(); if (!disposed && version === componentVersion) applyComponent(status); } catch (e) { emit('error', String(e)); } finally { startingDownload.value = false; } }
async function cancelDownload() { cancelling.value = true; try { await native.cancelComponentDownload(); } catch (e) { cancelling.value = false; emit('error', String(e)); } }
onMounted(async () => { try { const stop = await listen<ComponentDownload>('components-changed', e => { if (!disposed) { ++componentVersion; applyComponent(e.payload); } }); if (disposed) { stop(); return; } stopComponents = stop; const version = componentVersion; const status = await native.componentStatus(); if (!disposed && version === componentVersion) applyComponent(status); } catch (e) { if (!disposed) emit('error', String(e)); } });
onUnmounted(() => { disposed = true; stopComponents?.(); });
const keys = [{ id: 'clipboard', name: '唤起剪贴板' }, { id: 'capture', name: '截图' }, { id: 'stick', name: '贴图' }, { id: 'ocr', name: '框选识字' }];
async function save() { saving.value = true; try { draft.value.ignoredApps = ignored.value.split('\n').map(v => v.trim()).filter(Boolean); await native.settings(draft.value); emit('saved'); } catch (e) { emit('error', String(e)); } finally { saving.value = false; } }
async function choose() { try { const path = await open({ title: '选择本地组件配置 JSON', multiple: false, filters: [{ name: '组件配置', extensions: ['json'] }] }); if (typeof path === 'string') draft.value.componentPath = path; } catch (e) { emit('error', String(e)); } }
function captureKey(event: KeyboardEvent, id: string) {
  if (event.key === 'Tab') return; event.preventDefault(); event.stopPropagation(); shortcutError.value = '';
  if (event.key === 'Escape') { draft.value.shortcuts[id] = props.settings.shortcuts[id] || ''; return; }
  if (event.key === 'Backspace' || event.key === 'Delete') { draft.value.shortcuts[id] = ''; return; }
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;
  if (!event.ctrlKey && !event.altKey && !event.metaKey) { shortcutError.value = '请使用包含 Ctrl、Alt 或 Win 的组合键'; return; }
  const key = event.code.startsWith('Key') ? event.code.slice(3) : event.code.startsWith('Digit') ? event.code.slice(5) : event.code;
  draft.value.shortcuts[id] = [event.ctrlKey ? 'Ctrl' : '', event.altKey ? 'Alt' : '', event.shiftKey ? 'Shift' : '', event.metaKey ? 'Super' : '', key].filter(Boolean).join('+');
}
async function clearImages() { if (!confirmImages.value) { confirmImages.value = true; return; } try { await native.clearImages(); confirmImages.value = false; } catch (e) { emit('error', String(e)); } }
</script>
<template>
  <div class="cn-settings-overlay" @pointerdown.self="emit('close')"><section class="cn-dialog cn-settings-dialog" role="dialog" aria-label="设置"><div class="cn-dialog-heading"><div><h3>设置</h3><p>常用操作，简单一点</p></div><button title="关闭设置" @click="emit('close')"><Icon name="X" /></button></div>
    <nav class="cn-settings-nav" aria-label="设置分类"><button v-for="item in [{ id: 'general', label: '常规' }, { id: 'shortcuts', label: '快捷键' }, { id: 'storage', label: '记录与空间' }]" :key="item.id" :class="{ 'is-active': pane === item.id }" @click="pane = item.id">{{ item.label }}</button></nav>
    <div class="cn-setting-scroll"><section v-show="pane === 'general'">
      <label class="cn-setting-row"><span>窗口置顶</span><input v-model="draft.alwaysOnTop" type="checkbox" /></label>
      <div class="cn-setting-row"><span>关闭主窗口<small>继续记录剪贴板，转换任务继续处理</small></span><strong>留在托盘</strong></div>
      <h4>离线转换组件</h4>
      <div class="cn-component-card">
        <div class="cn-component-heading"><span><strong>{{ componentBusy ? '正在准备转换组件' : draft.componentPath ? '转换组件已配置' : '一键准备，之后离线使用' }}</strong><small>{{ componentBusy ? (cancelling ? '正在取消，请稍候…' : component?.message) : component?.status === 'failed' ? '下载未完成，可以重试' : 'Word ⇄ PDF · 文件仅在本机处理' }}</small></span><Icon :name="componentBusy ? 'Download' : draft.componentPath ? 'Check' : 'Package'" /></div>
        <template v-if="componentBusy || ['failed', 'cancelled', 'ready'].includes(component?.status || '')"><progress class="cn-component-progress" :value="downloadPercent" max="100"></progress><div class="cn-component-meta"><span>{{ component?.message }}</span><span>{{ ((component?.downloaded || 0) / 1048576).toFixed(1) }} / {{ Math.ceil((component?.total || 0) / 1048576) }} MB</span></div></template>
        <p v-if="component?.error" class="cn-component-error" role="alert">{{ component.error }}</p>
        <div class="cn-component-buttons"><button v-if="!componentBusy" class="cn-primary" :disabled="!component?.available" @click="download"><Icon name="Download" />{{ component?.status === 'ready' ? '使用已下载组件' : component?.status === 'failed' || component?.status === 'cancelled' ? '重试下载' : '自动下载组件' }}</button><button v-else class="cn-secondary" :disabled="cancelling" @click="cancelDownload">{{ cancelling ? '正在取消…' : '取消下载' }}</button><span v-if="!componentBusy">约 {{ Math.ceil((component?.total || 317409058) / 1048576) }} MB · 建议预留 2 GB 空间</span></div>
        <small>首次联网下载，完成后自动配置。关闭设置或收起窗口仍会继续；转换文件不会上传。</small>
        <details class="cn-component-notices"><summary>组件来源与许可</summary><p>拾光源码采用 AGPL-3.0-only，可按协议修改及分发，不提供担保。完整协议和第三方说明随安装包提供；源码：github.com/p516757794/shiguang。</p><p>使用 LibreOffice Portable、Python、PyPI 和 7-Zip 发行包，逐包校验 SHA-256。7-Zip 官方地址连接失败时使用 gh-proxy.com 第三方备用传输，仍核对固定版本哈希。pdf2docx 使用 MIT 许可；其 PyMuPDF 依赖采用 AGPL-3.0 或商业许可。许可和版权文件随组件保留。</p></details>
      </div>
      <button class="cn-component-advanced" :disabled="componentBusy" @click="advanced = !advanced">{{ advanced ? '收起手动配置' : '高级：手动配置' }}</button><div v-if="advanced" class="cn-component-choice"><span :title="draft.componentPath">{{ draft.componentPath || '尚未配置' }}</span><button class="cn-secondary" @click="choose">选择配置</button></div>
      <small>暂停转换后继续会重新处理当前文件。</small>
      <button class="cn-exit-button cn-secondary" @click="emit('exit')"><Icon name="LogOut" />退出拾光</button>
      </section><section v-show="pane === 'shortcuts'">
      <h4>全局快捷键</h4><label v-for="key in keys" :key="key.id" class="cn-shortcut-row"><span>{{ key.name }}</span><input v-model="draft.shortcuts[key.id]" :aria-label="`${key.name}快捷键`" placeholder="点击后按下组合键" spellcheck="false" @keydown="captureKey($event, key.id)" /></label><small>点击输入框并按组合键；Esc 取消，Backspace 清除。冲突时恢复原快捷键。</small><p v-if="shortcutError" class="cn-shortcut-error">{{ shortcutError }}</p>
      </section><section v-show="pane === 'storage'">
      <label class="cn-setting-row"><span>重复内容合并<small>相同内容和格式合并为一条，移到最新日期</small></span><input v-model="draft.mergeDuplicates" type="checkbox" /></label>
      <div class="cn-setting-row"><span>记录保留<small>收藏与常用短语长期保留</small></span><strong>最近 7 天</strong></div>
      <label class="cn-setting-row"><span>图片存储上限<small>超出后清理最旧的普通图片，收藏不受影响</small></span><select v-model.number="draft.imageBudgetMb"><option :value="100">100 MB</option><option :value="500">500 MB</option><option :value="1024">1 GB</option><option :value="2048">2 GB</option></select></label>
      <div class="cn-storage-usage">图片占用 <strong>{{ (storageBytes / 1024 / 1024).toFixed(1) }} MB</strong><div class="cn-storage-track"><span :style="{ width: `${Math.min(100, storageBytes / (draft.imageBudgetMb * 1024 * 1024) * 100)}%` }"></span></div></div><button class="cn-secondary" @click="clearImages"><Icon name="Trash2" />{{ confirmImages ? '确认清理未收藏图片' : '清理未收藏图片' }}</button><small>仅移除工具记录，保留电脑上的原图片。活动贴图关闭前暂时保留其图片数据。</small>
      <h4>不记录的窗口</h4><textarea v-model="ignored" class="cn-ignore-text" placeholder="每行填写一个窗口标题关键词"></textarea><small>匹配复制时的窗口标题；敏感信息请先暂停记录。</small>
      </section></div><div class="cn-dialog-footer"><button class="cn-secondary" :disabled="saving" @click="emit('close')">取消</button><button class="cn-primary" :disabled="saving" @click="save">{{ saving ? '保存中…' : '保存设置' }}</button></div>
  </section></div>
</template>
