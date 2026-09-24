# OpenAI 订阅接入实现方案

日期：2026-09-24
状态：已实施并部署本地；用户已确认真实账号登录成功，三个订阅模型的普通与流式基础调用已验证

## 目标与范围

个人使用，每个 Provider 条目只绑定一份 API Key 配置或一个订阅账号。API Key 与订阅是明确的接入类型；模型协议继续由 channels 声明，仅启用上游实际支持的协议，不做跨协议转换、账号池或自动切换计费方式。

供应商页面不增加分类。卡片标识接入类型，展示和配置表单自动适配。删除内置的 OpenAI API Key 条目（id: openai），新增 OpenAI 订阅条目（id: openai_subscription）；其他供应商继续支持 API Key，复用现有模型管理、同名模型消歧、路由与统计。

首版交付 OpenAI 订阅的 PKCE 登录、凭据持久化及刷新、模型同步、Responses HTTP/SSE 接入、Admin 配置。暂不实现设备码、WebSocket、订阅额度查询和 Codex CLI 凭据导入。

## 当前代码基础

- `config.rs` 的 ProviderDef/ChannelDef 定义供应商和通道；定义来自 providers.json，用户文件按现有规则覆盖。
- `provider_config` 混存启用状态和 api_key/workspace_id/cost_api_key；`ProviderRoute` 复制静态 Key。
- `provider_svc::refresh_routes` 跳过 api_key 为空的供应商；模型手动同步及后台探测也依赖静态 Key。
- `proxy.rs` 已有独立 Responses 入口、模型名改写、SSE 转发、取消传播与 response.usage 提取。
- Admin 的模型同步接口通过查询参数接收 Key；订阅路径不能沿用这个凭据流。
- `crypto.rs` 的 seal/reveal 允许明文兼容回退；OAuth 凭据需要单独的严格接口。

## 配置定义

ProviderDef 新增 `access_type` 枚举（api_key/subscription，缺省 api_key）以及可选 `adapter`。首版 subscription 必须使用 `openai_chatgpt`；普通 API Key 保持现有转发方式。不建立动态插件框架。

```json
{
  "id": "openai_subscription",
  "name": "OpenAI 订阅",
  "icon": "openai.svg",
  "access_type": "subscription",
  "adapter": "openai_chatgpt",
  "channels": [
    {
      "type": "openai_responses",
      "base_url": "https://chatgpt.com/backend-api/codex"
    }
  ]
}
```

订阅功能交付时同步删除 providers.json 的现有 openai 条目，替换为上面的 openai_subscription 条目，不保留内置 OpenAI API Key 卡片。旧 openai 的数据库配置、模型和历史调用记录不自动删除，也不迁移为订阅账号；缺少供应商定义后不生成卡片或路由。用户私有 providers.json 中自行声明的供应商继续按既有规则加载，不修改用户文件。

订阅的授权、Token 和模型目录 endpoint 由适配器管理，不暴露成通用 OAuth 配置。校验 subscription/adapter/channel/base_url 的组合；OpenAI 订阅只接受指定官方 HTTPS 地址，拒绝覆盖到其他主机。错误沿用 config_error 展示，禁止建路由。

接入类型以配置文件为准，不在数据库重复保存。不能仅靠改变已有条目的类型转换凭据；类型不匹配的残留凭据不参与认证，不自动复制或删除。

## 数据库与迁移

公共配置和专属凭据分表，所有凭据表均以 provider_id 为主键，不增加账号列表或账号池。

```text
provider_config
  provider_id PK
  is_enabled

provider_api_key_credentials
  provider_id PK
  api_key
  workspace_id
  cost_api_key

provider_subscription_accounts
  provider_id PK
  account_id
  account_label                 可空，仅显示账号摘要
  access_token_encrypted
  refresh_token_encrypted
  expires_at                    UTC Unix 秒
  credential_version            每次登录生成全新随机值；刷新成功更换
  auth_status                   authorized / reauth_required
  updated_at                    UTC Unix 秒
```

未登录表示没有订阅记录，access token 自然过期不等于需要重新登录。只有明确的不可恢复刷新错误才标记 reauth_required；网络错误保留凭据并返回可重试错误。

迁移在单一事务中建表、复制现有 API Key 三字段、校验复制结果、移除旧字段并记录完成标记。先判定迁移是否已完成，再执行依赖旧字段的 SQL，保证重启幂等。复制失败或任一步失败全部回滚；不使用 INSERT OR REPLACE 覆盖已有凭据。

