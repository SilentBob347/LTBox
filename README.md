# LTBox

[🇰🇷 한국어](READMEs/README_ko-KR.md) / [🇨🇳 简体中文](READMEs/README_zh-CN.md)

[![License: CC BY-NC-SA 4.0][cc-by-nc-sa-shield]][cc-by-nc-sa]

## ⚠️ Disclaimer

**Educational purposes only.** Modifying firmware can brick your device, cause data loss, or void your warranty. The author assumes **no liability**. You are solely responsible. **Use at your own risk.**

---

## 🔑 What Is This?

Certain Lenovo tablets ship a bootloader (ABL) with the **AOSP test public key** embedded as the Android Verified Boot (AVB) root of trust (`AvbRSAPublicKey` struct). The matching private key is published in AOSP source (`external/avb/test/data/testkey_rsa4096.pem`), so anyone can sign a `vbmeta` image that the locked bootloader still accepts as authentic.

LTBox exploits this to enable:

- 🌍 **Region conversion** — switch between PRC (China) and ROW (Global) firmware
- 🔓 **Root** — install Magisk, KernelSU, APatch, and more on a locked bootloader
- 🛡️ **Anti-rollback bypass** — flash older/newer firmware without rollback protection blocking you
- ⚡ **Partition flashing** — read/write partitions via EDL (Emergency Download) mode

<a id="fnref-patched"></a>

### Supported Devices [‡](#fn-patched)

