<script setup lang="ts">
import { onMounted } from "vue";
import { useAccountsStore } from "../stores/accounts";

const props = defineProps<{
  token: string;
  games: { sn: string; sid: string; sname: string }[];
}>();
const emit = defineEmits<{ saved: [] }>();
const store = useAccountsStore();

onMounted(() => {
  const id = crypto.randomUUID();

  const nameMem: Record<string, string> = JSON.parse(
    localStorage.getItem("kusei:name_memory") ?? "{}"
  );

  // Restore the sub-account order the user arranged last time (by sid).
  const games = [...props.games];
  const orderMem: Record<string, string[]> = JSON.parse(
    localStorage.getItem("kusei:suborder_memory") ?? "{}"
  );
  const orderKey = games.map((g) => g.sid).slice().sort().join("|");
  const savedOrder = orderMem[orderKey];
  if (savedOrder) {
    const rank = new Map(savedOrder.map((sid, i) => [sid, i]));
    games.sort((a, b) => (rank.get(a.sid) ?? Infinity) - (rank.get(b.sid) ?? Infinity));
  }

  store.addAccount({
    id,
    alias: "Beanfun 帳號",
    email: "",
    token: props.token,
    gameAccounts: games.map((g) => ({
      ...g,
      localName: nameMem[g.sid] ?? null,
    })),
  });

  const key = props.games.map(g => g.sid).sort().join("|");
  const aliasMem: Record<string, string> = JSON.parse(
    localStorage.getItem("kusei:alias_memory") ?? "{}"
  );
  if (aliasMem[key]) {
    store.updateAlias(id, aliasMem[key]);
  }

  emit("saved");
});
</script>

<template>
  <div class="ok-page">
    <div class="ok-icon">
      <svg viewBox="0 0 48 48" fill="none" xmlns="http://www.w3.org/2000/svg" width="48" height="48">
        <circle cx="24" cy="24" r="22" stroke="var(--green)" stroke-width="1.5" opacity="0.25"/>
        <path d="M14 24l7 7L34 16" stroke="var(--green)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </div>
    <h2>登入成功</h2>
    <p>正在儲存…</p>
  </div>
</template>

<style scoped>
.ok-page {
  display: flex; flex-direction: column; align-items: center;
  justify-content: center; gap: 14px; flex: 1; padding: 24px;
}
h2 { font-size: 16px; font-weight: 600; color: var(--text); }
p  { font-size: 13px; color: var(--text2); }
</style>
