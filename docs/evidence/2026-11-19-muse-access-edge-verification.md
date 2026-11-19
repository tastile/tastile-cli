# Muse 接続経路と edge-case 検証証跡 (2026-11-19)

対象: root#105 / cli#6。Core 2.0.0 検証 stack への Muse 側アクセス経路の構成と、
例外日・label 専用・優先度競合・reflow 影響・通知 fake の隔離環境検証。
実運用 VM・Muse 本番 DB・port 31400/5433 には接続していない。

## 1. 検証実体 (前回証跡からの差分のみ)

| 要素 | 値 |
| --- | --- |
| Core | `tastile-core-verify:0e6ecb2f` (= `origin/release-2-0-0` tip `0e6ecb2f00038cbaa0fca149e5268f31386e8fe7`)、api `127.0.0.1:31410`、DB `127.0.0.1:55432` |
| CLI | `tastile-cli release-1-1-0 @ 582d7e1a`、`tastile 1.1.0` |
| Muse 用 owner | `muse-verify-shadow` (`b6b05a6c-40a7-4315-a592-793bb2711c4a`)、label `muse-verify-shadow`、scopes `all`、expires `2026-12-31` |
| credential | `~/.config/tastile-muse/credential.json` (mode 600、開発機ローカルのみ。repo・Issue・PR に値を書かない) |
| 自動 E2E | `scripts/muse-shadow-e2e.sh` (本 PR で追加)。同一 owner で 32 項目中 31 PASS + state read-back 修正後の再確認が残る |

## 2. Muse → 検証 Core の接続経路

Muse VM から検証 Core への経路は **SSH local forward** を採用した。

- Muse 側で `ssh -N -L 127.0.0.1:31498:127.0.0.1:31410` を開き、
  `TASTILE_API_URL=http://127.0.0.1:31498` を使う。Muse 側から見て
  loopback であるため、CLI の全 fail-closed ゲート (loopback +
  `TASTILE_ISOLATED_TEST_STACK=confirmed` + file credential + port 31400 拒否)
  がそのまま有効。
- 開発機側は Tailscale `mac` (`100.83.203.34`) の sshd のみを使う。
  専用鍵 `~/.config/tastile-muse/id_ed25519` (ed25519、passphrase なし、
  転送専用) を生成し、`~/.ssh/authorized_keys` に
  `command="/usr/bin/false",no-pty,no-user-rc,no-agent-forwarding,no-X11-forwarding,permitopen="127.0.0.1:31410"`
  付きで登録した。
- `restrict` keyword はこの host の sshd (OpenSSH_10.5p1) で
  permitopen と併用すると forwarding 自体が拒否されたため、
  同等の効果を個別 option + forced `command=/usr/bin/false` で構成した。

### fail-closed 実測 (開発機側で検証)

| 試行 | 結果 |
| --- | --- |
| `-L 31498:127.0.0.1:31410` | 成功。`/v1/health` が Core 2.0.0 を返す |
| shell 実行 (`ssh host 'echo ...'`) | forced command により終了コード 1、出力なし |
| `-L 31497:127.0.0.1:31499` (別 port への forward) | `administratively prohibited: open failed` |

これにより、鍵が漏洩・誤用されても取得できるのは検証 API への
loopback 相当の read/write のみで、開発機の shell・他 port・他 host
への forward は不可。

### 誤接続対策

- 運用 API は Muse VM 側の `127.0.0.1:31400`。検証側は Muse 側でも
  `127.0.0.1:31498` という別 port を使い、かつ CLI は port 31400 を
  read 含め全拒否する。二つの loopback が混同される経路はない。
- 検証 credential の `api_base_url` は `http://127.0.0.1:31498` に pin。
  別 base への credential 流用は CLI が送信前に拒否する。
- 検証 DB の owner は `muse-verify-shadow` 専用。既存 E2E owner
  (`tcli-e2e-*`) や運用 owner とは別 subject で、互いの source tile は
  見えない。

## 3. edge-case 検証 (muse-verify-shadow owner、隔離 DB)

`scripts/muse-shadow-e2e.sh` による自動実行 + 手動確認の結果。
証跡 JSON は実行ごとの一時 dir (`$MUSE_E2E_OUT`) に保存される。

