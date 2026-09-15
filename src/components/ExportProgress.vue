<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  title: string;
  total: number;
  done: number;
  ok: number;
  running: boolean;
  stopped: boolean;
  error: string;
  copyError: string;
  hasData: boolean;
  expiresAt: number | null;
  copied: boolean;
}>();

const emit = defineEmits<{ stop: []; recopy: []; close: [] }>();

const failed = computed(() => props.done - props.ok);

// 到期時間用「第一筆」起算：第一筆最早死，寫最後一筆會給出過度樂觀的死線。
const expiry = computed(() => {
  if (!props.expiresAt) return "";
  const d = new Date(props.expiresAt);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
});

const percent = computed(() =>
  props.total ? Math.round((props.done / props.total) * 100) : 0
);
</script>

<template>
  <Teleport to=".page-container">
  <div class="overlay">
    <div class="card">
      <div class="title">{{ title }}</div>

      <template v-if="error">
        <div class="err">{{ error }}</div>
      </template>

      <template v-else>
        <div class="count">
          <span class="num">{{ running ? done : ok }}</span>
          <span class="sep">/</span>
          <span class="total">{{ total }}</span>
        </div>
        <div class="bar"><div class="fill" :style="{ width: percent + '%' }"></div></div>
        <div class="note">
          <template v-if="running">取得中，請稍候…</template>
          <template v-else>
            <template v-if="copied">已複製到剪貼簿</template>
            <template v-else-if="copyError">複製到剪貼簿失敗，資料還在，可按「再複製一次」</template>
            <template v-else>沒有可複製的資料</template>
            <template v-if="stopped">（已停止）</template>
            <template v-if="expiry"> · 約 {{ expiry }} 前有效</template>
            <div v-if="failed > 0" class="failed">{{ failed }} 筆取得失敗，那幾列已標註在表上</div>
            <div v-if="copyError" class="failed">{{ copyError }}</div>
          </template>
        </div>
      </template>

      <div class="actions">
        <template v-if="running">
          <button class="btn" @click="emit('stop')">停止</button>
        </template>
        <template v-else>
          <button v-if="hasData" class="btn" @click="emit('recopy')">再複製一次</button>
          <button class="btn primary" @click="emit('close')">關閉</button>
        </template>
      </div>
    </div>
  </div>
  </Teleport>
</template>

<style scoped>
.overlay {
  /* absolute 不是 fixed：Teleport 到 .page-container，所以蓋的是標題列以下的
     整個程式範圍，圓角由視窗外框裁切。 */
  position: absolute;
  inset: 0;
  z-index: 2100;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(0, 0, 0, 0.42);
  backdrop-filter: blur(3px);
  -webkit-backdrop-filter: blur(3px);
}
.card {
  width: 100%;
  max-width: 300px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 20px;
  box-shadow: var(--ctx-shadow);
}
.title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
  margin-bottom: 14px;
}
.count {
  display: flex;
  align-items: baseline;
  justify-content: center;
  gap: 4px;
  font-variant-numeric: tabular-nums;
}
.num { font-size: 30px; font-weight: 600; color: var(--text); }
.sep { font-size: 18px; color: var(--text3); }
.total { font-size: 18px; color: var(--text2); }
.bar {
  margin-top: 12px;
  height: 4px;
  border-radius: 2px;
  background: var(--spin-track);
  overflow: hidden;
}
.fill {
  height: 100%;
  background: var(--primary-color);
  transition: width 0.15s ease;
}
.note {
  margin-top: 12px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text2);
  text-align: center;
}
.failed { color: var(--warn); margin-top: 2px; }
.err {
  font-size: 13px;
  line-height: 1.6;
  color: var(--red);
}
.actions {
  display: flex;
  gap: 8px;
  margin-top: 18px;
}
.btn {
  flex: 1;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface2);
  color: var(--text2);
  font-size: 13px;
  cursor: pointer;
}
.btn:hover { background: var(--surface3); color: var(--text); }
.btn.primary {
  background: var(--primary-bg);
  border-color: var(--primary-border);
  color: var(--primary-color);
}
.btn.primary:hover { background: var(--primary-bg-hover); }
</style>
