# LTBox

[🇺🇸 English](../README.md) / [🇰🇷 한국어](README_ko-KR.md)

[![License: CC BY-NC-SA 4.0][cc-by-nc-sa-shield]][cc-by-nc-sa]

## ⚠️ 免责声明

**仅供教育用途。** 修改固件可能导致设备变砖、数据丢失或保修失效。作者**不承担任何责任**。所有后果由用户自行承担。**使用风险自负。**

---

## 🔑 这是什么？

某些联想平板电脑出厂时使用了公开的 AOSP 测试密钥签名的固件。因此，即使引导加载程序处于**锁定状态**，也会信任并引导使用这些密钥签名的镜像。

LTBox 利用这一特性实现：

- 🌍 **区域转换** — 在 PRC（中国）和 ROW（国际）固件之间切换
- 🔓 **Root** — 在锁定的引导加载程序上安装 Magisk、KernelSU、APatch 等
- 🛡️ **反回滚绕过** — 绕过回滚保护，刷入更旧/更新的固件
- ⚡ **分区刷写** — 通过 EDL（紧急下载）模式读写分区

### 支持的设备

| 设备 | 备注 |
|---|---|
| Legion Tab Y700 第2、3代 | 完全支持 |
| Legion Tab Y700 第4代 | ZUXOS ≤ 1.5.10.138 |
| Yoga Pad Pro AI / Yoga Tab Plus AI | 完全支持 |
| 小新 Pad Pro GT / Yoga Tab 11.1 AI | 完全支持 |

> **注意：** 2026年及之后发布的设备（如 Y700 第5代）已修补此漏洞。

---

## 🚀 快速开始

1. 下载[最新版本](../../releases/latest)并解压（路径中不要包含空格或特殊字符）
2. 双击 **`ltbox.exe`**
3. 从侧边栏选择任务并按向导操作

目前发布 Windows x86_64 和 aarch64 两种架构的构建。

---

## 📋 功能介绍

侧边栏驱动的 GUI，每个入口打开一个引导式向导。

| 侧边栏条目 | 说明 |
|---|---|
| **仪表盘** | 设备状态、区域、最近使用的文件夹、一键操作 |
| **固件刷写** | 一键操作：区域 → 目标 → 清除/保留 → 刷写。端到端处理区域转换和回滚 |
| **系统更新** | 禁用或启用 OTA 更新；**启动恢复**用于抢救区域转换后 OTA 导致启动失败的设备 |
| **Root 设备** | 使用 KernelSU / KernelSU Next / SukiSU / ReSukiSU / APatch / FolkPatch / Magisk（及分支）获取 Root |
| **取消 Root** | 从之前的 Root 备份恢复原始引导镜像 |
| **重启** | 跳转到系统 / Recovery / Bootloader / EDL |
| **高级菜单** | 单独的流水线步骤供手动控制 — 见下方 |
| **设置** | 语言 (en/ko/zh/ru)、主题、默认区域、回滚预设、跳过 ADB |

### 高级菜单

逐步手动控制流水线：

- 区域转换（vendor_boot + vbmeta 重建）
- devinfo 和 persist 分区的转储 / 补丁 / 刷写
- 检测并补丁反回滚索引
- 解密 `.x` 文件 → XML
- 修改 XML 用于刷写（清除或保留数据）
- 通过 EDL 刷写固件或选定分区
- 为修改后的镜像重建 vbmeta
- 签名并刷写自定义 Recovery
- 查看 `.img` AVB 元数据

---

## 🔧 工作原理（简述）

**区域转换** 补丁 `vendor_boot.img` 中的字节（PRC↔ROW 区域标识符），然后使用 AOSP 测试密钥重新签名镜像，并重建 `vbmeta.img` 使引导加载程序接受它。

**Root** 解包 `boot.img` 或 `init_boot.img`，将 Root 方案文件注入 ramdisk，重新打包并使用原始 AVB 密钥重新签名。由于引导加载程序信任测试密钥签名，设备会引导修改后的镜像。

**反回滚绕过** 通过 Fastboot 读取设备当前的回滚索引，然后使用匹配的索引重新签名目标固件镜像，使引导加载程序不会将其视为"旧版本"而拒绝。

**所有刷写** 都通过 EDL 模式进行 — LTBox 处理完整流程：ADB → Fastboot → EDL 过渡、程序上传、分区读写和重置。

---

## 🏗️ 项目结构

| Crate | 职责 |
|---|---|
| `ltbox-core` | 基础原语 — 错误、设置、日志、GitHub/nightly.link 客户端、加密、XML 解密 |
| `ltbox-device` | 传输层 — ADB、Fastboot、EDL / QDL、serialport 探测 |
| `ltbox-patch` | 镜像流水线 — AVB、引导镜像 ramdisk 补丁、区域转换、回滚、Root 方案集成 |
| `ltbox-gui` | `iced` 桌面应用 — `ltbox.exe` 二进制 |

CI 在 `windows-latest` 运行器上通过 `cargo build --release` 同时为 `x86_64-pc-windows-msvc` 和 `aarch64-pc-windows-msvc` 两个目标构建与签名。

---

## 🙏 致谢

- **Anonymous [ㅇㅇ](https://gall.dcinside.com/board/lists?id=tabletpc)**
- **[갓파더](https://ppomppu.co.kr/zboard/view.php?id=androidtab&page=1&divpage=38&no=197457)**
- **[limzei89](https://note.com/limzei89/n/nd5217eb57827)**
- **[hitin911](https://xdaforums.com/m/hitin911.12861404/)**

---

## 📄 许可证

本作品基于 [CC BY-NC-SA 4.0][cc-by-nc-sa] 许可证发布。

[![CC BY-NC-SA 4.0][cc-by-nc-sa-image]][cc-by-nc-sa]

[cc-by-nc-sa]: http://creativecommons.org/licenses/by-nc-sa/4.0/
[cc-by-nc-sa-image]: https://licensebuttons.net/l/by-nc-sa/4.0/88x31.png
[cc-by-nc-sa-shield]: https://img.shields.io/badge/License-CC%20BY--NC--SA%204.0-lightgrey.svg
