#!/usr/bin/env python3
"""実装計画の機械検査。

Wave 2c のレビューで4回続けて見落とした「タスク境界をまたぐ前方参照」と、
段階的なフィールド追加で起きる未使用の検出を自動化する。
"""
import re
import sys
import unicodedata
from collections import Counter, defaultdict


def rust_blocks(text):
    """```rust ... ``` の中身を (開始オフセット, 中身) で返す。"""
    for m in re.finditer(r"```rust\n(.*?)```", text, re.S):
        yield m.start(), m.group(1)


def task_of(offset, bounds):
    for index, start in enumerate(bounds):
        end = bounds[index + 1] if index + 1 < len(bounds) else float("inf")
        if start <= offset < end:
            return index + 1
    return 0


def engine_field_regions(text):
    """`RoundEngine` と `Outstanding` のフィールドを宣言している範囲。

    `pub fn start(..)` の引数と区別するため、構造体の本体と
    「// RoundEngine へ」で始まる追加指示の範囲だけを見る。
    """
    for m in re.finditer(r"(?:pub )?struct (?:RoundEngine|Outstanding) \{\n(.*?)\n\}", text, re.S):
        yield m.start(), m.group(1)
    for m in re.finditer(r"^// RoundEngine へ\n(.*?)(?=^//|\n```)", text, re.S | re.M):
        yield m.start(), m.group(1)


def check_forward_references(text):
    """あるタスクのコードが、後のタスクで足す名前を使っていないか。

    - 定義: `fn` の名前、`const` の名前、構造体のフィールド名
    - 使用: `self.名前`、`変数.名前(`、裸の `名前(`。ただし定義済みの名前に限る

    `window.from()` のように他の型のメソッドは、その名前が定義側に無ければ
    そもそも突き合わせない。
    """
    bounds = [m.start() for m in re.finditer(r"^### Task ", text, re.M)]
    if not bounds:
        return []

    defined = {}
    for offset, body in engine_field_regions(text):
        task = task_of(offset, bounds)
        if task == 0:
            continue
        for line in body.split("\n"):
            field = re.match(r"\s*(?:pub )?(\w+)\s*:\s*[A-Za-z\[&<]", line)
            if field:
                name = field.group(1)
                defined[name] = min(defined.get(name, task), task)

    for offset, block in rust_blocks(text):
        task = task_of(offset, bounds)
        if task == 0:
            continue
        for line in block.split("\n"):
            for pattern in (r"\bfn (\w+)\s*\(", r"^\s*(?:pub(?:\(\w+\))?\s+)?const (\w+)\s*:"):
                m = re.search(pattern, line)
                if m:
                    name = m.group(1)
                    defined[name] = min(defined.get(name, task), task)

    used = defaultdict(list)
    for offset, block in rust_blocks(text):
        task = task_of(offset, bounds)
        if task == 0:
            continue
        for line in block.split("\n"):
            for name in re.findall(r"\bself\.(\w+)", line):
                used[name].append((task, line.strip()))
            for name in re.findall(r"[.\b](\w+)\s*\(", line):
                if name in defined:
                    used[name].append((task, line.strip()))

    problems = []
    for name, places in used.items():
        if name not in defined:
            continue
        first_use = min(task for task, _ in places)
        if first_use < defined[name]:
            line = next(l for t, l in places if t == first_use)
            problems.append(
                f"{name}: Task {defined[name]} で定義 / Task {first_use} で使用 -- {line[:70]}"
            )
    return sorted(problems)