迁移前通过已有备份能力保存一致性数据库备份。旧二进制不能直接使用迁移后的库；回滚须恢复对应备份。模型、通道、余额快照和 usage_records 表不改结构。

现有 API Key 字段保持其原存储语义，本任务不顺带改造旧密钥加密。订阅 Token 使用已有 database.encryption_key 加密，但新增返回 Result 的严格 seal/reveal：错误不得回退为明文。未配置密钥时 API Key 功能照常，订阅登录入口提示先配置密钥；密钥错误时报告本地凭据不可读，不冒充上游登录失效。

所有 provider_config 的旧字段查询同步迁移，包括余额服务的定时探测、费用 Key 读取和并发结果校验。公共配置、API Key、通道和模型的原子保存语义保持不变。

## 服务边界

建议新增 `src/providers/`，使用枚举和明确的 match 分派即可：

- `credentials.rs`：根据 Provider 定义读取静态 Key 或解析订阅凭据，集中管理刷新锁。
- `openai_subscription/auth.rs`：OAuth 登录会话、回调监听、Token 交换和刷新。
- `openai_subscription/models.rs`：订阅模型目录请求及转换。
- `openai_subscription/responses.rs`：同协议请求适配、响应流识别及终态处理。

ProviderRoute 改为保存接入类型/适配器及凭据引用，不携带订阅 Token 快照。建路由以供应商启用、通道启用、已配置模型和对应凭据状态为条件；不再统一要求 api_key 非空。订阅凭据可刷新时即视为可用，不因为 access token 临时过期就移除路由。

同步模型、后台模型探测和推理请求共用 get_valid_credentials。订阅 Token 不进入 Debug 输出、Admin JSON 或诊断日志。

## OAuth 登录生命周期

Admin 保留现有端口 10020；同一个进程仅在登录期间临时监听 loopback:1455，回调地址固定为 `http://localhost:1455/auth/callback`。不假定 OpenAI 接受自定义 Admin 端口。

1. 用户点击登录，后端先验证供应商类型、加密密钥和回调端口可用性。
2. 创建随机 session_id/state 和 PKCE verifier/challenge，保存到内存，10 分钟有效。个人使用首版全局最多一个进行中的登录会话，重复启动返回明确冲突。
3. 向前端返回授权 URL 和 session_id；前端打开授权页并轮询登录状态。OpenAI client_id、scope 和端点集中在适配器常量中。
4. GET /auth/callback 校验有效会话、state 和授权结果。自动回调与手动提交共享同一处理函数，原子认领会话，授权码只交换一次。
5. 服务端使用 code_verifier 交换 Token，读取账号 ID，计算过期时间，严格加密并原子保存。返回页面只含成功/失败信息，成功后 303 跳回配置生成的 Admin 地址，不携带 code/Token。
6. 成功、取消、超时、失败均关闭回调监听并清除登录会话；进程重启后未完成会话失效。数据库旧账号仅在新登录保存成功后替换。

端口占用时明确提示关闭占用的登录流程后重试，不终止其他进程。远程浏览器的 localhost 指向浏览器所在机器，支持把完整回调 URL 粘贴到 Admin，经 POST 提交并严格校验 state；不宣称该场景能自动回调。

登录不会自动开启供应商、选择模型或发送推理请求。更换为不同账号后，清除旧账号模型目录快照及已选模型，要求重新同步和选择，避免继承旧账号权限；同账号重新登录保留模型选择。

新增登录/退出/手动回调管理接口校验本地管理请求的 Origin（允许同源和无 Origin 的本地工具调用），请求体使用 JSON；OAuth GET 回调依靠一次性 state 校验，不依靠浏览器会话 Cookie。回调 URL、code、verifier、Token 不写日志。

## 刷新、退出与并发

- 请求前检查 expires_at，距到期不足 60 秒则刷新。以 provider_id 为单位使用异步互斥锁，获取锁后重新读取凭据，避免并发重复刷新。
- 网络请求期间不持有 SQLite 写事务。刷新结束后，按旧 credential_version 条件更新 Token、过期时间和新版本；更新为零行说明凭据已替换或删除，丢弃旧结果并重新解析当前状态。
- 退出先取消同供应商的登录会话，再删除凭据，立即刷新路由。旧刷新任务不能重新插入记录。重新登录使用全新随机版本，防止退出再登录后版本重用。
- 本地退出不承诺撤销已经发给上游的请求或服务端令牌；已在执行的请求可结束，后续请求不得使用已退出账号。
- 上游明确返回认证失败且尚未向下游输出时，最多刷新并重试一次；其他 4xx/429 不触发刷新。网络不确定失败、刷新请求结果不明或流已开始时不盲目重放。
- 不切换到 API Key，不更换账号，不改变模型。限流信息保留为可理解的错误；不实现账号池或限流绕过。

