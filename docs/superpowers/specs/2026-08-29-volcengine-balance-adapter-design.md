# Provider 余额查询 — 火山引擎（火山方舟）volcengine adapter

Date: 2026-08-29
Status: Approved

## 背景与目标

volcark（火山方舟）是内置 provider 中最后一个无 `usage` 配置的。调研结论：火山官方余额路径存在且被官方工具（volcengine/mcp-server billing 模块、byteplus-sdk/ark-cli `doctor account`）使用，但**凭证体系特殊**——方舟推理 key（`ark-` 前缀 Bearer）调不了费用中心与控制面网关，必须火山引擎 IAM 的 AK/SK + OpenAPI V4 签名。

需求为两段都要（bigmodel 式复合载荷）：

- **balance**：账户现金余额（按量付费），费用中心 `QueryBalanceAcct`
- **plan**：订阅配额窗口，Agent Plan（`GetAFPUsage`）与 Coding Plan（`GetCodingPlanUsage`）双探测自动识别订阅的是哪种

## 关键设计决策

1. **凭证放 `usage.params.{access_key, secret_key}`，不消费推理 key**：两套凭证体系，推理 key 在 billing/控制面网关被 400 `InvalidAuthorization` 拒绝（格式层，非权限问题）。AK/SK 明文落盘 `~/.mb/providers.json`，与 `provider_config.api_key` 明文 SQLite 同威胁模型（上游密钥加密本就 out of scope）。
2. **内置 adapter，声明式 http adapter 表达不了签名**。模块为 `src/admin/balance_svc/volcengine.rs`（随本次拆分为目录模块），`fetch_balance` 增 match arm，`check_params` 白名单 `["access_key", "secret_key"]`。
3. **三路探测的差异项聚成 `VolcApi {service, region, query, body}` 常量**：
   - `QueryBalanceAcct`：service=billing，scope 区域 **cn-north-1**（Go SDK 口径），body `{}`
   - `GetAFPUsage` / `GetCodingPlanUsage`：service=ark，区域 **cn-beijing**，body 空
   - 两个 region 不同是各自实测验证过的 scope，不是笔误。
4. **签名是 AWS SigV4 的火山变体，三处差异照搬标准 SigV4 必败**：algorithm 串 `HMAC-SHA256`（无 AWS4 前缀）、credential scope 结尾 `request`（非 `aws4_request`）、派生密钥首层直接 `HMAC(SK, date)`（SK 不加前缀）。SignedHeaders 按字母序（官方 Go SDK 口径；网关按 Authorization 声明的顺序校验）。`volc_sign` 以 `now` 参数化，golden vector 由逐行对照官方 Go SDK 的 Python 参考实现生成，跨语言逐字节比对。
5. **信封优先于 HTTP status**：网关对签名/凭据错误常返 4xx（多为 400）且带与 200 路径相同的 `ResponseMetadata.Error`，业务错误也可能 200+Error——只看 status 会误判（bigmodel 同款教训，http adapter 的语义在这里不适用）。
6. **merge 语义对齐 bigmodel**：任一子探测成功即 ok 快照（失败段置 null + debug log），三路全 Err 才 error 行。plan 段选择：AFP 窗口非空 → `agent_plan`；空则 Coding Plan 窗口非空 → `coding_plan`；再空 → null（**未订阅不算失败**）。AFP 窗口 `Quota<=0` = 未订阅/未启用；`AFPDaily` 跳过（官方控制台隐藏，其 Quota 常高于周上限的历史默认值）。
7. **载荷契约**：`{balance: {available, cash, frozen, arrears, credit_limit, currency} | null, plan_source, plan: [{label: 5H|7D|M, used_pct, resets_at}] | null}`。`plan` 直接是数组、与 bigmodel plan 段同形——前端 `planChips()` 本就与 adapter 名解耦，**零改动复用**；`balanceText` 加 `case 'volcengine'`，tooltip 明细沿用 `bigmodelDetail` 模式。
8. **仓库 `providers.json` 不给 volcark 预置 usage**：AK/SK 不在 DB，预置空 usage 会让未配置的用户每轮探测刷 error 行（bigmodel 没这个问题——它的 key 就在 `provider_config.api_key`）。

## 配套改动一：`~/.mb/providers.json` 同 id 覆盖改为浅合并

原「整体替换」语义要求覆盖条目全量重复 channels/name——`ProviderDef.usage` 字段注释里本就记着这个 footgun（「覆盖时若不带 usage 会一并丢失余额查询」）。且 volcengine 的凭证在 params 里，用户覆盖条目若须全量重复 channels 则极易抄漏。

新语义（`config.rs::merge_user_defs`，Value 层合并——struct 层分不清「没写」和「写了默认值」）：

- 同 id：用户写到的 key 覆盖，没写的 key 继承内置；数组/对象值整体替换（不做元素级合并）；显式 `"usage": null` 才移除
- 新 id：追加，条目仍须完整
- 兼容：全量覆盖条目行为不变（所有 key 都在 → 全赢）；只有「省略字段」从「丢字段」变「继承」——丢字段正是被记录的坑
- 文件级失败语义不变：读不出 / JSON 语法坏 / 条目缺 string id / 合并后反序列化不过 → warn + 整体弃用用户文件

配置示例（增量覆盖，channels/name/icon 自动继承内置）：

```json
[{
  "id": "volcark",
  "usage": {
    "adapter": "volcengine",
    "params": { "access_key": "AKLT...", "secret_key": "..." }
  }
}]
```

## 配套改动二：`balance_svc/` 目录拆分（纯搬移）

`balance_svc.rs`（880 行）拆为目录模块，外部调用路径不变（`main.rs`/`admin.rs` 两处零改动）：

```
src/admin/balance_svc/
├── mod.rs          # dispatch fetch_balance、check_params/endpoint_param、DB 落库、probe_one/probe_balances
├── deepseek.rs / openrouter.rs / bigmodel.rs / http.rs   # 现有 adapter 纯搬移，测试跟随
└── volcengine.rs   # 本次新增
```

## 测试与验证

- 签名 golden vector ×2（billing/ark 两路），Python 参考实现生成、body hash 与公开 SHA-256 值双重交叉验证
- `volcengine_merge` 六象限纯函数测试（含「afp 成功但空窗口 + 其余失败仍 ok」的边界）
- 三路探测 wiremock：签名头存在性、200+信封错误、400+信封错误（须报 code 非裸 HTTP 400）、裸非 2xx
- 浅合并测试：继承 / channels 整体替换 / 显式 null 移除 / 新 id 追加 / 坏条目拒绝
- 真实账号实测（配 AK/SK 后 `POST /api/admin/providers/volcark/balance/refresh`）
