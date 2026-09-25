"""Builds the runtime dictionaries in resources/dict from the raw sources in third_party/raw.

Sources:
  * FrequencyWords (hermitdave), content CC-BY-SA 4.0: ru_50k.txt, en_50k.txt
  * eyo-kernel (hcodes), MIT: eyo_safe.txt

Outputs (UTF-8, one entry per line):
  resources/dict/ru.txt  -> "word count"   (ё folded into е, Cyrillic only)
  resources/dict/en.txt  -> "word count"   (Latin only, contractions added)
  resources/dict/yo.txt  -> "word"         (words with a guaranteed ё; "_" prefix = lowercase only)

Run:  python tools/build_dicts.py
"""
import io
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RAW = os.path.join(ROOT, "third_party", "raw")
OUT = os.path.join(ROOT, "resources", "dict")

RU_SINGLE = set("авиксуоя")
EN_SINGLE = {"a", "i"}

# Fragments that FrequencyWords produced by splitting contractions on the apostrophe.
EN_FRAGMENTS = {
    "t", "s", "d", "m", "ll", "re", "ve", "don", "didn", "doesn", "isn", "wasn", "aren",
    "couldn", "wouldn", "shouldn", "haven", "hasn", "hadn", "weren", "mustn", "needn", "ain",
}

EN_CONTRACTIONS = {
    "don't": "don", "can't": "can", "won't": "won", "isn't": "isn", "aren't": "aren",
    "wasn't": "wasn", "weren't": "weren", "doesn't": "doesn", "didn't": "didn",
    "haven't": "haven", "hasn't": "hasn", "hadn't": "hadn", "couldn't": "couldn",
    "wouldn't": "wouldn", "shouldn't": "shouldn", "mustn't": "mustn", "needn't": "needn",
    "ain't": "ain", "i'm": "m", "i've": "ve", "i'll": "ll", "i'd": "d", "you're": "re",
    "you've": "ve", "you'll": "ll", "you'd": "d", "he's": "s", "she's": "s", "it's": "s",
    "we're": "re", "we've": "ve", "we'll": "ll", "we'd": "d", "they're": "re",
    "they've": "ve", "they'll": "ll", "they'd": "d", "that's": "s", "there's": "s",
    "what's": "s", "who's": "s", "let's": "s", "here's": "s", "where's": "s",
    "how's": "s", "he'll": "ll", "she'll": "ll", "he'd": "d", "she'd": "d",
    "that'll": "ll", "it'll": "ll", "o'clock": None, "y'all": None,
}


def read_freq(name):
    path = os.path.join(RAW, name)
    out = []
    with io.open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.split()
            if len(parts) != 2:
                continue
            word, count = parts[0].lower(), int(parts[1])
            out.append((word, count))
    return out


def build_ru():
    rx = re.compile(r"^[а-яё]+(-[а-яё]+)*$")
    acc = {}
    for word, count in read_freq("ru_50k.txt"):
        if not rx.match(word):
            continue
        if len(word) == 1 and word not in RU_SINGLE:
            continue
        key = word.replace("ё", "е")
        acc[key] = acc.get(key, 0) + count
    return acc


def build_en():
    rx = re.compile(r"^[a-z]+(-[a-z]+)*$")
    raw = read_freq("en_50k.txt")
    freq = dict(raw)
    acc = {}
    for word, count in raw:
        if not rx.match(word):
            continue
        if len(word) == 1 and word not in EN_SINGLE:
            continue
        if word in EN_FRAGMENTS:
            continue
        # Subtitle OCR junk: "lf" for "if", "lt" for "it", "ls" for "is"...
        if word.startswith("l") and len(word) <= 3:
            twin = "i" + word[1:]
            if freq.get(twin, 0) >= 10 * count:
                continue
        acc[word] = acc.get(word, 0) + count
    for word, frag in EN_CONTRACTIONS.items():
        base = freq.get(frag, 0) if frag else 0
        acc[word] = max(acc.get(word, 0), base // 3, 50000)
    return acc


def expand_eyo(line):
    """Expands eyo-kernel notation: 'стем(а|у|)' -> ['стема', 'стему', 'стем']."""
    line = line.split("#")[0].strip()
    if not line:
        return []
    if "(" not in line:
        return [line]
    parts = re.split(r"[(|)]", line)
    stem = parts[0]
    return [stem + suffix for suffix in parts[1:-1]]


# Colloquial words eyo-kernel leaves out ("че" also reads as a name), but in typed
# Russian they are always "ё".
EXTRA_YO = ["чё", "ничё", "щас", "нечё"]


def build_yo():
    words = []
    seen = set()
    with io.open(os.path.join(RAW, "eyo_safe.txt"), encoding="utf-8") as f:
        for line in f:
            for w in expand_eyo(line):
                if "ё" not in w.lower() or w in seen:
                    continue
                seen.add(w)
                words.append(w)
    words.extend(w for w in EXTRA_YO if "ё" in w and w not in seen)
    return words


def write_freq(name, acc):
    items = sorted(acc.items(), key=lambda kv: (-kv[1], kv[0]))
    with io.open(os.path.join(OUT, name), "w", encoding="utf-8", newline="\n") as f:
        for word, count in items:
            f.write(f"{word} {count}\n")
    print(f"{name}: {len(items)} words")


def main():
    os.makedirs(OUT, exist_ok=True)
    write_freq("ru.txt", build_ru())
    write_freq("en.txt", build_en())
    yo = build_yo()
    with io.open(os.path.join(OUT, "yo.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(yo) + "\n")
    print(f"yo.txt: {len(yo)} words")


if __name__ == "__main__":
    main()