## 同协议代理适配

客户端继续使用 `/openai-responses/v1/responses` 和 mb- Key。订阅通道发送到官方 `/backend-api/codex/responses`，注入 OAuth Bearer、账号头及经真实验证需要的协议头。

客户端携带的 Authorization、chatgpt-account-id 等网关自管头不透传；构造最终 HeaderMap 后再发送，避免 reqwest append 产生重复认证头。标识使用 Model Bridge 的真实身份，不把 OpenClaw 的 originator/User-Agent 原样冒用；具体接受值列入首轮实测。

协议仍是 Responses。仅做该接入方式必要的处理：填充空缺 instructions、规范化 input 为上游接受形式、将缺省 store 设为 false、为 HTTP 传输请求流式结果。工具及工具结果、推理字段、加密推理内容保持语义，不翻译为其他协议。

明确请求 store:true、background:true 或首版不支持的服务端状态续接（如 previous_response_id）时返回明确 400，不静默忽略。其他字段按实测和契约逐项判定，不从 OpenClaw 未使用某字段推断上游一定不支持。需要会话历史的客户端自行发送完整 input。

下游 stream:true 转发 Responses SSE；stream:false 或缺省时消费上游 SSE 并返回终态中的完整 Response JSON，设置有界缓冲（64 MiB）和超时，不把 SSE 直接返回非流式客户端。此操作属于同协议传输适配。

订阅响应不能只凭 HTTP 200 判定成功：识别 completed、failed、incomplete、错误事件和无终态 EOF，正确记录状态与已知 usage。流开始后无法修改 HTTP 状态时，通过协议事件/断流向客户端表达失败；不得伪造 completed。若缺少 Content-Type，只在该适配器内进行有界 SSE 前缀验证，不把 HTML 错误页当流。

复用现有取消传播和连接释放逻辑。使用订阅专属响应分支收口兼容处理，普通 API Key 通道保持已有协议行为。

## 模型目录与路由

订阅模型目录由后端请求 `/backend-api/codex/models?client_version=...`，使用同一账号的 Token 和账号头。client_version 为经过验证的适配器兼容常量，不是管理员输入项。

将上游 models 条目转换为现有模型选择结构，按账号可见性过滤。成功空目录是有效结果；网络失败保留原快照并标记失败；认证被拒绝不得伪装为成功或以硬编码模型授权兜底。

保留现有“同步获取候选 → 用户选择 → 保存生效”和后台漂移提示流程。退出立即撤销路由但可保留模型选择；不同账号登录成功时清理旧选择。跨供应商同名模型沿用现有限定名规则，不引入新别名机制。

## Admin API 与界面

公共 PUT `/api/admin/providers/{id}` 继续原子保存启停、通道和模型；API Key 条目同时允许保存旧有凭据字段，保持旧客户端兼容。订阅条目拒绝非空 API Key 专属字段。

新增接口：

| 方法/路径 | 行为 |
| --- | --- |
| POST `/api/admin/providers/{id}/subscription/login` | 创建登录会话，返回 session_id、authorization_url、expires_at |
| GET `/api/admin/providers/{id}/subscription/login/{session_id}` | 查询 pending/exchanging/succeeded/failed/cancelled/expired |
| POST `/api/admin/providers/{id}/subscription/login/{session_id}/callback` | 提交完整回调 URL |
| DELETE `/api/admin/providers/{id}/subscription/login/{session_id}` | 取消登录 |
| DELETE `/api/admin/providers/{id}/subscription/account` | 本地退出并撤销路由 |
| POST `/api/admin/providers/{id}/models/query` | 返回候选模型，不改已选模型；按类型解析凭据 |

models/query 的 API Key 分支允许 JSON 请求体携带尚未保存的 Key（延续配置表单试探能力）；订阅分支只读后端凭据。旧 GET fetch-models 保留 API Key 兼容，前端迁移到 POST；后台探测直接复用服务函数。

ProviderSummary/Detail 返回 access_type 和按类型的认证摘要。订阅只返回账号摘要、authorized/reauth_required/unconfigured 状态；不返回密文、Token 或授权码。API Key 旧详情行为保持兼容。

