# 开发与贡献

当前目标是 Windows x64；macOS 适配尚未完成。界面使用 Vue 3 / TypeScript，桌面能力使用 Tauri 2 / Rust，本地记录使用 SQLite。

## 开发环境

- Node.js 20+ 与 npm；当前产物使用 Node.js 24。
- Rust 1.90+、MSVC 工具链、Windows SDK 和 WebView2。
- 首次安装依赖及构建需要访问 npm / Cargo 包源。

## 常用命令

    npm ci
    npm run tauri -- dev

生成 Windows NSIS 安装包：

    npm run tauri -- build --bundles nsis

产物位于 src-tauri/target/release/bundle/nsis。项目已经提供图标与中文安装配置，完成页可创建桌面快捷方式。

## 文件位置

- src/components：剪贴板、预览、贴图、框选、转换与设置界面。
- src-tauri/src：原生窗口、剪贴板、OCR、转换队列及组件下载。
- components/download-manifest.json：固定版本、来源、大小及 SHA-256；修改版本时须同步维护生成脚本和组件来源说明。
- docs/demo.html：无需依赖的离线 HTML 动画演示。

普通开发环境按标准构建即可。scripts/export-build-assets.cjs 用于可靠导出资源；finish-windows-installer.cjs 仅用于已生成 NSIS 输入受本机透明加密影响的特殊环境，不应改变系统保护或跳过下载校验。

## 提交修改

请使用 UTF-8、可读中文并保持现有文件风格。修改前说明涉及的功能与文件，交接时区分代码完成、构建结果和实际验证结果。请勿提交 node_modules、target、运行数据库、私人剪贴板、令牌或本机交接资料。

贡献代码须有权按项目 AGPL-3.0-only 协议提供。第三方代码请保留原始许可及版权说明。
