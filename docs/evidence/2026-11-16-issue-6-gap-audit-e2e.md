# tastile-cli #6 — acceptance gap 監査と実 E2E

実施日: 2026-11-16 JST
CLI branch: `6`（base `b03ad5a`）
OpenAPI submodule: `0c130c5c2e8aa1afbf74b3eca5c4b5d433e6a28c`（OpenAPI 3.1.0 / API 1.1.0）

## Acceptance gap 監査

| 項目 | 判定 | 確認内容と残る制約 |
| --- | --- | --- |
| 書き込み先と資格情報の結合 | 対応済み | `StoredToken.api_base_url` を保持し、設定 URL との正規化比較と期限を共通書き込みゲートで検査する。loopback は隔離確認を要求し、port `31400` は全ホストで拒否する。全 mutation command と `schedule publish --apply` がゲートを通る。`auth status` と `doctor` は bearer を表示しない。契約に `whoami` がないため、保存 subject と Core owner ID の独立照合はできない。 |
| source tile の全置換更新 | API契約内で対応 | `PUT` に必要な title/metadata/plan/flows/horizon/generation/window/split policy/relations/`source_client_local_id` を明示した全 draft を要求する。revision CAS を使い、dry-run は read model で見える差分を表示して、見えない値を比較できないと警告する。OpenAPI に PATCH/merge がないため、隠れた request 値は利用者が元の JSON から引き継ぐ必要がある。 |
| 未配置理由 | 修正済み | CommandResponse の `aggregate_meta.unplaced_occurrences` と source detail の `blocked` を表形式で表示し、数値理由を `INSUFFICIENT_CAPACITY` / `LIMIT_CONFLICT` / `PRIORITY_TIE_CONFLICT` に変換する。競合元 source ID も表示する。 |
| doctor の版・migration 表示 | 現行契約内で対応 | CLI/OpenAPI の版と API 到達性を表示し、token は隠す。pin に Core の health/version/migration operation がないので Core binary version・適用 migration・owner identity は表示／照合できない。未統合 Core#301 の field は仮定しない。 |
| `today` の stale projection | 修正済み | `exclude_future=false` で全 projection を取得し、`projected_next_start_at` が現在時刻以降の行だけ残して最も早いものを選ぶ。過去時刻と null は unit regression test で除外を確認した。 |
| schedule publish | 実 E2E 済み | credential なし・到達不能 URL で preview が成功し、API request を送らない。隔離 Core への `--apply` は idempotency key 必須で、source API readback と worker が作った open placement を確認した。preview 単独は materialization の証明にならない。 |
| cursor 完走 | 実 E2E 済み | 実 Core の source list は `count=500` と `next_cursor` を返した。HTTP API で追加した501 fixtureを含む計505行を CLI が読み切った。`/v1/tiles` の cursorless legacy応答は旧500件capで fail closed。prompt listにはpin上paginationがない。 |
| monthly recurrence | 明示的に拒否 | 現行 schema は one-time/recurring のみ。`--monthly` は daily に近似せずエラーにする。Core#292の月次対応を先取りしない。 |

## 隔離 Core / PostgreSQL

| 項目 | 値 |
| --- | --- |
| Core source | `/Users/rebui/work/tc-integ`、clean detached checkout、`5318815373b270065532b7a88c039726c17dc9ef` |
| Core binaries | `cargo build --release --manifest-path crates-v1/Cargo.toml -p api -p worker -p cli`、task専用 target dir `/tmp/tcli6-core-target` |
| CLI binary | `mise exec -- cargo build --release -p tastile-cli` 成功 |
| PostgreSQL | `tcli6-pg`、`postgres:16-alpine`、image `sha256:f377d590e5474681d4d7838635031a2b50647923b7abfa803ce9b5ff35d5c663` |
| Database / bind | `tastile_verify` / `127.0.0.1:55503` |
| Core API | `127.0.0.1:31603`、development env |
| Worker delivery | `TASTILE_DELIVERY_PROVIDER=fake` |
| CLI credential | task専用 owner/token を `tastile_verify` に作成。loopback 専用 `TASTILE_CREDENTIAL_FILE` は mode `0600`。token は出力・証跡に記録していない。 |

Core migration runner は同じ DB に成功した。隔離 PostgreSQL container は長時間のCore build中に clean stop していたため、同一 `tcli6-pg` を再起動して DB を保持した状態で API/worker を起動した。運用 VM、Muse、port `31400`/`31401`/`5433`、他 lane の port には接続していない。

## 実行結果

| Flow | 結果 |
| --- | --- |
| credential / `auth status` | file credential を読み、`Reachable: yes`。bearer は出力されない。 |
| source tile create / list / get | CLI create が成功し、一覧からIDを取得。detail は occurrence と placement を返した。 |
| update lossless / revision | full JSON draft の元 plan、flows、horizon、relations、client-local ID を保持して title のみ変更。dry-run 中は変更なし、revision `1 → 2` の update/readback 成功。古い `expected_revision=0` は HTTP 409 となり、別 source の title/revision は変わらない。hidden request fields は API read model にないのでサーバーからの完全 round-trip 比較はできない。 |
| reflow | dry-run は1 placementの scope を表示。apply 後、source revision と placement の readback 成功。 |
| worker / placements / today | fake worker が失敗なしで起動。open placement を読め、`today --json` は未来の `projected_next_start_at` を持つ main tile を選んだ。 |
| cancel | dry-run scope 表示後、reason code `0` で取消。readback `source_state=3`。 |
| priority tie | 同一優先度・競合時間のsourceを Core に作成。Core detail `blocked.reason=2` と勝者 source ID を返し、CLI create output は `PRIORITY_TIE_CONFLICT` を表示した。 |
| schedule publish | offline preview成功。loopback Coreへのapplyは command response を返し、source list/getで保存を確認。worker後にpublished sourceのopen placementを1件確認。 |
| `doctor` | CLI `1.0.0`、pinned OpenAPI `1.1.0`、API probe `401` を表示。credential bearer は出力にない。Core version/migration はpin上取得不可。 |
| cursor boundary | 実Coreの最初の source list page は500件・continuation cursorあり。CLIは2 pageを完走して計505件、501 cursor fixtureすべてを返した。 |

## CI

`mise run ci` 成功:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features` — 121 tests passed, 0 failed
- `scripts/check-openapi-drift.sh` — pinned submodule一致、19/19 operation surface coverage

## 残る契約上の制約

- Core owner identity / server version / applied migration の照合には pinned `whoami`/health/version operation が必要。
- `source-tiles update` は全置換。planなどGETで返らない値の自動復元・dry-run比較には Core の PATCH/merge またはreadback契約が必要。
- `prompts list` にcursor contractがないため、将来サーバー側capが導入された場合の完走を証明できない。
- monthly recurrence はCore/OpenAPI contract追加まで拒否。
- Museとの運用上のshadow比較は実施していない。隔離E2EはCore実装の基本的意味論を示すもので、Muse parityを証明しない。
