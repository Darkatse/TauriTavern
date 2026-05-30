#!/bin/sh

set -eu

LANG_CODE=""
SILLYTAVERN_ROOT=""
WORK_DIR=""
ZIP_PATH=""
FINAL_ZIP_PATH=""
INCLUDE_BACKUPS=0
IS_TERMUX=0
READ_INPUT=""
ACTIVE_CHILD_PID=""

cleanup() {
    if [ -n "${ACTIVE_CHILD_PID:-}" ]; then
        kill "$ACTIVE_CHILD_PID" >/dev/null 2>&1 || true
        wait "$ACTIVE_CHILD_PID" 2>/dev/null || true
        ACTIVE_CHILD_PID=""
    fi

    if [ -n "${WORK_DIR:-}" ] && [ -d "$WORK_DIR" ]; then
        rm -rf "$WORK_DIR" || true
    fi
}

handle_signal() {
    exit_status=$1
    trap - EXIT INT TERM
    printf '\n' >&2
    cleanup
    exit "$exit_status"
}

trap cleanup EXIT
trap 'handle_signal 130' INT
trap 'handle_signal 143' TERM

msg() {
    case "${LANG_CODE}:$1" in
        ":language_title"|"zh:language_title") printf '%s' "请选择脚本语言 / Please choose a language" ;;
        "en:language_title") printf '%s' "Choose a script language / 请选择脚本语言" ;;
        ":language_option_zh"|"zh:language_option_zh") printf '%s' "1) 中文" ;;
        "en:language_option_zh") printf '%s' "1) Chinese" ;;
        ":language_option_en"|"zh:language_option_en") printf '%s' "2) English" ;;
        "en:language_option_en") printf '%s' "2) English" ;;
        ":language_prompt"|"zh:language_prompt") printf '%s' "请输入 1 或 2，直接回车默认中文: " ;;
        "en:language_prompt") printf '%s' "Enter 1 or 2. Press Enter for English: " ;;
        "zh:banner_title") printf '%s' "TauriTavern SillyTavern Migration Export" ;;
        "en:banner_title") printf '%s' "TauriTavern SillyTavern Migration Export" ;;
        "zh:banner_subtitle") printf '%s' "这个脚本会生成一个可直接导入 TauriTavern 的 zip。" ;;
        "en:banner_subtitle") printf '%s' "This script creates a zip that can be imported directly by TauriTavern." ;;
        "zh:not_in_root") printf '%s' "当前目录不是 SillyTavern 根目录。" ;;
        "en:not_in_root") printf '%s' "The current directory is not a SillyTavern root." ;;
        "zh:root_prompt") printf '%s' "请输入 SillyTavern 根目录路径：" ;;
        "en:root_prompt") printf '%s' "Enter the SillyTavern root path:" ;;
        "zh:root_requirements") printf '%s' "路径内需要包含 data/default-user 和 public/scripts/extensions/third-party。" ;;
        "en:root_requirements") printf '%s' "The path must contain data/default-user and public/scripts/extensions/third-party." ;;
        "zh:root_invalid") printf '%s' "这个路径看起来不是有效的 SillyTavern 根目录，请重新输入。" ;;
        "en:root_invalid") printf '%s' "That path does not look like a valid SillyTavern root. Please try again." ;;
        "zh:detected_root") printf '%s' "已检测到 SillyTavern 根目录：" ;;
        "en:detected_root") printf '%s' "Detected SillyTavern root:" ;;
        "zh:ask_backups") printf '%s' "是否导出 data/default-user/backups？跳过导出备份会明显更快。 [y/N]: " ;;
        "en:ask_backups") printf '%s' "Export data/default-user/backups as well? Choosing No is usually much faster. [y/N]: " ;;
        "zh:summary_title") printf '%s' "执行计划" ;;
        "en:summary_title") printf '%s' "Execution Plan" ;;
        "zh:summary_root") printf '%s' "SillyTavern 根目录" ;;
        "en:summary_root") printf '%s' "SillyTavern root" ;;
        "zh:summary_backups") printf '%s' "导出 backups" ;;
        "en:summary_backups") printf '%s' "Export backups" ;;
        "zh:summary_global_ext") printf '%s' "全局扩展映射" ;;
        "en:summary_global_ext") printf '%s' "Global extension mapping" ;;
        "zh:yes") printf '%s' "是" ;;
        "en:yes") printf '%s' "Yes" ;;
        "zh:no") printf '%s' "否" ;;
        "en:no") printf '%s' "No" ;;
        "zh:step_prepare") printf '%s' "准备临时工作区" ;;
        "en:step_prepare") printf '%s' "Preparing temporary workspace" ;;
        "zh:step_copy_user") printf '%s' "复制 default-user 数据" ;;
        "en:step_copy_user") printf '%s' "Copying default-user data" ;;
        "zh:step_skip_backups") printf '%s' "已按选择跳过 backups。" ;;
        "en:step_skip_backups") printf '%s' "Skipping backups as requested." ;;
        "zh:step_copy_extensions") printf '%s' "复制全局 third-party 扩展" ;;
        "en:step_copy_extensions") printf '%s' "Copying global third-party extensions" ;;
        "zh:step_count_entries") printf '%s' "统计压缩条目数量" ;;
        "en:step_count_entries") printf '%s' "Counting archive entries" ;;
        "zh:step_zip") printf '%s' "开始压缩 zip" ;;
        "en:step_zip") printf '%s' "Creating zip archive" ;;
        "zh:zip_requires") printf '%s' "未找到 zip 命令。请先安装 zip 后再运行。" ;;
        "en:zip_requires") printf '%s' "The zip command is missing. Please install zip first." ;;
        "zh:zip_requires_termux") printf '%s' "在 Termux 中可先执行: pkg install zip" ;;
        "en:zip_requires_termux") printf '%s' "On Termux you can install it with: pkg install zip" ;;
        ":input_required"|"zh:input_required") printf '%s' "无法读取交互输入。请在可交互终端中运行这个脚本。" ;;
        "en:input_required") printf '%s' "Unable to read interactive input. Please run this script from an interactive terminal." ;;
        "zh:zip_progress") printf '%s' "压缩进度" ;;
        "en:zip_progress") printf '%s' "Zip progress" ;;
        "zh:step_move_termux") printf '%s' "尝试移动到 Android 下载目录" ;;
        "en:step_move_termux") printf '%s' "Trying to move the zip to the Android Downloads folder" ;;
        "zh:termux_auth") printf '%s' "正在调用 termux-setup-storage 申请存储授权..." ;;
        "en:termux_auth") printf '%s' "Requesting storage access with termux-setup-storage..." ;;
        "zh:termux_auth_failed") printf '%s' "存储授权失败，zip 将保留在原位置。" ;;
        "en:termux_auth_failed") printf '%s' "Storage authorization failed. The zip will stay where it is." ;;
        "zh:step_move_downloads") printf '%s' "尝试移动到 Downloads" ;;
        "en:step_move_downloads") printf '%s' "Trying to move the zip to Downloads" ;;
        "zh:move_success") printf '%s' "已成功移动 zip。" ;;
        "en:move_success") printf '%s' "The zip was moved successfully." ;;
        "zh:move_failed") printf '%s' "移动失败，zip 将保留在原位置。" ;;
        "en:move_failed") printf '%s' "Moving failed. The zip will stay at the original location." ;;
        "zh:final_title") printf '%s' "导出完成" ;;
        "en:final_title") printf '%s' "Export Completed" ;;
        "zh:final_path_label") printf '%s' "zip 文件位置" ;;
        "en:final_path_label") printf '%s' "Zip file location" ;;
        "zh:final_hint") printf '%s' "现在可以在 TauriTavern 的 data-migration 扩展中导入这个 zip。" ;;
        "en:final_hint") printf '%s' "You can now import this zip from the data-migration extension in TauriTavern." ;;
        ":warning"|"zh:warning") printf '%s' "警告" ;;
        "en:warning") printf '%s' "Warning" ;;
        ":info"|"zh:info") printf '%s' "信息" ;;
        "en:info") printf '%s' "Info" ;;
        ":error"|"zh:error") printf '%s' "错误" ;;
        "en:error") printf '%s' "Error" ;;
        *)
            printf '%s' "$1"
            ;;
    esac
}

