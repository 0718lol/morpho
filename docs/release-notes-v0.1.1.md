# Morpho v0.1.1 — 万物皆可转 🦋

本版本修复了 Windows 安装版"点转换没反应"及安装包无法构建的一系列问题,并包含 PDF→Word、扫描件自动 OCR 等新功能。

## 🛠 关键修复(Windows)

- **GUI 转换无反应**:同步 Tauri 命令在主线程调用 `tokio::spawn` 导致 panic。现已改为 async 命令 + 队列内建 fallback 运行时 — 影响**所有**格式的 GUI 转换,不只 docx→PDF。
- **LibreOffice 引擎不完整**:NSIS 资源通配符会丢掉子目录,导致安装版缺少 `program/soffice.exe`,docx→PDF 报"engine binary not found"。现改为目录映射打包,1.5GB LibreOffice(19373 个文件)完整落地。
- **安装包无法构建**:Tauri 默认 NSIS 模板写死 solid 压缩,单数据块 2GB 上限扛不住 2.26GB 的引擎载荷。现内置非 solid 模板(zlib),安装包 700MB。
- 历史记录目录自动创建,不再出现 `history open failed`。
- docx→HTML 输出内嵌图片资源(`--embed-resources`),不再出现裂图。

## ✨ 新功能

- PDF→Word 版面还原引擎、可搜索 PDF 生成、扫描件自动 OCR(4f55096)
- 转换历史:归档 / 恢复 / 彻底删除,双击确认防误删(4dad470)

## ✅ 验证

Windows 11 上通过 GUI IPC 实测 5 条转换路径全部通过:
docx→PDF(LibreOffice)· pdf→PNG(poppler)· md→docx(pandoc)· mp4→MP3(ffmpeg)· gif→PNG(内置图像引擎),6 个引擎状态全部就绪。

## 📦 安装

下载 `Morpho_0.1.1_x64-setup.exe`(约 700MB,含全部 6 个本地引擎,无需联网),双击安装即可。支持简体中文/英文界面。

**Full Changelog**: https://github.com/0718lol/morpho/compare/v0.1.0...v0.1.1
