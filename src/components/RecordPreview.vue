<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';
import DOMPurify from 'dompurify';
import type { ClipRecord } from '../types';
import { native, imageUrl } from '../services/native';
import Icon from './Icon.vue';
const props = defineProps<{ record: ClipRecord | null; newPhrase: boolean }>();
const emit = defineEmits<{ close: []; error: [message: string] }>();
const rich = ref(true), editing = ref(props.newPhrase), saving = ref(false), text = ref(props.record?.text || '');
const editor = ref<HTMLElement | null>(null);
const phraseName = ref(props.record?.name || '');
const html = computed(() => sanitize(props.record?.html || ''));
function sanitize(value: string) { return DOMPurify.sanitize(value, { USE_PROFILES: { html: true }, FORBID_TAGS: ['style', 'form', 'input', 'button', 'iframe', 'video', 'audio'], FORBID_ATTR: ['autofocus', 'srcset'] }); }
async function edit() { editing.value = true; text.value = props.record?.text || ''; await nextTick(); if (editor.value) editor.value.innerHTML = html.value; }
async function save() {
  saving.value = true;
  try {
    const formatted = rich.value && !!props.record?.html;
    const content = formatted ? editor.value?.innerText || '' : text.value;
    const richContent = formatted ? sanitize(editor.value?.innerHTML || '') : null;
    await native.edit(props.record?.id || null, content, richContent, props.newPhrase || props.record?.kind === 'phrase', phraseName.value || null); emit('close');
  } catch (e) { emit('error', String(e)); } finally { saving.value = false; }
}
async function action(callback: () => Promise<unknown>) { try { await callback(); } catch (e) { emit('error', String(e)); } }
</script>
<template>
  <div class="cn-preview-backdrop" @pointerdown.self="emit('close')">
    <section class="cn-detail" role="dialog" aria-label="内容预览">
      <div class="cn-detail-heading"><span>{{ newPhrase ? '新增常用短语' : '内容预览' }}</span><div class="cn-preview-heading-actions"><label v-if="record?.kind !== 'image'" class="cn-format-check"><input v-model="rich" type="checkbox" :disabled="editing" />带格式</label><button title="关闭预览" @click="emit('close')"><Icon name="X" /></button></div></div>
      <div class="cn-detail-content">
        <label v-if="editing && (newPhrase || record?.kind === 'phrase')" class="cn-phrase-label">名称<input v-model="phraseName" maxlength="60" placeholder="例如：收到回复" /></label>
        <img v-if="record?.imagePath" class="cn-preview-image" :src="imageUrl(record.imagePath)" alt="完整图片" />
        <template v-else-if="editing"><div v-if="rich && record?.html" ref="editor" class="cn-rich-preview cn-editable" contenteditable="true" role="textbox" aria-label="编辑格式内容" @click="($event.target as HTMLElement).closest('a') && $event.preventDefault()"></div><textarea v-else v-model="text" class="cn-edit-text" autofocus aria-label="编辑文字"></textarea></template>
        <div v-else-if="rich && record?.html" class="cn-rich-preview" @click.prevent v-html="html"></div>
        <div v-else class="cn-text-preview">{{ record?.text }}</div>
      </div>
      <div v-if="record" class="cn-detail-meta"><span>{{ record.source }}</span><span>{{ new Date(record.createdAt).toLocaleString('zh-CN') }}</span><span v-if="record.html">包含原格式</span></div>
      <div class="cn-detail-actions"><template v-if="editing"><button class="cn-primary" :disabled="saving" @click="save"><Icon name="Check" />{{ saving ? '保存中…' : '保存' }}</button><button class="cn-secondary" :disabled="saving" @click="newPhrase ? emit('close') : editing = false">取消</button></template><template v-else><button v-if="record?.kind !== 'image'" class="cn-secondary" @click="edit"><Icon name="Pencil" />编辑</button><button class="cn-primary" @click="action(() => native.copy(record!.id, rich))"><Icon name="Copy" />复制</button><button v-if="record?.kind === 'image'" class="cn-secondary" @click="action(() => native.pin(record!.id))"><Icon name="Pin" />贴图</button></template></div>
    </section>
  </div>
</template>
