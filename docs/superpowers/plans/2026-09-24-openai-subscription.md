# OpenAI 订阅接入 Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development for independent modules; integrate and review together. No commits or pushes during implementation.

**Goal:** 在现有网关提供单账号 OpenAI 订阅登录与 Responses 接入。
**Architecture:** 按 access_type 区分配置与凭据，共用模型路由；OAuth 服务独占 Token 生命周期，订阅适配器保持 Responses 协议。
**Tech Stack:** Rust/Axum/SQLx/reqwest，Vue/TypeScript。
**Spec:** ../specs/2026-09-24-openai-subscription-design.md

## Global Constraints
- 单账号；不做账号池、跨协议转换或自动切换计费方式。
- Admin 端口不变，登录期间临时监听 localhost:1455。
- 卡片直接区分类型；删除内置 openai，替换 openai_subscription。
- 凭据不进入前端、日志；OAuth 加解密失败必须报错。

## Review Focus
- 并发刷新与退出/重新登录竞争：旧结果不能恢复已删除凭据。
- SSE EOF 缺少 completed/failed/incomplete 终态：不得误报成功。
- 凭据迁移重启：旧字段查询不得在第二次启动失败。
- API Key 模型同步及余额探测：分表后仍可用。
- localhost 回调与管理页面不同源：一次性 state 负责授权回调校验。

## Task 1: 配置与存储
- [x] 为缺省类型、非法订阅地址、迁移幂等/数据保留、严格加密写回归测试并确认失败。
- [x] config.rs 增加 AccessType::{ApiKey, Subscription}、ProviderDef.access_type/adapter；定义校验。
- [x] db/schema.rs 分离 API Key 凭据表并新增订阅表；迁移事务与一致性备份。
- [x] crypto.rs 增加 seal_required/reveal_required 返回 Result；providers.json 替换内置条目。
- [x] 聚焦测试通过；交接查询变更给集成任务。

## Task 2: OAuth 与凭据
- [x] 新增 providers/mod.rs、providers/openai_subscription/，独立 SubscriptionService 存在 AppState.subscription: Arc<SubscriptionService>。
- [x] 服务构造 SubscriptionService::new(db, client, encryption_key)；登录及刷新状态使用服务内部锁。
- [x] 服务暴露 start_login(provider_id, admin_url)、login_status、submit_callback、cancel_login、logout、summary、credentials、models，均返回 anyhow::Result 或可序列化 DTO。
- [x] 使用模拟 HTTP 测试 Token 交换/刷新、state 校验、版本条件更新、退出并发与模型目录解析。
- [x] 不读取现有用户凭据；真实登录留给完成后的界面验收。

## Task 3: Admin 界面
- [x] Providers.vue 类型新增 access_type 和 auth（status/account_label）；卡片与认证区按类型渲染。
- [x] 对接 spec 登录接口和 POST models/query；Token 不进入表单。
- [x] 更新帮助及生产构建，验证普通 API Key 表单保持可用。

## Task 4: 后端集成与 Responses
- [x] AppState 注入 SubscriptionService；全部测试构造器同步更新。
- [x] provider_svc 查询分表，用订阅摘要决定建路由；订阅模型同步和后台探测共用服务。
- [x] Admin 路由实现登录/退出/查询接口，公共保存仍原子，校验接入类型与同源。
- [x] 新增订阅 Responses 请求校验、认证注入、SSE/JSON 同协议响应；编写模拟上游路由测试并确认失败后实现。
- [x] 修正订阅失败终态、缺少终态、取消和 usage；保留普通代理行为。

## Task 5: 验证与审阅
- [x] 聚焦测试后 cargo test / cargo check / cargo clippy。
- [x] npm run build，cargo clean -p model-bridge，cargo build。
- [x] 独立代码审阅，修复实质问题并重跑相关验证。
- [x] 记录真实 OAuth/订阅调用尚需用户账号授权，交付操作说明。

## 执行记录
- 已获用户实施授权；默认使用独立模块并行实现，统一集成。当前功能分支 feat/openai-subscription，保留工作区未提交变更。
- 基线沙箱 cargo test 出现网络/监听权限相关失败，将在允许监听的环境复验，不把环境失败视为产品回归。

- 最终验证：243 项 Rust 测试通过，cargo check 与 cargo clippy --all-targets 通过；npm run build 通过，并执行 cargo clean -p model-bridge 后重新 cargo build，确认嵌入最新前端。
- 独立审阅已完成并修复账号切换重试、模型快照并发、解密错误提示与 SSE 终态问题；新增对应回归测试。
- 使用临时数据库和无头 Chrome 实测订阅卡片/登录弹窗与 API Key 弹窗；前端异步登录状态另有 4 项临时脚本检查。未操作实际用户数据库或凭据。
- 尚未验证真实 OpenAI 授权、模型目录及推理调用。使用时保留或配置 database.encryption_key，在 Admin 的 OpenAI 订阅卡片登录，随后同步、选择模型并启用。管理端口保持不变，OAuth 临时回调使用 localhost:1455。

- 真实账号验收：用户确认登录成功；通过本地网关对 gpt-6-luna、gpt-6-sol、gpt-6-astra 各执行普通 JSON 和 SSE 请求，6 项均 HTTP 200、completed 并返回 OK。
- 实测发现上游终态 output 为空但 output_item.done 含完整消息；已修复非流式聚合，保留终态已有输出，并新增消息/工具项排序回归。245 项测试、Clippy 与 release 构建通过，修复已部署。真实工具调用往返及 Token 到期刷新尚未实测。
