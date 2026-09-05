import { ref } from "vue";

// Discord webhook 出口（單一來源）。所有送往頻道的訊息都走這裡，
// 這樣 username／頭像／footer 只有一處定義，兩種卡片不會長得像兩個工具發的。
const WEBHOOK_KEY = "kusei:discord_webhook";
const SHARE_KEY_KEY = "kusei:share_key_to_discord";
const ADVANCED_KEY = "kusei:advanced_unlocked";

const WEBHOOK_NAME = "久世登入器";
const WEBHOOK_AVATAR =
  "https://raw.githubusercontent.com/xense999/KZ-Login/master/public/avatar.png";

export const EMBED_COLOR_LINK = 0x3dd6c3;
export const EMBED_COLOR_KEY = 0xf0a13c;

type Embed = {
  title: string;
  url?: string;
  description?: string;
  color: number;
};

export function webhookUrl(): string {
  return localStorage.getItem(WEBHOOK_KEY) ?? "";
}

// 送出即成功——刻意不看 HTTP 狀態碼（使用者拍板），只有 fetch 本身拋錯才算失敗。
// 回傳 false 代表沒設定 webhook 或送不出去，呼叫端據此決定提示文字。
export async function sendEmbed(embed: Embed): Promise<boolean> {
  const url = webhookUrl();
  if (!url) return false;
  try {
    await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        username: WEBHOOK_NAME,
        avatar_url: WEBHOOK_AVATAR,
        embeds: [
          {
            ...embed,
            timestamp: new Date().toISOString(),
            footer: { text: WEBHOOK_NAME },
          },
        ],
      }),
    });
    return true;
  } catch {
    return false;
  }
}

// 進階選項：設定頁「通知設定」標題連點五下才顯示的金鑰同步開關。
// 解鎖與開關值分開存——開關開著卻因為藏起來而關不掉，是唯一要避免的死角。
const unlocked = ref(localStorage.getItem(ADVANCED_KEY) === "1");
const shareKeyToDiscord = ref(localStorage.getItem(SHARE_KEY_KEY) === "1");

export function useDiscordShare() {
  function unlockAdvanced() {
    unlocked.value = true;
    localStorage.setItem(ADVANCED_KEY, "1");
  }
  function setShareKeyToDiscord(on: boolean) {
    shareKeyToDiscord.value = on;
    localStorage.setItem(SHARE_KEY_KEY, on ? "1" : "0");
  }
  return { unlocked, shareKeyToDiscord, unlockAdvanced, setShareKeyToDiscord };
}