print_banner() {
    printf '\n============================================================\n'
    printf '%s\n' "$(msg "banner_title")"
    printf '%s\n' "$(msg "banner_subtitle")"
    printf '============================================================\n\n'
}

print_step() {
    printf '\n[%s] %s\n' "$1" "$2"
}

print_info() {
    printf '%s: %s\n' "$(msg "info")" "$1"
}

print_warning() {
    printf '%s: %s\n' "$(msg "warning")" "$1" >&2
}

print_error() {
    printf '%s: %s\n' "$(msg "error")" "$1" >&2
}

read_user_input() {
    READ_INPUT=""

    if [ -t 0 ]; then
        IFS= read -r READ_INPUT
        return $?
    fi

    if [ -t 1 ] || [ -t 2 ]; then
        IFS= read -r READ_INPUT 2>/dev/null < /dev/tty
        return $?
    fi

    return 1
}

read_user_input_or_exit() {
    if read_user_input; then
        return
    fi

    printf '\n' >&2
    print_error "$(msg "input_required")"
    exit 1
}

normalize_input_path() {
    printf '%s' "$1" | sed "s/^[[:space:]]*//;s/[[:space:]]*$//;s/^'//;s/'$//;s/^\"//;s/\"$//"
}

is_sillytavern_root() {
    candidate=$1
    [ -d "$candidate/data/default-user" ] \
        && [ -d "$candidate/public/scripts/extensions/third-party" ] \
        && [ -f "$candidate/package.json" ]
}

