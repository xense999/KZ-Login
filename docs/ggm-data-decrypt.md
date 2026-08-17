# GGM Data Blob 解密演算法（新版 OTP 來源）

## 背景

2026-08-17 beanfun 改版（`ggm.js?0817`）：網頁版 OTP 端點
`get_webstart_otp.ashx` 開始回 `0;Query String Error`，舊的 5-step OTP 流程失效。

新流程把啟動資訊（含 `LaunchTicket`）直接加密打包在 `game_start_step2.aspx`
回應的 `m_objData.data` 裡，交給本機 GGM（`gamaniagames://` protocol，
`GGMWebStart.exe`）解密啟動。

本文件記錄從 `GGMWebStart.dll`（.NET 6、DES、未混淆）反編譯出來、
並獨立重現驗證通過的解密演算法。

## 演算法

輸入：`data` = `m_objData.data`（hex 字串，約 368+ 字元）

```
1. v       = parse_hex_digit(data[0])          // 第一個 hex 字元 → 0..15
2. n       = v % 4                             // alphabet 索引
3. rest    = data[1..]
4. decoded = 逐字元替換(rest)：
             每個字元 c → to_hex_digit( ALPHABET[n].index_of(c) )
             （單向 nibble 替換，輸出長度 == rest 長度）
5. off     = v + 1                             // ★注意：用完整 v，非 n
6. key     = decoded[off .. off+8]             // 8 ASCII 字元
7. cipher  = decoded[..off] + decoded[off+8..] // 即 Remove(off, 8)
8. plain   = DES_ECB_NoPadding_decrypt(hex_to_bytes(cipher), key.as_utf8_bytes())
             然後 trim_end 掉 '\0'
9. 解析 plain：
             params_part = plain.split(';')[0]
             params_part.split("&&&&") → 每段 "K=V"
```

## Alphabet 替換表（16 字元 hex 排列）

`b.A` 靜態欄位，共 8 組；`n = v % 4` 只會用到前 4 組：

```
[0] = bac987d65e432f10
[1] = 3bc4d5e6f2a79108
[2] = cdbeaf9012456378
[3] = 4e6fb81a3c5d7092
[4] = bdef1246789ac530   （TW 啟動流程未用到）
[5] = 5f82cb4093e71d6a
[6] = df1468ace0357b92
[7] = b50c61a4f93e82d7
```

替換方向：`decoded` 的每個字元 = `ALPHABET[n]` 中，`rest` 對應字元所在的
索引值（0..15）轉成 hex 字元。等價於一張 nibble → nibble 的對照表。

## DES 參數

- 演算法：DES
- Mode：ECB（IL 常數 2）
- Padding：None（IL 常數 1）
- Key：`Encoding.UTF8.GetBytes(key字串)`（8 字元 → 8 bytes）
- 密文：`cipher` hex 字串 → bytes
- 輸出：`Encoding.UTF8.GetString(明文).TrimEnd('\0')`

（與舊版 OTP 的 DES/ECB/NoPadding 同款，登入器現有 `des` crate 可直接沿用。）

## 解出的欄位範例

```
LaunchTicket   = <64 hex>      ← 新版啟動票／OTP 等價物
ServiceCode    = 610074
ServiceRegion  = T9
ServiceAccount = T9xxxxxxxxxxxxxxxxx
BeanfunUrl     = https://tw.beanfun.com/
WebStartPatch  = http://tw.patch.beanfun.gamania.com/beanfun05/
```

`plain` 尾端 `;` 後還有一段數字（範例 `61281`），用途未定（疑似校驗）。

## 待決

- `LaunchTicket`（64 hex）是否等同使用者要手動輸入的「密碼」，或遊戲端
  改由 GGM/protocol 直接吞、不再手打？需確認登入器的消費方式再決定 UI。

## 驗證方式

`GGMWebStart.dll` 是 .NET 6 未混淆，可用反射載入直接呼叫 `DecryptParam`
或轉換方法 `b.A(Int32,String)` 取 ground truth。上述純演算法重現與其
逐字元一致（`decoded match: True`），且解出的 `LaunchTicket` 與
`DecryptParam` 輸出相同。相依 `log4net.dll` 需從 GGM 安裝資料夾一併載入。
