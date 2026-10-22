<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { ConversionJob } from '../types';
import { native } from '../services/native';
import Icon from './Icon.vue';
const props = defineProps<{ jobs: ConversionJob[]; configured: boolean; showProgress: boolean }>();
const emit = defineEmits<{ error: [message: string]; settings: [] }>();
const tab = ref<'pending' | 'done'>('pending'), selected = ref<string[]>([]), dragging = ref(false);
let unlisten: UnlistenFn | undefined;
let disposed = false;
const completed = computed(() => props.jobs.filter(j => j.status === 'completed'));
const pending = computed(() => props.jobs.filter(j => j.status !== 'completed'));
const visible = computed(() => tab.value === 'done' ? completed.value : pending.value);
const active = computed(() => pending.value.filter(j => ['queued', 'running', 'paused'].includes(j.status)));
const labels: Record<string, string> = { queued: '排队中', running: '转换中', paused: '已暂停', completed: '已完成', failed: '转换失败', cancelled: '已取消' };
async function action(callback: () => Promise<unknown>) { try { await callback(); } catch (e) { emit('error', String(e)); } }
async function choose() { await action(async () => { const paths = await open({ multiple: true, title: '批量选择 Word / PDF', filters: [{ name: 'Word / PDF', extensions: ['doc', 'docx', 'pdf'] }] }); if (paths) await native.queue(typeof paths === 'string' ? [paths] : paths); }); }
async function controlAll(actionName: string) { await action(async () => { for (const job of active.value) if ((actionName === 'pause' && job.status !== 'paused') || (actionName === 'resume' && job.status === 'paused') || actionName === 'cancel') await native.controlJob(job.id, actionName); }); }
async function remove(ids: string[]) { await action(async () => { await native.removeJobs(ids); selected.value = []; }); }
function select(id: string) { selected.value = selected.value.includes(id) ? selected.value.filter(v => v !== id) : [...selected.value, id]; }
function name(path: string) { return path.split(/[\\/]/).pop() || path; }
onMounted(async () => { try { const stop = await getCurrentWebview().onDragDropEvent(event => {
  if (disposed) return;
  const payload = event.payload;
  if (payload.type === 'over' || payload.type === 'enter') dragging.value = true;
  if (payload.type === 'leave' || payload.type === 'drop') dragging.value = false;
  if (payload.type === 'drop') void action(() => native.queue(payload.paths));
}); if (disposed) stop(); else unlisten = stop; } catch (e) { if (!disposed) emit('error', String(e)); } });
onUnmounted(() => { disposed = true; unlisten?.(); });
</script>
<template>
  <section class="cn-conversion">
    <div class="cn-convert-heading"><div><h2>文件转换</h2><p>Word ⇄ PDF · 本地离线处理</p></div><button class="cn-secondary" @click="emit('settings')"><Icon name="Settings2" />组件设置</button></div>
    <button class="cn-drop-zone" :class="{ 'is-drag-over': dragging }" @click="choose"><Icon name="Files" /><strong>拖入 Word 或 PDF 文件</strong><span>支持批量 · 也可点击选择</span><small>输出到原文件夹，重名自动编号，原文件保留</small></button>
    <div v-if="!configured" class="cn-component-missing"><Icon name="Info" /><span>首次使用请下载离线转换组件</span><button @click="emit('settings')">自动下载</button></div>
    <div class="cn-convert-tabs"><button :class="{ 'is-active': tab === 'pending' }" @click="tab = 'pending'; selected = []">未转换 <small>{{ pending.length }}</small></button><button :class="{ 'is-active': tab === 'done' }" @click="tab = 'done'; selected = []">已转换 <small>{{ completed.length }}</small></button><span></span><template v-if="tab === 'pending' && active.length"><button title="暂停后继续将从当前文件开头转换" @click="controlAll('pause')"><Icon name="Pause" />暂停</button><button @click="controlAll('resume')"><Icon name="Play" />继续</button><button @click="controlAll('cancel')"><Icon name="X" />取消</button></template></div>
    <div v-if="tab === 'done' && completed.length" class="cn-manage-bar"><label><input type="checkbox" :checked="completed.every(j => selected.includes(j.id))" @change="selected = ($event.target as HTMLInputElement).checked ? completed.map(j => j.id) : []" />全选</label><span>只删除记录，保留文件</span><button :disabled="!selected.length" @click="remove(selected)"><Icon name="Trash2" />删除记录</button></div>
    <div class="cn-conversion-list"><article v-for="job in visible" :key="job.id" class="cn-conversion-row"><input v-if="tab === 'done'" type="checkbox" :checked="selected.includes(job.id)" aria-label="选择记录" @change="select(job.id)" /><Icon :name="job.input.toLowerCase().endsWith('.pdf') ? 'FileText' : 'FileType2'" /><div class="cn-job-info"><strong :title="job.input">{{ name(job.input) }}</strong><small v-if="job.output" :title="job.output">已保存：{{ job.output }}</small><small v-else-if="job.error" class="cn-job-error">{{ job.error }}</small><small v-else>{{ job.status === 'paused' ? '继续后从该文件开头重新转换' : job.status === 'running' ? '正在处理，具体时长取决于文件内容' : '默认保存到来源文件夹' }}</small></div><span class="cn-job-status" :class="job.status">{{ labels[job.status] }}</span><div class="cn-job-actions"><template v-if="['queued', 'running'].includes(job.status)"><button title="暂停当前任务" @click="action(() => native.controlJob(job.id, 'pause'))"><Icon name="Pause" /></button><button title="取消" @click="action(() => native.controlJob(job.id, 'cancel'))"><Icon name="X" /></button></template><template v-else-if="job.status === 'paused'"><button title="重新开始当前文件" @click="action(() => native.controlJob(job.id, 'resume'))"><Icon name="Play" /></button><button title="取消" @click="action(() => native.controlJob(job.id, 'cancel'))"><Icon name="X" /></button></template><template v-else-if="job.status === 'failed' || job.status === 'cancelled'"><button title="重试" @click="action(() => native.controlJob(job.id, 'retry'))"><Icon name="RotateCcw" /></button><button title="删除记录" @click="remove([job.id])"><Icon name="Trash2" /></button></template><button v-else title="删除记录，保留文件" @click="remove([job.id])"><Icon name="Trash2" /></button></div></article><div v-if="!visible.length" class="cn-empty">{{ tab === 'done' ? '还没有已转换记录' : '没有等待处理的文件' }}</div></div>
    <div v-if="showProgress" class="cn-convert-progress"><div><span>{{ active.length ? active.every(j => j.status === 'paused') ? '转换已暂停' : '正在处理转换队列' : '本轮处理已结束' }}</span><small>已完成 {{ completed.length }} / {{ jobs.length }}</small></div><progress :value="completed.length" :max="Math.max(1, jobs.length)"></progress><small>{{ active.length ? '按成功文件数显示进度；当前文件处理期间不估算百分比' : '成功结果已保存，失败项可在未转换列表重试' }}</small></div>
  </section>
</template>