| 項目 | 手順 | 結果 |
| --- | --- | --- |
| 例外日 | `--weekly --days mon,wed --exclude 2026-11-23 --exclude 2026-11-30` | excluded 2 日付の placement 0 件、非除外曜日は生成。UTC date = local date (09:00 JST = 00:00Z) で検証 |
| label 専用 | `--from-json` draft `plan_role: 1`、daily generation | 33 placement 生成、timeline `--include-labels` で `role=1` (LABEL lane) 表示、busy time に非計上の注意書きあり。`--include-labels` なしでは非表示 |
| 優先度競合 | 同一 daily 10:00 JST・duration 120min の 2 source (priority 10 vs 30) | 初回 materialization では両者が同一 slot に open placement として並存 (重複を許容)。明示 `reflow` で容量解決が走り、priority 30 側 7 件 open 維持、priority 10 側 7 件すべて closed (`INSUFFICIENT_CAPACITY`)、他 source の placement id 集合は完全一致で不変 |
| 他 source への reflow 影響 | 上記ペアで low のみ reflow | high の placement_id 集合・open 数が前後で不変 (script が hash 比較) |
| 通知 fake の発火・重複抑止 | worker 常駐下で unplaced occurrence を発生させ DB 直接確認 | `v1_source_decision_required_outbox` 454 件すべて `published_at` 済み、`v1_source_decision_required_notification` 454 件で `outbox_id` 重複 0。`occurrence_id` UNIQUE + `outbox_id` PK により worker tick をまたいだ重複 enqueue/通知は発生しない。`v1_notification`/`v1_delivery` は endpoint 未登録のため 0 件 (配信先なし = 正常) |
| task lifecycle | once source → placement → start/pause/resume/finish | revision 1→4、finish kind=2 で完了。finished placement は reflow 対象外 (前回証跡 §3.4) |
| prompt 滞留 | `prompts request --kind 1` → `prompts list` → `resolve --resolution ack` | pending 検出・解決とも成功 |

### 観測された意味論 (Core 2.0.0、要 handoff 記載)

- 同一 window の複数 source は初回配置で重複 open placement を持ち得る。
  優先度による排他は **明示 reflow / 最適化 pass** で初めて効く。
  日常利用で「予定が重なったまま」に見えるのは正常で、reflow が
  容量競合を `INSUFFICIENT_CAPACITY` として closed 化し、
  decision-required notification を発行する。
- `today` は label-only source も actionable として返し得る
  (next start 昇順)。LABEL lane の扱いは timeline 側の `--include-labels` と
  一致させて読む必要がある。

## 4. worker advisory lock 約 7.5 秒ログの計測

`docker logs tastile-verify-worker` の `slow statement` を全期間で集計:

- `pg_advisory_xact_lock` の 7.53s WARN は **worker boot 直後の初回大 drain**
  (progress=345) の 1 回のみ (2026-11-19T13:13:23Z)。同時刻に API 起動・
  初回 fill が重なっており、lock 競合は起動時の一過性。
- 13:16:18Z に 1.07s の同種 WARN が 1 件 (同じく起動〜初期 drain 期)。
- worker restart (13:17:38Z) 以降約 1 時間、5 秒周期の drain は
  slow statement 0 件。定常処理の遅延ではない。

**判定**: 起動時の待ち時間であり、Muse 日常利用の体感に影響する
定常遅延は確認できなかった。修正は不要と判断。再発する場合は
Core Issue 化する (現時点で Issue 未起票)。

## 5. Core#322 recurring 差分 19 件の追跡状態

`docs/evidence/2026-11-16-recurring-diff-classification/` (core repo) の
分類結果を確認した。結論は確定済み:

- 19 件中 2 件は replay adapter の回帰 (day0 anchor) で修正済み、Core 回帰なし。
- 残り 17 件は匿名化 fixture に recurrence anchor がなく occurrence を
  一意対応付けできないため「③ 匿名化情報欠落」。Core の実装差が原因とは
  確定できず、実運用影響の有無は **Muse 実データとの認可済み read-only
  比較 (shadow)** でしか判定できない。
- `unplaced_by_scheduler` 4 tile は core#295 関連の別分類で、core#295 は
  CLOSED。実運用上の残件は「shadow 比較で実データの一致を確認する」
  のみで、コード修正の残件はない。

## 6. 未検証・残課題 (silent approximation なし)

- DemandDriven (generation kind=2) は Core/CLI とも未対応。fixture replay
  でも `unsupported_demand` 分類済み。近似実装はしていない。
- relation 起因の DecisionRequired (kind=2 prompt) は relation 定義が
  複雑で今回の CLI シナリオでは未発火。source-level decision-required
  notification の重複抑止は §3 で確認済みだが、relation 系は別経路。
- Muse 実データとの比較は未実施 (認可済み read-only 経路の確定待ち)。
- `build_identity.commit_sha` が image で null のまま (Core 側 gap、
  provenance は外部証跡依存)。
- v2 artifact runtime の実証は別タスク。
