# SignPath Foundation — 申请要填的内容

免费计划：<https://signpath.io/solutions/open-source-projects>
申请入口：注册后在 Organization › Foundation plan 里提交项目。

## 可以直接粘贴的答案

**Project name**

```
BobbyCat
```

**Project URL**

```
https://github.com/bobby663698/bobbycat
```

**License**

```
AGPL-3.0-or-later
```

**Description**

```
BobbyCat is a local-first video editor written in Rust with a Slint interface,
for Windows, macOS and Linux. It edits on a timeline, burns in captions,
transcribes speech on the machine with whisper, does TTS and cutout, renders
with wgpu/D3D12 and encodes with FFmpeg. It has no accounts, no telemetry and
no cloud; the only network calls are the ones a person asks for (checking
releases, downloading a model, the optional local API).

Windows builds are made only by GitHub Actions from tagged commits
(.github/workflows/build-app.yml), which compiles the app, launches it to
check it starts, scans the binary against an instruction-set allowlist, then
builds the Inno Setup and WiX installers. The repository also carries an
artifact configuration and a signing workflow
(.github/workflows/sign-windows.yml) ready for this plan.
```

**Build system / CI**

```
GitHub Actions, rust-toolchain pinned in src/rust-toolchain.toml
```

**What is to be signed**

```
The two Windows installers: BobbyCat-<version>-windows-x86_64-setup.exe
(Inno Setup) and BobbyCat-<version>-windows-x86_64.msi (WiX).
```

**Artifact configuration**

`signing/signpath-artifact-config.xml` — 直接在 SignPath 的 Artifact
configurations 里新建，粘贴文件内容。

## 通过之后要给我的五样东西

| 名称 | 在哪里拿 |
|---|---|
| API token | SignPath 用户设置页 |
| Organization ID | 组织设置页 |
| Project slug | 建项目时自己填的，建议 `bobbycat` |
| Signing policy slug | 建议 `release-signing` |
| Artifact configuration slug | 建配置时自己填的 |

拿到后我写进仓库 secret / variables，跑一次 `Sign (Windows, SignPath)`，
把签好的安装包换到 Release 上。之后每次发版都走这条路，用户下载即装，
不装证书、不点「仍要运行」。

## 如果 SignPath 不收

Certum 的 Open Source Code Signing 也是 0 元，证书签给项目维护者本人：

1. <https://certum.store/code-signing/open-source-code-signing> 提交申请（开源项目、仓库地址、许可证）。
2. 身份核验通过后，证书发到 SimplySign 云签名（桌面端 + 手机 TOTP）。
3. 签名在本机做一次：下载 CI 产物 → SimplySign 签两个安装包 → 重新生成 `SHA256SUMS.txt` → 传到 Release。

代价：每次发版多一步人工签名；好处：发布者显示的是你自己的名字，不是基金会。
