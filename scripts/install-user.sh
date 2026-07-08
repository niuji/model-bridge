#!/usr/bin/env bash
# Model Bridge 用户级 systemd 服务一键安装脚本。
# 一条命令完成：装二进制 + 生成配置(自动生成 encryption_key) + 写 unit + 自启 + 启动。
#
# 不开 linger：服务在你登录期间运行，登出后停止、开机时不自启（下次登录随 user manager 拉起）。
#
# 用法：
#   bash scripts/install-user.sh                  # 自动取 target/release，否则下载最新 release
#   bash scripts/install-user.sh --build          # 先 npm run build + cargo build --release 再装（版本变更时用）
#   bash scripts/install-user.sh --binary /path/to/model-bridge
#   bash scripts/install-user.sh --uninstall
#
# 装好后：
#   systemctl --user status model-bridge
#   journalctl --user -u model-bridge -f
#   代理 http://127.0.0.1:10010  |  管理 UI http://127.0.0.1:10020

set -euo pipefail

REPO="niuji/model-bridge"
ASSET="model-bridge-linux-amd64.tar.gz"

BIN_DIR="$HOME/.local/bin"
CFG_DIR="$HOME/.config/model-bridge"
DATA_DIR="$HOME/.local/share/model-bridge"
UNIT_DIR="$HOME/.config/systemd/user"
UNIT_FILE="$UNIT_DIR/model-bridge.service"
BIN_PATH="$BIN_DIR/model-bridge"
CFG_PATH="$CFG_DIR/model-bridge.toml"

BINARY=""
BUILD=0
UNINSTALL=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build) BUILD=1; shift ;;
    --binary)
      [[ $# -ge 2 ]] || { echo "--binary 需要一个路径参数" >&2; exit 2; }
      BINARY="$2"; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help) awk 'NR>1{if(/^set -euo pipefail$/)exit;print}' "$0"; exit 0 ;;
    *) echo "未知参数: $1" >&2; exit 2 ;;
  esac
done

# --- 卸载 ---
if [[ "$UNINSTALL" -eq 1 ]]; then
  echo ">> 将删除：$BIN_PATH  $UNIT_FILE  $CFG_PATH"
  echo ">> 将删除数据目录：$DATA_DIR（含全部 API key 与用量记录）"
  read -r -p "确认卸载并删除以上数据？[y/N] " ans || ans=""
  if [[ "${ans,,}" != "y" ]]; then
    echo "已取消"
    exit 0
  fi
  systemctl --user disable --now model-bridge 2>/dev/null || true
  rm -f "$BIN_PATH" "$UNIT_FILE" "$CFG_PATH"
  rm -rf "$DATA_DIR"
  systemctl --user daemon-reload
  echo ">> 已卸载（二进制、unit、配置、数据目录已删除）"
  exit 0
fi

# --- --build：从源码构建（npm + cargo）再继续安装 ---
# 版本号在 vite build 时从 Cargo.toml 烘进前端 bundle，所以 Cargo.toml 版本一变就必须重跑
# npm run build，否则侧边栏版本会滞后于二进制 --version。
if [[ "$BUILD" -eq 1 ]]; then
  if [[ -n "$BINARY" ]]; then
    echo "--build 与 --binary 互斥" >&2
    exit 2
  fi
  [[ -f ./Cargo.toml && -f ./web/package.json ]] || { echo "--build 需在仓库根目录运行（需有 Cargo.toml 和 web/）" >&2; exit 1; }
  command -v npm >/dev/null 2>&1 || { echo "--build 需要 npm" >&2; exit 1; }
  command -v cargo >/dev/null 2>&1 || { echo "--build 需要 cargo" >&2; exit 1; }
  if [[ ! -d ./web/node_modules ]]; then
    echo ">> web/node_modules 缺失，先 npm install..."
    (cd web && npm install) || { echo "npm install 失败" >&2; exit 1; }
  fi
  echo ">> npm run build（前端版本从 Cargo.toml 烘入 bundle）"
  (cd web && npm run build) || { echo "npm run build 失败" >&2; exit 1; }
  echo ">> cargo build --release"
  cargo build --release || { echo "cargo build 失败" >&2; exit 1; }
