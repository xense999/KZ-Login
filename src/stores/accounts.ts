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

  // 哪張卡片是這批遊戲帳號的主人。遊戲帳號的 `sn` 是 beanfun 發的序號，不會變、
  // 也不會屬於兩個 beanfun 帳號，所以只要有一個對得上就是同一個帳號——不管這次
  // 是掃碼、打帳密還是 GamaPass 登入的，也不管暱稱、信箱、手機後來改成什麼。
  function findByGames(games: { sn: string }[]): BeanfunAccount | undefined {
    const sns = new Set(games.map((g) => g.sn));
    return accounts.value.find((a) => a.gameAccounts.some((g) => sns.has(g.sn)));
  }

  // GamaPass 帳號與 beanfun 帳號是兩個名字空間：同一串字（例如一支手機號碼）在
  // 兩邊是不同的人，所以找卡片時登入方式也要同一邊。
  function findByLoginAccount(account: string, method: LoginMethod): BeanfunAccount | undefined {
    const gamapass = method === "gamapass";
    return accounts.value.find((a) =>
      a.loginAccount !== null &&
      (a.loginMethod === "gamapass") === gamapass &&
      sameLoginAccount(a.loginAccount, account));
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
    // A login that names no account (QR, or a GamaPass one the user finished by
    // hand) keeps the one we had — unless the card is changing sides: GamaPass
    // and beanfun accounts are separate namespaces, and a name kept across would
    // be looked up in the wrong one.
    const switchedSides = (acc.loginMethod === "gamapass") !== (login.method === "gamapass");
    acc.loginMethod = login.method;
    if (login.account) acc.loginAccount = login.account;
    else if (switchedSides) acc.loginAccount = null;
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

  return { accounts, lastUsedSn, findByGames, findByLoginAccount, markUsed, addAccount, updateAlias, removeAccount, moveAccount, moveGameAccount, updateGameName, invalidateToken, updateToken };
});
