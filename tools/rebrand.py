#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""一键改牌：把仓库里的产品名、应用标识、更新源换成自己的。

幂等：每次从代码里读当前品牌，再换成新的，可以反复跑。

用法:
    python tools/rebrand.py --name "BobbyCat" --id "com.bobbycat.editor" \
        --repo "bobbycat/bobbycat" [--dry] [--android]

不动 crate 名、模块名、i18n key，只改用户可见字符串与三处常量，
因此不会破坏编译。
"""
import argparse
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TEXT_EXT = (".rs", ".slint", ".toml", ".yml", ".yaml", ".json", ".iss", ".nsi")
SKIP_DIRS = {".git", "target", "node_modules", "assets", "models", "docs"}


def read(rel):
    with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
        return f.read()


def write(rel, text):
    with open(os.path.join(ROOT, rel), "w", encoding="utf-8", newline="") as f:
        f.write(text)


def current_brand():
    """从源码里读出现在的 名字 / 标识 / 仓库 / slug。"""
    ident = re.search(r'pub const IDENTIFIER: &str = "([^"]+)"',
                      read("src/crates/concat-host/src/dirs.rs")).group(1)
    repo = re.search(r'pub const REPO: &str = "([^"]+)"',
                     read("src/crates/concat-host/src/updates.rs")).group(1)
    name = re.search(r'title: "([^"]+)";', read("src/crates/concat/ui/app.slint")).group(1)
    slug = re.search(r'"name": "([^"]+)",', read("src/crates/concat-mcp/src/protocol.rs")).group(1)
    return {"name": name, "id": ident, "repo": repo, "slug": slug}


def files():
    for dirpath, dirnames, filenames in os.walk(ROOT):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for f in filenames:
            if f.endswith(TEXT_EXT):
                yield os.path.join(dirpath, f)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", required=True, help="产品显示名，如 BobbyCat")
    ap.add_argument("--id", required=True, help="应用标识，如 com.bobbycat.editor")
    ap.add_argument("--repo", required=True, help="GitHub 仓库，如 bobbycat/bobbycat")
    ap.add_argument("--dry", action="store_true")
    ap.add_argument("--android", action="store_true", help="连 Android 包名一起改")
    a = ap.parse_args()

    cur = current_brand()
    new_slug = re.sub(r"[^a-z0-9]", "", a.name.lower())
    print(f"当前品牌: {cur['name']} / {cur['id']} / {cur['repo']}")
    print(f"目标品牌: {a.name} / {a.id} / {a.repo}\n")

    # 顺序重要：先长串后短串，避免 id 被 name 规则切碎
    pairs = [
        (cur["id"], a.id),
        (cur["repo"], a.repo),
        (f'"name": "{cur["slug"]}",', f'"name": "{new_slug}",'),
        (f'"{cur["name"]}"', f'"{a.name}"'),
        (f'"{cur["name"]}\'s', f'"{a.name}\'s'),          # 所有格：Concat's
        (f'"{cur["name"]} could not start', f'"{a.name} could not start'),
        (f'{cur["name"]}Files', f'{a.name}Files'),
    ]

    total = 0
    touched = 0
    for path in files():
        try:
            text = open(path, encoding="utf-8").read()
        except (UnicodeDecodeError, OSError):
            continue
        if not any(old in text for old, _ in pairs):
            continue
        orig = text
        for old, new in pairs:
            text = text.replace(old, new)
        n = sum(orig.count(old) for old, _ in pairs)
        rel = os.path.relpath(path, ROOT)
        if not a.dry:
            write(rel, text)
        print(f"OK    {rel:<58} {n} 处")
        total += n
        touched += 1

    if a.android:
        total += rebrand_android_java(a.id, a.dry)

    print(f"\n{'[dry] ' if a.dry else ''}{touched} 个文件，{total} 处替换")
    if not a.dry:
        print("下一步: python tools/set_icon.py 图标.png")
    return 0


def rebrand_android_java(new_id, dry):
    base = os.path.join(ROOT, "android-native", "app", "src", "main", "java")
    old_pkg = os.path.join(base, *"app.concat.editor".split("."))
    new_pkg = os.path.join(base, *new_id.split("."))
    if not os.path.isdir(old_pkg):
        return 0
    if new_pkg != old_pkg:
        if dry:
            print(f"DRY   移目录 {old_pkg} -> {new_pkg}")
        else:
            os.makedirs(os.path.dirname(new_pkg), exist_ok=True)
            os.rename(old_pkg, new_pkg)
            print(f"OK    移目录 -> {new_pkg}")
    n = 0
    for dirpath, _, fs in os.walk(base):
        for f in fs:
            if f.endswith((".java", ".kt")):
                p = os.path.join(dirpath, f)
                t = open(p, encoding="utf-8").read()
                if "app.concat.editor" in t:
                    if not dry:
                        open(p, "w", encoding="utf-8", newline="").write(
                            t.replace("app.concat.editor", new_id))
                    n += 1
    return n


if __name__ == "__main__":
    sys.exit(main())