fi

# --- 定位二进制：--binary > 本地编译产物(--build 后存在) > 下载最新 release ---
if [[ -n "$BINARY" ]]; then
  :
elif [[ -x "./target/release/model-bridge" ]]; then
  BINARY="./target/release/model-bridge"
else
  if [[ "$(uname -m)" != "x86_64" ]]; then
    echo "当前架构 $(uname -m) 无预编译 release；请本地 cargo build --release 后用 --binary 指定。" >&2
    exit 1
  fi
  command -v curl >/dev/null 2>&1 || { echo "需要 curl 下载 release（或用 --binary 指定本地二进制）。" >&2; exit 1; }
  echo ">> 下载最新 release..."
  TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
  URL="https://github.com/$REPO/releases/latest/download/$ASSET"
  if ! curl -fL "$URL" -o "$TMP/$ASSET"; then
    echo "下载失败：$URL" >&2
    echo "若仓库为私有或尚未发布 release，请本地 cargo build --release 后用 --binary 指定。" >&2
    exit 1
  fi
  tar -xzf "$TMP/$ASSET" -C "$TMP"
  BINARY="$TMP/model-bridge"
fi

# --- 建目录 + 装二进制 ---
mkdir -p "$BIN_DIR" "$CFG_DIR" "$DATA_DIR" "$UNIT_DIR"
install -m 0755 "$BINARY" "$BIN_PATH"
echo ">> 二进制已安装：$BIN_PATH ($("$BIN_PATH" --version 2>/dev/null || echo unknown))"

# --- 生成配置（已存在则保留，不覆盖用户的密钥/设置）---
if [[ ! -f "$CFG_PATH" ]]; then
  command -v openssl >/dev/null 2>&1 || { echo "生成配置需要 openssl（生成 encryption_key）。" >&2; exit 1; }
  KEY="$(openssl rand -base64 32)"
  cat > "$CFG_PATH" <<EOF
# 由 install-user.sh 生成。改端口/host 等直接编辑本文件，然后 systemctl --user restart model-bridge
[proxy]
# 本机用：仅 loopback。需服务局域网时改 0.0.0.0（代理有 mb- key 鉴权）。
host = "127.0.0.1"
port = 10010

[admin]
# 默认仅 loopback：admin API 无应用层鉴权。
host = "127.0.0.1"
port = 10020

[database]
# 相对路径，按 unit 的 WorkingDirectory 解析，落在 ~/.local/share/model-bridge
path = "model-bridge.db"
encryption_key = "$KEY"

[bridge]
refresh_interval_min = 10
EOF
  chmod 600 "$CFG_PATH"
  echo ">> 配置已生成：$CFG_PATH（encryption_key 已自动生成）"
else
  echo ">> 配置已存在，保留：$CFG_PATH"
fi

# --- 写 unit（始终覆盖，由脚本管理）---
cat > "$UNIT_FILE" <<'EOF'
[Unit]
Description=Model Bridge LLM API gateway
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=%h/.local/share/model-bridge
ExecStart=%h/.local/bin/model-bridge --config %h/.config/model-bridge/model-bridge.toml
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF
echo ">> unit 已写入：$UNIT_FILE"

# --- 生效 ---
systemctl --user daemon-reload
systemctl --user enable model-bridge

if systemctl --user is-active --quiet model-bridge; then
  if systemctl --user restart model-bridge; then
    echo ">> 已重启"
  else
    echo ">> 重启失败，请查看下方状态" >&2
  fi
else
  if systemctl --user start model-bridge; then
    echo ">> 已启动"
  else
    echo ">> 启动失败，请查看下方状态" >&2
  fi
fi

echo
echo "=== 安装完成（核对下方服务状态）==="
systemctl --user --no-pager --full status model-bridge || true
echo
echo "代理：http://127.0.0.1:10010   管理 UI：http://127.0.0.1:10020"
echo "日志：journalctl --user -u model-bridge -f"
echo "改配置后：systemctl --user restart model-bridge"
echo "卸载：bash scripts/install-user.sh --uninstall"
