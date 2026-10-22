<script setup lang="ts">
import MainWindow from './components/MainWindow.vue';
import CaptureOverlay from './components/CaptureOverlay.vue';
import PinWindow from './components/PinWindow.vue';
const mode = new URLSearchParams(location.search).get('mode');
</script>
<template>
  <CaptureOverlay v-if="mode === 'capture'" />
  <PinWindow v-else-if="mode === 'pin'" />
  <MainWindow v-else />
</template>
