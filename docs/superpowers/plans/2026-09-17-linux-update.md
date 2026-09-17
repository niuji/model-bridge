# Linux 自动更新实现方案

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Linux x86_64 用户级 systemd 安装提供自动检查版本、手动一键安装和启动验证失败恢复。

**Architecture:** 主服务负责检查版本和展示状态，独立 systemd oneshot 服务运行旧版本程序的更新子命令，负责下载、备份、替换、启动验证和恢复。更新日志保存于数据库之外；新版在事务提交前不接收业务请求，避免恢复数据库时丢失新业务数据。

**Tech Stack:** Rust、现有 reqwest/sha2/sqlx、semver、tar/flate2、Linux 文件锁与 rename、systemd --user、Vue 3/Naive UI。

**Spec:** 本文“范围与约束”“协议与流程”是本次 Linux 优先方案的设计依据；以下方案已实现；验证范围见文末实施记录。

## 范围与约束

- 第一版自动检查、用户点击安装；不默认无人值守安装。
- 仅支持安装脚本管理的 Linux x86_64 用户级 systemd 实例。Windows、ARM、容器、源码运行显示不支持安装的原因。
- 检查默认每 24 小时一次，启动延迟 60 秒并加入抖动；人工检查有 60 秒冷却；离线不影响代理。
- 发布源固定为 niuji/model-bridge；稳定版、语义版本严格递增；不支持任意下载 URL、任意命令或降级。
- GitHub Release 必须有匹配的 Linux 包、SHA256SUMS 和 update-manifest.json。manifest 包含 version、target、asset、sha256、size、update_protocol=1；拒绝缺失或不匹配的发布物。
- 第一版信任 GitHub HTTPS 和仓库发布权限。SHA-256 用于一致性校验，不宣称具有发布者签名认证能力。
- 安装路径和配置路径由安装器登记，不由 Web 请求传入。执行 systemctl 使用参数数组，不通过 shell 拼接。
- 配置、密钥、用户 provider 文件不覆盖；数据库备份权限 0600，更新目录 0700。
- 下载、解压和磁盘权限预检完成后才停服；更新会短暂停止服务，不承诺零停机。
- 排空上限 120 秒，systemd TimeoutStopSec=150s。超时记录未正常完成，取消本次安装并启动旧版；超长 SSE 仍可能被中断，界面必须提前说明。
- 完成第一版的引导安装需手动运行新安装脚本一次，部署新程序及 updater unit；此前旧版本无法自行获得更新能力。

## 协议与流程

### 对外 API

| 方法 | 路径 | 语义 |
| --- | --- | --- |
| GET | /api/admin/update | 当前版本、候选版本、支持状态、检查时间、任务阶段、最近错误 |
| POST | /api/admin/update/check | 手动检查，后台执行；返回 202，冷却期间返回 429 |
| POST | /api/admin/update/apply | JSON {"version":"x.y.z"}，只接受已检查候选版本；返回 202 + job_id |
| GET | /api/admin/update/readiness | 返回运行版本、job_id、初始化阶段、两个监听器状态；供 worker 验证 |

apply 重复请求返回同一活动任务；不同目标版本冲突返回 409。客户端不得传入下载地址或路径。
新增写接口要求 application/json、自定义请求头并检查浏览器 Origin；保留无 CORS 策略。自动安装仅在管理端绑定 loopback、安装路径匹配且用户 systemd 可用时开放。

### 进程与文件

- 主进程在数据库初始化前分派内部 update-worker 子命令，worker 本身不得执行业务数据库迁移。
- 安装器新增 model-bridge-update.service，Type=oneshot，不设置与主服务绑定生命周期的 PartOf/BindsTo。
- 主服务提交任务时，将当前可执行文件复制到 ~/.local/share/model-bridge/update/worker，再请求 systemd 启动独立 unit。worker 始终执行该稳定副本，不随安装路径替换而改变。
- ExecStart 使用固定 worker 路径和登记的配置；其 WorkingDirectory 与主服务相同，数据库相对路径按该目录解析。
- update/job.json 保存任务、目标版本、固定 asset 标识、摘要、各阶段和错误；update/worker.lock 提供跨进程互斥；update/check.json 保存检查结果。
- job.json 使用同目录临时文件、sync_all、rename 和目录同步提交；二进制替换也使用目标目录临时文件和原子 rename，禁止直接覆盖运行文件。
- 备份目录使用 job_id 分隔，保留最近一次成功更新前的程序和数据库，以及未解决失败任务的资料。

### 正常流程

