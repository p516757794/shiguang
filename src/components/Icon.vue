<script setup lang="ts">
import { computed } from 'vue';
import { Circle, ScanLine, Image, ScanText, Pin, Minus, X, Clipboard, Star, MessageSquareText, Files,
  Search, Plus, ListChecks, Clock3, Type, FileType2, Eye, CircleAlert, Settings2, Copy, Check, Pencil,
  Text, Layers, Minimize2, ChevronUp, ChevronDown, FileText, Pause, Play, RotateCcw, Trash2, Info, LogOut,
  PictureInPicture2, Ellipsis, CalendarX, Download, Package } from 'lucide-vue-next';
const icons = { Circle, ScanLine, Image, ScanText, Pin, Minus, X, Clipboard, Star, MessageSquareText, Files,
  Search, Plus, ListChecks, Clock3, Type, FileType2, Eye, CircleAlert, Settings2, Copy, Check, Pencil,
  Text, Layers, Minimize2, ChevronUp, ChevronDown, FileText, Pause, Play, RotateCcw, Trash2, Info, LogOut,
  PictureInPicture2, Ellipsis, CalendarX, Download, Package };
const props = defineProps<{ name: string }>();
const icon = computed(() => icons[props.name as keyof typeof icons] || icons.Circle);
</script>
<template><component :is="icon" :size="16" :stroke-width="1.6" aria-hidden="true" /></template>
