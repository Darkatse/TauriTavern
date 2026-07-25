#!/bin/sh

# TauriTavern Linux installer.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh | sh
#   sh scripts/install-linux.sh --dry-run
#
# Keep all side effects behind main(). A truncated download therefore cannot run
# a partially received installer.

set -eu

REPOSITORY_ORIGIN="https://packages.tauritavern.com"
PACKAGE_NAME="tauri-tavern"
PRIMARY_KEY_FINGERPRINT="C75284E78972F19A0DDD88C487F5B8530682A857"
SIGNING_KEY_FINGERPRINT="D609D0B174E0073BB3980A1BEDC6CEF924B6C529"
NIX_INSTALLABLE="${TAURITAVERN_NIX_INSTALLABLE:-github:Darkatse/TauriTavern#tauritavern}"
NIX_CACHE="https://nix-cache.tauritavern.com"

DRY_RUN=0
NO_COLOR_REQUESTED=0
REQUESTED_METHOD="auto"
CHANNEL="stable"
PACKAGE_SYSTEM=""
SYSTEM_NAME=""
SYSTEM_ARCHITECTURE=""
PRIVILEGE_MODE=""
WORK_DIR=""
CURRENT_STEP=""
FAILURE_REPORTED=0
COLOR_RESET=""
COLOR_BOLD=""
COLOR_BLUE=""
COLOR_GREEN=""
COLOR_YELLOW=""
COLOR_RED=""
TOTAL_STEPS=4

command_exists() {
    command -v "$1" >/dev/null 2>&1
}

setup_colors() {
    if [ "$NO_COLOR_REQUESTED" -eq 0 ] &&
        [ -z "${NO_COLOR:-}" ] &&
        [ "${TERM:-dumb}" != "dumb" ] &&
        [ -t 1 ]; then
        COLOR_RESET="$(printf '\033[0m')"
        COLOR_BOLD="$(printf '\033[1m')"
        COLOR_BLUE="$(printf '\033[34m')"
        COLOR_GREEN="$(printf '\033[32m')"
        COLOR_YELLOW="$(printf '\033[33m')"
        COLOR_RED="$(printf '\033[31m')"
    fi
}

print_banner() {
    printf '\n%s%sTauriTavern%s  Linux Installer\n' \
        "$COLOR_BOLD" "$COLOR_BLUE" "$COLOR_RESET"
    printf '%s\n\n' "────────────────────────────────────────"
}

print_info() {
    printf '%s→%s %s\n' "$COLOR_BLUE" "$COLOR_RESET" "$1"
}

print_success() {
    printf '%s✓%s %s\n' "$COLOR_GREEN" "$COLOR_RESET" "$1"
}

print_warning() {
    printf '%s!%s %s\n' "$COLOR_YELLOW" "$COLOR_RESET" "$1" >&2
}

print_error() {
    printf '%s×%s %s\n' "$COLOR_RED" "$COLOR_RESET" "$1" >&2
}

print_step() {
    CURRENT_STEP=$2
    printf '\n%s%s[%s/%s]%s %s\n' \
        "$COLOR_BOLD" "$COLOR_BLUE" "$1" "$TOTAL_STEPS" "$COLOR_RESET" "$2"
}

usage() {
    cat <<'EOF'
Install TauriTavern through its native package repository or Nix flake.

Usage:
  install-linux.sh [--channel stable|canary] [--method auto|native|nix]
                   [--dry-run] [--no-color]

Options:
  --channel   Install from the stable or Canary channel. The default is stable.
  --method    Choose automatic detection, the native APT/RPM repository,
              or the Nix flake. The default is auto.
  --dry-run   Detect the system and print the installation plan.
  --no-color  Disable ANSI colors.
  -h, --help  Show this help.

Supported systems:
  Debian 12+              amd64, arm64
  Ubuntu 22.04 LTS+       amd64, arm64
  Fedora                  x86_64, aarch64
  openSUSE Leap 16.0      x86_64, aarch64
  Nix / NixOS             x86_64-linux, aarch64-linux

Native installs verify the complete OpenPGP primary and signing-subkey
fingerprints. Nix installs use the flake and its signed binary cache.
EOF
}

die() {
    FAILURE_REPORTED=1
    print_error "$1"
    exit 1
}

cleanup() {
    if [ -n "${WORK_DIR:-}" ] && [ -d "$WORK_DIR" ]; then
        case "$WORK_DIR" in
            */tauritavern-install.*) rm -rf -- "$WORK_DIR" ;;
            *) print_warning "Unexpected temporary path was not removed: $WORK_DIR" ;;
        esac
    fi
}