1. 主服务在锁保护下验证安装能力，持久化任务和 worker 副本，启动独立 unit；启动失败标记任务失败，旧服务继续运行。
2. worker 持有文件锁，重新确认固定版本 release/manifest，下载指定资产；下载至临时文件，失败可安全重试，不漂移到新的 latest。
3. 使用独立 reqwest Client，HTTPS-only、请求超时、有限重定向。GitHub 下载允许必要重定向，不改变现有上游请求禁止重定向的策略；不转发业务凭据。
4. 限制压缩包和解包大小（各 256 MiB）；仅提取根目录的普通文件 model-bridge，拒绝链接、路径穿越及重复条目。校验摘要、目标架构与 --version，检查目标目录写权限与备份空间。
5. 写入 stopping 阶段，systemctl --user stop model-bridge；等待停服成功及排空完成标记。后台定时任务停止，两个 HTTP 服务和用量写入任务完成后关闭连接池。
6. 停服后，通过独立 SQLite 连接执行 VACUUM INTO 创建一致性快照，关闭并同步备份文件，执行 integrity_check。备份旧程序。任一失败均不替换程序，恢复旧服务。
7. 持久化 prepared 阶段，然后原子替换二进制，记录 validating，再 systemctl start 主服务。
8. 新版发现未提交的更新事务，进入验证模式：允许迁移和必要缓存初始化，绑定两个监听器；拒绝业务请求和管理写操作，暂停所有后台任务。就绪检查不得调用付费上游接口；迁移或必要缓存加载失败不能报告 ready。
9. worker 在 60 秒内核对 readiness 的 job_id、目标版本、数据库及监听器状态；成功后先持久化 committed，再让主服务根据提交状态开放流量和启动后台任务。确认已激活后记录 succeeded。
10. 前端在服务断开期间显示“正在重启”，恢复后读取持久化结果；成功后刷新页面加载新版静态资源。

### 失败和恢复边界

- 停服前失败：旧服务继续运行，记录失败原因。
- 停服后、替换前失败：确认旧程序与原数据库完整后启动旧服务。
- 替换后、committed 前失败：停止新版并确认进程退出，将迁移后的数据库及 WAL/SHM 整组移到故障目录，恢复数据库快照和旧程序；验证旧版后开放流量并记录 rolled_back。不能把旧快照与新 WAL 混用。
- committed 是不可逆的业务边界：提交后可能已有新数据，禁止自动恢复旧数据库；后续激活故障只重试启动并报告，不做数据回退。
- worker 崩溃或机器重启：启动逻辑在迁移前读取事务。未提交且阶段含糊时保持维护状态，禁止猜测备份是否完整；重新启动 updater unit 根据日志、文件摘要和备份完成标记恢复。第一版不承诺无人值守断电恢复。
- 失败恢复也失败时保留所有资料，状态为 recovery_required；通过 journalctl --user -u model-bridge-update 和持久化错误定位，禁止循环回滚或删除最后可用备份。
- worker 锁与任务阶段共同阻止重复安装；终态任务可发起新任务，未解决任务必须先完成恢复。

## 实施顺序与验证

### Task 1：发布契约与版本检查

**Files:** .github/workflows/release.yml、Cargo.toml、Cargo.lock、src/update/mod.rs、src/update/release.rs、src/main.rs。

- [ ] 补充版本比较与 wiremock 回归用例：0.5.9 < 0.5.21、预发布过滤、旧版不升级、缺资产、缺摘要、manifest/version/target 不一致、403/429、断网、重定向限制。
- [ ] 执行 cargo test update::release，确认新增行为用例先失败。
- [ ] 实现 ReleaseInfo 和 check_release；固定下载资产，解析 manifest 并进行严格校验。
- [ ] CI 在所有构建产物齐全后生成 SHA256SUMS、update-manifest.json 并一起发布；以测试 fixture 验证文件名与摘要。
- [ ] 重跑聚焦测试；检查失败不改变服务运行状态，也不把旧候选版本当成本次成功结果。

### Task 2：生命周期和验证模式

**Files:** src/main.rs、src/state.rs、src/router/mod.rs、src/router/proxy.rs、src/update/lifecycle.rs。

- [ ] 先补“admin 先结束而 SSE 仍存活”的退出回归，以及待写入用量日志、超时退出、验证模式拒绝业务请求的测试。
- [ ] 改成统一退出通知，等待两个 HTTP 服务；追踪现有 tokio::spawn 的用量写入任务并在退出时等待，不只等待 HTTP handler。
- [ ] 退出时停止定时任务、排空请求与写入任务、关闭连接池，最后写排空完成标记。超时不得伪造完成标记。
- [ ] 增加提交前验证模式和 readiness；提交之前管理写操作、代理调用及后台探测均不能执行。
- [ ] 验证缓存加载失败、迁移失败、任一端口占用时不 ready；无 provider 的合法空配置可以 ready。

### Task 3：独立更新器与恢复事务

