import { defineStore } from "pinia";
import { ref } from "vue";

export interface GameAccount {
  sn: string;
  sid: string;
  sname: string;
  localName: string | null;
}

export type LoginMethod = "qr" | "password" | "gamapass";

// A game account as a login returns it, before any local renaming.
export type LoginGame = { sn: string; sid: string; sname: string };

// beanfun ignores case in account names, which are ASCII. Mirrors
// `same_account` in the Rust credentials module; change both together.
const foldAscii = (s: string) => s.trim().replace(/[A-Z]/g, (c) => c.toLowerCase());

export function sameLoginAccount(a: string, b: string): boolean {
  return foldAscii(a) !== "" && foldAscii(a) === foldAscii(b);
}

export interface BeanfunAccount {
  id: string;
  alias: string;
  email: string;
  token: string | null;
  gameAccounts: GameAccount[];
  loginMethod: LoginMethod;
  // The beanfun account typed into the password form; never the password.
  loginAccount: string | null;
}

export interface LoginResult {
  token: string;
  games: LoginGame[];
  method: LoginMethod;
  account: string | null;
}

export const useAccountsStore = defineStore("accounts", () => {
  const accounts = ref<BeanfunAccount[]>([]);
  const lastUsedSn = ref<string | null>(null);

  function findByLoginAccount(account: string): BeanfunAccount | undefined {
    return accounts.value.find((a) => a.loginAccount !== null && sameLoginAccount(a.loginAccount, account));
  }

  function markUsed(sn: string) {
    lastUsedSn.value = sn;
  }

  function addAccount(account: BeanfunAccount) {
    accounts.value.push(account);
  }

  function updateAlias(accountId: string, alias: string) {
    const acc = accounts.value.find((a) => a.id === accountId);
    if (acc) acc.alias = alias;
  }

  function updateGameName(accountId: string, sn: string, name: string) {
    const acc = accounts.value.find((a) => a.id === accountId);
    const game = acc?.gameAccounts.find((g) => g.sn === sn);
    if (game) game.localName = name || null;
  }

  function removeAccount(accountId: string) {
    const idx = accounts.value.findIndex((a) => a.id === accountId);
    if (idx !== -1) accounts.value.splice(idx, 1);
  }

  function moveAccount(fromIdx: number, toIdx: number) {
    if (fromIdx === toIdx) return;
    const list = [...accounts.value];
    const [item] = list.splice(fromIdx, 1);
    list.splice(toIdx, 0, item);
    accounts.value = list;
  }

  function moveGameAccount(accountId: string, fromIdx: number, toIdx: number) {
    const acc = accounts.value.find((a) => a.id === accountId);
    if (!acc || fromIdx === toIdx) return;
    const games = [...acc.gameAccounts];
    const [item] = games.splice(fromIdx, 1);
    games.splice(toIdx, 0, item);
    acc.gameAccounts = games;
  }

  function invalidateToken(accountId: string) {
    const acc = accounts.value.find((a) => a.id === accountId);
    if (acc) acc.token = null;
  }

  function updateToken(accountId: string, login: LoginResult) {
    const acc = accounts.value.find((a) => a.id === accountId);
    if (!acc) return;
    const { token, games: newGames } = login;
    acc.token = token;
    acc.loginMethod = login.method;
    // A QR login says nothing about the typed account, so keep the one we had.
    if (login.account) acc.loginAccount = login.account;
    const existingMap = new Map(acc.gameAccounts.map((g) => [g.sn, g]));
    const newMap = new Map(newGames.map((g) => [g.sn, g]));
    // Preserve existing custom order; append new accounts sorted by sn
    const preserved = acc.gameAccounts
      .filter((g) => newMap.has(g.sn))
      .map((g) => {
        const fresh = newMap.get(g.sn)!;
        return { sn: fresh.sn, sid: fresh.sid, sname: fresh.sname, localName: g.localName };
      });
    const added = [...newGames]
      .reverse()
      .filter((g) => !existingMap.has(g.sn))
      .map((g) => ({ sn: g.sn, sid: g.sid, sname: g.sname, localName: null }));
    acc.gameAccounts = [...preserved, ...added];
  }

  return { accounts, lastUsedSn, findByLoginAccount, markUsed, addAccount, updateAlias, removeAccount, moveAccount, moveGameAccount, updateGameName, invalidateToken, updateToken };
});