| Device | Notes |
|---|---|
| Legion Tab Y700 2nd, 3rd Gen | Full support |
| Legion Tab Y700 4th Gen | ZUXOS ≤ **`1.5.10.138`** <a id="fnref-y700-4th-gen-cutoff"></a>[†](#fn-y700-4th-gen-cutoff) |
| Yoga Pad Pro AI / Yoga Tab Plus AI | Full support |
| Xiaoxin Pad Pro GT / Yoga Tab 11.1 AI | Full support |

---

## 🚀 Quick Start

<details>
<summary><strong>🪟 Windows</strong> — <code>x86_64</code> / <code>arm64</code></summary>

<br>

1. Download the [latest release](../../releases/latest) and extract the zip (no spaces or special chars in the path)
2. Double-click **`ltbox.exe`**
3. Pick a task from the sidebar and follow the wizard

> **Qualcomm USB drivers:** the dashboard shows an "Install drivers" banner when the Qualcomm USB drivers are missing. Clicking it downloads and installs the latest `qcom-usb-kernel-drivers` release from GitHub via `pnputil`. Run LTBox as Administrator the first time so `pnputil` can land the `.inf` files.

</details>

<details>
<summary><strong>🐧 Linux</strong> — <code>x86_64</code> / <code>aarch64</code></summary>

<br>

1. Install runtime deps (Debian/Ubuntu shown — adapt for your distro):
   ```bash
   sudo apt install \
     libusb-1.0-0 libudev1 \
     libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 \
     libxcb1 libxcb-render0 libxcb-shape0 libxcb-xfixes0 \
     libfontconfig1 \
     xdg-utils
   ```
2. Download the [latest release](../../releases/latest) Linux tarball (`tar -xzf LTBox-linux_*.tar.gz`). The executable bit on `ltbox` is preserved.
3. Install the udev rules so the desktop session can open the Qualcomm 9008 / Lenovo USB devices without root:
   ```bash
   sudo ./ltbox --install-udev
   ```
4. **Replug** any connected device.
5. (Optional) Add an app-menu entry + icon (per-user, no root):
   ```bash
   ./ltbox --install-desktop
   ```
   Drops a `.desktop` file under `~/.local/share/applications/` and the SVG icon under `~/.local/share/icons/hicolor/scalable/apps/`. GNOME / KDE pick it up within a few seconds. Re-run after moving the binary.
6. Run `./ltbox`.

</details>

---

## 📋 What Can It Do?

The app is a sidebar-driven GUI. Each entry opens a guided wizard.

| Sidebar entry | What it does |
|---|---|
| **Dashboard** | Device status, region, recent folders, one-click actions |
| **Flash Firmware** | All-in-one: region → target → wipe/keep → flash. Drives region conversion + rollback handling end-to-end. |
| **System Update** | Disable or enable OTA updates; **Boot Recovery** for rescuing a device that failed to boot after an OTA on a converted region |
| **Root Device** | Root with KernelSU / KernelSU Next / SukiSU / ReSukiSU / APatch / FolkPatch / Magisk (+ forks) |
| **Unroot Device** | Restore the stock boot image from a prior Root backup |
| **Reboot** | Jump to System, Recovery, Bootloader, or EDL |
| **Advanced** | Individual pipeline steps for manual control — see below |
| **Settings** | Language (en/ko/zh/ru), theme (system/light/dark), default EDL loader path |

### Advanced Menu

<details>
<summary>Step-by-step manual control over the pipeline, grouped into three sections</summary>

<br>

**Region & patch**
- Convert region (vendor_boot + vbmeta rebuild)
- Patch devinfo / persist

**Rollback**
- Inspect `.img` AVB metadata
- Detect anti-rollback index
- Patch anti-rollback index
- Rebuild vbmeta for modified images

**EDL ops**
- Decrypt `.x` files → XML
- Dump / flash partitions by name (GPT-by-name, EDL)
- Dump / flash physical LUNs (whole-LUN, EDL)

</details>

---

## 🔧 How It Works (Briefly)

**Region conversion** patches bytes in `vendor_boot.img` (PRC↔ROW region identifiers), then re-signs the image with AOSP test keys and rebuilds `vbmeta.img` so the bootloader accepts it.

**Rooting** unpacks `boot.img` or `init_boot.img`, injects root provider files into the ramdisk, repacks, and re-signs with the original AVB keys. The device boots the modified image because the bootloader trusts the test key signature.

**Anti-rollback bypass** reads the device's current rollback index via Fastboot, then re-signs the target firmware images with a matching index so the bootloader doesn't reject them as "older" builds.

**All flashing** goes through EDL mode — LTBox handles the full flow: ADB → Fastboot → EDL transition, programmer upload, partition read/write, and reset. AVB-signed images use the embedded AOSP `testkey_rsa2048` / `testkey_rsa4096` specs from `avbtool-rs` (no PEM file needed) so re-signed `vbmeta` and root-injected `boot` images verify against the bootloader's pinned test keys.

---

## 🏗️ Project Layout

| Crate | Role |
|---|---|
| `ltbox-core` | Primitives — errors, settings, logging, GitHub / nightly.link / Lenovo PTSTPD clients, crypto, XML decrypt, live-log sink |
| `ltbox-device` | Transport layer — ADB, Fastboot, EDL / QDL, serialport discovery, Windows Qualcomm USB driver probe + auto-install |
| `ltbox-patch` | Image pipeline — AVB (embedded AOSP testkey specs), boot image ramdisk patching, region conversion, rollback index handling, root provider integration |
| `ltbox-gui` | `iced` desktop app — the `ltbox.exe` binary |

---

## 📝 Notes

<a id="fn-patched"></a>

<details>
<summary><strong>‡ Patched devices — vulnerability fixed in newer hardware.</strong> <a href="#fnref-patched">↩</a></summary>

<br>

Devices released in 2026+ (e.g. Y700 5th Gen) ship with the AOSP test key removed from the ABL trust path at the factory. The vulnerability is no longer exploitable by LTBox on those devices.

</details>

<a id="fn-y700-4th-gen-cutoff"></a>

<details>
<summary><strong>† Y700 4th Gen cutoff — ZUXOS <code>1.5.10.138</code> is the last vulnerable build.</strong> <a href="#fnref-y700-4th-gen-cutoff">↩</a></summary>

<br>

ZUXOS `1.5.10.183` ships an updated ABL: the embedded `AvbRSAPublicKey` is swapped from the AOSP test key to the Y700 5th Gen production RSA-4096 public key.

</details>

---

## 🙏 Credits

- **Anonymous [ㅇㅇ](https://gall.dcinside.com/board/lists?id=tabletpc)**
- **[갓파더](https://ppomppu.co.kr/zboard/view.php?id=androidtab&page=1&divpage=38&no=197457)**
- **[limzei89](https://note.com/limzei89/n/nd5217eb57827)**
- **[hitin911](https://xdaforums.com/m/hitin911.12861404/)**

---

## 📄 License

This work is licensed under [CC BY-NC-SA 4.0][cc-by-nc-sa].

[![CC BY-NC-SA 4.0][cc-by-nc-sa-image]][cc-by-nc-sa]

[cc-by-nc-sa]: http://creativecommons.org/licenses/by-nc-sa/4.0/
[cc-by-nc-sa-image]: https://licensebuttons.net/l/by-nc-sa/4.0/88x31.png
[cc-by-nc-sa-shield]: https://img.shields.io/badge/License-CC%20BY--NC--SA%204.0-lightgrey.svg