select_language() {
    choice=""
    while :; do
        printf '%s\n' "$(msg "language_title")"
        printf '%s\n' "$(msg "language_option_zh")"
        printf '%s\n' "$(msg "language_option_en")"
        printf '%s' "$(msg "language_prompt")"
        read_user_input_or_exit
        choice=$READ_INPUT
        case "$choice" in
            ""|1)
                LANG_CODE="zh"
                return
                ;;
            2)
                LANG_CODE="en"
                return
                ;;
        esac
    done
}

resolve_sillytavern_root() {
    current_dir=$(pwd -P)
    if is_sillytavern_root "$current_dir"; then
        SILLYTAVERN_ROOT=$current_dir
        print_info "$(msg "detected_root") $SILLYTAVERN_ROOT"
        return
    fi

    print_info "$(msg "not_in_root")"
    print_info "$(msg "root_requirements")"

    while :; do
        printf '%s\n' "$(msg "root_prompt")"
        printf '> '
        read_user_input_or_exit
        input_path=$READ_INPUT
        input_path=$(normalize_input_path "$input_path")
        if [ -z "$input_path" ]; then
            print_warning "$(msg "root_invalid")"
            continue
        fi
        if ! is_sillytavern_root "$input_path"; then
            print_warning "$(msg "root_invalid")"
            continue
        fi
        SILLYTAVERN_ROOT=$(cd "$input_path" && pwd -P)
        print_info "$(msg "detected_root") $SILLYTAVERN_ROOT"
        return
    done
}

ask_backups() {
    printf '%s' "$(msg "ask_backups")"
    read_user_input_or_exit
    answer=$READ_INPUT
    case $(printf '%s' "$answer" | tr '[:upper:]' '[:lower:]') in
        y|yes|1|是)
            INCLUDE_BACKUPS=1
            ;;
        *)
            INCLUDE_BACKUPS=0
            ;;
    esac
}

