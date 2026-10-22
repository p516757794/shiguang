import { computed, ref } from 'vue';
import { listen, type Event, type UnlistenFn } from '@tauri-apps/api/event';
import { native } from './native';
import type { ClipRecord, ConversionJob, Settings } from '../types';

export function useDesktop() {
  const records = ref<ClipRecord[]>([]), settings = ref<Settings | null>(null), jobs = ref<ConversionJob[]>([]);
  const error = ref(''), toast = ref(''), ocrText = ref<string | null>(null), exitRequested = ref(false);
  const ready = ref(false), completionUntil = ref(0), clock = ref(Date.now());
  const storageBytes = ref(0);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false, recordVersion = 0, settingsVersion = 0, jobsVersion = 0;
  const unlisteners: UnlistenFn[] = [];
  const interval = setInterval(() => { clock.value = Date.now(); }, 250);
  const active = computed(() => jobs.value.filter(j => ['running', 'queued', 'paused'].includes(j.status)));
  const completed = computed(() => jobs.value.filter(j => j.status === 'completed'));
  const showProgress = computed(() => active.value.length > 0 || (completed.value.length > 0 && completionUntil.value > clock.value));
  function message(text: string) { toast.value = text; clearTimeout(toastTimer); toastTimer = setTimeout(() => { toast.value = ''; }, 4200); }
  async function perform<T>(action: () => Promise<T>): Promise<T | undefined> {
    try { return await action(); } catch (e) { if (!disposed) error.value = String(e); return undefined; }
  }
  function setJobs(next: ConversionJob[]) {
    const previouslyDone = new Set(completed.value.map(j => j.id));
    if (next.some(j => j.status === 'completed' && !previouslyDone.has(j.id))) completionUntil.value = Date.now() + 5000;
    if (!next.some(j => j.status === 'completed')) completionUntil.value = 0;
    jobs.value = next;
  }
  async function subscribe<T>(name: string, callback: (event: Event<T>) => void) {
    if (disposed) return;
    const stop = await listen<T>(name, e => { if (!disposed) callback(e); });
    if (disposed) stop(); else unlisteners.push(stop);
  }
  async function refreshRecords() {
    const version = ++recordVersion;
    const [items, bytes] = await Promise.all([native.records(), native.storage()]);
    if (!disposed && version === recordVersion) { records.value = items; storageBytes.value = bytes; }
  }
  async function initialize() {
    await perform(async () => {
      await subscribe('records-changed', () => { void perform(refreshRecords); });
      await subscribe<Settings>('settings-changed', e => { ++settingsVersion; settings.value = e.payload; });
      await subscribe<ConversionJob[]>('jobs-changed', e => { ++jobsVersion; setJobs(e.payload); });
      await subscribe<string>('app-error', e => { error.value = e.payload; });
      await subscribe<string>('ocr-result', e => { ocrText.value = e.payload; });
      await subscribe('exit-request', () => { exitRequested.value = true; });
      if (disposed) return;
      const before = { records: recordVersion, settings: settingsVersion, jobs: jobsVersion };
      const result = await native.bootstrap();
      if (disposed) return;
      // 初始化期间收到的新事件优先，旧的异步读取不能覆盖新状态。
      if (recordVersion === before.records) { ++recordVersion; records.value = result.records; storageBytes.value = result.storageBytes; }
      if (settingsVersion === before.settings) settings.value = result.settings;
      if (jobsVersion === before.jobs) jobs.value = result.jobs;
      ready.value = true;
    });
  }
  async function updateSettings(change: Partial<Settings>) {
    if (!settings.value) return;
    await perform(async () => { await native.settings({ ...settings.value!, ...change }); });
  }
  function dispose() { disposed = true; unlisteners.splice(0).forEach(fn => fn()); clearTimeout(toastTimer); clearInterval(interval); }
  return { records, settings, jobs, error, toast, ocrText, exitRequested, ready, active, completed, showProgress, storageBytes, clock,
    message, perform, initialize, updateSettings, dispose };
}