handle_exit() {
    exit_code=$?
    trap - 0
    cleanup

    if [ "$exit_code" -ne 0 ] && [ "$FAILURE_REPORTED" -eq 0 ]; then
        if [ -n "$CURRENT_STEP" ]; then
            print_error "Installation stopped during: $CURRENT_STEP"
        else
            print_error "Installation stopped before completion."
        fi
    fi

    exit "$exit_code"
}

handle_signal() {
    signal_code=$1
    FAILURE_REPORTED=1
    print_error "Installation interrupted."
    exit "$signal_code"
}

parse_arguments() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --dry-run)
                DRY_RUN=1
                ;;
            --no-color)
                NO_COLOR_REQUESTED=1
                ;;
            --method)
                [ "$#" -ge 2 ] || die "--method requires auto, native, or nix."
                REQUESTED_METHOD=$2
                shift
                ;;
            --channel)
                [ "$#" -ge 2 ] || die "--channel requires stable or canary."
                CHANNEL=$2
                shift
                ;;
            -h | --help)
                usage
                exit 0
                ;;
            --)
                shift
                break
                ;;
            *)
                die "Unknown option: $1"
                ;;
        esac
        shift
    done

    if [ "$#" -gt 0 ]; then
        die "Unexpected argument: $1"
    fi

    case "$REQUESTED_METHOD" in
        auto | native | nix) ;;
        *) die "Unsupported installation method: $REQUESTED_METHOD." ;;
    esac
    case "$CHANNEL" in
        stable | canary) ;;
        *) die "Unsupported release channel: $CHANNEL." ;;
    esac

    if [ -z "${TAURITAVERN_NIX_INSTALLABLE:-}" ]; then
        if [ "$CHANNEL" = "canary" ]; then
            NIX_INSTALLABLE="github:Darkatse/TauriTavern/Canary#canary"
        else
            NIX_INSTALLABLE="github:Darkatse/TauriTavern#tauritavern"
        fi
    fi
}

require_linux() {
    command_exists uname || die "The uname command is required."
    kernel_name=$(uname -s)
    if [ "$DRY_RUN" -eq 1 ] && [ -n "${TAURITAVERN_TEST_KERNEL:-}" ]; then
        kernel_name=$TAURITAVERN_TEST_KERNEL
    fi
    [ "$kernel_name" = "Linux" ] || die "This installer only supports Linux."
}

numeric_version_parts() {
    version_value=$1
    VERSION_MAJOR=${version_value%%.*}

    if [ "$VERSION_MAJOR" = "$version_value" ]; then
        VERSION_MINOR=0
    else
        version_remainder=${version_value#*.}
        VERSION_MINOR=${version_remainder%%.*}
    fi

    case "$VERSION_MAJOR" in
        "" | *[!0-9]*) die "Unsupported version identifier: $version_value" ;;
    esac
    case "$VERSION_MINOR" in
        "" | *[!0-9]*) die "Unsupported version identifier: $version_value" ;;
    esac
}

detect_system() {
    os_release_file="/etc/os-release"
    if [ "$DRY_RUN" -eq 1 ] && [ -n "${TAURITAVERN_TEST_OS_RELEASE:-}" ]; then
        os_release_file=$TAURITAVERN_TEST_OS_RELEASE
    fi

    if [ ! -r "$os_release_file" ]; then
        if [ "$REQUESTED_METHOD" = "nix" ]; then
            SYSTEM_NAME="Linux"
            PACKAGE_SYSTEM="nix"
            return
        fi
        die "Cannot read $os_release_file."
    fi

    ID=""
    VERSION_ID=""
    PRETTY_NAME=""
    # /etc/os-release is supplied by the operating system and is the canonical
    # distribution identity used by the supported package managers.
    # shellcheck disable=SC1090
    . "$os_release_file"

    [ -n "${ID:-}" ] || die "$os_release_file does not define ID."
    SYSTEM_NAME=${PRETTY_NAME:-$ID}

    if [ "$REQUESTED_METHOD" = "nix" ]; then
        PACKAGE_SYSTEM="nix"
        return
    fi

    case "$ID" in
        debian)
            [ -n "${VERSION_ID:-}" ] || die "Debian VERSION_ID is missing."
            numeric_version_parts "$VERSION_ID"
            [ "$VERSION_MAJOR" -ge 12 ] ||
                die "Debian $VERSION_ID is unsupported; Debian 12 or later is required."
            PACKAGE_SYSTEM="apt"
            ;;
        ubuntu)
            [ -n "${VERSION_ID:-}" ] || die "Ubuntu VERSION_ID is missing."
            numeric_version_parts "$VERSION_ID"
            if [ "$VERSION_MAJOR" -lt 22 ] ||
                { [ "$VERSION_MAJOR" -eq 22 ] && [ "$VERSION_MINOR" -lt 4 ]; }; then
                die "Ubuntu $VERSION_ID is unsupported; Ubuntu 22.04 LTS or later is required."
            fi
            PACKAGE_SYSTEM="apt"
            ;;
        fedora)
            PACKAGE_SYSTEM="dnf"
            ;;
        opensuse-leap)
            [ "${VERSION_ID:-}" = "16.0" ] ||
                die "openSUSE Leap ${VERSION_ID:-unknown} is unsupported; Leap 16.0 is required."
            PACKAGE_SYSTEM="zypper"
            ;;
        nixos)
            if [ "$REQUESTED_METHOD" = "native" ]; then
                die "NixOS does not use the APT/RPM repository. Use --method nix."
            fi
            PACKAGE_SYSTEM="nix"
            ;;
        *)
            die "Unsupported Linux distribution: $SYSTEM_NAME."
            ;;
    esac
}