def check_unread_fields(text):
    """宣言したタスクの中で一度も読まれないフィールド。

    `-D warnings` は「書かれるだけで読まれないフィールド」を dead_code に
    する。段階的に組み立てる計画では、これで各タスクの完了条件が崩れる。
    """
    bounds = [m.start() for m in re.finditer(r"^### Task ", text, re.M)]
    if not bounds:
        return []

    declared = {}
    for offset, body in engine_field_regions(text):
        task = task_of(offset, bounds)
        if task == 0:
            continue
        for line in body.split("\n"):
            field = re.match(r"\s*(?:pub )?(\w+)\s*:\s*[A-Za-z\[&<]", line)
            if field:
                name = field.group(1)
                declared[name] = min(declared.get(name, task), task)

    read = defaultdict(set)
    for offset, block in rust_blocks(text):
        task = task_of(offset, bounds)
        if task == 0:
            continue
        # `self.field` だけでなく `open.field` のようなローカル経由も数える。
        # 対象は宣言済みのフィールド名に限るので、他の型の項目は拾わない。
        for name in re.findall(r"\.(\w+)\b", block):
            if name in declared:
                read[name].add(task)

    problems = []
    for name, task in sorted(declared.items()):
        tasks = read.get(name, set())
        if not tasks:
            problems.append(f"{name}: Task {task} で宣言 / どこでも読まれない")
        elif min(tasks) > task:
            problems.append(f"{name}: Task {task} で宣言 / 最初に読むのは Task {min(tasks)}")
    return problems


def check_characters(text):
    bad = set()
    for ch in text:
        if ord(ch) < 128 or ch in "、。「」『』（）ー・…—→×":
            continue
        name = unicodedata.name(ch, "")
        if name.startswith(("CJK UNIFIED", "HIRAGANA", "KATAKANA", "FULLWIDTH",
                            "IDEOGRAPHIC", "BOX DRAWINGS", "HORIZONTAL ELLIPSIS")):
            continue
        bad.add(f"{ch!r} ({name})")
    return sorted(bad)


def check_test_counts(text):
    bounds = [m.start() for m in re.finditer(r"^### Task ", text, re.M)] + [len(text)]
    problems = []
    for index in range(len(bounds) - 1):
        section = text[bounds[index]:bounds[index + 1]]
        actual = len(re.findall(r"#\[test\]", section))
        claimed = re.search(r"Expected: (\d+)テスト PASS", section)
        if claimed and int(claimed.group(1)) != actual:
            problems.append(f"Task {index + 1}: 実数 {actual} / 記述 {claimed.group(1)}")
    return problems


def check_placeholders(text):
    return [w for w in ("TBD", "第2版", "...省略", "適宜", "後で書く") if w in text]


def parse_hand(notation):
    tiles, buffer = [], []
    for ch in notation:
        if ch.isdigit():
            buffer.append(int(ch))
        elif ch in "mpsz":
            for n in buffer:
                if ch == "z" and not 1 <= n <= 7:
                    raise ValueError(f"字牌 {n}z は存在しない")
                tiles.append(("z", n) if ch == "z" else (ch, 5 if n == 0 else n))
            buffer = []
        else:
            raise ValueError(f"不明な文字 {ch!r}")
    if buffer:
        raise ValueError("末尾に種別が無い")
    return tiles


def check_hands(text):
    problems = []
    for notation in sorted(set(re.findall(r'parse_hand\("([^"]+)"\)', text))):
        try:
            tiles = parse_hand(notation)
        except ValueError as error:
            problems.append(f"{notation}: {error}")
            continue
        counts = Counter(tiles)
        over = [k for k, v in counts.items() if v > 4]
        if over:
            problems.append(f"{notation}: 同一牌が5枚以上 {over}")
        if len(tiles) not in (2, 3, 4, 10, 11, 13, 14):
            problems.append(f"{notation}: 枚数が {len(tiles)} で不自然")
    return problems


def main(path):
    text = open(path).read()
    sections = [
        ("タスク境界をまたぐ前方参照", check_forward_references(text)),
        ("宣言したタスクで読まれないフィールド", check_unread_fields(text)),
        ("文字の混入", check_characters(text)),
        ("テスト数の不一致", check_test_counts(text)),
        ("プレースホルダ", check_placeholders(text)),
        ("手牌の記法と枚数", check_hands(text)),
    ]
    failed = False
    for title, problems in sections:
        if problems:
            failed = True
            print(f"[NG] {title}")
            for problem in problems:
                print(f"     {problem}")
        else:
            print(f"[OK] {title}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
