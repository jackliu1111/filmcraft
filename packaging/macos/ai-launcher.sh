#!/bin/bash
# Finder entry for an AI-enabled bundle. No Python or development environment is needed.
set -euo pipefail
MACOS="$(cd -P "$(dirname "$0")" && pwd)"
CORE="$MACOS/FilmCraft-core"
SERVICE="$MACOS/../Resources/ai-runtime/service/filmcraft-ai"

# Informational commands must work without mounting data storage or opening dialogs.
case "${1:-}" in
  -h|--help|-V|--version) exec "$CORE" "$@" ;;
esac

fail() { printf 'FilmCraft: %s\n' "$*" >&2; exit 1; }
[ -x "$CORE" ] || fail "原生应用缺失或不可执行，请重新安装完整应用。"
# Match the native worker's explicit developer selection, including set-but-empty
# overrides. Keep the developer's data/native options without frozen preflight.
if [ "${FILMCRAFT_APEX_PYTHON+x}" = x ] || [ "${FILMCRAFT_APEX_BRIDGE+x}" = x ]; then
  exec "$CORE" "$@"
fi
[ -x "$SERVICE" ] || fail "包内 AI 服务缺失或不可执行，请重新安装完整应用。"

PREFERENCE_DIR="$HOME/Library/Application Support/FilmCraft"
PREFERENCE="$PREFERENCE_DIR/ai-data-directory"
DATA="${APEX_AI_DATA_DIR:-${FILMCRAFT_AI_DATA_DIR:-}}"
EXPLICIT=0
if [ -n "$DATA" ]; then
  EXPLICIT=1
elif [ -f "$PREFERENCE" ]; then
  # A NUL-terminated literal path preserves spaces, Unicode and newlines without eval/source.
  IFS= read -r -d '' DATA < "$PREFERENCE" || DATA=''
fi

choose_data() {
  /usr/bin/osascript - "$1" <<'APPLESCRIPT'
on run argv
  try
    set destination to choose folder with prompt (item 1 of argv)
    return POSIX path of destination
  on error number -128
    return ""
  end try
end run
APPLESCRIPT
}

PROMPT='请选择已挂载的外接 APFS/HFS（Mac OS 扩展）磁盘或 APFS 磁盘映像中的 AI 数据文件夹。FilmCraft 会将分析数据、缓存及临时文件保存在此处，并记住这个位置。'
while :; do
  if [ -z "$DATA" ]; then
    DATA="$(choose_data "$PROMPT")" || fail "无法选择 AI 数据目录。"
    [ -n "$DATA" ] || fail "已取消选择 AI 数据目录。"
  fi
  if CHECK="$(APEX_AI_DATA_DIR="$DATA" FILMCRAFT_AI_DATA_DIR="$DATA" "$SERVICE" --check-runtime 2>&1)"; then
    break
  fi
  if [ "$EXPLICIT" = 1 ]; then
    fail "指定的 AI 数据目录或包内运行时不可用：$CHECK"
  fi
  DATA=''
  PROMPT="AI 数据目录或运行时不可用：$CHECK
请接好外接磁盘并重新选择数据文件夹；运行时损坏时请取消并重新安装应用。"
done

DATA="$(cd -P "$DATA" && pwd)"
mkdir -p "$DATA/filmcraft"
NATIVE_DATA="$(cd -P "$DATA/filmcraft" && pwd)"
case "$NATIVE_DATA/" in
  "$DATA/"*) ;;
  *) fail "应用数据目录不能通过符号链接越出所选 AI 数据目录。" ;;
esac
export APEX_AI_DATA_DIR="$DATA" FILMCRAFT_AI_DATA_DIR="$DATA" FILMCRAFT_DATA_DIR="$NATIVE_DATA"
export TMPDIR="$DATA/tmp" TMP="$DATA/tmp" TEMP="$DATA/tmp"
export TORCH_HOME="$DATA/cache/torch" HF_HOME="$DATA/cache/huggingface"
export XDG_CACHE_HOME="$DATA/cache" NUMBA_CACHE_DIR="$DATA/cache/numba" MPLCONFIGDIR="$DATA/cache/matplotlib"
export PYTHONDONTWRITEBYTECODE=1

# Explicit command-line environment overrides are deliberately not remembered.
if [ "$EXPLICIT" = 0 ]; then
  mkdir -p "$PREFERENCE_DIR"
  SETTING="$(mktemp "$PREFERENCE_DIR/.ai-data-directory.XXXXXX")"
  trap 'rm -f "$SETTING"' EXIT
  chmod 600 "$SETTING"
  printf '%s\0' "$DATA" > "$SETTING"
  mv -f "$SETTING" "$PREFERENCE"
  trap - EXIT
fi
exec "$CORE" "$@"