detect_architecture() {
    if [ "$DRY_RUN" -eq 1 ] && [ -n "${TAURITAVERN_TEST_ARCHITECTURE:-}" ]; then
        SYSTEM_ARCHITECTURE=$TAURITAVERN_TEST_ARCHITECTURE
    fi

    case "$PACKAGE_SYSTEM" in
        apt)
            if [ -z "$SYSTEM_ARCHITECTURE" ]; then
                command_exists dpkg || die "dpkg is required on Debian and Ubuntu."
                SYSTEM_ARCHITECTURE=$(dpkg --print-architecture)
            fi
            case "$SYSTEM_ARCHITECTURE" in
                amd64 | arm64) ;;
                *) die "Unsupported APT architecture: $SYSTEM_ARCHITECTURE." ;;
            esac
            ;;
        dnf | zypper)
            if [ -z "$SYSTEM_ARCHITECTURE" ]; then
                SYSTEM_ARCHITECTURE=$(uname -m)
            fi
            case "$SYSTEM_ARCHITECTURE" in
                x86_64 | aarch64) ;;
                *) die "Unsupported RPM architecture: $SYSTEM_ARCHITECTURE." ;;
            esac
            ;;
        nix)
            if [ -z "$SYSTEM_ARCHITECTURE" ]; then
                SYSTEM_ARCHITECTURE=$(uname -m)
            fi
            case "$SYSTEM_ARCHITECTURE" in
                x86_64) SYSTEM_ARCHITECTURE="x86_64-linux" ;;
                aarch64 | arm64) SYSTEM_ARCHITECTURE="aarch64-linux" ;;
                x86_64-linux | aarch64-linux) ;;
                *) die "Unsupported Nix system: $SYSTEM_ARCHITECTURE." ;;
            esac
            ;;
        *)
            die "Internal error: unknown package system $PACKAGE_SYSTEM."
            ;;
    esac
}

detect_privilege_mode() {
    command_exists id || die "The id command is required."

    if [ "$(id -u)" -eq 0 ]; then
        PRIVILEGE_MODE="root"
    elif command_exists sudo; then
        PRIVILEGE_MODE="sudo"
    elif command_exists doas; then
        PRIVILEGE_MODE="doas"
    else
        die "Root access is required. Install sudo or doas, or run this script as root."
    fi
}

run_as_root() {
    case "$PRIVILEGE_MODE" in
        root) "$@" ;;
        sudo) sudo "$@" ;;
        doas) doas "$@" ;;
        *) die "Internal error: privilege mode is not configured." ;;
    esac
}

require_package_manager() {
    case "$PACKAGE_SYSTEM" in
        apt)
            command_exists apt-get || die "apt-get is required on Debian and Ubuntu."
            ;;
        dnf)
            command_exists dnf || die "dnf is required on Fedora."
            ;;
        zypper)
            command_exists zypper || die "zypper is required on openSUSE Leap."
            ;;
    esac
    command_exists install || die "The install command is required."
}

run_nix() {
    nix \
        --extra-experimental-features 'nix-command flakes' \
        --accept-flake-config \
        "$@"
}

require_nix() {
    command_exists nix || die "Nix is required for --method nix. Install Nix first, then run this script again."
    run_nix profile --help >/dev/null
}

