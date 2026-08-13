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
            for pattern in (r"\bfn (\w+)\s*[<(]", r"^\s*(?:pub(?:\(\w+\))?\s+)?const (\w+)\s*:"):
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
            # 受け手を self と engine に限る。`tile.kind()` のような
            # 他の型のメソッドと名前が衝突しても誤検出しない。
            for name in re.findall(r"(?:self|engine)\.(\w+)\s*\(", line):
                used[name].append((task, line.strip()))
            for name in re.findall(r"(?<![.\w:!])(\w+)\s*\(", line):
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


RUST_KEYWORDS = {
    "pub", "if", "while", "for", "match", "return", "fn", "let", "mut", "else",
    "unsafe", "impl", "where", "as", "in", "ref", "move", "dyn", "crate", "super",
}

BUILTIN_NAMES = {
    "Some", "None", "Ok", "Err", "String", "Vec", "Box", "Default",
    "from_fn", "take", "replace", "min", "max", "swap", "usize", "u8", "u32", "u64", "i32",
    "drop", "matches", "assert", "assert_eq", "assert_ne", "format", "panic", "vec",
}

# 標準ライブラリと prelude の型。use を書かなくても使える。
BUILTIN_TYPES = {
    "Some", "None", "Ok", "Err", "Option", "Result", "String", "Vec", "Box",
    "Default", "Iterator", "Clone", "Copy", "PartialEq", "Eq", "Debug", "Self",
    "Send", "Sync", "Sized", "Drop", "From", "Into", "Ord", "PartialOrd", "Hash",
}


def target_files(text, base):
    """計画が Modify / Create と書いたファイルのうち、実在するもの。

    既存ファイルへ差分を当てる計画では、親モジュールの use や定義が
    計画本文に現れない。対象ファイルを読まないとスコープを見誤る。
    """
    import os

    for path in re.findall(r"- (?:Modify|Create): `([^`]+)`", text):
        full = os.path.join(base, path)
        if os.path.isfile(full):
            yield open(full).read()


def scope_of_source(source):
    """ソースが親モジュールへ持ち込む名前。"""
    source = join_multiline_uses(source)
    names = set()
    inside_test = False
    for line in source.split("\n"):
        if re.match(r"^mod \w+ \{", line):
            inside_test = True
        elif line == "}":
            inside_test = False
        if inside_test:
            continue
        names.update(names_from_use(line))
        for pattern in (r"(?:struct|enum|trait|type) (\w+)", r"\bfn (\w+)", r"const (\w+)\s*:"):
            names.update(re.findall(pattern, line))
    return names


def test_modules(text):
    """`mod xxx_tests { ... }` の範囲を (開始オフセット, 名前, 中身) で返す。"""
    for m in re.finditer(r"^mod (\w+) \{\n(.*?)^\}", text, re.S | re.M):
        yield m.start(), m.group(1), m.group(2)


def join_multiline_uses(text):
    """複数行にまたがる `use a::{\n b,\n c,\n};` を1行へ畳む。

    行ごとに読む検査が、折り返した use を見落として本物の import を
    「解決できない」と誤判定するのを防ぐ。
    """
    return re.sub(
        r"use\s+[\w:]+::\{[^}]*\};",
        lambda m: " ".join(m.group(0).split()),
        text,
        flags=re.S,
    )


def names_from_use(line):
    """use 文が導入する名前。`a::b::{c, d as e}` から c と e を取る。"""
    m = re.match(r"\s*use ([\w:]+)(?:::\{([^}]*)\})?;", line)
    if not m:
        return []
    if m.group(2):
        out = []
        for part in m.group(2).split(","):
            part = part.strip()
            if not part:
                continue
            out.append(part.split(" as ")[-1].strip())
        return out
    return [m.group(1).split("::")[-1]]


