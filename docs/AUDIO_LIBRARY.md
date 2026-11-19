# The BobbyCat sound library

The Library's **Audio** page carries a **BobbyCat** shelf of built-in sounds
and music. Nothing is compiled into the app: the shelf reads a manifest the
first time someone asks for it (a "Load BobbyCat sounds" button, never at
startup), and each sound is fetched the first time it is played or used, then
checked against its SHA-256 and kept in the app's cache.

Every sound on the shelf is synthesised by `tools/make_audio.py` — noise,
sine and envelopes, no third-party samples — so the library is original work
released CC0-1.0. The script writes both halves of what the shelf needs: the
WAV files and the manifest that describes them.

```sh
python3 tools/make_audio.py     # rewrites assets/audio/ and its manifest
```

## Layout

```text
assets/audio/
  manifest.json
  whoosh-air-short.wav
  whoosh-deep-slow.wav
  impact-low-thud.wav
  impact-metal-tick.wav
  riser-short-tension.wav
  sub-drop-down.wav
  click-ui-soft.wav
  pop-bubble.wav
  ambience-room-tone.wav
  music-pulse-loop.wav
```

The files sit flat in `assets/audio/`. The app fetches the manifest from:

```text
https://raw.githubusercontent.com/bobby663698/bobbycat/main/assets/audio/manifest.json
```

and each sound from, in order:

1. the release mirror: `https://github.com/bobby663698/bobbycat/releases/download/audio-v1/<file>`
2. the repository itself: `https://raw.githubusercontent.com/bobby663698/bobbycat/main/assets/audio/<file>`

Upload the files to an `audio-v1` release as well when one is made, so a file
that moves in the repository cannot break the shelf. Whichever source the
bytes come from, they are checked against the manifest's `sha256`. The URLs
are constants in `src/crates/concat-host/src/models.rs`: `SOUNDS_MANIFEST`,
`SOUNDS_REPO` and `SOUNDS_RELEASE`.

## The manifest

`version` is 1. `sounds` is a list, in the order the shelf shows them, of:

| field | what it is |
| --- | --- |
| `id` | stable for ever, `[a-z0-9-]`; the cache is keyed on it |
| `name` | the row's title |
| `category` | Whoosh, Impact, Riser, Ambience or Music |
| `file` | the file's name in `assets/audio/`, with no folder in it |
| `duration` | seconds |
| `bytes` | the size, for the progress bar |
| `license` | `CC0-1.0`; anything else is not offered |
| `source` | how the sound was made |
| `author` | `BobbyCat` |
| `sha256` | lowercase hex of the bytes; a download that misses it is thrown away |

An entry missing its digest, its plain file name, or its CC0 licence is
dropped before the shelf ever sees it. If the manifest itself cannot be
fetched, the shelf says the library is coming soon rather than showing a
half-filled list.