prepare_package_tools() {
    case "$PACKAGE_SYSTEM" in
        apt)
            export DEBIAN_FRONTEND=noninteractive
            run_as_root apt-get update
            run_as_root apt-get install -y ca-certificates curl gnupg
            ;;
        dnf)
            run_as_root dnf install -y ca-certificates curl gnupg2
            ;;
        zypper)
            run_as_root zypper --non-interactive refresh
            run_as_root zypper --non-interactive install ca-certificates curl gpg2
            ;;
    esac

    command_exists curl || die "curl is unavailable after installing package tools."
    if command_exists gpg; then
        GPG_COMMAND="gpg"
    elif command_exists gpg2; then
        GPG_COMMAND="gpg2"
    else
        die "GnuPG is unavailable after installing package tools."
    fi
}

download_and_verify_key() {
    key_file="$WORK_DIR/tauritavern-archive-keyring.asc"
    gnupg_home="$WORK_DIR/gnupg"
    mkdir -m 0700 "$gnupg_home"

    curl \
        --fail \
        --silent \
        --show-error \
        --location \
        --proto '=https' \
        --tlsv1.2 \
        --retry 3 \
        "$REPOSITORY_ORIGIN/keys/tauritavern-archive-keyring.asc" \
        --output "$key_file"

    key_metadata=$(
        "$GPG_COMMAND" \
            --homedir "$gnupg_home" \
            --batch \
            --show-keys \
            --with-colons \
            --fingerprint \
            "$key_file" 2>/dev/null
    )

    if ! printf '%s\n' "$key_metadata" | awk -F: \
        -v primary="$PRIMARY_KEY_FINGERPRINT" \
        -v signing="$SIGNING_KEY_FINGERPRINT" '
            $1 == "pub" { key_kind = "primary" }
            $1 == "sub" { key_kind = "subkey" }
            $1 == "fpr" && key_kind == "primary" && $10 == primary {
                primary_found = 1
            }
            $1 == "fpr" && key_kind == "subkey" && $10 == signing {
                signing_found = 1
            }
            END { exit !(primary_found && signing_found) }
        '; then
        die "Repository key fingerprint verification failed."
    fi

    print_success "Repository key verified"
    printf '  Primary: %s\n' "$PRIMARY_KEY_FINGERPRINT"
    printf '  Signing: %s\n' "$SIGNING_KEY_FINGERPRINT"
}

configure_apt_repository() {
    source_file="$WORK_DIR/tauritavern.sources"
    cat >"$source_file" <<EOF
Types: deb
URIs: $REPOSITORY_ORIGIN/apt
Suites: $CHANNEL
Components: main
Architectures: $SYSTEM_ARCHITECTURE
Signed-By: /etc/apt/keyrings/tauritavern-archive-keyring.asc
EOF

    run_as_root install -d -m 0755 /etc/apt/keyrings
    run_as_root install -m 0644 \
        "$WORK_DIR/tauritavern-archive-keyring.asc" \
        /etc/apt/keyrings/tauritavern-archive-keyring.asc
    run_as_root install -m 0644 \
        "$source_file" \
        /etc/apt/sources.list.d/tauritavern.sources
}

configure_rpm_key() {
    run_as_root install -d -m 0755 /etc/pki/rpm-gpg
    run_as_root install -m 0644 \
        "$WORK_DIR/tauritavern-archive-keyring.asc" \
        /etc/pki/rpm-gpg/RPM-GPG-KEY-tauritavern
    run_as_root rpm --import /etc/pki/rpm-gpg/RPM-GPG-KEY-tauritavern
}

configure_dnf_repository() {
    repository_file="$WORK_DIR/tauritavern.repo"
    cat >"$repository_file" <<EOF
[tauritavern]
name=TauriTavern
baseurl=$REPOSITORY_ORIGIN/rpm/fedora/$CHANNEL/\$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-tauritavern
sslverify=1
EOF

    configure_rpm_key
    run_as_root install -m 0644 \
        "$repository_file" \
        /etc/yum.repos.d/tauritavern.repo
}

configure_zypper_repository() {
    repository_file="$WORK_DIR/tauritavern.repo"
    if [ "$CHANNEL" = "canary" ]; then
        opensuse_repository_path="16.0/canary"
    else
        opensuse_repository_path="16.0"
    fi
    cat >"$repository_file" <<EOF
[tauritavern]
name=TauriTavern
type=rpm-md
baseurl=$REPOSITORY_ORIGIN/rpm/opensuse/$opensuse_repository_path/\$basearch
enabled=1
autorefresh=1
gpgcheck=1
repo_gpgcheck=1
pkg_gpgcheck=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-tauritavern
EOF

    configure_rpm_key
    run_as_root install -m 0644 \
        "$repository_file" \
        /etc/zypp/repos.d/tauritavern.repo
}