**Files:** src/config.rs、src/main.rs、src/update/worker.rs、src/update/journal.rs、src/update/backup.rs、scripts/install-user.sh。

- [ ] 对文件操作和进程调用设置可注入测试边界；先写重复执行、空间不足、坏包、符号链接包、每个持久化阶段中断的失败用例。
- [ ] 实现 worker 子命令、独立 unit、文件锁、固定 worker 副本与 journal；保留现有无子命令启动方式。
- [ ] 按上述正常流程实现下载、快照、同目录原子替换、验证和提交。
- [ ] 实现提交前恢复；重点测试带 WAL 数据、数据库迁移后失败、恢复过程中再中断以及提交后绝不恢复快照。
- [ ] 修改安装器：生成更新 unit、主服务 TimeoutStopSec、登记安装信息；安装替换使用临时文件和 rename；保留用户配置。
- [ ] 在隔离的 Linux systemd 用户环境安装两个测试版本，验证更新 worker 在主服务 stop 后仍运行。

### Task 4：管理 API 与界面

**Files:** src/router/mod.rs、src/router/update.rs、src/state.rs、web/src/App.vue、web/src/components/UpdateDialog.vue。

- [ ] 先补四个 API 的状态码、重复提交、任意 URL/路径拒绝、跨源请求拒绝和不支持部署的路由测试。
- [ ] 接入定时检查和 API；状态读取来自持久化文件，避免数据库恢复覆盖更新结果。
- [ ] 在现有侧边栏版本号增加新版本提示和弹窗，展示发布说明链接、目标版本、短暂停服提示及更新按钮。
- [ ] 前端轮询阶段，服务断开显示重启中，超时提示查看日志；不得仅凭请求断开判断更新失败或成功。
- [ ] 浏览器验证检查失败、安装失败、成功刷新、恢复旧版、恢复失败、不支持平台及按钮防重复操作。

### Task 5：完整验证与引导发布

**Files:** README.md、scripts/install-user.sh、.github/workflows/release.yml，及上述测试文件。

- [ ] 顺序执行：cd web && npm ci && npm run build；cargo clean -p model-bridge；cargo build；cargo test；cargo clippy。
- [ ] Linux systemd 实测：更新成功、迁移失败回滚、目标端口占用、SSE 排空与超时、worker 被终止后恢复、committed 后重启不丢业务数据。
- [ ] 对比升级前后配置、密钥、provider 文件和数据库业务数据；校验备份权限及磁盘不足时原服务可恢复。
- [ ] README 写明首次手动安装要求、支持环境、120 秒排空限制、更新日志和手动重启 updater 的恢复方式。
- [ ] 首个版本仅建立更新能力；使用第二个符合发布契约的版本完成真实升级演练后再宣布功能可用。

## 资料

- GitHub Releases API：https://docs.github.com/en/rest/releases/releases#get-the-latest-release
- systemd kill 行为：https://github.com/systemd/systemd/blob/main/man/systemd.kill.xml
- SQLite 一致性备份：https://www.sqlite.org/backup.html
- VACUUM INTO 及持久化注意事项：https://www.sqlite.org/lang_vacuum.html

## 实施记录（2026-09-17）

- 已实现版本检查、固定版本下载与校验、独立 systemd worker、原子替换、SQLite 一致性备份、提交前回滚、恢复事务、管理 API 和界面。
- 请求排空同时等待两个 HTTP 服务与用量写入任务；真实 HTTP SSE 回归验证 admin 先退出时仍等待流结束并记录用量。
- 为避免读到已 rename 但尚未同步目录的提交记录，流量开放前读取方也同步提交文件与目录；安装信息异常且存在未完成事务时拒绝启动迁移。
- 新增 restarting_old 阶段：恢复原服务通过就绪检查后才标记失败终态。手动安装建立新的版本基线并归档已结束任务。
- Linux ARM 仍允许原有源码安装，只禁用网页自动安装；localhost 就绪检查保留原主机名以兼容 IPv6 解析。
- 已验证：196 项 Rust 测试、3 项发布元数据及 systemd unit 解析测试、前端生产构建、组件交互逻辑检查、Rust 构建、Clippy 全 target 且拒绝警告、安装脚本语法及 diff 空白检查。
- systemd-analyze 验证发现并修复 WorkingDirectory 引号导致路径无法解析的问题；回归测试使用真实 systemd 解析器验证安装器输出。
- 构建产物临时实例验证了 readiness、非托管实例禁止安装、嵌入界面与 SIGTERM 正常退出。
- 更新事务测试使用真实文件/SQLite，systemd 命令与 readiness 外部边界使用测试替身。未执行真实 GitHub Release → 已安装 systemd 服务的完整升级或浏览器视觉验收；首次引导安装和第二版本发布演练仍需发布时完成。