卡片直接增加“API Key”或“订阅”标识，不增加分类导航。订阅卡片显示登录状态，配置弹窗显示账号摘要、登录/重新登录/退出按钮、授权进度及必要时的手动回调输入。公共通道和模型区保持一致。关闭登录弹窗时取消未完成会话。

首版不展示伪造的订阅余额或剩余额度；已有 API Key 余额功能不回归。停用保留登录，退出清除凭据，两个操作的文案区分。

## 实施分解

1. **验证上游边界**：用用户授权的测试账号验证 PKCE 回调、Token 交换、账号模型目录、一次 SSE 请求、工具结果往返及身份头；记录接受的参数和错误结构。此阶段不在日志或仓库保存凭据。不能以源码静态分析替代此验证。
2. **类型与存储**：配置枚举、校验、公共/专属库表迁移、严格 OAuth 加密、其他供应商 API Key 和余额查询兼容；交付时将内置 openai 条目替换为 openai_subscription。
3. **认证生命周期**：凭据服务、临时监听、登录/取消/退出接口、刷新锁与版本条件更新。
4. **模型与路由**：按类型获取模型、后台探测、路由凭据引用及退出/换账号后的失效处理。
5. **Responses 适配**：认证头、请求约束、SSE 和非流式返回、错误终态、usage 与取消处理。
6. **界面与帮助**：卡片标识、自适应表单、登录状态、手动回调、订阅接入说明。
7. **集成验证**：回归测试、前端构建、重编译嵌入资源及真实账号端到端验收。

实现文件主要涉及 config.rs、providers.json、db/schema.rs、db/models.rs、state.rs、crypto.rs、admin/provider_svc.rs、admin/balance_svc/mod.rs、router/admin.rs、router/mod.rs、router/proxy.rs、router/request_log.rs、main.rs、web/src/views/Providers.vue 和帮助页；新适配器文件按前述目录收口。不扩展成通用多租户身份平台。

## 验证与成功标准

- 配置：缺省类型兼容；非法枚举、适配器、通道和目标地址拒绝建路由。
- 内置供应商替换：默认定义中不再有 openai API Key 条目，只有 openai_subscription；旧配置不生成悬空路由，历史记录仍可查询，用户私有定义不被修改。
- 迁移：旧数据库所有 Key/工作区/费用 Key、启停、模型不丢；失败回滚；重复执行幂等；余额服务回归。
- OAuth：有效/错误 state、过期会话、重复回调、手动与自动竞争、端口占用、取消、重启；任何摘要/日志不含凭据。
- 刷新：多个并发请求只刷新一次；旋转 Token 原子持久化；退出或重登录后旧刷新不能复活/覆盖；网络错误不标记重新登录。
- 代理：Wiremock 断言唯一认证头和目标地址、客户端 Key 不外泄、stream true/false、工具调用、推理字段、401 最多一次刷新、429 不重放、失败终态、无终态断流、分块 SSE 和取消；usage 不重复累计。
- UI：卡片不分类但类型明确，表单自动适配；登录、取消、重试、退出和模型选择可用；普通供应商编辑与余额展示不回归。
- 构建：先准备 web/dist；聚焦测试后运行 cargo test、cargo check、cargo clippy；前端变更执行 npm run build、cargo clean -p model-bridge、cargo build，浏览器检查两类卡片及弹窗。
- 实测：客户端只配置 Model Bridge 地址及 mb- Key，能通过选中的订阅模型完成多轮与工具调用；重启后可恢复，过期可刷新，退出后新请求不能使用旧凭据。

## 外部依据与限制

以下为研究时读取的 main 分支实现，实施时应固定验证所依据的提交，避免随上游变化误认为永久协议保证：

- [OpenClaw OAuth 授权](https://github.com/openclaw/openclaw/blob/main/extensions/openai/openai-chatgpt-oauth-authorization.runtime.ts)
- [OpenClaw Token 交换与刷新](https://github.com/openclaw/openclaw/blob/main/extensions/openai/openai-chatgpt-oauth-token.runtime.ts)
- [OpenClaw 订阅 Responses 传输](https://github.com/openclaw/openclaw/blob/main/packages/ai/src/providers/openai-chatgpt-responses.ts)
- [OpenClaw 模型发现](https://github.com/openclaw/openclaw/blob/main/extensions/openai/openai-provider.ts)
- [Codex 回调监听与白名单端口注释](https://github.com/openai/codex/blob/main/codex-rs/login/src/server.rs)

固定 OAuth client_id 的实际可用性、账号权限、身份头与参数兼容性仍需真实授权测试确认。当前仅完成代码研究和方案整理，未执行登录、模型调用或产品代码变更。
