# Code signing policy

**Windows releases are signed.** The setup `.exe`, the `.msi`, and every
`.exe`/`.dll` shipped beside the program carry an Authenticode signature made
in CI with SHA-256 and a DigiCert timestamp, so a signature stays valid after
the certificate expires.

The certificate is currently **self-signed**, issued to `BobbyCat`. Windows has
not been told to trust it, so the first run on a machine that has not installed
it still shows a warning. Install the public half once and the publisher line
reads `BobbyCat` from then on:

```powershell
# 下载发布页附带的 bobbycat-cert.cer，然后
certutil -addstore -user Root bobbycat-cert.cer
```

Without it, Windows shows **More info** › **Run anyway**; where Smart App
Control is on, it refuses to run the program at all until the certificate is
trusted.

## Free trusted signing, when the project is accepted

[SignPath.io](https://signpath.io/) signs open-source projects for free, and
its certificate chains to a CA Windows already trusts — nothing to install on
the user's side, and no prompt on first run. The repository already carries
what the plan needs:

- `.github/workflows/sign-windows.yml` — takes a finished Build app run, signs
  the two installers, rewrites `SHA256SUMS.txt` over the signed bytes.
- `signing/signpath-artifact-config.xml` — the artifact configuration to paste
  into SignPath.
- `signing/SIGNPATH-APPLICATION.md` — the answers the application asks for.

Turning it on needs the project's application to be accepted, then one secret
and four variables:

| 名称 | 值 |
|---|---|
| `SIGNPATH_API_TOKEN`（secret） | SignPath 用户页里的 token |
| `SIGNPATH_ORGANIZATION_ID` | 组织 id |
| `SIGNPATH_PROJECT_SLUG` | `bobbycat` |
| `SIGNPATH_POLICY_SLUG` | `release-signing` |
| `SIGNPATH_ARTIFACT_CONFIG` | zip 配置的 slug |

Its publisher line reads `SignPath Foundation`, not `BobbyCat`: the certificate
belongs to the foundation that vouches for the project.

## What gets signed

Everything in the Windows download: the program (`bobbycat.exe`), its
installers (the setup `.exe` and the `.msi`), and the libraries shipped beside
it. Smart App Control checks every library a program loads, so a signed program
with unsigned libraries is still refused.

## macOS

The bundle is signed ad-hoc, which is what a build without a paid Apple
Developer account gets. The first launch needs **right-click › Open**, or:

```bash
xattr -dr com.apple.quarantine /Applications/BobbyCat.app
```

Notarisation needs the paid programme; the workflow
(`build-app.yml`, `Sign and notarise (macOS)`) already does the full
`codesign` › `notarytool submit` › `stapler staple` sequence once these exist:

| 名称 | 值 |
|---|---|
| `APPLE_CERTIFICATE_P12` | Developer ID Application 证书，base64 |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 p12 时的密码 |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: 名字 (TEAMID)` |
| `APPLE_ID` | Apple ID |
| `APPLE_TEAM_ID` | 10 位 Team ID |
| `APPLE_APP_PASSWORD` | App 专用密码 |

## How a release is made

- Every released file is built by GitHub Actions from a tagged commit of
  <https://github.com/bobby663698/bobbycat>, by the workflows in
  [`.github/workflows`](.github/workflows). Nothing built on a person's machine
  is released.
- A release is built only after the tests pass on the tagged commit, and each
  bundle is launched before it is uploaded (see [`scripts/ci`](scripts/ci)).
- Every file's SHA-256 is written into `SHA256SUMS.txt` beside it, so a
  download can be checked without trusting the connection it came over.
- The workflows and build scripts are part of the source and are reviewed like
  any other change.

## What BobbyCat sends over the network

BobbyCat reaches the network only when you ask it to:

- **Settings › Version** asks GitHub for the list of releases when you open it
  or press **Check for updates**, and installing one downloads it from GitHub.
- **Downloading a model** (transcription, speech, cutout) fetches it from the
  source picked under **Downloads** in Settings › General, and tries the other
  sources listed there if that one fails.
- **The Remote API** is off by default. Turned on, it listens on this computer
  (`127.0.0.1`) unless you give it another address, and answers only callers
  that hold its token.

Nothing is sent about you, your projects or your media, and there is no
telemetry.
