# Distribution

本目录保存已经构建完成的软件包如何进入公开软件源的发布工具。构建配方位于
`packaging/`，GitHub Actions 的编排仍位于 `.github/workflows/`，生成产物不进入
本目录。

- `apt-rpm/publish.sh`：汇入 DEB/RPM，生成并签署 Stable 或 Canary 软件源元数据，
  再按安全顺序发布到 R2。
- `apt-rpm/rewrite-deb-version.sh`：为 Canary DEB 写入符合 Debian 排序规则的版本。
- `flatpak/publish.sh`：在现有 OSTree 仓库上导出、签署并按安全顺序发布 Stable
  Flatpak。
- `nix-cache/publish.sh`：校验 Nix cache key，签署 store closure 并发布缓存对象。

这些工具由 CI 调用，并在执行前校验所需环境变量和外部命令。用户安装入口仍为
`scripts/install-linux.sh`。