configure_repository() {
    case "$PACKAGE_SYSTEM" in
        apt) configure_apt_repository ;;
        dnf) configure_dnf_repository ;;
        zypper) configure_zypper_repository ;;
    esac
}

install_package() {
    case "$PACKAGE_SYSTEM" in
        apt)
            run_as_root apt-get update
            run_as_root apt-get install -y "$PACKAGE_NAME"
            ;;
        dnf)
            run_as_root dnf -y makecache --refresh
            run_as_root dnf install -y "$PACKAGE_NAME"
            ;;
        zypper)
            run_as_root zypper \
                --non-interactive \
                --gpg-auto-import-keys \
                refresh \
                tauritavern
            run_as_root zypper --non-interactive install "$PACKAGE_NAME"
            ;;
    esac
}

install_nix_package() {
    if run_nix profile add --help >/dev/null 2>&1; then
        profile_action="add"
    else
        profile_action="install"
    fi
    run_nix profile "$profile_action" "$NIX_INSTALLABLE"
}

print_plan() {
    printf '%-16s %s\n' "System" "$SYSTEM_NAME"
    printf '%-16s %s\n' "Architecture" "$SYSTEM_ARCHITECTURE"
    printf '%-16s %s\n' "Install method" "$PACKAGE_SYSTEM"
    printf '%-16s %s\n' "Channel" "$CHANNEL"

    if [ "$PACKAGE_SYSTEM" = "nix" ]; then
        printf '%-16s %s\n' "Flake" "$NIX_INSTALLABLE"
        printf '%-16s %s\n' "Binary cache" "$NIX_CACHE"
        printf '\nThe installer will add TauriTavern to the current user Nix profile.\n'
        printf 'The Nix daemon must trust the cache; otherwise Nix may build locally.\n'
    else
        case "$PACKAGE_SYSTEM" in
            apt)
                repository_location="$REPOSITORY_ORIGIN/apt (suite: $CHANNEL)"
                ;;
            dnf)
                repository_location="$REPOSITORY_ORIGIN/rpm/fedora/$CHANNEL/\$basearch"
                ;;
            zypper)
                if [ "$CHANNEL" = "canary" ]; then
                    repository_location="$REPOSITORY_ORIGIN/rpm/opensuse/16.0/canary/\$basearch"
                else
                    repository_location="$REPOSITORY_ORIGIN/rpm/opensuse/16.0/\$basearch"
                fi
                ;;
        esac
        printf '%-16s %s\n' "Repository" "$repository_location"
        printf '%-16s %s\n' "Package" "$PACKAGE_NAME"
        printf '\nThe installer will:\n'
        printf '  1. Install curl, CA certificates, and GnuPG if needed.\n'
        printf '  2. Download and verify the repository OpenPGP key.\n'
        printf '  3. Configure the %s TauriTavern package repository.\n' "$CHANNEL"
        printf '  4. Install or update %s.\n' "$PACKAGE_NAME"
    fi
}

install_with_nix() {
    TOTAL_STEPS=1
    require_nix
    print_step 1 "Installing TauriTavern into the Nix profile"
    install_nix_package

    printf '\n'
    print_success "TauriTavern is installed in the current user profile."
    print_info "Run tauritavern, or launch it from your desktop application menu."
}

install_with_native_repository() {
    detect_privilege_mode
    require_package_manager
    WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/tauritavern-install.XXXXXX")

    print_step 1 "Preparing package tools"
    prepare_package_tools

    print_step 2 "Verifying repository identity"
    download_and_verify_key

    print_step 3 "Configuring the $CHANNEL repository"
    configure_repository
    print_success "Repository configured"

    print_step 4 "Installing TauriTavern"
    install_package

    printf '\n'
    print_success "TauriTavern is installed."
    print_info "Launch it from your desktop application menu."
}

main() {
    trap handle_exit 0
    trap 'handle_signal 129' HUP
    trap 'handle_signal 130' INT
    trap 'handle_signal 143' TERM

    parse_arguments "$@"
    setup_colors
    print_banner
    require_linux
    detect_system
    detect_architecture

    print_success "Detected $SYSTEM_NAME ($SYSTEM_ARCHITECTURE)"
    print_plan

    if [ "$DRY_RUN" -eq 1 ]; then
        printf '\n'
        print_success "Dry run complete; no system changes were made."
        return
    fi

    if [ "$PACKAGE_SYSTEM" = "nix" ]; then
        install_with_nix
    else
        install_with_native_repository
    fi
}

main "$@"