detect_termux() {
    if command -v termux-setup-storage >/dev/null 2>&1; then
        IS_TERMUX=1
        return
    fi
    case "${PREFIX:-}" in
        /data/data/com.termux/*)
            IS_TERMUX=1
            ;;
        *)
            IS_TERMUX=0
            ;;
    esac
}

require_zip() {
    if command -v zip >/dev/null 2>&1; then
        return
    fi

    print_error "$(msg "zip_requires")"
    if [ "$IS_TERMUX" -eq 1 ]; then
        print_error "$(msg "zip_requires_termux")"
    fi
    exit 1
}

copy_directory_children() {
    source_dir=$1
    destination_dir=$2
    exclude_name=${3:-}

    mkdir -p "$destination_dir"

    if [ -n "$exclude_name" ]; then
        find "$source_dir" -mindepth 1 -maxdepth 1 ! -name "$exclude_name" -exec cp -R {} "$destination_dir" \;
        return
    fi

    find "$source_dir" -mindepth 1 -maxdepth 1 -exec cp -R {} "$destination_dir" \;
}

render_progress() {
    label=$1
    current=$2
    total=$3
    width=32

    if [ "$total" -le 0 ]; then
        total=1
    fi
    if [ "$current" -gt "$total" ]; then
        current=$total
    fi

    percent=$((current * 100 / total))
    filled=$((current * width / total))
    empty=$((width - filled))

    filled_bar=$(printf '%*s' "$filled" '' | tr ' ' '#')
    empty_bar=$(printf '%*s' "$empty" '' | tr ' ' '-')

    printf '\r%s [%s%s] %3s%% (%s/%s)' "$label" "$filled_bar" "$empty_bar" "$percent" "$current" "$total"
}

create_staging_zip() {
    source_data_root="$SILLYTAVERN_ROOT/data"
    source_global_extensions="$SILLYTAVERN_ROOT/public/scripts/extensions/third-party"
    staging_data_root="$WORK_DIR/data"
    staging_default_user="$staging_data_root/default-user"
    staging_global_extensions="$staging_data_root/extensions/third-party"

    print_step "1/5" "$(msg "step_prepare")"
    mkdir -p "$staging_default_user"
    mkdir -p "$staging_global_extensions"

    print_step "2/5" "$(msg "step_copy_user")"
    if [ "$INCLUDE_BACKUPS" -eq 1 ]; then
        copy_directory_children "$source_data_root/default-user" "$staging_default_user"
    else
        copy_directory_children "$source_data_root/default-user" "$staging_default_user" "backups"
        print_info "$(msg "step_skip_backups")"
    fi

    if [ "$INCLUDE_BACKUPS" -eq 1 ] && [ ! -d "$staging_default_user/backups" ]; then
        mkdir -p "$staging_default_user/backups"
    fi

    print_step "3/5" "$(msg "step_copy_extensions")"
    copy_directory_children "$source_global_extensions" "$staging_global_extensions"

    print_step "4/5" "$(msg "step_count_entries")"
    entry_count=$(find "$staging_data_root" | wc -l | awk '{print $1}')
    if [ -z "$entry_count" ] || [ "$entry_count" -le 0 ]; then
        entry_count=1
    fi

    ZIP_PATH="$SILLYTAVERN_ROOT/tauritavern-data-$(date '+%Y%m%d-%H%M%S').zip"
    FINAL_ZIP_PATH="$ZIP_PATH"
    progress_fifo="$WORK_DIR/zip-progress.fifo"
    mkfifo "$progress_fifo"

    print_step "5/5" "$(msg "step_zip")"
    (
        cd "$WORK_DIR"
        zip -r "$ZIP_PATH" data >"$progress_fifo" 2>&1
    ) &
    zip_pid=$!
    ACTIVE_CHILD_PID=$zip_pid

    processed=0
    while IFS= read -r line; do
        case "$line" in
            *"adding:"*)
                processed=$((processed + 1))
                render_progress "$(msg "zip_progress")" "$processed" "$entry_count"
                ;;
            *"warning:"*|*"error:"*)
                printf '\n%s\n' "$line" >&2
                ;;
        esac
    done <"$progress_fifo"

    zip_status=0
    wait "$zip_pid" || zip_status=$?
    ACTIVE_CHILD_PID=""
    rm -f "$progress_fifo"

    if [ "$processed" -lt "$entry_count" ]; then
        render_progress "$(msg "zip_progress")" "$entry_count" "$entry_count"
    fi
    printf '\n'

    if [ "$zip_status" -ne 0 ]; then
        exit "$zip_status"
    fi
}

try_move_to_downloads() {
    if [ "$IS_TERMUX" -eq 1 ]; then
        download_dir="$HOME/storage/shared/Download"
        print_step "Post" "$(msg "step_move_termux")"
        print_info "$(msg "termux_auth")"
        if ! termux-setup-storage; then
            print_warning "$(msg "termux_auth_failed")"
            return
        fi
    else
        download_dir="$HOME/Downloads"
        print_step "Post" "$(msg "step_move_downloads")"
    fi

    if [ ! -d "$download_dir" ]; then
        print_warning "$(msg "move_failed")"
        return
    fi

    if mv "$ZIP_PATH" "$download_dir/"; then
        FINAL_ZIP_PATH="$download_dir/$(basename "$ZIP_PATH")"
        print_info "$(msg "move_success")"
        return
    fi

    print_warning "$(msg "move_failed")"
}

print_summary() {
    backups_label=$(msg "no")
    if [ "$INCLUDE_BACKUPS" -eq 1 ]; then
        backups_label=$(msg "yes")
    fi

    printf '\n------------------------------\n'
    printf '%s\n' "$(msg "summary_title")"
    printf '%s %s: %s\n' "-" "$(msg "summary_root")" "$SILLYTAVERN_ROOT"
    printf '%s %s: %s\n' "-" "$(msg "summary_backups")" "$backups_label"
    printf '%s %s: public/scripts/extensions/third-party -> data/extensions/third-party\n' "-" "$(msg "summary_global_ext")"
    printf '%s\n\n' "------------------------------"
}

print_final_location() {
    printf '\n============================================================\n'
    printf '%s\n' "$(msg "final_title")"
    printf '%s: %s\n' "$(msg "final_path_label")" "$FINAL_ZIP_PATH"
    printf '%s\n' "$(msg "final_hint")"
    printf '============================================================\n'
}

main() {
    select_language
    print_banner
    detect_termux
    require_zip
    resolve_sillytavern_root
    ask_backups
    print_summary

    WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/tauritavern-migration.XXXXXX")

    create_staging_zip

    if [ "$IS_TERMUX" -eq 1 ]; then
        try_move_to_downloads
    fi

    print_final_location
}

main "$@"
