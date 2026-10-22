export type View = 'clipboard' | 'favorites' | 'phrases' | 'conversion';
export type CaptureMode = 'copy' | 'stick' | 'text';
export interface ClipRecord {
  id: string;
  name: string | null;
  kind: 'text' | 'image' | 'phrase';
  text: string;
  html: string | null;
  imagePath: string | null;
  createdAt: number;
  updatedAt: number;
  favorite: boolean;
  historyDeleted: boolean;
  source: string;
  sortOrder: number;
  mergeCount: number;
  width: number;
  height: number;
}
export interface Settings {
  pasteFormatted: boolean;
  mergeDuplicates: boolean;
  paused: boolean;
  alwaysOnTop: boolean;
  retentionDays: number;
  imageBudgetMb: number;
  ignoredApps: string[];
  shortcuts: Record<string, string>;
  componentPath: string;
}
export interface ConversionJob {
  id: string;
  input: string;
  output: string | null;
  status: 'queued' | 'running' | 'paused' | 'completed' | 'failed' | 'cancelled';
  progress: number | null;
  error: string | null;
  createdAt: number;
}
export interface CaptureSession {
  id: string;
  mode: CaptureMode;
  imagePath: string;
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface PinData {
  recordId: string;
  imagePath: string;
  width: number;
  height: number;
}
export interface Bootstrap {
  records: ClipRecord[];
  settings: Settings;
  jobs: ConversionJob[];
  storageBytes: number;
}
export interface ComponentDownload {
  status: 'idle' | 'downloading' | 'installing' | 'ready' | 'failed' | 'cancelled';
  message: string;
  downloaded: number;
  total: number;
  configPath: string;
  error: string | null;
  available: boolean;
}
