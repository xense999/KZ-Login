// 右鍵選單貼著視窗邊緣時往內收，不然最右／最下那一條會掉到視窗外。
// 尺寸照 main.css 的 .ctx-menu／.ctx-item／.ctx-sep 與主視窗大小算，改那邊要一起改這裡。
const MENU_RIGHT = 252;
const MENU_BOTTOM = 623;
const MENU_PAD = 10;
const MENU_ITEM_H = 31;
const MENU_SEP_H = 7;

export function contextMenuStyle(x: number, y: number, items: number, seps = 0) {
  const height = MENU_PAD + items * MENU_ITEM_H + seps * MENU_SEP_H;
  return {
    left: `${Math.min(x, MENU_RIGHT)}px`,
    top: `${Math.min(y, MENU_BOTTOM - height)}px`,
  };
}