def check_unresolved_calls(text, base="."):
    """テストモジュール内の、どこからも入ってこない関数呼び出し。

    `use super::*;` は親の私有 use も取り込むので、親の import も許す。
    兄弟モジュールの項目は取り込まれないため、明示 use が無ければ落ちる。
    """
    text = join_multiline_uses(text)
    module_spans = [(start, start + len(body)) for start, _, body in test_modules(text)]

    def in_module(offset):
        return any(a <= offset < b for a, b in module_spans)

    parent_scope = set(BUILTIN_NAMES)
    for source in target_files(text, base):
        parent_scope.update(scope_of_source(source))
    for offset, block in rust_blocks(text):
        for line in block.split("\n"):
            absolute = offset + block.index(line) if line in block else offset
            if in_module(absolute):
                continue
            parent_scope.update(names_from_use(line))
            for pattern in (r"\bfn (\w+)\s*[<(]", r"const (\w+)\s*:", r"(?:struct|enum) (\w+)"):
                m = re.search(pattern, line)
                if m:
                    parent_scope.add(m.group(1))

    problems = []
    for _, name, body in test_modules(text):
        scope = set(parent_scope)
        for line in body.split("\n"):
            scope.update(names_from_use(line))
            m = re.search(r"\bfn (\w+)\s*[<(]", line)
            if m:
                scope.add(m.group(1))
            m = re.search(r"const (\w+)\s*:", line)
            if m:
                scope.add(m.group(1))
            m = re.search(r"let (?:mut )?(\w+) = \|", line)  # クロージャ
            if m:
                scope.add(m.group(1))
        # 属性とコメントは呼び出しではない。
        code = "\n".join(
            line
            for line in body.split("\n")
            if not line.lstrip().startswith(("#[", "//", "///"))
        )
        for called in sorted(set(re.findall(r"(?<![.\w:!])(\w+)\s*\(", code))):
            if called in RUST_KEYWORDS:
                continue
            if called not in scope:
                problems.append(f"{name}: {called}() を解決できない")
    return problems


def check_unresolved_types(text, base="."):
    """テストモジュール内の、どこからも入ってこない型名。

    `RiichiState` のように `use` を書き忘れた型を捕まえる。大文字始まりの
    識別子だけを見て、モジュール自身の use・親の use・親の定義・標準の型の
    いずれにも無いものを報告する。
    """
    text = join_multiline_uses(text)
    module_spans = [(start, start + len(body)) for start, _, body in test_modules(text)]

    def in_module(offset):
        return any(a <= offset < b for a, b in module_spans)

    parent_scope = set(BUILTIN_TYPES)
    for source in target_files(text, base):
        parent_scope.update(scope_of_source(source))
    for offset, block in rust_blocks(text):
        for line in block.split("\n"):
            if in_module(offset + block.find(line)):
                continue
            parent_scope.update(names_from_use(line))
            for pattern in (r"(?:struct|enum|trait|type) (\w+)", r"\bfn (\w+)"):
                for name in re.findall(pattern, line):
                    parent_scope.add(name)

    problems = []
    for _, name, body in test_modules(text):
        scope = set(parent_scope)
        for line in body.split("\n"):
            scope.update(names_from_use(line))
            for pattern in (r"(?:struct|enum|trait|type) (\w+)", r"\bfn (\w+)"):
                for found in re.findall(pattern, line):
                    scope.add(found)
        code = "\n".join(
            line
            for line in body.split("\n")
            if not line.lstrip().startswith(("#[", "//", "///"))
        )
        # 完全修飾（`crate::state::RiichiState`）は use が要らない。
        code = re.sub(r"\b\w+(?:::\w+)+", lambda m: m.group(0).split("::")[0], code)
        # 型が置かれる位置だけを見る。値の側まで見ると列挙子と衝突して騒がしい。
        used_types = set()
        for pattern in (r"->\s*([A-Z]\w*)", r":\s*&?(?:mut )?([A-Z]\w*)"):
            used_types.update(re.findall(pattern, code))
        for used in sorted(used_types):
            if used not in scope:
                problems.append(f"{name}: {used} を解決できない")
    return problems


def check_characters(text):
    bad = set()
    for ch in text:
        if ord(ch) < 128 or ch in "、。「」『』（）ー・…—→×〜§":
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
        # 行頭のものだけを数える。散文中の `#[test]` という言及は宣言ではない。
        actual = len(re.findall(r"^[ \t]*#\[(?:tokio::)?test[\](]", section, re.M))
        claimed = (re.search(r"Expected: (\d+)テスト PASS", section)
                   or re.search(r"Expected: (\d+) passed$", section, re.M))
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
        # 副露があると手牌は 14 未満のいろいろな枚数になる。槓なら副露が
        # 4枚を占めるので、7枚のような値も正しい。上限だけを見る。
        if not 1 <= len(tiles) <= 14:
            problems.append(f"{notation}: 枚数が {len(tiles)} で範囲外")
    return problems


def main(path):
    import os

    text = open(path).read()
    # 計画のパスから見たリポジトリの根。docs/superpowers/plans/ の3つ上。
    base = os.path.abspath(os.path.join(os.path.dirname(path), "..", "..", ".."))
    if not re.search(r"^### Task ", text, re.M):
        print("[NG] 計画の形式")
        print("     '### Task N:' の見出しが1つも無い。タスクを分けた検査が"
              "すべて空振りするため、ここで止める。")
        return 1

    sections = [
        ("タスク境界をまたぐ前方参照", check_forward_references(text)),
        ("宣言したタスクで読まれないフィールド", check_unread_fields(text)),
        ("解決できない関数呼び出し", check_unresolved_calls(text, base)),
        ("解決できない型名", check_unresolved_types(text, base)),
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
