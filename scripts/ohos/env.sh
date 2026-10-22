#!/usr/bin/env bash
# Source in the build shell. Do not replace the host compiler used by build.rs.
: "${OHOS_HOME:?}" "${TARGET_TRIPLE:?}"
export OHOS_NDK_HOME="$OHOS_HOME" OHOS_NATIVE_HOME="$OHOS_HOME/native"
case "$TARGET_TRIPLE" in
  aarch64-unknown-linux-ohos) ohos_arch=aarch64; export OHOS_ARCH=arm64-v8a ;;
  x86_64-unknown-linux-ohos) ohos_arch=x86_64; export OHOS_ARCH=x86_64 ;;
  *) echo "Unsupported OpenHarmony target: $TARGET_TRIPLE" >&2; return 2 ;;
esac
wrapper_dir="${RUNNER_TEMP:-/tmp}/tauritavern-ohos-compilers"
mkdir -p "$wrapper_dir"
for compiler in clang clang++; do
  cat > "$wrapper_dir/$ohos_arch-$compiler" <<WRAPPER
#!/usr/bin/env bash
exec "$OHOS_NATIVE_HOME/llvm/bin/$compiler" --target=$ohos_arch-linux-ohos --sysroot="$OHOS_NATIVE_HOME/sysroot" -D__MUSL__ "\$@"
WRAPPER
  chmod +x "$wrapper_dir/$ohos_arch-$compiler"
done
underscore="${TARGET_TRIPLE//-/_}"
export "CC_$underscore=$wrapper_dir/$ohos_arch-clang"
export "CXX_$underscore=$wrapper_dir/$ohos_arch-clang++"
export "AR_$underscore=$OHOS_NATIVE_HOME/llvm/bin/llvm-ar"
export "RANLIB_$underscore=$OHOS_NATIVE_HOME/llvm/bin/llvm-ranlib"
export "CXXSTDLIB_$underscore=c++"
export "CARGO_TARGET_${underscore^^}_LINKER=$wrapper_dir/$ohos_arch-clang"
export "BINDGEN_EXTRA_CLANG_ARGS_$underscore=--target=$ohos_arch-linux-ohos --sysroot=$OHOS_NATIVE_HOME/sysroot"
